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

/// 直播录制器
#[derive(Debug)]
pub struct LiveRecorder {
    /// 输出文件路径
    output_path: PathBuf,
    /// FFmpeg进程
    ffmpeg_process: Option<Child>,
    /// 录制状态
    status: RecordStatus,
    /// 录制统计
    stats: RecordStats,
    /// 流URL
    stream_url: Option<String>,
}

impl LiveRecorder {
    #[allow(dead_code)] // 录制器方法，部分暂时未使用但需要保留
    /// 创建新的录制器
    /// 
    /// # Arguments
    /// * `output_path` - 输出文件路径
    pub fn new<P: AsRef<Path>>(output_path: P) -> Self {
        Self {
            output_path: output_path.as_ref().to_path_buf(),
            ffmpeg_process: None,
            status: RecordStatus::Idle,
            stats: RecordStats::default(),
            stream_url: None,
        }
    }

    /// 开始录制
    /// 
    /// # Arguments
    /// * `stream_url` - 直播流地址
    pub async fn start(&mut self, stream_url: String) -> Result<()> {
        if self.status == RecordStatus::Recording {
            return Err(anyhow!("录制器已在录制中"));
        }

        info!("开始录制流: {}", stream_url);
        debug!("输出文件: {:?}", self.output_path);

        // 确保输出目录存在
        if let Some(parent) = self.output_path.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| LiveError::FileError(e))?;
        }

        // 检查ffmpeg是否可用
        if !self.is_ffmpeg_available().await {
            return Err(anyhow!("FFmpeg不可用，请确保已安装FFmpeg"));
        }

        // 构建FFmpeg命令参数
        let args = self.build_ffmpeg_args(&stream_url)?;
        
        debug!("FFmpeg命令参数: {:?}", args);

        // 启动FFmpeg进程
        let mut process = Command::new("ffmpeg")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| LiveError::RecorderStartError(format!("启动FFmpeg失败: {}", e)))?;

        // 异步读取stderr输出以便记录错误信息
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

        self.ffmpeg_process = Some(process);
        self.status = RecordStatus::Recording;
        self.stream_url = Some(stream_url);
        self.stats.start_time = Some(Instant::now());
        self.stats.is_recording = true;

        info!("录制已启动，PID: {:?}", self.ffmpeg_process.as_ref().map(|p| p.id()));
        Ok(())
    }

    /// 停止录制
    pub async fn stop(&mut self) -> Result<()> {
        if self.status != RecordStatus::Recording {
            return Ok(());
        }

        info!("停止录制");

        if let Some(mut process) = self.ffmpeg_process.take() {
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
        if let Ok(metadata) = tokio::fs::metadata(&self.output_path).await {
            self.stats.file_size = metadata.len();
        }

        info!("录制已停止，文件大小: {} 字节", self.stats.file_size);
        Ok(())
    }

    /// 检查录制状态
    pub fn is_recording(&self) -> bool {
        self.status == RecordStatus::Recording
    }

    /// 获取录制状态
    pub fn status(&self) -> RecordStatus {
        self.status
    }

    /// 获取输出文件路径
    pub fn output_path(&self) -> Option<&Path> {
        Some(&self.output_path)
    }

    /// 获取录制统计信息
    pub fn stats(&self) -> &RecordStats {
        &self.stats
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
    fn build_ffmpeg_args(&self, stream_url: &str) -> Result<Vec<String>> {
        let mut args = Vec::new();

        // 输入重连选项（应该放在-i之前）
        args.extend_from_slice(&[
            "-reconnect".to_string(), "1".to_string(),           // 启用重连
            "-reconnect_at_eof".to_string(), "1".to_string(),    // 在EOF时重连
            "-reconnect_streamed".to_string(), "1".to_string(),  // 流式重连
            "-reconnect_delay_max".to_string(), "10".to_string(), // 最大重连延迟
        ]);

        // 输入选项
        args.extend_from_slice(&[
            "-y".to_string(),                                    // 覆盖输出文件
            "-i".to_string(), stream_url.to_string(),            // 输入流
            "-c".to_string(), "copy".to_string(),                // 直接复制流，不重编码
            "-avoid_negative_ts".to_string(), "make_zero".to_string(), // 避免负时间戳
        ]);

        // 根据输出文件扩展名决定格式
        let output_ext = self.output_path
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
        args.push(self.output_path.to_string_lossy().to_string());

        Ok(args)
    }

    /// 检查FFmpeg进程是否仍在运行
    pub fn check_process_status(&mut self) -> Result<bool> {
        if let Some(ref mut process) = self.ffmpeg_process {
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

    /// 获取当前文件大小（如果文件存在）
    pub async fn current_file_size(&self) -> Result<u64> {
        match tokio::fs::metadata(&self.output_path).await {
            Ok(metadata) => Ok(metadata.len()),
            Err(e) => {
                debug!("无法获取文件大小: {}", e);
                Ok(0)
            }
        }
    }
}

impl Drop for LiveRecorder {
    fn drop(&mut self) {
        // 确保在录制器被销毁时停止录制进程
        if let Some(mut process) = self.ffmpeg_process.take() {
            if let Err(e) = process.kill() {
                error!("销毁录制器时终止进程失败: {}", e);
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

    /// 创建MP4格式录制器（需要重编码，质量更高但占用更多CPU）
    pub fn create_mp4_recorder<P: AsRef<Path>>(output_path: P) -> LiveRecorder {
        let recorder = LiveRecorder::new(output_path);
        // TODO: 在未来版本中添加对不同格式的支持
        recorder
    }
}