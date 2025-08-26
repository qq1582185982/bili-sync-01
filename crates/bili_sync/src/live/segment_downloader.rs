use anyhow::{anyhow, Result};
use futures::future;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use m3u8_rs;

use crate::bilibili::BiliClient;
use super::api::Quality;
use super::m3u8_parser::{M3u8Parser, SegmentInfo};

/// 分片下载器状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStatus {
    Idle,
    Downloading,
    Error,
}

/// 分片下载器
pub struct SegmentDownloader {
    /// B站API客户端（使用其内部的reqwest客户端）
    client: Arc<BiliClient>,
    /// M3U8解析器
    parser: M3u8Parser,
    /// 当前状态
    status: DownloadStatus,
    /// 工作目录
    work_dir: PathBuf,
    /// 房间ID
    room_id: i64,
    /// 录制质量
    quality: Quality,
    /// 当前M3U8 URL
    current_m3u8_url: Option<String>,
    /// 基础URL（用于相对路径分片）
    base_url: String,
    /// 下载统计
    download_stats: DownloadStats,
}

/// 下载统计信息
#[derive(Debug, Default)]
pub struct DownloadStats {
    pub total_segments: u64,
    pub successful_downloads: u64,
    pub failed_downloads: u64,
    pub total_bytes: u64,
    pub start_time: Option<Instant>,
}

impl SegmentDownloader {
    /// 创建新的分片下载器
    pub async fn new(
        client: Arc<BiliClient>,
        work_dir: PathBuf,
        room_id: i64,
        quality: Quality,
    ) -> Result<Self> {
        // 确保工作目录存在
        tokio::fs::create_dir_all(&work_dir).await
            .map_err(|e| anyhow!("创建工作目录失败: {}", e))?;
        
        info!("分片下载器已初始化，工作目录: {:?}", work_dir);

        Ok(Self {
            client,
            parser: M3u8Parser::new(),
            status: DownloadStatus::Idle,
            work_dir,
            room_id,
            quality,
            current_m3u8_url: None,
            base_url: String::new(),
            download_stats: DownloadStats::default(),
        })
    }

    /// 开始分片下载，支持回调函数处理下载完成的分片
    pub async fn start<F>(&mut self, segment_callback: F) -> Result<()> 
    where
        F: Fn(SegmentInfo, u64, PathBuf) + Send + Sync + 'static,
    {
        if self.status == DownloadStatus::Downloading {
            return Err(anyhow!("分片下载器已在运行中"));
        }

        info!("开始分片录制，房间: {}, 质量: {:?}", self.room_id, self.quality);
        debug!("📥 SegmentDownloader::start 已接收到回调函数");
        
        self.status = DownloadStatus::Downloading;
        self.download_stats.start_time = Some(Instant::now());
        
        // 获取初始M3U8 URL
        self.refresh_m3u8_url().await?;
        
        // 下载初始化段（DASH格式需要）
        info!("🔍 开始检查和下载初始化段...");
        match self.download_initialization_segment().await {
            Ok(Some(header_path)) => {
                info!("✅ 初始化段已保存到: {}", header_path);
            }
            Ok(None) => {
                warn!("⚠️  未找到初始化段，继续录制常规分片");
            }
            Err(e) => {
                error!("❌ 下载初始化段时发生错误: {}", e);
                warn!("⚠️  继续录制常规分片");
            }
        }
        
        // 复刻bili-shadowreplay的segment下载循环
        info!("🎬 开始分片下载循环...");
        let mut segment_counter = 0;
        let mut last_sequence = 0u64;

        while self.status == DownloadStatus::Downloading {
            // 刷新M3U8获取最新分片列表
            if let Err(e) = self.refresh_m3u8_url().await {
                error!("刷新M3U8失败: {}", e);
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }

            // 获取并解析M3U8内容
            let m3u8_url = match &self.current_m3u8_url {
                Some(url) => url.clone(),
                None => {
                    error!("M3U8 URL为空");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            let playlist_content = match self.fetch_playlist(&m3u8_url).await {
                Ok(content) => content,
                Err(e) => {
                    error!("获取M3U8内容失败: {}", e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            // 使用m3u8-rs解析播放列表（复刻bili-shadowreplay的方法）
            let playlist = match m3u8_rs::parse_playlist_res(playlist_content.as_bytes()) {
                Ok(playlist) => playlist,
                Err(e) => {
                    error!("解析M3U8失败: {:?}", e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            // 处理MediaPlaylist
            if let m3u8_rs::Playlist::MediaPlaylist(media_playlist) = playlist {
                let current_sequence = media_playlist.media_sequence;
                
                info!("解析到 {} 个分片，sequence从 {} 开始", 
                    media_playlist.segments.len(), current_sequence);

                // 收集本轮要下载的所有分片
                let mut download_tasks = vec![];
                
                for (i, ts_segment) in media_playlist.segments.iter().enumerate() {
                    let sequence = current_sequence + i as u64;
                    
                    // 跳过已下载的segments
                    if sequence <= last_sequence {
                        continue;
                    }

                    segment_counter += 1;
                    
                    // 构建完整的segment URL（基于base URL）
                    let segment_url = if ts_segment.uri.starts_with("http") {
                        ts_segment.uri.clone()
                    } else {
                        let uri_with_slash = if ts_segment.uri.starts_with('/') {
                            ts_segment.uri.clone()
                        } else {
                            format!("/{}", ts_segment.uri)
                        };
                        format!("{}{}", self.base_url.trim_end_matches('/'), uri_with_slash)
                    };

                    // 使用原始文件名（从URI中提取，如420516438.m4s）
                    let segment_filename = ts_segment.uri.split('/').last()
                        .unwrap_or(&format!("{}.m4s", sequence))
                        .to_string();
                    let segment_path = self.work_dir.join(&segment_filename);
                    let segment_path_clone = segment_path.clone();

                    // 复制需要的数据用于异步任务
                    let http_client = self.client.client.clone();
                    let duration = ts_segment.duration as f64;
                    
                    info!("📥 准备下载分片 {}: {}", segment_counter, ts_segment.uri);

                    // 创建并行下载任务
                    let download_task = tokio::spawn(async move {
                        // 直接使用HTTP下载，不依赖aria2
                        let response = http_client
                            .get(&segment_url)
                            .timeout(Duration::from_secs(10))
                            .send()
                            .await;
                        
                        match response {
                            Ok(resp) if resp.status() == 404 => {
                                // 404错误直接跳过，不重试
                                debug!("分片不存在(404)，跳过: {}", segment_url);
                                return Ok(None);
                            }
                            Ok(resp) if resp.status().is_success() => {
                                let bytes = resp.bytes().await?;
                                tokio::fs::write(&segment_path, &bytes).await?;
                                
                                // 返回成功结果
                                let segment_info = SegmentInfo {
                                    url: segment_url,
                                    sequence,
                                    duration,
                                    timestamp: chrono::Utc::now().timestamp_millis(),
                                    is_initialization: false,
                                    initialization_url: None,
                                };
                                return Ok(Some((segment_info, bytes.len(), segment_counter, segment_path_clone)));
                            }
                            Ok(resp) => {
                                return Err(anyhow!("HTTP错误: {}", resp.status()));
                            }
                            Err(e) => {
                                return Err(anyhow!("网络错误: {}", e));
                            }
                        }
                    });
                    
                    download_tasks.push(download_task);
                    last_sequence = sequence;
                    self.download_stats.total_segments += 1;
                }
                
                // 并行等待所有下载任务完成
                if !download_tasks.is_empty() {
                    info!("🚀 开始并行下载 {} 个分片", download_tasks.len());
                    let results = future::join_all(download_tasks).await;
                    
                    // 处理下载结果
                    for result in results {
                        match result {
                            Ok(Ok(Some((segment_info, size, counter, file_path)))) => {
                                info!("✅ 分片 {} 下载完成: {} bytes", counter, size);
                                self.download_stats.successful_downloads += 1;
                                self.download_stats.total_bytes += size as u64;
                                
                                // 调用回调函数
                                debug!("🔄 调用回调函数，分片: {}, 大小: {} bytes, 路径: {:?}", segment_info.sequence, size, file_path);
                                segment_callback(segment_info, size as u64, file_path);
                            }
                            Ok(Ok(None)) => {
                                // 404跳过的分片
                                debug!("⚪ 分片不存在，已跳过");
                            }
                            Ok(Err(e)) => {
                                error!("❌ 分片下载失败: {}", e);
                                self.download_stats.failed_downloads += 1;
                            }
                            Err(e) => {
                                error!("❌ 下载任务异常: {}", e);
                                self.download_stats.failed_downloads += 1;
                            }
                        }
                    }
                }
            } else {
                warn!("收到MasterPlaylist而不是MediaPlaylist，跳过此轮");
            }

            // 休眠等待新分片
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        info!("🔚 分片下载完成，总共处理{}个分片", segment_counter);
        info!("📊 下载统计 - 成功: {}, 失败: {}, 总大小: {} bytes", 
            self.download_stats.successful_downloads,
            self.download_stats.failed_downloads, 
            self.download_stats.total_bytes);
        
        Ok(())
    }

    /// 停止分片下载
    pub async fn stop(&mut self) -> Result<()> {
        if self.status != DownloadStatus::Downloading {
            return Ok(());
        }

        info!("停止分片录制");
        self.status = DownloadStatus::Idle;
        
        // 输出统计信息
        let stats = &self.download_stats;
        let duration = stats.start_time.map(|t| t.elapsed()).unwrap_or_default();
        
        info!(
            "分片录制统计 - 总分片: {}, 成功: {}, 失败: {}, 总大小: {} bytes, 耗时: {:?}",
            stats.total_segments,
            stats.successful_downloads,
            stats.failed_downloads,
            stats.total_bytes,
            duration
        );

        Ok(())
    }

    /// 获取初始化段URL（从M3U8播放列表中解析）
    async fn get_initialization_segment_url(&self) -> Result<Option<String>> {
        let m3u8_url = match &self.current_m3u8_url {
            Some(url) => url,
            None => return Ok(None),
        };

        // 获取M3U8播放列表内容
        let empty_params = HashMap::new();
        let playlist_content = self.client
            .get_text_with_params(m3u8_url, &empty_params)
            .await
            .map_err(|e| anyhow!("获取M3U8播放列表失败: {}", e))?;

        // 使用正则表达式查找初始化段（bili-shadowreplay的方法）
        // 查找类似 "h123.m4s" 的初始化段URL
        let re = Regex::new(r"h.*\.m4s").unwrap();
        if let Some(captures) = re.captures(&playlist_content) {
            let header_filename = captures.get(0).unwrap().as_str();
            
            // 构建完整的初始化段URL
            let base_url = self.extract_base_url_from_m3u8(m3u8_url);
            let full_header_url = format!("{}{}", base_url, header_filename);
            
            info!("找到初始化段: {}", header_filename);
            return Ok(Some(full_header_url));
        }

        debug!("未在M3U8中找到初始化段");
        Ok(None)
    }

    /// 从M3U8 URL中提取基础URL
    fn extract_base_url_from_m3u8(&self, m3u8_url: &str) -> String {
        if let Some(last_slash_pos) = m3u8_url.rfind('/') {
            format!("{}/", &m3u8_url[..last_slash_pos])
        } else {
            m3u8_url.to_string()
        }
    }

    /// 下载初始化段
    async fn download_initialization_segment(&mut self) -> Result<Option<String>> {
        if let Some(header_url) = self.get_initialization_segment_url().await? {
            let filename = header_url.split('/').last().unwrap_or("header.m4s");
            let file_path = self.work_dir.join(filename);
            
            info!("下载初始化段: {} -> {:?}", header_url, file_path);
            
            // 使用HTTP客户端直接下载初始化段
            let response = self.client.client
                .get(&header_url)
                .timeout(Duration::from_secs(10))
                .send()
                .await;
                
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let bytes = resp.bytes().await
                        .map_err(|e| anyhow!("读取初始化段内容失败: {}", e))?;
                    
                    tokio::fs::write(&file_path, &bytes).await
                        .map_err(|e| anyhow!("写入初始化段失败: {}", e))?;
                    
                    let size = bytes.len();
                    info!("✅ 初始化段下载成功: {} bytes", size);
                    
                    if size > 0 {
                        // 创建初始化段的SegmentInfo（备用，暂不使用）
                        let _header_segment = SegmentInfo {
                            url: header_url,
                            sequence: 0, // 初始化段序列号为0
                            duration: 0.0,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                            is_initialization: true,
                            initialization_url: None,
                        };
                        
                        return Ok(Some(file_path.to_string_lossy().to_string()));
                    } else {
                        warn!("初始化段文件大小为0，可能下载失败");
                    }
                }
                Ok(resp) => {
                    error!("❌ 初始化段下载失败，HTTP状态: {}", resp.status());
                }
                Err(e) => {
                    error!("❌ 初始化段下载失败: {}", e);
                }
            }
        }
        
        Ok(None)
    }

    /// 刷新M3U8播放列表URL（使用正确的HLS API）
    pub async fn refresh_m3u8_url(&mut self) -> Result<()> {
        debug!("获取HLS master playlist，房间: {}", self.room_id);

        // 使用正确的HLS API端点（从bili-shadowreplay项目发现）
        let mut params = HashMap::new();
        params.insert("cid".to_string(), self.room_id.to_string());
        params.insert("pt".to_string(), "h5".to_string());
        params.insert("p2p_type".to_string(), "-1".to_string());
        params.insert("net".to_string(), "0".to_string());
        params.insert("free_type".to_string(), "0".to_string());
        params.insert("build".to_string(), "0".to_string());
        params.insert("feature".to_string(), "2".to_string());
        params.insert("qn".to_string(), (self.quality as i32).to_string());

        // 直接获取HLS master playlist内容
        let master_playlist_content = self.client
            .get_text_with_params("https://api.live.bilibili.com/xlive/play-gateway/master/url", &params)
            .await
            .map_err(|e| anyhow!("获取HLS master playlist失败: {}", e))?;

        info!("获取到HLS master playlist内容: {} bytes", master_playlist_content.len());
        debug!("Master playlist前200字符: {}", &master_playlist_content.chars().take(200).collect::<String>());

        // 解析master playlist，提取第一个变体流的URL
        // Master playlist格式示例:
        // #EXTM3U
        // #EXT-X-VERSION:6
        // #EXT-X-STREAM-INF:BANDWIDTH=1234567,RESOLUTION=1920x1080,CODECS="avc1.640028,mp4a.40.2",BILI-DISPLAY="原画"
        // https://host/path/index.m3u8?params
        
        let lines: Vec<&str> = master_playlist_content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.starts_with("#EXT-X-STREAM-INF:") {
                // 下一行应该是实际的M3U8 URL
                if i + 1 < lines.len() {
                    let variant_url = lines[i + 1].trim();
                    if variant_url.contains(".m3u8") {
                        self.current_m3u8_url = Some(variant_url.to_string());
                        
                        // 提取基础URL
                        if let Some(last_slash) = variant_url.rfind('/') {
                            self.base_url = variant_url[..last_slash + 1].to_string();
                        }
                        
                        info!("✅ 从HLS master playlist提取到变体流URL: {}", variant_url);
                        return Ok(());
                    }
                }
            }
        }

        // 如果没有找到标准的EXT-X-STREAM-INF格式，尝试查找任何m3u8链接
        for line in lines.iter() {
            let line = line.trim();
            if line.starts_with("http") && line.contains(".m3u8") {
                self.current_m3u8_url = Some(line.to_string());
                
                // 提取基础URL
                if let Some(last_slash) = line.rfind('/') {
                    self.base_url = line[..last_slash + 1].to_string();
                }
                
                info!("✅ 从master playlist直接提取到M3U8 URL: {}", line);
                return Ok(());
            }
        }

        // 如果解析失败，输出完整内容用于调试
        warn!("无法从master playlist中提取M3U8 URL");
        warn!("完整的master playlist内容:\n{}", master_playlist_content);
        
        Err(anyhow!("无法从HLS master playlist中提取变体流URL"))
    }

    /// 执行一轮分片下载
    pub async fn download_round(&mut self) -> Result<Vec<(SegmentInfo, u64)>> {
        let m3u8_url = self.current_m3u8_url.as_ref()
            .ok_or_else(|| anyhow!("M3U8 URL未初始化"))?;

        // 获取M3U8播放列表
        let playlist_content = self.fetch_playlist(m3u8_url).await?;
        
        // 解析新分片
        let new_segments = self.parser.parse_playlist(&playlist_content, &self.base_url);
        
        debug!("发现 {} 个新分片", new_segments.len());
        
        // 下载新分片，返回成功下载的分片信息和文件大小
        let mut downloaded_segments = Vec::new();
        
        for segment in new_segments {
            match self.download_segment(&segment).await {
                Ok(file_size) => {
                    downloaded_segments.push((segment, file_size));
                    self.download_stats.successful_downloads += 1;
                }
                Err(e) => {
                    error!("下载分片失败: {}, 错误: {}", segment.url, e);
                    self.download_stats.failed_downloads += 1;
                }
            }
            self.download_stats.total_segments += 1;
        }
        
        debug!("成功下载 {} 个分片", downloaded_segments.len());
        Ok(downloaded_segments)
    }

    /// 获取M3U8播放列表内容
    async fn fetch_playlist(&self, url: &str) -> Result<String> {
        debug!("获取播放列表: {}", url);
        
        let response = self.client.client
            .get(url)
            .send()
            .await
            .map_err(|e| anyhow!("请求播放列表失败: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!("播放列表请求失败，状态码: {}", response.status()));
        }

        let content = response.text().await
            .map_err(|e| anyhow!("读取播放列表内容失败: {}", e))?;

        debug!("播放列表大小: {} bytes", content.len());
        Ok(content)
    }

    /// 下载单个分片
    async fn download_segment(&mut self, segment: &SegmentInfo) -> Result<u64> {
        let filename = format!("segment_{:06}.ts", segment.sequence);
        let file_path = self.work_dir.join(&filename);
        
        debug!("下载分片: {} -> {:?}", segment.url, file_path);
        
        let start_time = Instant::now();
        
        // 使用HTTP客户端直接下载分片
        let response = self.client.client
            .get(&segment.url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| anyhow!("请求分片失败: {}", e))?;
            
        if !response.status().is_success() {
            return Err(anyhow!("分片下载失败，状态码: {}", response.status()));
        }
        
        let bytes = response.bytes().await
            .map_err(|e| anyhow!("读取分片内容失败: {}", e))?;
            
        tokio::fs::write(&file_path, &bytes).await
            .map_err(|e| anyhow!("写入分片文件失败: {}", e))?;
        
        // 获取文件大小
        let size = bytes.len() as u64;
        
        let download_time = start_time.elapsed();
        self.download_stats.total_bytes += size;
        
        debug!(
            "分片 {} 下载完成，大小: {} bytes，耗时: {:?}",
            segment.sequence, size, download_time
        );

        Ok(size)
    }

    /// 检查下载器状态
    pub fn status(&self) -> DownloadStatus {
        self.status
    }

    /// 获取下载统计
    pub fn stats(&self) -> &DownloadStats {
        &self.download_stats
    }

    /// 获取工作目录
    pub fn work_dir(&self) -> &PathBuf {
        &self.work_dir
    }
}

impl DownloadStats {
    /// 计算下载成功率
    pub fn success_rate(&self) -> f64 {
        if self.total_segments == 0 {
            1.0
        } else {
            self.successful_downloads as f64 / self.total_segments as f64
        }
    }

    /// 计算平均下载速度（bytes/sec）
    pub fn average_speed(&self) -> f64 {
        if let Some(start_time) = self.start_time {
            let elapsed = start_time.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                return self.total_bytes as f64 / elapsed;
            }
        }
        0.0
    }
}