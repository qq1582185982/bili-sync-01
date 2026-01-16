use anyhow::Result;
use std::path::Path;
use tracing::{info, warn};

use crate::aria2_downloader::Aria2Downloader;
use crate::bilibili::Client;
use crate::downloader::Downloader;

/// 统一下载器，可以在原生下载器和aria2下载器之间切换
pub enum UnifiedDownloader {
    Native(Downloader),
    Aria2(Aria2Downloader),
}

impl UnifiedDownloader {
    /// 创建原生下载器
    #[allow(dead_code)]
    pub fn new_native(client: Client) -> Self {
        Self::Native(Downloader::new(client))
    }

    /// 创建aria2下载器
    #[allow(dead_code)]
    pub async fn new_aria2(client: Client) -> Result<Self> {
        let aria2_downloader = Aria2Downloader::new(client).await?;
        Ok(Self::Aria2(aria2_downloader))
    }

    /// 智能创建下载器：根据配置决定使用哪种下载器
    pub async fn new_smart(client: Client) -> Self {
        // 获取最新配置
        let config = crate::config::reload_config();
        let parallel = &config.concurrent_limit.parallel_download;

        // 检查是否启用了多线程下载
        if !parallel.enabled {
            info!("多线程下载已禁用，使用原生下载器");
            return Self::Native(Downloader::new(client));
        }

        // 如果用户关闭了 aria2，则直接使用原生多线程分片下载
        if !parallel.use_aria2 {
            info!("已关闭aria2，使用原生多线程下载");
            return Self::Native(Downloader::new(client));
        }

        // 如果启用了多线程下载，尝试使用aria2
        match Aria2Downloader::new(client.clone()).await {
            Ok(aria2_downloader) => {
                info!("成功初始化aria2下载器");
                Self::Aria2(aria2_downloader)
            }
            Err(e) => {
                warn!("aria2下载器初始化失败，回退到原生下载器: {:#}", e);
                Self::Native(Downloader::new(client))
            }
        }
    }

    /// 下载文件，支持多个URL备选
    pub async fn fetch_with_fallback(&self, urls: &[&str], path: &Path) -> Result<()> {
        match self {
            Self::Native(downloader) => downloader.fetch_with_fallback(urls, path).await,
            Self::Aria2(downloader) => downloader.fetch_with_aria2_fallback(urls, path).await,
        }
    }

    /// 合并视频和音频文件
    pub async fn merge(&self, video_path: &Path, audio_path: &Path, output_path: &Path) -> Result<()> {
        match self {
            Self::Native(downloader) => downloader.merge(video_path, audio_path, output_path).await,
            Self::Aria2(downloader) => downloader.merge(video_path, audio_path, output_path).await,
        }
    }

    /// 智能下载：根据文件大小和配置决定使用哪种下载方式
    #[allow(dead_code)]
    pub async fn smart_fetch(&self, url: &str, path: &Path) -> Result<()> {
        match self {
            Self::Native(downloader) => {
                // 原生下载器会根据配置决定是否启用分片多线程下载
                info!("原生下载器开始下载");
                downloader.fetch(url, path).await
            }
            Self::Aria2(downloader) => {
                // aria2下载器：使用智能下载功能
                downloader.smart_fetch(url, path).await
            }
        }
    }

    /// 重新启动下载器（用于配置更新）
    #[allow(dead_code)]
    pub async fn restart(&mut self) -> Result<()> {
        match self {
            Self::Native(_) => {
                // 原生下载器不需要重启操作
                Ok(())
            }
            Self::Aria2(downloader) => downloader.restart().await,
        }
    }

    /// 优雅关闭下载器
    #[allow(dead_code)]
    pub async fn shutdown(&self) -> Result<()> {
        match self {
            Self::Native(_) => {
                // 原生下载器不需要特殊关闭操作
                Ok(())
            }
            Self::Aria2(downloader) => downloader.shutdown().await,
        }
    }

    /// 检查是否为aria2下载器
    #[allow(dead_code)]
    pub fn is_aria2(&self) -> bool {
        matches!(self, Self::Aria2(_))
    }

    /// 检查是否为原生下载器
    #[allow(dead_code)]
    pub fn is_native(&self) -> bool {
        matches!(self, Self::Native(_))
    }
}
