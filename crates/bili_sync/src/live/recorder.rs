use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use super::LiveError;

/// 录制状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatus {
    /// 空闲
    Idle,
    /// 录制中
    Recording,
    /// 已停止
    Stopped,
    /// 出错
    Error,
}

/// 录制统计信息
#[derive(Debug, Default)]
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

/// 分段录制信息
#[derive(Debug, Clone)]
pub struct RecordSegment {
    /// 分段文件路径
    pub path: PathBuf,
    /// 开始时间
    #[allow(dead_code)] // 开始时间字段，用于分段统计和调试
    pub start_time: Instant,
    /// 结束时间（录制中为None）
    pub end_time: Option<Instant>,
    /// 文件大小（字节）
    pub file_size: Option<u64>,
    /// 分段序号
    pub sequence: u32,
    /// 使用的CDN节点
    pub cdn_node: String,
}

/// 直播录制器（支持分段和无缝切换）
#[derive(Debug)]
pub struct LiveRecorder {
    /// 基础输出路径（不含扩展名）
    base_output_path: PathBuf,
    /// 最终合并文件路径
    final_output_path: PathBuf,
    /// 主FFmpeg进程
    primary_process: Option<Child>,
    /// 备用FFmpeg进程（用于无缝切换）
    secondary_process: Option<Child>,
    /// 录制状态
    status: RecordStatus,
    /// 录制统计
    stats: RecordStats,
    /// 当前使用的流URL
    current_stream_url: Option<String>,
    /// 录制分段列表
    segments: Vec<RecordSegment>,
    /// 当前分段序号
    current_segment: u32,
    /// 是否启用分段模式
    segment_mode: bool,
}

impl LiveRecorder {
    #[allow(dead_code)] // 录制器方法，部分暂时未使用但需要保留
    /// 创建新的录制器
    /// 
    /// # Arguments
    /// * `output_path` - 输出文件路径
    pub fn new<P: AsRef<Path>>(output_path: P) -> Self {
        let final_output_path = output_path.as_ref().to_path_buf();
        
        // 创建基础路径（去掉扩展名）
        let mut base_output_path = final_output_path.clone();
        base_output_path.set_extension("");
        
        Self {
            base_output_path,
            final_output_path,
            primary_process: None,
            secondary_process: None,
            status: RecordStatus::Idle,
            stats: RecordStats::default(),
            current_stream_url: None,
            segments: Vec::new(),
            current_segment: 0,
            segment_mode: true, // 默认启用分段模式
        }
    }
    

    /// 开始录制
    /// 
    /// # Arguments
    /// * `stream_url` - 直播流地址
    pub async fn start(&mut self, stream_url: String) -> Result<()> {
        self.start_with_cdn(&stream_url, "unknown").await
    }
    
    /// 开始录制（指定CDN节点）
    /// 
    /// # Arguments
    /// * `stream_url` - 直播流地址
    /// * `cdn_node` - CDN节点标识
    pub async fn start_with_cdn(&mut self, stream_url: &str, cdn_node: &str) -> Result<()> {
        if self.status == RecordStatus::Recording {
            return Err(anyhow!("录制器已在录制中"));
        }

        info!("开始录制流: {}, CDN: {}", stream_url, cdn_node);
        
        // 确保输出目录存在
        if let Some(parent) = self.final_output_path.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| LiveError::FileError(e))?;
        }

        // 检查ffmpeg是否可用
        if !self.is_ffmpeg_available().await {
            return Err(anyhow!("FFmpeg不可用，请确保已安装FFmpeg"));
        }

        // 启动第一个分段
        self.start_new_segment(stream_url, cdn_node).await?;

        self.status = RecordStatus::Recording;
        self.current_stream_url = Some(stream_url.to_string());
        self.stats.start_time = Some(Instant::now());
        self.stats.is_recording = true;

        info!("录制已启动，分段模式: {}", self.segment_mode);
        Ok(())
    }
    
    /// 启动新的录制分段
    async fn start_new_segment(&mut self, stream_url: &str, cdn_node: &str) -> Result<()> {
        self.current_segment += 1;
        
        let segment_path = if self.segment_mode {
            // 分段模式：创建临时分段文件
            let extension = self.final_output_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("flv");
            self.base_output_path
                .with_file_name(format!("{}_segment_{:03}.{}", 
                    self.base_output_path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy(), 
                    self.current_segment, 
                    extension))
        } else {
            // 非分段模式：直接使用最终路径
            self.final_output_path.clone()
        };

        debug!("启动新分段: {:?}, CDN: {}", segment_path, cdn_node);

        // 构建FFmpeg命令参数
        let args = self.build_ffmpeg_args(stream_url, &segment_path)?;
        
        debug!("FFmpeg命令参数: {:?}", args);

        // 启动FFmpeg进程
        let mut process = Command::new("ffmpeg")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| LiveError::RecorderStartError(format!("启动FFmpeg失败: {}", e)))?;

        // 异步读取stderr输出
        if let Some(stderr) = process.stderr.take() {
            tokio::spawn(async move {
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(stderr);
                
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            let line = line.trim();
                            if !line.is_empty() {
                                if line.contains("error") || line.contains("Error") || line.contains("ERROR") {
                                    error!("FFmpeg错误: {}", line);
                                } else if line.contains("warning") || line.contains("Warning") || line.contains("WARNING") {
                                    warn!("FFmpeg警告: {}", line);
                                } else {
                                    debug!("FFmpeg输出: {}", line);
                                }
                            }
                        }
                        Err(e) => {
                            debug!("读取FFmpeg stderr失败: {}", e);
                            break;
                        }
                    }
                }
            });
        }

        // 保存进程到主进程槽
        self.primary_process = Some(process);

        // 创建分段记录
        if self.segment_mode {
            let segment = RecordSegment {
                path: segment_path,
                start_time: Instant::now(),
                end_time: None,
                file_size: None,
                sequence: self.current_segment,
                cdn_node: cdn_node.to_string(),
            };
            
            self.segments.push(segment);
            debug!("已添加分段记录，总分段数: {}", self.segments.len());
        }

        info!("新分段已启动，PID: {:?}", self.primary_process.as_ref().map(|p| p.id()));
        Ok(())
    }

    /// 停止录制
    pub async fn stop(&mut self) -> Result<()> {
        if self.status != RecordStatus::Recording {
            return Ok(());
        }

        info!("停止录制");

        if let Some(mut process) = self.primary_process.take() {
            // 尝试优雅地终止进程
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                // 发送SIGTERM信号
                unsafe {
                    libc::kill(process.id() as i32, libc::SIGTERM);
                }
                
                // 等待进程结束，最多等待10秒
                let timeout = Duration::from_secs(10);
                let start = Instant::now();
                
                loop {
                    match process.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) => {
                            if start.elapsed() > timeout {
                                warn!("进程未在超时时间内结束，强制终止");
                                let _ = process.kill();
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        Err(e) => {
                            error!("等待进程结束时出错: {}", e);
                            let _ = process.kill();
                            break;
                        }
                    }
                }
            }

            #[cfg(windows)]
            {
                // Windows下直接终止进程
                if let Err(e) = process.kill() {
                    warn!("终止FFmpeg进程失败: {}", e);
                }
            }

            // 等待进程完全退出
            if let Err(e) = process.wait() {
                warn!("等待FFmpeg进程退出失败: {}", e);
            }
        }

        self.status = RecordStatus::Stopped;
        self.stats.is_recording = false;

        // 更新统计信息
        if let Some(start_time) = self.stats.start_time {
            self.stats.duration = start_time.elapsed();
        }

        // 获取文件大小
        if let Ok(metadata) = tokio::fs::metadata(&self.final_output_path).await {
            self.stats.file_size = metadata.len();
        }

        info!("录制已停止，文件大小: {} 字节", self.stats.file_size);
        Ok(())
    }

    /// 获取输出文件路径
    pub fn output_path(&self) -> Option<&Path> {
        Some(&self.final_output_path)
    }

    /// 检查FFmpeg是否可用
    async fn is_ffmpeg_available(&self) -> bool {
        match Command::new("ffmpeg")
            .arg("-version")
            .output()
        {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// 构建FFmpeg命令参数
    fn build_ffmpeg_args(&self, stream_url: &str, output_path: &Path) -> Result<Vec<String>> {
        let mut args = Vec::new();

        // 输入重连选项（应该放在-i之前）
        args.extend_from_slice(&[
            "-reconnect".to_string(), "1".to_string(),           // 启用重连
            "-reconnect_at_eof".to_string(), "1".to_string(),    // 在EOF时重连
            "-reconnect_streamed".to_string(), "1".to_string(),  // 流式重连
            "-reconnect_delay_max".to_string(), "10".to_string(), // 最大重连延迟
        ]);

        // 添加HTTP头部选项来解决403错误
        // FFmpeg的headers参数格式是多个头用\r\n分隔
        args.extend_from_slice(&[
            "-headers".to_string(), 
            "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36\r\nReferer: https://live.bilibili.com/".to_string(),
        ]);

        // 输入选项
        args.extend_from_slice(&[
            "-y".to_string(),                                    // 覆盖输出文件
            "-i".to_string(), stream_url.to_string(),            // 输入流
            "-c".to_string(), "copy".to_string(),                // 直接复制流，不重编码
            "-avoid_negative_ts".to_string(), "make_zero".to_string(), // 避免负时间戳
        ]);

        // 根据输出文件扩展名决定格式
        let output_ext = output_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("flv")
            .to_lowercase();

        match output_ext.as_str() {
            "mp4" => {
                args.extend_from_slice(&[
                    "-f".to_string(), "mp4".to_string(),
                    // 移除 faststart，因为它与直播流录制不兼容
                    // faststart 需要完整文件才能工作，而直播流是实时的
                ]);
            }
            "flv" => {
                args.extend_from_slice(&[
                    "-f".to_string(), "flv".to_string(),
                ]);
            }
            "mkv" => {
                args.extend_from_slice(&[
                    "-f".to_string(), "matroska".to_string(),
                ]);
            }
            "ts" => {
                args.extend_from_slice(&[
                    "-f".to_string(), "mpegts".to_string(),
                ]);
            }
            _ => {
                // 默认使用FLV格式，最适合直播流录制
                args.extend_from_slice(&[
                    "-f".to_string(), "flv".to_string(),
                ]);
            }
        }

        // 输出文件
        args.push(output_path.to_string_lossy().to_string());

        Ok(args)
    }

    /// 检查FFmpeg进程是否仍在运行
    pub fn check_process_status(&mut self) -> Result<bool> {
        if let Some(ref mut process) = self.primary_process {
            match process.try_wait() {
                Ok(Some(status)) => {
                    // 进程已退出
                    if status.success() {
                        info!("FFmpeg进程正常退出");
                        self.status = RecordStatus::Stopped;
                    } else {
                        // 获取退出码和详细错误信息
                        let exit_info = if let Some(code) = status.code() {
                            format!("退出码: {}", code)
                        } else {
                            "进程被信号终止".to_string()
                        };
                        
                        error!("FFmpeg进程异常退出: {}", exit_info);
                        
                        // 根据退出码提供更具体的错误信息
                        match status.code() {
                            Some(1) => error!("FFmpeg错误: 一般性错误，可能是输入文件问题或参数错误"),
                            Some(2) => error!("FFmpeg错误: 参数解析失败"),
                            Some(69) => error!("FFmpeg错误: 无法打开输入文件或网络连接失败"),
                            Some(code) if code < 0 => error!("FFmpeg错误: 进程被信号终止"),
                            _ => error!("FFmpeg错误: 未知错误"),
                        }
                        
                        self.status = RecordStatus::Error;
                    }
                    self.stats.is_recording = false;
                    Ok(false)
                }
                Ok(None) => {
                    // 进程仍在运行
                    Ok(true)
                }
                Err(e) => {
                    error!("检查进程状态失败: {}", e);
                    self.status = RecordStatus::Error;
                    self.stats.is_recording = false;
                    Err(anyhow!("检查进程状态失败: {}", e))
                }
            }
        } else {
            Ok(false)
        }
    }


    /// 无缝切换到新的流URL
    /// 
    /// # Arguments
    /// * `new_stream_url` - 新的流地址
    /// * `cdn_node` - CDN节点标识
    pub async fn seamless_switch(&mut self, new_stream_url: String, cdn_node: &str) -> Result<()> {
        if !self.segment_mode {
            warn!("非分段模式下无法进行无缝切换，使用普通重启");
            return self.restart_with_new_url(new_stream_url).await;
        }

        info!("开始无缝切换到新URL，CDN: {}", cdn_node);

        // 1. 启动备用进程开始录制下一个分段
        if self.secondary_process.is_some() {
            warn!("备用进程已存在，先停止备用进程");
            self.stop_secondary_process().await?;
        }

        self.start_secondary_segment(&new_stream_url, cdn_node).await?;

        // 2. 等待短暂时间确保备用进程开始录制
        tokio::time::sleep(Duration::from_secs(2)).await;

        // 3. 停止主进程
        self.stop_primary_process().await?;

        // 4. 将备用进程提升为主进程
        self.promote_secondary_to_primary();

        // 5. 更新当前使用的URL
        self.current_stream_url = Some(new_stream_url);

        // 6. 完成当前分段记录
        self.finalize_current_segment().await;

        info!("无缝切换完成，新CDN: {}", cdn_node);
        Ok(())
    }

    /// 启动备用进程录制新分段
    async fn start_secondary_segment(&mut self, stream_url: &str, _cdn_node: &str) -> Result<()> {
        let next_segment_num = self.current_segment + 1;
        
        let extension = self.final_output_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("flv");
        
        let segment_path = self.base_output_path
            .with_file_name(format!("{}_segment_{:03}.{}", 
                self.base_output_path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy(), 
                next_segment_num, 
                extension));

        debug!("启动备用进程录制分段: {:?}", segment_path);

        // 构建FFmpeg命令参数
        let args = self.build_ffmpeg_args(stream_url, &segment_path)?;
        
        // 启动备用FFmpeg进程
        let mut process = Command::new("ffmpeg")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| LiveError::RecorderStartError(format!("启动备用FFmpeg失败: {}", e)))?;

        // 异步读取stderr输出
        if let Some(stderr) = process.stderr.take() {
            tokio::spawn(async move {
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(stderr);
                
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            let line = line.trim();
                            if !line.is_empty() {
                                if line.contains("error") || line.contains("Error") || line.contains("ERROR") {
                                    error!("备用FFmpeg错误: {}", line);
                                } else if line.contains("warning") || line.contains("Warning") || line.contains("WARNING") {
                                    warn!("备用FFmpeg警告: {}", line);
                                } else {
                                    debug!("备用FFmpeg输出: {}", line);
                                }
                            }
                        }
                        Err(e) => {
                            debug!("读取备用FFmpeg stderr失败: {}", e);
                            break;
                        }
                    }
                }
            });
        }

        // 保存备用进程
        self.secondary_process = Some(process);

        // 创建新分段记录（但暂时不添加到segments列表，等切换成功后再添加）
        info!("备用进程已启动，准备录制分段: {}", next_segment_num);
        Ok(())
    }

    /// 停止主进程
    async fn stop_primary_process(&mut self) -> Result<()> {
        if let Some(mut process) = self.primary_process.take() {
            info!("停止主录制进程");
            
            #[cfg(unix)]
            {
                // 发送SIGTERM信号优雅退出
                unsafe {
                    libc::kill(process.id() as i32, libc::SIGTERM);
                }
                
                // 等待进程结束，最多等待5秒
                let timeout = Duration::from_secs(5);
                let start = Instant::now();
                
                loop {
                    match process.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) => {
                            if start.elapsed() > timeout {
                                warn!("主进程未在超时时间内结束，强制终止");
                                let _ = process.kill();
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        Err(e) => {
                            error!("等待主进程结束时出错: {}", e);
                            let _ = process.kill();
                            break;
                        }
                    }
                }
            }

            #[cfg(windows)]
            {
                if let Err(e) = process.kill() {
                    warn!("终止主FFmpeg进程失败: {}", e);
                }
            }

            if let Err(e) = process.wait() {
                warn!("等待主FFmpeg进程退出失败: {}", e);
            }
        }
        Ok(())
    }

    /// 停止备用进程
    async fn stop_secondary_process(&mut self) -> Result<()> {
        if let Some(mut process) = self.secondary_process.take() {
            info!("停止备用录制进程");
            
            if let Err(e) = process.kill() {
                warn!("终止备用FFmpeg进程失败: {}", e);
            }
            
            if let Err(e) = process.wait() {
                warn!("等待备用FFmpeg进程退出失败: {}", e);
            }
        }
        Ok(())
    }

    /// 将备用进程提升为主进程
    fn promote_secondary_to_primary(&mut self) {
        if let Some(secondary) = self.secondary_process.take() {
            self.primary_process = Some(secondary);
            self.current_segment += 1;
            info!("备用进程已提升为主进程，当前分段: {}", self.current_segment);
            
            // 在分段模式下，添加新的分段记录
            if self.segment_mode {
                let extension = self.final_output_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("flv");
                
                let segment_path = self.base_output_path
                    .with_file_name(format!("{}_segment_{:03}.{}", 
                        self.base_output_path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy(), 
                        self.current_segment, 
                        extension));
                
                let segment = RecordSegment {
                    path: segment_path,
                    start_time: Instant::now(),
                    end_time: None,
                    file_size: None,
                    sequence: self.current_segment,
                    cdn_node: "switched".to_string(), // 临时标记，实际应从参数传入
                };
                
                self.segments.push(segment);
            }
        } else {
            warn!("尝试提升备用进程为主进程，但备用进程不存在");
        }
    }

    /// 完成当前分段记录
    async fn finalize_current_segment(&mut self) {
        if let Some(last_segment) = self.segments.last_mut() {
            last_segment.end_time = Some(Instant::now());
            
            // 尝试获取分段文件大小
            if let Ok(metadata) = tokio::fs::metadata(&last_segment.path).await {
                last_segment.file_size = Some(metadata.len());
                debug!("分段 {} 完成，大小: {} bytes", 
                    last_segment.sequence, metadata.len());
            }
        }
    }

    /// 普通重启（非分段模式的后备方案）
    async fn restart_with_new_url(&mut self, new_stream_url: String) -> Result<()> {
        info!("使用普通重启方式切换URL");
        
        // 停止当前录制
        self.stop().await?;
        
        // 重新开始录制
        self.start(new_stream_url).await?;
        
        Ok(())
    }

    /// 获取分段列表
    pub fn get_segments(&self) -> &[RecordSegment] {
        &self.segments
    }

    /// 检查是否需要切换URL（根据时间或错误情况）
    pub fn should_switch_url(&self) -> bool {
        // 如果主进程存在且正在运行，检查是否需要切换
        if let Some(ref _process) = self.primary_process {
            // 这里可以添加更复杂的切换逻辑
            // 比如检查进程状态、录制时间、错误计数等
            
            // 简单的时间基准切换：每10分钟切换一次
            if let Some(start_time) = self.stats.start_time {
                let elapsed = start_time.elapsed();
                // 每10分钟或发生错误时建议切换
                return elapsed.as_secs() > 600; // 10分钟
            }
        }
        false
    }

    /// 合并所有分段文件为最终文件
    pub async fn merge_segments(&self) -> Result<()> {
        if !self.segment_mode || self.segments.is_empty() {
            debug!("非分段模式或无分段文件，跳过合并");
            return Ok(());
        }

        info!("开始合并 {} 个分段文件", self.segments.len());

        // 检查所有分段文件是否存在
        let mut existing_segments: Vec<&Path> = Vec::new();
        for segment in &self.segments {
            if tokio::fs::metadata(&segment.path).await.is_ok() {
                existing_segments.push(segment.path.as_path());
            } else {
                warn!("分段文件不存在: {:?}", segment.path);
            }
        }

        if existing_segments.is_empty() {
            return Err(anyhow!("没有可用的分段文件进行合并"));
        }

        info!("找到 {} 个有效分段文件，开始合并", existing_segments.len());

        // 使用FFmpeg合并文件
        if existing_segments.len() == 1 {
            // 只有一个分段，直接重命名
            self.merge_single_segment(existing_segments[0]).await?;
        } else {
            // 多个分段，使用FFmpeg concat协议合并
            self.merge_multiple_segments(&existing_segments).await?;
        }

        info!("分段文件合并完成，输出文件: {:?}", self.final_output_path);
        Ok(())
    }

    /// 合并单个分段文件（重命名）
    async fn merge_single_segment(&self, segment_path: &Path) -> Result<()> {
        debug!("单分段合并：重命名 {:?} -> {:?}", segment_path, self.final_output_path);
        
        tokio::fs::rename(segment_path, &self.final_output_path).await
            .map_err(|e| anyhow!("重命名分段文件失败: {}", e))?;
            
        info!("单分段文件重命名完成");
        Ok(())
    }

    /// 合并多个分段文件
    async fn merge_multiple_segments(&self, segment_paths: &[&Path]) -> Result<()> {
        debug!("多分段合并，分段数量: {}", segment_paths.len());

        // 创建临时的concat文件列表
        let concat_file_path = self.base_output_path.with_extension("txt");
        let mut concat_content = String::new();
        
        for path in segment_paths {
            // FFmpeg concat文件格式要求使用绝对路径并转义特殊字符
            let abs_path = path.canonicalize()
                .map_err(|e| anyhow!("无法获取分段文件的绝对路径: {}", e))?;
                
            let path_str = abs_path.to_string_lossy();
            // 在Windows下，需要将反斜杠替换为正斜杠
            let escaped_path = if cfg!(windows) {
                path_str.replace("\\", "/")
            } else {
                path_str.to_string()
            };
            
            concat_content.push_str(&format!("file '{}'\n", escaped_path));
        }
        
        // 写入concat文件
        tokio::fs::write(&concat_file_path, concat_content).await
            .map_err(|e| anyhow!("创建concat文件失败: {}", e))?;

        debug!("创建concat文件: {:?}", concat_file_path);

        // 构建FFmpeg合并命令
        let mut args = Vec::new();
        args.extend_from_slice(&[
            "-f".to_string(), "concat".to_string(),
            "-safe".to_string(), "0".to_string(),  // 允许不安全的文件路径
            "-i".to_string(), concat_file_path.to_string_lossy().to_string(),
            "-c".to_string(), "copy".to_string(),   // 直接复制流，不重编码
            "-y".to_string(),                       // 覆盖输出文件
            self.final_output_path.to_string_lossy().to_string(),
        ]);

        debug!("FFmpeg合并参数: {:?}", args);

        // 执行FFmpeg合并
        let output = tokio::process::Command::new("ffmpeg")
            .args(&args)
            .output()
            .await
            .map_err(|e| anyhow!("执行FFmpeg合并失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("FFmpeg合并失败: {}", stderr);
            return Err(anyhow!("FFmpeg合并失败: {}", stderr));
        }

        // 清理concat文件
        if let Err(e) = tokio::fs::remove_file(&concat_file_path).await {
            warn!("删除concat文件失败: {}", e);
        }

        info!("多分段文件合并完成");
        Ok(())
    }

    /// 清理分段文件
    pub async fn cleanup_segments(&self) -> Result<()> {
        if !self.segment_mode {
            return Ok(());
        }

        info!("开始清理 {} 个分段文件", self.segments.len());
        
        let mut cleaned_count = 0;
        let mut error_count = 0;

        for segment in &self.segments {
            match tokio::fs::remove_file(&segment.path).await {
                Ok(()) => {
                    debug!("删除分段文件: {:?}", segment.path);
                    cleaned_count += 1;
                }
                Err(e) => {
                    warn!("删除分段文件失败 {:?}: {}", segment.path, e);
                    error_count += 1;
                }
            }
        }

        info!("分段文件清理完成，成功删除: {}, 失败: {}", cleaned_count, error_count);
        
        if error_count > 0 {
            warn!("部分分段文件清理失败，请手动检查");
        }

        Ok(())
    }
}

impl Drop for LiveRecorder {
    fn drop(&mut self) {
        // 确保在录制器被销毁时停止录制进程
        if let Some(mut process) = self.primary_process.take() {
            if let Err(e) = process.kill() {
                error!("销毁录制器时终止主进程失败: {}", e);
            }
        }
        
        // 也要停止备用进程
        if let Some(mut process) = self.secondary_process.take() {
            if let Err(e) = process.kill() {
                error!("销毁录制器时终止备用进程失败: {}", e);
            }
        }
    }
}

/// 录制器工厂，用于创建不同配置的录制器
#[derive(Debug)]
#[allow(dead_code)] // 录制器工厂，暂时未使用但需要保留
pub struct RecorderFactory;

impl RecorderFactory {
    #[allow(dead_code)] // 工厂方法，暂时未使用但需要保留
    /// 创建FLV格式录制器
    pub fn create_flv_recorder<P: AsRef<Path>>(output_path: P) -> LiveRecorder {
        LiveRecorder::new(output_path)
    }

}