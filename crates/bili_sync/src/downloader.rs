use core::str;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use futures::TryStreamExt;
use reqwest::Method;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tokio_util::io::StreamReader;
use tracing::{warn, error};

use crate::bilibili::Client;
pub struct Downloader {
    client: Client,
}

impl Downloader {
    // Downloader 使用带有默认 Header 的 Client 构建
    // 拿到 url 后下载文件不需要任何 cookie 作为身份凭证
    // 但如果不设置默认 Header，下载时会遇到 403 Forbidden 错误
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn fetch(&self, url: &str, path: &Path) -> Result<()> {
        // 创建父目录
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await?;
            }
        }
        
        let mut file = match File::create(path).await {
            Ok(f) => f,
            Err(e) => {
                error!("创建文件失败: {:#}", e);
                return Err(e.into());
            }
        };
        
        let resp = match self.client.request(Method::GET, url, None).send().await {
            Ok(r) => match r.error_for_status() {
                Ok(r) => r,
                Err(e) => {
                    error!("HTTP状态码错误: {:#}", e);
                    return Err(e.into());
                }
            },
            Err(e) => {
                error!("HTTP请求失败: {:#}", e);
                return Err(e.into());
            }
        };
        
        let expected = resp.content_length().unwrap_or_default();
        
        let mut stream_reader = StreamReader::new(resp.bytes_stream().map_err(std::io::Error::other));
        let received = match tokio::io::copy(&mut stream_reader, &mut file).await {
            Ok(size) => size,
            Err(e) => {
                error!("下载过程中出错: {:#}", e);
                return Err(e.into());
            }
        };
        
        file.flush().await?;
        
        ensure!(
            received >= expected,
            "received {} bytes, expected {} bytes",
            received,
            expected
        );
        
        Ok(())
    }

    pub async fn fetch_with_fallback(&self, urls: &[&str], path: &Path) -> Result<()> {
        if urls.is_empty() {
            bail!("no urls provided");
        }
        
        let mut res = Ok(());
        for (_i, url) in urls.iter().enumerate() {
            match self.fetch(url, path).await {
                Ok(_) => {
                    return Ok(());
                },
                Err(err) => {
                    warn!("下载失败: {:#}", err);
                    res = Err(err);
                }
            }
        }
        
        error!("所有URL尝试失败");
        res.with_context(|| format!("failed to download from {:?}", urls))
    }

    pub async fn merge(&self, video_path: &Path, audio_path: &Path, output_path: &Path) -> Result<()> {
        // 检查输入文件是否存在
        if !video_path.exists() {
            error!("视频文件不存在");
            bail!("视频文件不存在");
        }
        
        if !audio_path.exists() {
            error!("音频文件不存在");
            bail!("音频文件不存在");
        }
        
        // 确保输出目录存在
        if let Some(parent) = output_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await?;
            }
        }
        
        // 将Path转换为字符串，防止临时值过早释放
        let video_path_str = video_path.to_string_lossy().to_string();
        let audio_path_str = audio_path.to_string_lossy().to_string();
        let output_path_str = output_path.to_string_lossy().to_string();
        
        // 构建FFmpeg命令
        let args = [
            "-i", &video_path_str,
            "-i", &audio_path_str,
            "-c", "copy",
            "-strict", "unofficial",
            "-y",
            &output_path_str,
        ];
        
        let output = tokio::process::Command::new("ffmpeg")
            .args(args)
            .output()
            .await?;
            
        if !output.status.success() {
            let stderr = str::from_utf8(&output.stderr).unwrap_or("unknown");
            error!("FFmpeg错误: {}", stderr);
            bail!("ffmpeg error: {}", stderr);
        }
        
        Ok(())
    }
}
