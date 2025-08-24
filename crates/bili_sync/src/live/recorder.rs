use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
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


/// 直播录制器（支持分段录制）
#[derive(Debug)]
pub struct LiveRecorder {
    /// 最终合并文件路径
    final_output_path: PathBuf,
    /// 主FFmpeg进程
    primary_process: Option<Child>,
    /// 主FFmpeg进程的stdin（用于优雅停止）
    primary_stdin: Option<ChildStdin>,
    /// 录制状态
    status: RecordStatus,
    /// 录制统计
    stats: RecordStats,
    /// 当前使用的流URL
    current_stream_url: Option<String>,
    // 分段相关字段已移除，直接录制最终文件
}

impl LiveRecorder {
    #[allow(dead_code)] // 录制器方法，部分暂时未使用但需要保留
    /// 创建新的录制器
    /// 
    /// # Arguments
    /// * `output_path` - 输出文件路径
    pub fn new<P: AsRef<Path>>(output_path: P) -> Self {
        let final_output_path = output_path.as_ref().to_path_buf();
        
        Self {
            final_output_path,
            primary_process: None,
            primary_stdin: None,
            status: RecordStatus::Idle,
            stats: RecordStats::default(),
            current_stream_url: None,
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

        // 启动录制进程
        self.start_recording(stream_url, cdn_node).await?;

        self.status = RecordStatus::Recording;
        self.current_stream_url = Some(stream_url.to_string());
        self.stats.start_time = Some(Instant::now());
        self.stats.is_recording = true;

        info!("录制已启动，直接录制模式");
        Ok(())
    }
    
    /// 启动录制进程
    async fn start_recording(&mut self, stream_url: &str, cdn_node: &str) -> Result<()> {
        
        // 直接使用最终输出文件路径，不使用分段模式
        let segment_path = self.final_output_path.clone();

        debug!("启动录制: {:?}, CDN: {}", segment_path, cdn_node);

        // 构建FFmpeg命令参数
        let args = self.build_ffmpeg_args(stream_url, &segment_path)?;
        
        debug!("FFmpeg命令参数: {:?}", args);

        // 启动FFmpeg进程（使用tokio异步进程）
        let mut process = Command::new("ffmpeg")
            .args(&args)
            .stdin(std::process::Stdio::piped())  // 改为piped以便发送停止命令
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| LiveError::RecorderStartError(format!("启动FFmpeg失败: {}", e)))?;

        // 保存stdin句柄用于优雅停止
        self.primary_stdin = process.stdin.take();

        // 异步读取stdout进度信息
        if let Some(stdout) = process.stdout.take() {
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                let mut progress_data = HashMap::new();
                
                while let Ok(Some(line)) = lines.next_line().await {
                    // FFmpeg进度格式: key=value
                    if line.contains("=") {
                        let parts: Vec<&str> = line.splitn(2, '=').collect();
                        if parts.len() == 2 {
                            progress_data.insert(parts[0].to_string(), parts[1].to_string());
                        }
                    }
                    
                    // 当收到 progress=continue 时，处理这批数据
                    if line == "progress=continue" {
                        // 监控录制速度
                        if let Some(speed) = progress_data.get("speed") {
                            // 解析速度（如 "0.998x"）
                            if let Some(speed_str) = speed.strip_suffix('x') {
                                if let Ok(speed_val) = speed_str.parse::<f64>() {
                                    if speed_val < 0.95 {
                                        warn!("录制速度过慢: {}x，可能丢帧", speed_val);
                                    } else if speed_val > 1.05 {
                                        debug!("录制速度: {}x", speed_val);
                                    }
                                }
                            }
                        }
                        
                        // 监控帧率
                        if let Some(fps) = progress_data.get("fps") {
                            if let Ok(fps_val) = fps.parse::<f64>() {
                                if fps_val < 20.0 {
                                    warn!("当前FPS过低: {}", fps_val);
                                } else {
                                    debug!("当前FPS: {}", fps_val);
                                }
                            }
                        }
                        
                        // 监控比特率
                        if let Some(bitrate) = progress_data.get("bitrate") {
                            debug!("当前比特率: {}", bitrate);
                        }
                        
                        // 监控录制时长
                        if let Some(time) = progress_data.get("out_time_ms") {
                            if let Ok(time_ms) = time.parse::<u64>() {
                                let duration_sec = time_ms / 1_000_000;
                                debug!("已录制: {}秒", duration_sec);
                            }
                        }
                        
                        // 清空数据准备下一批
                        progress_data.clear();
                    }
                }
            });
        }

        // 异步读取stderr输出
        if let Some(stderr) = process.stderr.take() {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                
                while let Ok(Some(line)) = lines.next_line().await {
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
            });
        }

        // 保存进程到主进程槽
        self.primary_process = Some(process);

        // 不使用分段模式，直接录制到最终文件

        info!("录制已启动，PID: {:?}", self.primary_process.as_ref().map(|p| p.id()));
        Ok(())
    }

    /// 停止录制
    pub async fn stop(&mut self) -> Result<()> {
        if self.status != RecordStatus::Recording {
            return Ok(());
        }

        info!("停止录制");

        if let Some(mut process) = self.primary_process.take() {
            let pid = process.id();
            
            // 优先尝试优雅停止
            if let Some(mut stdin) = self.primary_stdin.take() {
                match stdin.write_all(b"q").await {
                    Ok(_) => {
                        if let Err(e) = stdin.flush().await {
                            warn!("flush stdin失败: {}", e);
                        }
                        info!("已发送优雅停止命令到FFmpeg进程 {:?}", pid);
                        
                        // 等待进程自然退出（最多10秒）
                        let timeout = Duration::from_secs(10);
                        let start = Instant::now();
                        
                        loop {
                            match process.try_wait() {
                                Ok(Some(status)) => {
                                    info!("FFmpeg优雅退出，PID: {:?}，状态: {:?}", pid, status);
                                    break;
                                }
                                Ok(None) => {
                                    if start.elapsed() > timeout {
                                        warn!("FFmpeg未在{}秒内响应停止命令，强制终止", timeout.as_secs());
                                        if let Err(e) = process.kill().await {
                                            error!("强制终止进程失败: {}", e);
                                        }
                                        break;
                                    }
                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                }
                                Err(e) => {
                                    error!("等待进程退出时出错: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("发送优雅停止命令失败: {}，强制终止进程", e);
                        if let Err(e) = process.kill().await {
                            error!("强制终止进程失败: {}", e);
                        }
                    }
                }
            } else {
                warn!("stdin不可用，强制终止FFmpeg进程 {:?}", pid);
                if let Err(e) = process.kill().await {
                    error!("强制终止进程失败: {}", e);
                }
            }
            
            // 确保进程完全退出
            if let Err(e) = process.wait().await {
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
            .await
        {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// 构建FFmpeg命令参数
    fn build_ffmpeg_args(&self, stream_url: &str, output_path: &Path) -> Result<Vec<String>> {
        let mut args = Vec::new();

        // 进度监控参数（放在最前面）
        args.extend_from_slice(&[
            "-progress".to_string(), "-".to_string(),            // 输出进度信息到stdout
            "-nostats".to_string(),                              // 禁用默认统计输出
        ]);

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
    /// 重启录制以使用新URL（替代无缝切换）
    pub async fn seamless_switch(&mut self, new_stream_url: String, cdn_node: &str) -> Result<()> {
        info!("使用新URL重启录制，CDN: {}", cdn_node);
        
        // 使用简单的重启策略替代复杂的无缝切换
        self.restart_with_new_url(new_stream_url).await
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

    /// 获取分段列表（已禁用分段模式，返回空列表）
    pub fn get_segments(&self) -> &[String] {
        &[]
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

    /// 合并分段文件（已禁用分段模式，此方法不执行任何操作）
    pub async fn merge_segments(&self) -> Result<()> {
        debug!("分段模式已禁用，直接录制到最终文件，无需合并");
        Ok(())
    }


    /// 清理分段文件（已禁用分段模式，此方法不执行任何操作）
    pub async fn cleanup_segments(&self) -> Result<()> {
        debug!("分段模式已禁用，无需清理分段文件");
        Ok(())
    }
}

impl Drop for LiveRecorder {
    fn drop(&mut self) {
        // 确保在录制器被销毁时停止录制进程
        if let Some(mut process) = self.primary_process.take() {
            if let Err(e) = process.start_kill() {
                error!("销毁录制器时终止主进程失败: {}", e);
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