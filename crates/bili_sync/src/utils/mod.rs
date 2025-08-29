pub mod bangumi_cache;
pub mod bangumi_name_extractor;
pub mod convert;
pub mod file_logger;
pub mod filenamify;
pub mod format_arg;
pub mod model;
pub mod nfo;
pub mod notification;
pub mod scan_collector;
pub mod scan_id_tracker;
pub mod signal;
pub mod status;
pub mod task_notifier;
pub mod time_format;

use std::fmt;
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;
use chrono::Local;

// 自定义控制台输出层，过滤直播日志
struct ConsoleLayer;

impl ConsoleLayer {
    fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for ConsoleLayer
where
    S: Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        // 如果是直播日志，直接跳过，不输出到控制台
        if event.metadata().target().starts_with("bili_sync::live") {
            return;
        }
        
        // 获取日志级别
        let level = event.metadata().level();
        
        // 提取日志消息
        let mut visitor = MessageVisitor::new();
        event.record(&mut visitor);
        
        if let Some(message) = visitor.message {
            // 获取当前时间
            let timestamp = Local::now().format("%b %d %H:%M:%S");
            
            // 根据日志级别设置颜色
            let (color_code, level_str) = match *level {
                tracing::Level::ERROR => ("\x1b[31m", "ERROR"), // 红色
                tracing::Level::WARN => ("\x1b[33m", " WARN"), // 黄色
                tracing::Level::INFO => ("\x1b[32m", " INFO"), // 绿色
                tracing::Level::DEBUG => ("\x1b[36m", "DEBUG"), // 青色
                tracing::Level::TRACE => ("\x1b[35m", "TRACE"), // 紫色
            };
            
            // 格式化并输出到控制台（带颜色）
            // 时间戳使用灰色（dim），日志级别使用各自的颜色
            println!("\x1b[2m{}\x1b[0m {}{:>5}\x1b[0m {}", timestamp, color_code, level_str, message);
        }
    }
}

// 自定义日志层，用于将日志添加到API缓冲区
struct LogCaptureLayer;

impl<S> Layer<S> for LogCaptureLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        use crate::api::handler::{add_log_entry, LogLevel};
        use crate::utils::time_format::now_standard_string;

        // 判断是否为直播相关日志
        let is_live_log = event.metadata().target().starts_with("bili_sync::live");
        
        let level = if is_live_log {
            LogLevel::Live
        } else {
            match *event.metadata().level() {
                tracing::Level::ERROR => LogLevel::Error,
                tracing::Level::WARN => LogLevel::Warn,
                tracing::Level::INFO => LogLevel::Info,
                tracing::Level::DEBUG => LogLevel::Debug,
                tracing::Level::TRACE => LogLevel::Debug, // 将TRACE映射到DEBUG
            }
        };

        let level_str = if is_live_log {
            "live"  // 对直播日志使用"live"级别字符串
        } else {
            match *event.metadata().level() {
                tracing::Level::ERROR => "error",
                tracing::Level::WARN => "warn",
                tracing::Level::INFO => "info",
                tracing::Level::DEBUG => "debug",
                tracing::Level::TRACE => "debug",
            }
        };

        // 提取日志消息
        let mut visitor = MessageVisitor::new();
        event.record(&mut visitor);

        if let Some(message) = visitor.message {
            let target = event.metadata().target().to_string();

            // 写入文件日志
            if let Some(ref writer) = *file_logger::FILE_LOG_WRITER {
                writer.write_log(&now_standard_string(), level_str, &message, Some(&target));
            }

            // 添加到内存缓冲区
            add_log_entry(level, message, Some(target));
        }
    }
}

// 用于提取日志消息的访问者
struct MessageVisitor {
    message: Option<String>,
}

impl MessageVisitor {
    fn new() -> Self {
        Self { message: None }
    }
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }
}

pub fn init_logger(log_level: &str) {
    // 构建优化的日志过滤器，降低sqlx慢查询等噪音
    let console_filter = build_console_filter(log_level);  // 控制台过滤器，排除直播日志
    let api_filter = build_api_filter("debug");            // API过滤器，包含所有日志

    // 自定义控制台输出层 - 直接在层内过滤直播日志
    let console_layer = ConsoleLayer::new().with_filter(console_filter);

    // API日志捕获层 - 使用API过滤器
    let log_capture_layer = LogCaptureLayer.with_filter(api_filter);

    tracing_subscriber::registry()
        .with(console_layer)
        .with(log_capture_layer)
        .try_init()
        .expect("初始化日志失败");
}

/// 构建控制台日志过滤器，排除直播日志以减少输出
fn build_console_filter(base_level: &str) -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::builder().parse_lossy(format!(
        "bili_sync::live=off,\
            {},\
            sqlx::query=error,\
            sqlx=error,\
            sea_orm::database=error,\
            sea_orm_migration=warn,\
            tokio_util=warn,\
            hyper=warn,\
            reqwest=warn,\
            h2=warn",
        base_level
    ))
}

/// 构建API日志过滤器，包含所有日志用于文件和前端显示
fn build_api_filter(base_level: &str) -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::builder().parse_lossy(format!(
        "{},\
            sqlx::query=error,\
            sqlx=error,\
            sea_orm::database=error,\
            sea_orm_migration=warn,\
            tokio_util=warn,\
            hyper=warn,\
            reqwest=warn,\
            h2=warn",
        base_level
    ))
}
