//! 直播录制功能模块
//! 
//! 提供B站直播间状态监控和录制功能，包括：
//! - 直播间状态检测
//! - 直播流地址获取  
//! - 自动录制管理
//! - 录制文件管理

/// 直播模块专用的日志宏
/// 这些宏会将日志标记为 "live" 级别，便于单独过滤和查看
#[macro_export]
macro_rules! live_debug {
    ($($arg:tt)*) => {
        tracing::event!(target: "bili_sync::live", tracing::Level::DEBUG, $($arg)*);
    };
}

#[macro_export]
macro_rules! live_info {
    ($($arg:tt)*) => {
        tracing::event!(target: "bili_sync::live", tracing::Level::INFO, $($arg)*);
    };
}

#[macro_export]
macro_rules! live_warn {
    ($($arg:tt)*) => {
        tracing::event!(target: "bili_sync::live", tracing::Level::WARN, $($arg)*);
    };
}

#[macro_export]
macro_rules! live_error {
    ($($arg:tt)*) => {
        tracing::event!(target: "bili_sync::live", tracing::Level::ERROR, $($arg)*);
    };
}

pub mod api;
pub mod monitor;
pub mod recorder;
pub mod ffmpeg_recorder;
pub mod ws_client;
pub mod segment_downloader;
pub mod segment_manager;
pub mod m3u8_parser;
pub mod config;
pub mod bilibili_api;

// 只导出实际使用的类型
pub use monitor::LiveMonitor;
// pub use config::{LiveRecordingConfig, AutoMergeConfig, MergeQuality};

/// 直播录制相关的错误类型
#[derive(Debug, thiserror::Error)]
pub enum LiveError {
    #[error("API请求失败: {0}")]
    Api(#[from] anyhow::Error),
    
    #[error("录制器启动失败: {0}")]
    RecorderStart(String),
    
    #[error("文件操作失败: {0}")]
    File(#[from] std::io::Error),
    
    #[error("数据库操作失败: {0}")]
    Database(#[from] sea_orm::DbErr),
}