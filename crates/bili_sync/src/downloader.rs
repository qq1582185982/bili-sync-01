use core::str;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use futures::{StreamExt, TryStreamExt, future};
use reqwest::{
    Method,
    header::{CONTENT_LENGTH, RANGE},
};
use std::io::SeekFrom;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::StreamReader;
use tracing::{debug, error, info, warn};

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

    /// 获取文件的 Content-Length，用于判断是否需要多线程下载
    pub async fn get_content_length(&self, url: &str) -> Result<u64> {
        let resp = self
            .client
            .request(Method::HEAD, url, None)
            .send()
            .await?
            .error_for_status()?;

        let content_length = resp
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| anyhow::anyhow!("无法获取 Content-Length"))?;

        Ok(content_length)
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

    /// 多线程分片下载单个文件
    ///
    /// # 参数
    /// * `url` - 文件下载地址
    /// * `path` - 保存路径
    /// * `concurrency` - 并发数，建议4-8
    pub async fn fetch_parallel(&self, url: &str, path: &Path, concurrency: usize) -> Result<()> {
        // 创建父目录
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await?;
            }
        }

        // 使用HEAD请求获取文件总大小
        let resp = self
            .client
            .request(Method::HEAD, url, None)
            .send()
            .await?
            .error_for_status()?;

        // 获取文件总大小
        let total_size = resp
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| anyhow::anyhow!("无法获取文件大小"))?;

        // 检查服务器是否支持Range请求
        if !resp.headers().contains_key("accept-ranges") {
            debug!("服务器不支持Range请求，回退到普通下载方式");
            return self.fetch(url, path).await;
        }

        info!(
            "开始多线程下载: {} (大小: {:.2} MB, 线程数: {})",
            path.file_name().unwrap_or_default().to_string_lossy(),
            total_size as f64 / 1_000_000.0,
            concurrency
        );

        // 创建空文件并预分配空间
        let file = File::create(path).await?;
        file.set_len(total_size).await?;
        drop(file);

        // 分块大小
        let chunk_size = total_size / concurrency as u64;

        // 创建下载任务
        let mut tasks = Vec::with_capacity(concurrency);

        for i in 0..concurrency {
            let start = i as u64 * chunk_size;
            let end = if i == concurrency - 1 {
                total_size - 1
            } else {
                (i + 1) as u64 * chunk_size - 1
            };

            let url = url.to_string();
            let path = path.to_path_buf();
            let client = self.client.clone();

            // 创建下载任务
            let task = tokio::spawn(async move {
                let mut retry_count = 0;
                const MAX_RETRIES: usize = 3;

                loop {
                    match Self::download_chunk(&client, &url, &path, start, end, i).await {
                        Ok(_) => break Ok(()),
                        Err(e) => {
                            retry_count += 1;
                            if retry_count >= MAX_RETRIES {
                                error!("分片 {} 下载失败，已重试 {} 次: {:#}", i, MAX_RETRIES, e);
                                break Err(e);
                            } else {
                                warn!(
                                    "分片 {} 下载失败，正在重试 ({}/{}): {:#}",
                                    i, retry_count, MAX_RETRIES, e
                                );
                                tokio::time::sleep(tokio::time::Duration::from_millis(1000 * retry_count as u64)).await;
                            }
                        }
                    }
                }
            });

            tasks.push(task);
        }

        // 等待所有任务完成
        let results = future::join_all(tasks).await;

        // 检查是否有任务失败
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    error!("分片 {} 下载失败: {:#}", i, e);
                    return Err(e);
                }
                Err(e) => {
                    error!("分片 {} 任务执行失败: {:#}", i, e);
                    return Err(anyhow::anyhow!("分片任务执行失败: {:#}", e));
                }
            }
        }

        info!("多线程下载完成: {}", path.display());
        Ok(())
    }

    /// 下载单个分片的辅助方法
    async fn download_chunk(
        client: &Client,
        url: &str,
        path: &Path,
        start: u64,
        end: u64,
        chunk_id: usize,
    ) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(path)
            .await
            .with_context(|| format!("打开文件失败: {}", path.display()))?;

        // 设置Range头
        let range_header = format!("bytes={}-{}", start, end);

        // 发送请求
        let resp = client
            .request(Method::GET, url, None)
            .header(RANGE, &range_header)
            .send()
            .await
            .with_context(|| format!("分片 {} HTTP请求失败", chunk_id))?
            .error_for_status()
            .with_context(|| format!("分片 {} HTTP状态码错误", chunk_id))?;

        // 将文件指针移动到对应位置
        file.seek(SeekFrom::Start(start))
            .await
            .with_context(|| format!("分片 {} 文件定位失败", chunk_id))?;

        // 下载数据并写入文件
        let mut stream = resp.bytes_stream();
        let mut downloaded = 0u64;
        let expected_size = end - start + 1;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.with_context(|| format!("分片 {} 读取数据失败", chunk_id))?;

            file.write_all(&chunk)
                .await
                .with_context(|| format!("分片 {} 写入文件失败", chunk_id))?;

            downloaded += chunk.len() as u64;
        }

        file.flush()
            .await
            .with_context(|| format!("分片 {} 刷新文件缓冲区失败", chunk_id))?;

        // 验证下载的数据大小
        ensure!(
            downloaded >= expected_size,
            "分片 {} 下载不完整: 已下载 {} 字节, 预期 {} 字节",
            chunk_id,
            downloaded,
            expected_size
        );

        debug!("分片 {} 下载完成: {} 字节", chunk_id, downloaded);
        Ok(())
    }

    pub async fn fetch_with_fallback(&self, urls: &[&str], path: &Path) -> Result<()> {
        if urls.is_empty() {
            bail!("no urls provided");
        }

        let mut last_error = None;
        for url in urls.iter() {
            match self.fetch(url, path).await {
                Ok(_) => {
                    return Ok(());
                }
                Err(err) => {
                    warn!("下载失败: {:#}", err);
                    last_error = Some(err);
                }
            }
        }

        error!("所有URL尝试失败");
        match last_error {
            Some(err) => Err(err).with_context(|| format!("failed to download from {:?}", urls)),
            None => bail!("no urls to try"),
        }
    }

    /// 使用多线程下载尝试多个URL
    pub async fn fetch_with_fallback_parallel(&self, urls: &[&str], path: &Path, concurrency: usize) -> Result<()> {
        if urls.is_empty() {
            bail!("no urls provided");
        }

        let mut last_error = None;

        for (i, url) in urls.iter().enumerate() {
            debug!("尝试多线程下载 URL {} / {}: {}", i + 1, urls.len(), url);

            match self.fetch_parallel(url, path, concurrency).await {
                Ok(_) => {
                    return Ok(());
                }
                Err(err) => {
                    warn!("多线程下载失败 (URL {} / {}): {:#}", i + 1, urls.len(), err);
                    last_error = Some(err);
                }
            }
        }

        warn!("所有URL多线程下载尝试失败，回退到普通下载");

        // 回退到普通下载
        match self.fetch_with_fallback(urls, path).await {
            Ok(_) => {
                info!("普通下载成功");
                Ok(())
            }
            Err(fallback_err) => {
                error!("普通下载也失败了");
                // 返回最后一个多线程下载的错误，因为它通常更有信息价值
                match last_error {
                    Some(err) => Err(err).with_context(|| format!("多线程和普通下载都失败，URLs: {:?}", urls)),
                    None => Err(fallback_err).with_context(|| format!("failed to download from {:?}", urls)),
                }
            }
        }
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
            "-i",
            &video_path_str,
            "-i",
            &audio_path_str,
            "-c",
            "copy",
            "-strict",
            "unofficial",
            "-y",
            &output_path_str,
        ];

        let output = tokio::process::Command::new("ffmpeg").args(args).output().await?;

        if !output.status.success() {
            let stderr = str::from_utf8(&output.stderr).unwrap_or("unknown");
            error!("FFmpeg错误: {}", stderr);
            bail!("ffmpeg error: {}", stderr);
        }

        Ok(())
    }
}
