use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::time::{Duration, Instant};
use crate::{live_debug, live_error, live_info, live_warn};

use crate::bilibili::BiliClient;
use super::api::Quality;
use super::segment_downloader::SegmentDownloader;
use super::segment_manager::SegmentManager;
use super::ffmpeg_recorder::FFmpegRecorder;

/// 录制模式
pub enum RecorderMode {
    /// FFmpeg模式
    FFmpeg(FFmpegRecorder),
    /// 分片下载模式（使用正确的HLS API）
    Segment(SegmentRecorder),
}

// 手动实现Debug以避免传播错误
impl std::fmt::Debug for RecorderMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecorderMode::FFmpeg(_) => write!(f, "RecorderMode::FFmpeg(..)"),
            RecorderMode::Segment(_) => write!(f, "RecorderMode::Segment(..)"),
        }
    }
}

impl std::fmt::Debug for SegmentRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentRecorder")
            .field("room_id", &self.room_id)
            .field("quality", &self.quality)
            .finish()
    }
}

/// 录制状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatus {
    /// 空闲
    Idle,
    /// 录制中
    Recording,
    /// 已停止
    Stopped,
}

/// 录制统计信息
#[derive(Debug, Default, Clone)]
pub struct RecordStats {
    /// 录制开始时间
    pub start_time: Option<Instant>,
    /// 录制持续时间
    pub duration: Duration,
    /// 文件大小（字节）
    pub file_size: u64,
    /// 是否正在录制
    pub is_recording: bool,
}


/// 直播录制器（支持双模式）
#[derive(Debug)]
pub struct LiveRecorder {
    /// 录制模式
    mode: RecorderMode,
    /// 录制状态
    status: RecordStatus,
    /// 录制统计
    stats: RecordStats,
}

// FFmpeg录制器已移至独立文件 ffmpeg_recorder.rs

/// 分片录制器（新增）
pub struct SegmentRecorder {
    /// 录制质量
    quality: Quality,
    /// 房间ID
    room_id: i64,
    /// 工作目录
    work_dir: PathBuf,
    /// 原始输出路径（包含完整文件名）
    output_path: PathBuf,
    /// B站客户端
    bili_client: Arc<BiliClient>,
    /// 下载循环任务句柄
    download_handle: Option<tokio::task::JoinHandle<()>>,
    /// 自动合并配置
    auto_merge_config: Option<super::config::AutoMergeConfig>,
}

impl SegmentRecorder {
    /// 创建分片录制器
    pub async fn new<P: AsRef<Path>>(
        output_path: P,
        room_id: i64,
        quality: Quality,
        bili_client: Arc<BiliClient>,
        auto_merge_config: Option<super::config::AutoMergeConfig>,
    ) -> Result<Self> {
        let output_path = output_path.as_ref().to_path_buf();
        
        // 从输出路径中提取工作目录（父目录）
        let work_dir = output_path.parent()
            .ok_or_else(|| anyhow::anyhow!("无法从输出路径获取工作目录"))?
            .to_path_buf();
        
        // 确保工作目录存在
        tokio::fs::create_dir_all(&work_dir).await
            .map_err(|e| anyhow::anyhow!("创建工作目录失败: {}", e))?;
        
        Ok(Self {
            quality,
            room_id,
            work_dir,
            output_path,
            bili_client,
            download_handle: None,
            auto_merge_config,
        })
    }
    
    /// 检查录制器是否正在运行
    pub fn is_running(&self) -> bool {
        self.download_handle.is_some() && 
        !self.download_handle.as_ref().unwrap().is_finished()
    }
    
    
    /// 开始分片下载
    pub async fn start(&mut self) -> Result<()> {
        live_info!("开始分片模式录制，房间ID: {}", self.room_id);
        
        if self.download_handle.is_some() {
            return Err(anyhow!("分片录制器已在运行中"));
        }
        
        // 克隆必要的数据用于异步任务
        let room_id = self.room_id;
        let quality = self.quality;
        let work_dir = self.work_dir.clone();
        let bili_client = self.bili_client.clone();
        let auto_merge_config = self.auto_merge_config.clone();
        
        // 启动分片录制主循环（复刻bililive-go的实现）
        let handle = tokio::spawn(async move {
            live_info!("分片录制主循环已启动，房间: {}", room_id);
            
            // 初始化下载器和管理器
            let mut downloader = match SegmentDownloader::new(
                bili_client,
                work_dir.clone(),
                room_id,
                quality,
            ).await {
                Ok(d) => d,
                Err(e) => {
                    live_error!("初始化分片下载器失败: {}", e);
                    return;
                }
            };
            
            let manager = match SegmentManager::new(&work_dir).await {
                Ok(mut m) => {
                    // 设置自动合并配置
                    use super::config::AutoMergeConfig;
                    let auto_config = if let Some(config) = &auto_merge_config {
                        live_info!("已设置自动合并配置: 启用={}, 阈值={}秒", config.enabled, config.duration_threshold);
                        config.clone()
                    } else {
                        live_warn!("未提供自动合并配置，使用默认配置");
                        AutoMergeConfig::default()
                    };
                    m.set_auto_merge_config(auto_config);
                    std::sync::Arc::new(tokio::sync::Mutex::new(m))
                },
                Err(e) => {
                    live_error!("初始化分片管理器失败: {}", e);
                    return;
                }
            };
            
            // 启动下载器，传入回调函数处理下载完成的分片
            let manager_for_callback = manager.clone();
            let segment_callback = move |segment_info: super::m3u8_parser::SegmentInfo, file_size: u64, file_path: std::path::PathBuf| {
                let manager_clone = manager_for_callback.clone();
                tokio::spawn(async move {
                    let mut manager_guard = manager_clone.lock().await;
                    if let Err(e) = manager_guard.add_segment(&segment_info, file_size, file_path).await {
                        live_error!("添加分片到管理器失败: {}", e);
                    } else {
                        live_debug!("分片已添加到管理器 - 序列号: {}, 时长: {:.2}秒", segment_info.sequence, segment_info.duration);
                        // 每添加一个分片后检查自动合并
                        if let Ok(Some(merged_file)) = manager_guard.perform_auto_merge().await {
                            live_info!("自动合并完成: {:?}", merged_file);
                        }
                    }
                });
            };
            
            live_debug!("🚀 SegmentRecorder 准备调用 downloader.start(segment_callback)");
            if let Err(e) = downloader.start(segment_callback).await {
                live_error!("启动分片下载器失败: {}", e);
                return;
            }
            live_debug!("✅ downloader.start(segment_callback) 调用成功");
            
            // 下载器现在独立运行，我们只需要等待并定期输出统计信息
            let mut stats_interval = tokio::time::interval(Duration::from_secs(60)); // 每60秒输出统计信息
            
            live_info!("分片录制主循环正在运行，下载器已启动");
            
            loop {
                tokio::select! {
                    // 定期输出统计信息和维护任务
                    _ = stats_interval.tick() => {
                        let downloader_stats = downloader.stats();
                        let manager_guard = manager.lock().await;
                        let manager_stats = manager_guard.stats();
                        
                        live_info!(
                            "录制统计 - 下载器: [总分片: {}, 成功: {}, 失败: {}, 总大小: {} MB, 成功率: {:.1}%]",
                            downloader_stats.total_segments,
                            downloader_stats.successful_downloads,
                            downloader_stats.failed_downloads,
                            downloader_stats.total_bytes / 1024 / 1024,
                            downloader_stats.success_rate() * 100.0
                        );
                        
                        live_info!(
                            "录制统计 - 管理器: [总分片: {}, 已下载: {}, 总时长: {:.1}s, 总大小: {} MB]",
                            manager_stats.total_segments,
                            manager_stats.downloaded_segments,
                            manager_stats.total_duration,
                            manager_stats.total_size / 1024 / 1024
                        );
                        
                        // 生成并保存M3U8播放列表
                        if let Err(e) = manager_guard.save_m3u8_playlist(true).await {
                            live_warn!("保存M3U8播放列表失败: {}", e);
                        } else {
                            live_debug!("M3U8播放列表已更新");
                        }
                        
                        // 释放 manager_guard 以避免长时间锁定
                        drop(manager_guard);
                        
                        // 智能清理磁盘空间（复刻bililive-go的段文件管理）
                        let mut manager_guard = manager.lock().await;
                        match manager_guard.smart_cleanup().await {
                            Ok(cleaned) => {
                                if cleaned > 0 {
                                    live_info!("智能清理完成：清理了 {} 个旧分片文件，当前保留 {} 个分片", 
                                          cleaned, manager_guard.segment_count());
                                }
                            }
                            Err(e) => {
                                live_warn!("智能清理失败: {}", e);
                            }
                        }
                    }
                }
            }
        });
        
        self.download_handle = Some(handle);
        
        live_info!("分片录制已启动，后台下载循环正在运行");
        Ok(())
    }
    
    /// 停止分片下载
    pub async fn stop(&mut self) -> Result<()> {
        live_info!("停止分片模式录制");
        
        if let Some(handle) = self.download_handle.take() {
            handle.abort();
            live_debug!("已终止下载循环任务");
        }
        
        Ok(())
    }
    
    /// 获取输出文件路径（录制停止后返回合并的MP4文件路径）
    pub async fn output_path(&self) -> Result<Option<PathBuf>> {
        // 生成预期的MP4文件路径（基于原始输出路径）
        let mp4_path = if self.output_path.extension().and_then(|s| s.to_str()) == Some("m4s") {
            self.output_path.with_extension("mp4")
        } else {
            self.output_path.clone()
        };
        
        // 如果MP4文件已存在（已合并），返回MP4路径
        if mp4_path.exists() {
            Ok(Some(mp4_path))
        } else {
            // 否则返回M3U8播放列表文件
            let playlist_path = self.work_dir.join("playlist.m3u8");
            
            if playlist_path.exists() {
                Ok(Some(playlist_path))
            } else {
                // 最后返回原始输出路径
                live_warn!("未找到输出文件，返回原始输出路径");
                Ok(Some(self.output_path.clone()))
            }
        }
    }
}

impl LiveRecorder {
    /// 创建FFmpeg模式录制器
    pub fn new_ffmpeg<P: AsRef<Path>>(output_path: P, max_file_size: i64) -> Self {
        let ffmpeg_recorder = FFmpegRecorder::new(output_path, max_file_size);
        
        Self {
            mode: RecorderMode::FFmpeg(ffmpeg_recorder),
            status: RecordStatus::Idle,
            stats: RecordStats::default(),
        }
    }
    
    /// 创建分片模式录制器
    pub async fn new_segment<P: AsRef<Path>>(
        output_path: P,
        room_id: i64,
        quality: Quality,
        bili_client: Arc<BiliClient>,
        auto_merge_config: Option<super::config::AutoMergeConfig>,
    ) -> Result<Self> {
        let segment_recorder = SegmentRecorder::new(
            output_path,
            room_id,
            quality,
            bili_client,
            auto_merge_config,
        ).await?;
        
        Ok(Self {
            mode: RecorderMode::Segment(segment_recorder),
            status: RecordStatus::Idle,
            stats: RecordStats::default(),
        })
    }
    
    /// 根据配置创建录制器
    #[allow(dead_code)]
    pub async fn new_with_mode<P: AsRef<Path>>(
        output_path: P,
        max_file_size: i64,
        use_segment_mode: bool,
        room_id: i64,
        quality: Quality,
        bili_client: Arc<BiliClient>,
    ) -> Result<Self> {
        if use_segment_mode {
            live_info!("创建分片模式录制器");
            Self::new_segment(output_path, room_id, quality, bili_client, None).await
        } else {
            live_info!("创建FFmpeg模式录制器");
            Ok(Self::new_ffmpeg(output_path, max_file_size))
        }
    }
    

    /// 开始录制
    pub async fn start(&mut self, stream_url: String) -> Result<()> {
        if self.status == RecordStatus::Recording {
            return Err(anyhow!("录制器已在录制中"));
        }

        match &mut self.mode {
            RecorderMode::FFmpeg(recorder) => {
                recorder.start_with_cdn(&stream_url, "unknown").await?;
            }
            RecorderMode::Segment(recorder) => {
                recorder.start().await?;
            }
        }

        self.status = RecordStatus::Recording;
        self.stats.start_time = Some(Instant::now());
        self.stats.is_recording = true;

        live_info!("录制已启动");
        Ok(())
    }
    
    /// 开始录制（指定CDN节点）
    /// 
    /// # Arguments
    /// * `stream_url` - 直播流地址
    /// * `cdn_node` - CDN节点标识
    #[allow(dead_code)]
    pub async fn start_with_cdn(&mut self, stream_url: &str, cdn_node: &str) -> Result<()> {
        if self.status == RecordStatus::Recording {
            return Err(anyhow!("录制器已在录制中"));
        }

        match &mut self.mode {
            RecorderMode::FFmpeg(recorder) => {
                recorder.start_with_cdn(stream_url, cdn_node).await?;
            }
            RecorderMode::Segment(recorder) => {
                // 分片模式不需要stream_url，直接启动
                recorder.start().await?;
            }
        }

        self.status = RecordStatus::Recording;
        self.stats.start_time = Some(Instant::now());
        self.stats.is_recording = true;

        live_info!("录制已启动，CDN: {}", cdn_node);
        Ok(())
    }
    

    /// 停止录制
    pub async fn stop(&mut self) -> Result<()> {
        if self.status != RecordStatus::Recording {
            return Ok(());
        }

        live_info!("停止录制");

        match &mut self.mode {
            RecorderMode::FFmpeg(recorder) => {
                recorder.stop().await?;
            }
            RecorderMode::Segment(recorder) => {
                recorder.stop().await?;
            }
        }

        self.status = RecordStatus::Stopped;
        self.stats.is_recording = false;

        // 更新统计信息
        if let Some(start_time) = self.stats.start_time {
            self.stats.duration = start_time.elapsed();
        }

        // 获取文件大小
        match &self.mode {
            RecorderMode::FFmpeg(recorder) => {
                if let Some(path) = recorder.output_path() {
                    if let Ok(metadata) = tokio::fs::metadata(path).await {
                        self.stats.file_size = metadata.len();
                    }
                }
                live_info!("FFmpeg录制已停止，文件大小: {} 字节", self.stats.file_size);
            }
            RecorderMode::Segment(recorder) => {
                // 分片模式需要合并分片为最终的MP4文件
                if let Ok(mut segment_manager) = super::segment_manager::SegmentManager::new(&recorder.work_dir).await {
                    // 先获取统计信息并克隆需要的数据
                    let (total_segments, downloaded_segments, total_size, total_duration) = {
                        let segment_stats = segment_manager.stats();
                        (
                            segment_stats.total_segments,
                            segment_stats.downloaded_segments,
                            segment_stats.total_size,
                            segment_stats.total_duration,
                        )
                    };
                    
                    live_info!("分片录制已停止 - 总分片: {}, 成功下载: {}, 总大小: {} MB, 总时长: {:.1} 秒", 
                          total_segments,
                          downloaded_segments,
                          total_size / 1024 / 1024,
                          total_duration);
                    
                    // 生成最终的MP4文件路径（使用保存的原始输出路径）
                    let mp4_path = {
                        let output_path = &recorder.output_path;
                        if output_path.extension().and_then(|s| s.to_str()) == Some("m4s") {
                            // 将 .m4s 扩展名改为 .mp4
                            output_path.with_extension("mp4")
                        } else {
                            // 如果不是 .m4s，直接使用原路径
                            output_path.clone()
                        }
                    };
                    
                    // 合并分片为MP4
                    match segment_manager.finalize_recording(&mp4_path, true).await {
                        Ok(final_path) => {
                            live_info!("✅ 分片合并成功，最终文件: {:?}", final_path);
                            
                            // 更新文件大小统计
                            if let Ok(metadata) = tokio::fs::metadata(&final_path).await {
                                self.stats.file_size = metadata.len();
                            }
                        }
                        Err(e) => {
                            live_error!("❌ 分片合并失败: {}", e);
                            live_warn!("保留分片文件和M3U8播放列表，可手动合并");
                            
                            // 合并失败时至少保存播放列表
                            if let Err(playlist_err) = segment_manager.save_m3u8_playlist(false).await {
                                live_error!("保存最终播放列表也失败: {}", playlist_err);
                            }
                            
                            // 使用分片总大小作为统计
                            self.stats.file_size = total_size;
                        }
                    }
                } else {
                    live_info!("分片录制已停止");
                }
            }
        }
        Ok(())
    }

    /// 获取输出文件路径
    pub async fn output_path(&self) -> Option<PathBuf> {
        match &self.mode {
            RecorderMode::FFmpeg(recorder) => {
                recorder.output_path().map(|p| p.to_path_buf())
            }
            RecorderMode::Segment(recorder) => {
                recorder.output_path().await.unwrap_or_default()
            }
        }
    }

    /// 检查录制器进程状态
    pub fn check_process_status(&mut self) -> Result<bool> {
        match &mut self.mode {
            RecorderMode::FFmpeg(recorder) => {
                let result = recorder.check_process_status()?;
                // 同步状态
                if !result {
                    self.status = RecordStatus::Stopped;
                    self.stats.is_recording = false;
                }
                Ok(result)
            }
            RecorderMode::Segment(recorder) => {
                // 检查分片录制器的实际运行状态
                let is_running = recorder.is_running();
                
                // 同步状态
                if !is_running && self.status == RecordStatus::Recording {
                    live_warn!("分片录制器已停止运行");
                    self.status = RecordStatus::Stopped;
                    self.stats.is_recording = false;
                    
                    // 更新统计信息
                    if let Some(start_time) = self.stats.start_time {
                        self.stats.duration = start_time.elapsed();
                    }
                }
                
                Ok(is_running)
            }
        }
    }





    
    /// 获取统计信息
    #[allow(dead_code)]
    pub fn stats(&self) -> &RecordStats {
        &self.stats
    }
    
    /// 获取详细的录制统计信息（支持分片模式）
    pub async fn get_detailed_stats(&self) -> RecordStats {
        let mut stats = self.stats.clone();
        
        match &self.mode {
            RecorderMode::FFmpeg(_) => {
                // FFmpeg模式使用现有统计
                stats
            }
            RecorderMode::Segment(recorder) => {
                // 分片模式需要从分片管理器获取更详细的信息
                
                // 尝试从工作目录获取分片管理器的统计信息
                if let Ok(segment_manager) = super::segment_manager::SegmentManager::new(&recorder.work_dir).await {
                    let segment_stats = segment_manager.stats();
                    
                    // 更新文件大小为分片总大小
                    stats.file_size = segment_stats.total_size;
                    
                    // 如果有开始时间，计算实际录制时长
                    if let Some(start_time) = stats.start_time {
                        if stats.is_recording {
                            stats.duration = start_time.elapsed();
                        }
                    }
                    
                    live_debug!("分片录制统计 - 分片数: {}, 下载: {}, 总大小: {} MB", 
                          segment_stats.total_segments,
                          segment_stats.downloaded_segments,
                          segment_stats.total_size / 1024 / 1024);
                }
                
                stats
            }
        }
    }
}

// Drop实现已移至各自的录制器中

