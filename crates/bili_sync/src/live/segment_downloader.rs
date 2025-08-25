use anyhow::{anyhow, Result};
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use m3u8_rs;

use crate::bilibili::BiliClient;
use crate::unified_downloader::UnifiedDownloader;
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
    /// 统一下载器，复用现有下载逻辑
    downloader: UnifiedDownloader,
    /// B站API客户端
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

        // 创建统一下载器
        let downloader = UnifiedDownloader::new_smart(client.client.clone()).await;
        
        info!("分片下载器已初始化，工作目录: {:?}", work_dir);

        Ok(Self {
            downloader,
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

    /// 开始分片下载
    pub async fn start(&mut self) -> Result<()> {
        if self.status == DownloadStatus::Downloading {
            return Err(anyhow!("分片下载器已在运行中"));
        }

        info!("开始分片录制，房间: {}, 质量: {:?}", self.room_id, self.quality);
        
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
        let max_segments = 20; // 临时限制用于测试

        while self.status == DownloadStatus::Downloading && segment_counter < max_segments {
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

                // 下载新的segments（复刻bili-shadowreplay逻辑）
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

                    // 生成segment文件名（使用.ts扩展名，与bili-shadowreplay一致）
                    let segment_filename = format!("segment_{:09}.ts", segment_counter);
                    let segment_path = self.work_dir.join(&segment_filename);

                    info!("📥 下载分片 {}/{}: {}", 
                        segment_counter, max_segments, ts_segment.uri);

                    // 下载segment（重试机制）
                    let mut retry_count = 0;
                    let max_retries = 3;
                    
                    while retry_count < max_retries {
                        match self.downloader.fetch_with_fallback(&[&segment_url], &segment_path).await {
                            Ok(_) => {
                                // 检查文件大小
                                match tokio::fs::metadata(&segment_path).await {
                                    Ok(metadata) => {
                                        let size = metadata.len();
                                        if size > 0 {
                                            info!("✅ 分片 {} 下载完成: {} bytes", segment_counter, size);
                                            self.download_stats.successful_downloads += 1;
                                            self.download_stats.total_bytes += size;
                                            break; // 成功，跳出重试循环
                                        } else {
                                            warn!("⚠️  分片 {} 文件大小为0", segment_counter);
                                        }
                                    }
                                    Err(e) => {
                                        warn!("⚠️  无法获取分片 {} 文件信息: {}", segment_counter, e);
                                    }
                                }
                            }
                            Err(e) => {
                                retry_count += 1;
                                if retry_count >= max_retries {
                                    error!("❌ 分片 {} 下载失败，已重试{}次: {}", 
                                        segment_counter, max_retries, e);
                                    self.download_stats.failed_downloads += 1;
                                } else {
                                    warn!("⚠️  分片 {} 下载失败，重试第{}次: {}", 
                                        segment_counter, retry_count, e);
                                    tokio::time::sleep(Duration::from_millis(500)).await;
                                }
                            }
                        }
                    }

                    last_sequence = sequence;
                    self.download_stats.total_segments += 1;
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
            
            // 使用统一下载器下载初始化段
            match self.downloader.fetch_with_fallback(&[&header_url], &file_path).await {
                Ok(_) => {
                    // 获取文件大小
                    let size = match tokio::fs::metadata(&file_path).await {
                        Ok(metadata) => metadata.len(),
                        Err(_) => 0,
                    };
                    
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
        
        // 使用统一下载器下载
        self.downloader
            .fetch_with_fallback(&[&segment.url], &file_path)
            .await?;
        
        // 获取文件大小
        let metadata = tokio::fs::metadata(&file_path).await?;
        let size = metadata.len();
        
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