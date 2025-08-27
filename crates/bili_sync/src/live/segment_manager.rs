use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};

use super::m3u8_parser::SegmentInfo;
use super::config::AutoMergeConfig;

/// 分片记录
#[derive(Debug, Clone)]
pub struct SegmentRecord {
    /// 分片URL
    pub url: String,
    /// 序列号
    pub sequence: u64,
    /// 时长（秒）
    pub duration: f64,
    /// 文件大小（字节）
    pub size: u64,
    /// 时间戳（毫秒）
    pub timestamp: i64,
    /// 本地文件路径
    pub file_path: PathBuf,
    /// 是否下载成功
    pub downloaded: bool,
}

/// 分片管理器
#[derive(Debug)]
pub struct SegmentManager {
    /// 分片记录列表
    segments: Vec<SegmentRecord>,
    /// 工作目录
    work_dir: PathBuf,
    /// 日志文件句柄
    log_file: Option<File>,
    /// 统计信息
    stats: SegmentStats,
    /// 自动合并配置
    auto_merge_config: Option<AutoMergeConfig>,
    /// 最后一次自动合并的时间戳
    last_auto_merge_timestamp: Option<i64>,
}

/// 分片统计信息
#[derive(Debug, Default)]
pub struct SegmentStats {
    /// 总分片数
    pub total_segments: usize,
    /// 成功下载的分片数
    pub downloaded_segments: usize,
    /// 总时长（秒）
    pub total_duration: f64,
    /// 总大小（字节）
    pub total_size: u64,
    /// 第一个分片的时间戳
    pub start_timestamp: Option<i64>,
    /// 最后一个分片的时间戳
    pub end_timestamp: Option<i64>,
}

impl SegmentManager {
    /// 创建新的分片管理器
    pub async fn new(work_dir: &Path) -> Result<Self> {
        // 确保工作目录存在
        tokio::fs::create_dir_all(work_dir).await
            .map_err(|e| anyhow!("创建工作目录失败: {}", e))?;

        let mut manager = Self {
            segments: Vec::new(),
            work_dir: work_dir.to_path_buf(),
            log_file: None,
            stats: SegmentStats::default(),
            auto_merge_config: None,
            last_auto_merge_timestamp: None,
        };

        // 初始化日志文件
        manager.init_log_file().await?;
        
        // 加载已有的分片信息
        manager.load_existing_segments().await?;

        info!("分片管理器已初始化，工作目录: {:?}", work_dir);
        Ok(manager)
    }

    /// 添加分片记录
    pub async fn add_segment(&mut self, segment_info: &SegmentInfo, file_size: u64, file_path: PathBuf) -> Result<()> {
        debug!("add_segment调用 - 序列号: {}, 时长: {:.2}秒, 文件大小: {} bytes, 路径: {:?}", 
               segment_info.sequence, segment_info.duration, file_size, file_path);

        let record = SegmentRecord {
            url: segment_info.url.clone(),
            sequence: segment_info.sequence,
            duration: segment_info.duration,
            size: file_size,
            timestamp: segment_info.timestamp,
            file_path,
            downloaded: true,
        };

        // 写入日志
        self.write_segment_log(&record).await?;
        
        // 添加到内存列表
        self.segments.push(record);
        
        debug!("分片已添加到管理器 - 总分片数: {}, 当前总时长: {:.2}秒", 
               self.segments.len(), self.stats.total_duration);
        
        // 更新统计
        self.update_stats();
        
        debug!("添加分片记录: 序列号={}, 大小={} bytes", segment_info.sequence, file_size);
        
        // 检查是否需要触发自动合并
        if self.should_auto_merge() {
            info!("触发自动合并条件，当前时长: {:.2}秒", self.stats.total_duration);
        }
        
        Ok(())
    }

    /// 标记分片下载失败
    pub async fn mark_segment_failed(&mut self, segment_info: &SegmentInfo) -> Result<()> {
        let filename = format!("segment_{:06}.ts", segment_info.sequence);
        let file_path = self.work_dir.join(&filename);

        let record = SegmentRecord {
            url: segment_info.url.clone(),
            sequence: segment_info.sequence,
            duration: segment_info.duration,
            size: 0,
            timestamp: segment_info.timestamp,
            file_path,
            downloaded: false,
        };

        // 写入失败日志
        self.write_segment_log(&record).await?;
        
        // 添加到内存列表
        self.segments.push(record);
        
        warn!("标记分片下载失败: 序列号={}", segment_info.sequence);
        Ok(())
    }

    /// 生成M3U8播放列表
    pub fn generate_m3u8(&self, is_live: bool) -> String {
        let mut m3u8 = String::from("#EXTM3U\n");
        m3u8.push_str("#EXT-X-VERSION:3\n");
        
        // 添加播放列表类型
        if is_live {
            m3u8.push_str("#EXT-X-PLAYLIST-TYPE:EVENT\n");
        } else {
            m3u8.push_str("#EXT-X-PLAYLIST-TYPE:VOD\n");
        }
        
        // 计算目标时长（最大分片时长的向上取整）
        let target_duration = self.segments
            .iter()
            .filter(|s| s.downloaded)
            .map(|s| s.duration)
            .fold(0.0, f64::max)
            .ceil() as u32;
        
        if target_duration > 0 {
            m3u8.push_str(&format!("#EXT-X-TARGETDURATION:{}\n", target_duration));
        }
        
        // 添加媒体序列号（第一个分片的序列号）
        if let Some(first_segment) = self.segments.first() {
            m3u8.push_str(&format!("#EXT-X-MEDIA-SEQUENCE:{}\n", first_segment.sequence));
        }
        
        // 添加分片信息
        for segment in &self.segments {
            if segment.downloaded {
                m3u8.push_str(&format!("#EXTINF:{:.3},\n", segment.duration));
                
                // 使用相对路径
                if let Some(filename) = segment.file_path.file_name() {
                    m3u8.push_str(&format!("{}\n", filename.to_string_lossy()));
                }
            }
        }
        
        // 如果不是直播模式，添加结束标记
        if !is_live {
            m3u8.push_str("#EXT-X-ENDLIST\n");
        }
        
        m3u8
    }

    /// 保存M3U8播放列表到文件
    pub async fn save_m3u8_playlist(&self, is_live: bool) -> Result<PathBuf> {
        let playlist_content = self.generate_m3u8(is_live);
        let playlist_path = self.work_dir.join("playlist.m3u8");
        
        tokio::fs::write(&playlist_path, playlist_content).await
            .map_err(|e| anyhow!("保存M3U8播放列表失败: {}", e))?;
        
        debug!("M3U8播放列表已保存: {:?}", playlist_path);
        Ok(playlist_path)
    }

    /// 清理旧分片文件
    pub async fn cleanup_segments(&mut self, keep_count: usize) -> Result<usize> {
        if self.segments.len() <= keep_count {
            return Ok(0);
        }

        let to_remove = self.segments.len() - keep_count;
        let mut removed = 0;
        let segments_to_remove = self.segments[..to_remove].to_vec();

        for segment in &segments_to_remove {
            if segment.downloaded && segment.file_path.exists() {
                match tokio::fs::remove_file(&segment.file_path).await {
                    Ok(_) => {
                        removed += 1;
                        debug!("清理分片文件: {:?}", segment.file_path);
                    }
                    Err(e) => {
                        warn!("清理分片文件失败: {:?}, 错误: {}", segment.file_path, e);
                    }
                }
            }
        }

        // 从内存中移除已清理的分片记录
        self.segments.drain(0..to_remove);
        
        // 更新统计信息
        self.update_stats();

        info!("清理了 {} 个旧分片文件", removed);
        Ok(removed)
    }

    /// 按总大小清理分片文件（保持在指定大小限制内）
    pub async fn cleanup_by_size(&mut self, max_size_mb: u64) -> Result<usize> {
        let max_size_bytes = max_size_mb * 1024 * 1024;
        let current_size = self.stats.total_size;
        
        if current_size <= max_size_bytes {
            return Ok(0);
        }

        let size_to_remove = current_size - max_size_bytes;
        let mut removed_size = 0u64;
        let mut removed_count = 0;
        let mut segments_to_keep = Vec::new();

        // 从最旧的分片开始删除
        for segment in &self.segments {
            if removed_size >= size_to_remove {
                segments_to_keep.push(segment.clone());
            } else if segment.downloaded && segment.file_path.exists() {
                match tokio::fs::remove_file(&segment.file_path).await {
                    Ok(_) => {
                        removed_size += segment.size;
                        removed_count += 1;
                        debug!("按大小清理分片: {:?}, 大小: {} bytes", segment.file_path, segment.size);
                    }
                    Err(e) => {
                        warn!("清理分片文件失败: {:?}, 错误: {}", segment.file_path, e);
                        // 删除失败的分片仍保留在列表中
                        segments_to_keep.push(segment.clone());
                    }
                }
            } else {
                segments_to_keep.push(segment.clone());
            }
        }

        // 更新分片列表
        self.segments = segments_to_keep;
        
        // 更新统计信息
        self.update_stats();

        info!("按大小清理了 {} 个分片文件，释放 {} MB 空间", 
              removed_count, removed_size / 1024 / 1024);
        Ok(removed_count)
    }

    /// 检查并清理磁盘空间（智能清理策略）
    pub async fn smart_cleanup(&mut self) -> Result<usize> {
        let segment_count = self.segments.len();
        let total_size_mb = self.stats.total_size / 1024 / 1024;
        
        // 策略1: 如果分片数量过多（超过200个），保留最近150个
        if segment_count > 200 {
            info!("分片数量过多 ({}个)，执行数量清理", segment_count);
            return self.cleanup_segments(150).await;
        }
        
        // 策略2: 如果总大小超过500MB，清理到400MB以下
        if total_size_mb > 500 {
            info!("分片总大小过大 ({} MB)，执行大小清理", total_size_mb);
            return self.cleanup_by_size(400).await;
        }
        
        // 策略3: 正常情况下保留最近100个分片
        if segment_count > 100 {
            debug!("执行常规清理，保留最近100个分片");
            return self.cleanup_segments(100).await;
        }

        Ok(0)
    }

    /// 紧急清理（磁盘空间不足时使用）
    pub async fn emergency_cleanup(&mut self) -> Result<usize> {
        warn!("执行紧急清理：磁盘空间不足！");
        
        // 紧急情况：只保留最近30个分片
        let keep_count = 30.min(self.segments.len());
        let cleaned = self.cleanup_segments(keep_count).await?;
        
        if cleaned > 0 {
            warn!("紧急清理完成：删除了 {} 个分片文件，仅保留最近 {} 个", cleaned, keep_count);
        }
        
        Ok(cleaned)
    }

    /// 获取工作目录的可用磁盘空间（MB）
    pub async fn get_available_disk_space(&self) -> Result<u64> {
        
        // 使用statvfs系统调用或Windows API获取可用空间
        // 这里提供一个简化实现，实际项目中可使用fs2或sysinfo crate
        match tokio::fs::metadata(&self.work_dir).await {
            Ok(_) => {
                // 简化实现：假设有足够空间，实际应该调用系统API
                // 在生产环境中，建议使用sysinfo::System::available_space()
                Ok(1024) // 返回假定的1GB可用空间
            }
            Err(_) => Ok(0)
        }
    }

    /// 检查磁盘空间是否足够
    pub async fn check_disk_space(&mut self, min_free_mb: u64) -> Result<bool> {
        let available = self.get_available_disk_space().await?;
        
        if available < min_free_mb {
            warn!("磁盘空间不足：可用 {} MB，需要 {} MB", available, min_free_mb);
            
            // 尝试紧急清理
            self.emergency_cleanup().await?;
            
            // 再次检查
            let available_after = self.get_available_disk_space().await?;
            Ok(available_after >= min_free_mb)
        } else {
            Ok(true)
        }
    }

    /// 获取统计信息
    pub fn stats(&self) -> &SegmentStats {
        &self.stats
    }

    /// 获取分片数量
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// 获取成功下载的分片数量
    pub fn downloaded_count(&self) -> usize {
        self.segments.iter().filter(|s| s.downloaded).count()
    }

    /// 初始化日志文件
    async fn init_log_file(&mut self) -> Result<()> {
        let log_path = self.work_dir.join("segments.log");
        
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
            .map_err(|e| anyhow!("打开日志文件失败: {}", e))?;
        
        self.log_file = Some(file);
        debug!("分片日志文件已初始化: {:?}", log_path);
        Ok(())
    }

    /// 写入分片日志
    async fn write_segment_log(&mut self, segment: &SegmentRecord) -> Result<()> {
        if let Some(log_file) = &mut self.log_file {
            let log_entry = format!(
                "{}|{}|{:.3}|{}|{}|{}\n",
                segment.sequence,
                segment.url,
                segment.duration,
                segment.size,
                segment.timestamp,
                if segment.downloaded { "OK" } else { "FAILED" }
            );
            
            log_file.write_all(log_entry.as_bytes()).await
                .map_err(|e| anyhow!("写入日志失败: {}", e))?;
            
            log_file.flush().await
                .map_err(|e| anyhow!("刷新日志失败: {}", e))?;
        }
        Ok(())
    }

    /// 从日志文件加载已有分片信息
    async fn load_existing_segments(&mut self) -> Result<()> {
        let log_path = self.work_dir.join("segments.log");
        
        if !log_path.exists() {
            debug!("分片日志文件不存在，跳过加载");
            return Ok(());
        }
        
        let file = tokio::fs::File::open(&log_path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        
        let mut loaded_count = 0;
        
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(segment) = self.parse_log_line(&line) {
                self.segments.push(segment);
                loaded_count += 1;
            }
        }
        
        // 更新统计
        self.update_stats();
        
        info!("从日志文件加载了 {} 个分片记录", loaded_count);
        Ok(())
    }

    /// 解析日志行
    fn parse_log_line(&self, line: &str) -> Result<SegmentRecord> {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 6 {
            return Err(anyhow!("日志行格式错误: {}", line));
        }

        let sequence = parts[0].parse()?;
        let url = parts[1].to_string();
        let duration = parts[2].parse()?;
        let size = parts[3].parse()?;
        let timestamp = parts[4].parse()?;
        let downloaded = parts[5] == "OK";

        // 从URL中提取实际的文件名，而不是使用硬编码格式
        let filename = if let Some(filename_from_url) = url.split('/').last() {
            filename_from_url.to_string()
        } else {
            // 如果无法从URL提取，回退到序列号命名
            format!("{}.m4s", sequence)
        };
        let file_path = self.work_dir.join(&filename);

        Ok(SegmentRecord {
            url,
            sequence,
            duration,
            size,
            timestamp,
            file_path,
            downloaded,
        })
    }

    /// 合并segments到MP4（完全复刻bili-shadowreplay的方法）
    pub async fn merge_segments_to_mp4(&self, output_path: &Path) -> Result<PathBuf> {
        info!("🎬 开始合并segments到MP4（bili-shadowreplay方式）: {:?}", output_path);
        
        // 获取所有成功下载的分片文件
        let downloaded_segments: Vec<_> = self.segments
            .iter()
            .filter(|s| s.downloaded && s.file_path.exists())
            .collect();
        
        if downloaded_segments.is_empty() {
            return Err(anyhow!("没有可合并的分片文件"));
        }
        
        info!("找到 {} 个可合并的分片文件", downloaded_segments.len());
        
        // 1. 生成完整的M3U8索引文件（复刻bili-shadowreplay的entry_store.manifest()）
        let m3u8_path = self.work_dir.join("index.m3u8");
        self.generate_bili_shadowreplay_m3u8(&downloaded_segments, &m3u8_path).await?;
        
        // 2. 使用FFmpeg从M3U8文件直接转换（复刻bili-shadowreplay的clip_from_m3u8）
        self.bili_shadowreplay_clip_from_m3u8(&m3u8_path, output_path).await?;
        
        // 3. 清理临时文件
        let _ = tokio::fs::remove_file(&m3u8_path).await;
        
        Ok(output_path.to_path_buf())
    }

    /// 生成M3U8清单文件（复刻bili-shadowreplay的EntryStore::manifest()）
    async fn generate_bili_shadowreplay_m3u8(&self, segments: &[&SegmentRecord], m3u8_path: &Path) -> Result<()> {
        info!("生成M3U8清单文件（bili-shadowreplay格式）: {:?}", m3u8_path);
        
        let mut m3u8_content = String::new();
        
        // M3U8头部（标准格式）
        m3u8_content.push_str("#EXTM3U\n");
        m3u8_content.push_str("#EXT-X-VERSION:3\n");
        
        // 计算target duration（最大分片时长的向上取整）
        let target_duration = segments
            .iter()
            .map(|s| s.duration)
            .fold(0.0, f64::max)
            .ceil() as u32;
        
        m3u8_content.push_str(&format!("#EXT-X-TARGETDURATION:{}\n", target_duration));
        m3u8_content.push_str("#EXT-X-MEDIA-SEQUENCE:0\n");
        
        // 检查是否需要初始化段（DASH/M4S格式）
        let init_segment = self.find_initialization_segment().await;
        if let Some(init_path) = init_segment {
            let init_filename = init_path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("header.m4s");
            m3u8_content.push_str(&format!("#EXT-X-MAP:URI=\"{}\"\n", init_filename));
            info!("在M3U8中包含初始化段: {}", init_filename);
        }
        
        // 排序segments（按序列号）
        let mut sorted_segments = segments.to_vec();
        sorted_segments.sort_by_key(|s| s.sequence);
        
        // 添加所有segment条目
        for segment in &sorted_segments {
            if let Some(filename) = segment.file_path.file_name().and_then(|s| s.to_str()) {
                // 使用实际的segment时长
                let duration = if segment.duration > 0.0 { 
                    segment.duration 
                } else {
                    // 如果没有时长信息，尝试从文件获取
                    self.get_segment_duration_from_file(&segment.file_path).await.unwrap_or(5.0)
                };
                
                m3u8_content.push_str(&format!("#EXTINF:{:.6},\n", duration));
                m3u8_content.push_str(&format!("{}\n", filename));
            }
        }
        
        // M3U8结尾标记（VOD模式）
        m3u8_content.push_str("#EXT-X-ENDLIST\n");
        
        // 写入M3U8文件
        tokio::fs::write(m3u8_path, m3u8_content).await
            .map_err(|e| anyhow!("写入M3U8文件失败: {}", e))?;
        
        info!("✅ M3U8清单生成完成，包含 {} 个分片", sorted_segments.len());
        Ok(())
    }

    /// 从M3U8文件转换为MP4（完全复刻bili-shadowreplay的clip_from_m3u8）
    async fn bili_shadowreplay_clip_from_m3u8(&self, m3u8_path: &Path, output_path: &Path) -> Result<()> {
        info!("🔄 使用FFmpeg从M3U8转换为MP4（bili-shadowreplay方式）...");
        
        // 确保输出目录存在
        if let Some(output_dir) = output_path.parent() {
            tokio::fs::create_dir_all(output_dir).await
                .map_err(|e| anyhow!("创建输出目录失败: {}", e))?;
        }

        // 构建FFmpeg命令（完全复刻bili-shadowreplay的参数）
        let mut cmd = tokio::process::Command::new("ffmpeg");
        
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        
        // bili-shadowreplay的标准参数
        cmd.args([
            "-i", &m3u8_path.to_string_lossy(),
            "-c", "copy", // 流复制，无损转换
            "-y", // 覆盖输出文件
            &output_path.to_string_lossy()
        ]);

        info!("执行FFmpeg命令: ffmpeg -i {:?} -c copy -y {:?}", m3u8_path, output_path);
        
        let output = cmd.output().await
            .map_err(|e| anyhow!("FFmpeg执行失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            
            error!("FFmpeg转换失败:");
            error!("stdout: {}", stdout);
            error!("stderr: {}", stderr);
            
            // 如果copy模式失败，尝试重编码模式（bili-shadowreplay的fallback策略）
            warn!("流复制失败，尝试重编码模式...");
            return self.bili_shadowreplay_clip_with_reencoding(m3u8_path, output_path).await;
        }

        // 检查输出文件
        if !output_path.exists() {
            return Err(anyhow!("输出文件未生成"));
        }

        let metadata = tokio::fs::metadata(output_path).await?;
        if metadata.len() == 0 {
            return Err(anyhow!("输出文件大小为0"));
        }

        info!("✅ MP4转换完成，文件大小: {:.2} MB", metadata.len() as f64 / 1024.0 / 1024.0);
        
        Ok(())
    }

    /// 重编码模式的M3U8到MP4转换（bili-shadowreplay的fallback）
    async fn bili_shadowreplay_clip_with_reencoding(&self, m3u8_path: &Path, output_path: &Path) -> Result<()> {
        info!("🔄 使用重编码模式转换M3U8到MP4...");
        
        let mut cmd = tokio::process::Command::new("ffmpeg");
        
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000);
        
        // bili-shadowreplay的重编码参数
        cmd.args([
            "-i", &m3u8_path.to_string_lossy(),
            "-c:v", "libx264",  // H.264视频编码
            "-c:a", "aac",      // AAC音频编码
            "-preset", "fast",  // 快速预设
            "-y",
            &output_path.to_string_lossy()
        ]);

        info!("执行重编码FFmpeg命令: {:?}", cmd);
        
        let output = cmd.output().await
            .map_err(|e| anyhow!("重编码FFmpeg执行失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("重编码转换失败: {}", stderr));
        }

        let metadata = tokio::fs::metadata(output_path).await?;
        info!("✅ 重编码转换完成，文件大小: {:.2} MB", metadata.len() as f64 / 1024.0 / 1024.0);
        
        Ok(())
    }

    /// 查找初始化段文件
    async fn find_initialization_segment(&self) -> Option<PathBuf> {
        // 查找工作目录中的初始化段文件（通常以h开头，.m4s结尾）
        if let Ok(entries) = tokio::fs::read_dir(&self.work_dir).await {
            let mut entries = entries;
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if let Some(filename) = path.file_name() {
                    if let Some(name_str) = filename.to_str() {
                        if name_str.starts_with('h') && name_str.ends_with(".m4s") {
                            info!("发现初始化段: {:?}", path);
                            return Some(path);
                        }
                    }
                }
            }
        }
        None
    }

    /// 获取segment的时长（使用ffprobe）
    async fn get_segment_duration_from_file(&self, segment_path: &Path) -> Option<f64> {
        let output = tokio::process::Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-show_entries", "format=duration",
                "-of", "csv=p=0",
                &segment_path.to_string_lossy()
            ])
            .output()
            .await
            .ok()?;

        if output.status.success() {
            let duration_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            duration_str.parse::<f64>().ok()
        } else {
            None
        }
    }
    
    /// 执行FFmpeg合并命令
    async fn run_ffmpeg_merge(&self, concat_list: &Path, output_path: &Path) -> Result<bool> {
        use tokio::process::Command;
        
        debug!("执行FFmpeg合并命令...");
        
        // 检查第一个分片文件的格式来决定合并策略
        let is_m4s_format = self.detect_segment_format().await;
        
        let mut cmd = Command::new("ffmpeg");
        
        if is_m4s_format {
            info!("检测到M4S格式分片，使用MPEG-DASH合并策略");
            // 对于M4S文件，不能直接使用concat协议，需要重新封装
            cmd.args(&[
                "-f", "concat",
                "-safe", "0", 
                "-i", &concat_list.to_string_lossy(),
                "-c", "copy",
                "-f", "mp4",          // 强制输出为MP4格式
                "-movflags", "+faststart", // 优化MP4文件结构
                "-y",
                &output_path.to_string_lossy(),
            ]);
        } else {
            info!("使用标准TS合并策略");
            // 标准TS文件合并
            cmd.args(&[
                "-f", "concat",
                "-safe", "0",
                "-i", &concat_list.to_string_lossy(),
                "-c", "copy",
                "-y",
                &output_path.to_string_lossy(),
            ]);
        }
        
        info!("FFmpeg命令: {:?}", cmd);
        
        // 执行命令
        let output = cmd.output().await
            .map_err(|e| anyhow!("启动FFmpeg失败: {}", e))?;
        
        if output.status.success() {
            info!("FFmpeg合并成功完成");
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            
            error!("FFmpeg合并失败:");
            error!("stdout: {}", stdout);
            error!("stderr: {}", stderr);
            
            // 如果标准方法失败，尝试其他合并策略
            if is_m4s_format {
                warn!("M4S合并失败，尝试替代方法");
                self.try_alternative_m4s_merge(output_path).await
            } else {
                Ok(false)
            }
        }
    }
    
    /// 检测分片文件格式
    async fn detect_segment_format(&self) -> bool {
        // 检查第一个下载成功的分片文件
        for segment in &self.segments {
            if segment.downloaded && segment.file_path.exists() {
                // 检查URL或文件内容来判断格式
                if segment.url.ends_with(".m4s") {
                    return true; // M4S格式
                }
                
                // 也可以通过检查文件头来判断
                if let Ok(content) = tokio::fs::read(&segment.file_path).await {
                    if content.len() >= 8 {
                        // M4S文件通常以ftyp box开头
                        let header = &content[4..8];
                        if header == b"ftyp" {
                            return true;
                        }
                    }
                }
                
                break; // 只检查第一个文件
            }
        }
        false // 默认为TS格式
    }
    
    /// 尝试M4S文件的替代合并方法
    async fn try_alternative_m4s_merge(&self, output_path: &Path) -> Result<bool> {
        use tokio::process::Command;
        
        warn!("尝试M4S文件的替代合并方法");
        
        // 方法1: 使用输入列表而不是concat协议
        let input_list_path = self.work_dir.join("input_list.txt");
        let mut input_content = String::new();
        
        for segment in &self.segments {
            if segment.downloaded && segment.file_path.exists() {
                input_content.push_str(&format!("file '{}'\n", 
                    segment.file_path.to_string_lossy().replace('\\', "/")));
            }
        }
        
        tokio::fs::write(&input_list_path, input_content).await
            .map_err(|e| anyhow!("创建输入列表失败: {}", e))?;
        
        // 使用不同的FFmpeg参数
        let mut cmd = Command::new("ffmpeg");
        cmd.args(&[
            "-f", "concat",
            "-safe", "0",
            "-i", &input_list_path.to_string_lossy(),
            "-c:v", "copy",   // 明确指定视频编解码器
            "-c:a", "copy",   // 明确指定音频编解码器  
            "-bsf:a", "aac_adtstoasc", // AAC比特流过滤器
            "-movflags", "+faststart",
            "-f", "mp4",
            "-y",
            &output_path.to_string_lossy(),
        ]);
        
        info!("尝试替代FFmpeg命令: {:?}", cmd);
        
        let output = cmd.output().await
            .map_err(|e| anyhow!("启动替代FFmpeg失败: {}", e))?;
        
        // 清理临时文件
        let _ = tokio::fs::remove_file(&input_list_path).await;
        
        if output.status.success() {
            info!("✅ 替代M4S合并方法成功");
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("❌ 替代M4S合并方法也失败: {}", stderr);
            Ok(false)
        }
    }
    
    /// 合并分片并清理源文件（录制结束时调用）
    pub async fn finalize_recording(&mut self, output_path: &Path, cleanup_segments: bool) -> Result<PathBuf> {
        info!("完成录制，开始最终化处理...");
        
        // 1. 保存最终的M3U8播放列表（VOD模式）
        self.save_m3u8_playlist(false).await?;
        
        // 2. 合并分片为MP4
        let merged_file = self.merge_segments_to_mp4(output_path).await?;
        
        // 3. 可选：清理分片文件
        if cleanup_segments {
            info!("清理分片源文件...");
            let mut cleaned_count = 0;
            
            for segment in &self.segments {
                if segment.downloaded && segment.file_path.exists() {
                    match tokio::fs::remove_file(&segment.file_path).await {
                        Ok(_) => {
                            cleaned_count += 1;
                            debug!("删除分片文件: {:?}", segment.file_path);
                        }
                        Err(e) => {
                            warn!("删除分片文件失败: {:?}, 错误: {}", segment.file_path, e);
                        }
                    }
                }
            }
            
            // 清理其他临时文件
            let files_to_clean = [
                self.work_dir.join("playlist.m3u8"),
                self.work_dir.join("segments.log"),
            ];
            
            for file in &files_to_clean {
                if file.exists() {
                    let _ = tokio::fs::remove_file(file).await;
                }
            }
            
            info!("已清理 {} 个分片源文件", cleaned_count);
        }
        
        info!("录制最终化处理完成，输出文件: {:?}", merged_file);
        Ok(merged_file)
    }

    /// 设置自动合并配置
    pub fn set_auto_merge_config(&mut self, config: AutoMergeConfig) {
        info!("已设置自动合并配置: 启用={}, 阈值={}秒", 
              config.enabled, config.duration_threshold);
        self.auto_merge_config = Some(config);
    }

    /// 获取自动合并配置
    pub fn get_auto_merge_config(&self) -> Option<&AutoMergeConfig> {
        self.auto_merge_config.as_ref()
    }

    /// 检查是否应该触发自动合并
    pub fn should_auto_merge(&self) -> bool {
        if let Some(config) = &self.auto_merge_config {
            debug!("auto_merge配置检查 - enabled: {}, 时长: {:.2}秒, 阈值: {}秒", 
                   config.enabled, self.stats.total_duration, config.duration_threshold);
            
            if config.enabled && config.should_auto_merge(self.stats.total_duration) {
                // 检查距离上次自动合并是否超过阈值
                if let Some(last_merge_time) = self.last_auto_merge_timestamp {
                    // 获取当前最新分片的时间戳
                    if let Some(latest_timestamp) = self.stats.end_timestamp {
                        let time_since_last_merge = (latest_timestamp - last_merge_time) as f64 / 1000.0;
                        debug!("上次合并后时间: {:.2}秒", time_since_last_merge);
                        return time_since_last_merge >= config.duration_threshold as f64;
                    }
                } else {
                    // 第一次检查，直接根据总时长判断
                    debug!("首次检查，时长达到阈值: {}", true);
                    return true;
                }
            }
        } else {
            debug!("未找到auto_merge配置");
        }
        false
    }

    /// 执行自动合并
    pub async fn perform_auto_merge(&mut self) -> Result<Option<PathBuf>> {
        debug!("perform_auto_merge调用 - 当前时长: {:.2}秒, should_auto_merge: {}", 
               self.stats.total_duration, self.should_auto_merge());
        
        if !self.should_auto_merge() {
            return Ok(None);
        }

        let Some(config) = self.auto_merge_config.clone() else {
            return Ok(None);
        };

        info!("开始执行自动合并，当前时长: {:.2}秒", self.stats.total_duration);

        // 生成带时间戳的输出文件名
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let output_filename = format!("auto_merged_{}.{}", timestamp, config.output_format);
        let output_path = self.work_dir.join(&output_filename);

        // 执行合并
        match self.auto_merge_segments_to_mp4(&output_path, &config).await {
            Ok(merged_file) => {
                // 更新最后一次自动合并的时间戳
                self.last_auto_merge_timestamp = self.stats.end_timestamp;
                
                info!("自动合并成功: {:?}", merged_file);

                // 根据配置决定是否清理分片文件
                if !config.keep_segments_after_merge {
                    info!("正在清理已合并的分片文件...");
                    let segments_to_clean = self.segments.clone();
                    for segment in &segments_to_clean {
                        if segment.downloaded && segment.file_path.exists() {
                            if let Err(e) = tokio::fs::remove_file(&segment.file_path).await {
                                warn!("删除分片文件失败: {:?}, 错误: {}", segment.file_path, e);
                            }
                        }
                    }
                    
                    // 清空内存中的分片列表
                    self.segments.clear();
                    self.update_stats();
                    
                    info!("已清理 {} 个分片文件", segments_to_clean.len());
                }

                Ok(Some(merged_file))
            }
            Err(e) => {
                warn!("自动合并失败: {}", e);
                Err(e)
            }
        }
    }

    /// 自动合并分片为MP4（内部方法）
    async fn auto_merge_segments_to_mp4(
        &self,
        output_path: &Path,
        config: &AutoMergeConfig,
    ) -> Result<PathBuf> {
        // 筛选已下载的分片
        let downloaded_segments: Vec<_> = self.segments
            .iter()
            .filter(|s| s.downloaded)
            .collect();

        if downloaded_segments.is_empty() {
            return Err(anyhow!("没有可合并的分片"));
        }

        info!("开始自动合并 {} 个分片", downloaded_segments.len());

        // 1. 生成M3U8索引文件
        let m3u8_path = self.work_dir.join("auto_merge_index.m3u8");
        self.generate_auto_merge_m3u8(&downloaded_segments, &m3u8_path).await?;

        // 2. 使用FFmpeg进行转换
        self.auto_merge_clip_from_m3u8(&m3u8_path, output_path, config).await?;

        // 3. 清理临时M3U8文件
        if m3u8_path.exists() {
            let _ = tokio::fs::remove_file(&m3u8_path).await;
        }

        Ok(output_path.to_path_buf())
    }

    /// 生成自动合并用的M3U8文件
    async fn generate_auto_merge_m3u8(
        &self,
        segments: &[&SegmentRecord],
        m3u8_path: &Path,
    ) -> Result<()> {
        let mut m3u8_content = String::new();
        m3u8_content.push_str("#EXTM3U\n");
        m3u8_content.push_str("#EXT-X-VERSION:3\n");
        m3u8_content.push_str("#EXT-X-TARGETDURATION:10\n");
        m3u8_content.push_str("#EXT-X-MEDIA-SEQUENCE:0\n");

        // 动态查找初始化段
        if let Some(init_segment_path) = self.find_initialization_segment().await {
            m3u8_content.push_str(&format!(
                "#EXT-X-MAP:URI=\"{}\"\n", 
                init_segment_path.file_name().unwrap().to_string_lossy()
            ));
            debug!("添加初始化段到M3U8: {:?}", init_segment_path.file_name().unwrap());
        } else {
            warn!("未找到初始化段文件");
        }

        // 添加所有分片
        for segment in segments {
            m3u8_content.push_str(&format!("#EXTINF:{:.3},\n", segment.duration));
            if let Some(filename) = segment.file_path.file_name() {
                m3u8_content.push_str(&format!("{}\n", filename.to_string_lossy()));
            }
        }

        m3u8_content.push_str("#EXT-X-ENDLIST\n");

        tokio::fs::write(m3u8_path, m3u8_content).await
            .map_err(|e| anyhow!("写入M3U8文件失败: {}", e))?;

        debug!("自动合并M3U8文件已生成: {:?}", m3u8_path);
        Ok(())
    }

    /// 从M3U8文件自动合并为MP4
    async fn auto_merge_clip_from_m3u8(
        &self,
        m3u8_path: &Path,
        output_path: &Path,
        config: &AutoMergeConfig,
    ) -> Result<()> {
        use std::process::Stdio;
        use tokio::process::Command;

        // 删除已存在的输出文件
        if output_path.exists() {
            tokio::fs::remove_file(output_path).await?;
        }

        let mut args = vec!["-i".to_string(), m3u8_path.to_string_lossy().to_string()];
        args.extend(config.output_quality.get_ffmpeg_args());
        args.push(output_path.to_string_lossy().to_string());

        info!("执行FFmpeg自动合并: ffmpeg {}", args.join(" "));

        let cmd = Command::new("ffmpeg")
            .args(&args)
            .current_dir(&self.work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("启动FFmpeg失败: {}", e))?;

        let output = cmd.wait_with_output().await
            .map_err(|e| anyhow!("等待FFmpeg完成失败: {}", e))?;

        if output.status.success() {
            info!("FFmpeg自动合并成功");
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("FFmpeg自动合并失败，尝试重编码: {}", stderr);

            // 尝试使用重编码参数
            let mut fallback_args = vec!["-i".to_string(), m3u8_path.to_string_lossy().to_string()];
            fallback_args.extend(config.output_quality.get_fallback_ffmpeg_args());
            fallback_args.push(output_path.to_string_lossy().to_string());

            info!("执行FFmpeg重编码: ffmpeg {}", fallback_args.join(" "));

            let fallback_cmd = Command::new("ffmpeg")
                .args(&fallback_args)
                .current_dir(&self.work_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| anyhow!("启动FFmpeg重编码失败: {}", e))?;

            let fallback_output = fallback_cmd.wait_with_output().await
                .map_err(|e| anyhow!("等待FFmpeg重编码完成失败: {}", e))?;

            if fallback_output.status.success() {
                info!("FFmpeg重编码合并成功");
                Ok(())
            } else {
                let fallback_stderr = String::from_utf8_lossy(&fallback_output.stderr);
                Err(anyhow!("FFmpeg合并失败: {}", fallback_stderr))
            }
        }
    }

    /// 获取统计信息
    pub fn get_stats(&self) -> &SegmentStats {
        &self.stats
    }

    /// 更新统计信息
    fn update_stats(&mut self) {
        self.stats.total_segments = self.segments.len();
        self.stats.downloaded_segments = self.segments.iter().filter(|s| s.downloaded).count();
        
        self.stats.total_duration = self.segments
            .iter()
            .filter(|s| s.downloaded)
            .map(|s| s.duration)
            .sum();
        
        self.stats.total_size = self.segments
            .iter()
            .filter(|s| s.downloaded)
            .map(|s| s.size)
            .sum();
        
        self.stats.start_timestamp = self.segments.first().map(|s| s.timestamp);
        self.stats.end_timestamp = self.segments.last().map(|s| s.timestamp);
    }
}