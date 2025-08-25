use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use super::LiveError;

/// FFmpeg录制器（原有逻辑）
#[derive(Debug)]
pub struct FFmpegRecorder {
    /// 最终合并文件路径
    pub final_output_path: PathBuf,
    /// 主FFmpeg进程
    pub primary_process: Option<Child>,
    /// 主FFmpeg进程的stdin（用于优雅停止）
    pub primary_stdin: Option<ChildStdin>,
    /// 当前使用的流URL
    pub current_stream_url: Option<String>,
    /// 最大文件大小（字节），0表示无限制
    pub max_file_size: i64,
}

impl FFmpegRecorder {
    /// 创建新的FFmpeg录制器
    pub fn new<P: AsRef<Path>>(output_path: P, max_file_size: i64) -> Self {
        let final_output_path = output_path.as_ref().to_path_buf();
        
        Self {
            final_output_path,
            primary_process: None,
            primary_stdin: None,
            current_stream_url: None,
            max_file_size,
        }
    }

    /// 开始录制（指定CDN节点）
    pub async fn start_with_cdn(&mut self, stream_url: &str, cdn_node: &str) -> Result<()> {
        info!("FFmpeg录制器开始录制流: {}, CDN: {}", stream_url, cdn_node);
        
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
        self.current_stream_url = Some(stream_url.to_string());

        info!("FFmpeg录制已启动");
        Ok(())
    }
    
    /// 启动录制进程
    async fn start_recording(&mut self, stream_url: &str, cdn_node: &str) -> Result<()> {
        debug!("启动FFmpeg录制: {:?}, CDN: {}", self.final_output_path, cdn_node);

        // 构建FFmpeg命令参数
        let args = self.build_ffmpeg_args(stream_url, &self.final_output_path)?;
        
        debug!("FFmpeg命令参数: {:?}", args);

        // 启动FFmpeg进程
        let mut process = Command::new("ffmpeg")
            .args(&args)
            .stdin(std::process::Stdio::piped())
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
                        
                        // 清空数据准备下一批
                        progress_data.clear();
                    }
                }
            });
        }

        // 异步读取stderr输出，检测403/404错误
        if let Some(stderr) = process.stderr.take() {
            let process_id = process.id();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim();
                    if !line.is_empty() {
                        // 检测URL过期错误
                        if line.contains("403 Forbidden") || line.contains("404 Not Found") || 
                           line.contains("HTTP error 403") || line.contains("HTTP error 404") {
                            error!("检测到URL过期错误，FFmpeg将快速失败: {}", line);
                        } else if line.contains("error") || line.contains("Error") || line.contains("ERROR") {
                            error!("FFmpeg错误: {}", line);
                        } else if line.contains("warning") || line.contains("Warning") || line.contains("WARNING") {
                            warn!("FFmpeg警告: {}", line);
                        } else {
                            debug!("FFmpeg输出: {}", line);
                        }
                    }
                }
                
                debug!("FFmpeg进程 {:?} 的stderr读取已结束", process_id);
            });
        }

        // 保存进程
        self.primary_process = Some(process);

        info!("FFmpeg录制已启动，PID: {:?}", self.primary_process.as_ref().map(|p| p.id()));
        Ok(())
    }

    /// 停止录制
    pub async fn stop(&mut self) -> Result<()> {
        info!("停止FFmpeg录制");

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

        info!("FFmpeg录制已停止");
        Ok(())
    }

    /// 检查进程状态
    pub fn check_process_status(&mut self) -> Result<bool> {
        if let Some(ref mut process) = self.primary_process {
            match process.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        info!("FFmpeg进程正常退出");
                    } else {
                        error!("FFmpeg进程异常退出: {:?}", status);
                    }
                    Ok(false)
                }
                Ok(None) => Ok(true), // 仍在运行
                Err(e) => {
                    error!("检查进程状态失败: {}", e);
                    Err(anyhow!("检查进程状态失败: {}", e))
                }
            }
        } else {
            Ok(false)
        }
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

        // 进度监控参数
        args.extend_from_slice(&[
            "-progress".to_string(), "-".to_string(),
            "-nostats".to_string(),
        ]);

        // 基础网络超时设置（3秒超时，快速失败）
        args.extend_from_slice(&[
            "-rw_timeout".to_string(), "3000000".to_string(),
        ]);

        // 添加HTTP头部选项来解决403错误
        args.extend_from_slice(&[
            "-headers".to_string(), 
            "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36\r\nReferer: https://live.bilibili.com/".to_string(),
        ]);

        // 输入选项
        args.extend_from_slice(&[
            "-y".to_string(),
            "-i".to_string(), stream_url.to_string(),
            "-c".to_string(), "copy".to_string(),
            "-avoid_negative_ts".to_string(), "make_zero".to_string(),
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
                // 默认使用FLV格式
                args.extend_from_slice(&[
                    "-f".to_string(), "flv".to_string(),
                ]);
            }
        }

        // 添加文件大小限制参数
        if self.max_file_size > 0 {
            args.extend_from_slice(&[
                "-fs".to_string(), 
                self.max_file_size.to_string(),
            ]);
        }

        // 输出文件
        args.push(output_path.to_string_lossy().to_string());

        Ok(args)
    }
}