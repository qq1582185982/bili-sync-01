#[macro_use]
extern crate tracing;

mod adapter;
mod api;
mod aria2_downloader;
mod auth;
mod bilibili;
mod config;
mod database;
mod downloader;
mod error;
mod initialization;
mod live;
mod task;
mod unified_downloader;
mod utils;
mod workflow;

use std::fmt::Debug;
use std::future::Future;
use std::sync::Arc;

// 移除未使用的Lazy导入
use task::{http_server, live_monitor_service, video_downloader};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::config::{init_config_with_database, ARGS};
use crate::database::setup_database;
use crate::utils::signal::terminate;
use crate::utils::{file_logger, init_logger};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    init();

    let connection = Arc::new(setup_database().await);

    // 初始化数据库配置系统
    if let Err(e) = init_config_with_database(connection.as_ref().clone()).await {
        warn!("数据库配置系统初始化失败: {}, 继续使用TOML配置", e);
    }

    // 恢复待处理的任务到内存队列
    if let Err(e) = crate::task::recover_pending_tasks(connection.as_ref()).await {
        warn!("恢复待处理任务失败: {:#}", e);
    } else {
        // 立即串行处理恢复的任务
        info!("开始优先处理恢复的任务...");
        let mut total_processed = 0u32;

        // 1. 处理配置任务（优先级最高，影响其他任务）
        match crate::task::process_config_tasks(connection.clone()).await {
            Ok(count) => {
                total_processed += count;
                if count > 0 {
                    info!("处理了 {} 个恢复的配置任务", count);
                }
            }
            Err(e) => warn!("处理恢复的配置任务失败: {:#}", e),
        }

        // 2. 处理添加任务
        match crate::task::process_add_tasks(connection.clone()).await {
            Ok(count) => {
                total_processed += count;
                if count > 0 {
                    info!("处理了 {} 个恢复的添加任务", count);
                }
            }
            Err(e) => warn!("处理恢复的添加任务失败: {:#}", e),
        }

        // 3. 处理删除任务
        match crate::task::process_delete_tasks(connection.clone()).await {
            Ok(count) => {
                total_processed += count;
                if count > 0 {
                    info!("处理了 {} 个恢复的删除任务", count);
                }
            }
            Err(e) => warn!("处理恢复的删除任务失败: {:#}", e),
        }

        // 4. 处理视频删除任务
        match crate::task::process_video_delete_tasks(connection.clone()).await {
            Ok(count) => {
                total_processed += count;
                if count > 0 {
                    info!("处理了 {} 个恢复的视频删除任务", count);
                }
            }
            Err(e) => warn!("处理恢复的视频删除任务失败: {:#}", e),
        }

        if total_processed > 0 {
            info!("恢复的任务处理完成，共处理 {} 个任务", total_processed);
        } else {
            info!("没有需要处理的恢复任务");
        }
    }

    // SQLite配置已经在database::setup_database中设置了mmap，不再需要额外的初始化

    let token = CancellationToken::new();
    let tracker = TaskTracker::new();

    spawn_task("HTTP 服务", http_server(connection.clone()), &tracker, token.clone());
    spawn_task("定时下载", video_downloader(connection.clone()), &tracker, token.clone());
    spawn_task("直播监控", live_monitor_service(connection), &tracker, token.clone());

    tracker.close();
    handle_shutdown(tracker, token).await;
    Ok(())
}

fn spawn_task(
    task_name: &'static str,
    task: impl Future<Output = impl Debug> + Send + 'static,
    tracker: &TaskTracker,
    token: CancellationToken,
) {
    tracker.spawn(async move {
        tokio::select! {
            res = task => {
                error!("「{}」异常结束，返回结果为：「{:?}」，取消其它仍在执行的任务..", task_name, res);
                token.cancel();
            },
            _ = token.cancelled() => {
                info!("「{}」接收到取消信号，终止运行..", task_name);
            }
        }
    });
}

/// 初始化日志系统，打印欢迎信息
fn init() {
    // 强制初始化文件日志系统（这会设置启动时间）
    let _ = &*file_logger::STARTUP_TIME;
    let _ = &*file_logger::FILE_LOG_WRITER;

    init_logger(&ARGS.log_level);
    info!("欢迎使用 Bili-Sync，当前程序版本：{}", config::version());
    info!("现项目地址：https://github.com/qq1582185982/bili-sync-01");
    info!("原项目地址：https://github.com/amtoaer/bili-sync");
    debug!("系统初始化完成，日志级别: {}", ARGS.log_level);
    // 移除配置文件强制加载 - 配置现在完全基于数据库
    // debug!("开始加载配置文件...");
    // Lazy::force(&CONFIG);
    // debug!("配置文件加载完成");
}

async fn handle_shutdown(tracker: TaskTracker, token: CancellationToken) {
    tokio::select! {
        _ = tracker.wait() => {
            error!("所有任务均已终止，程序退出");
            finalize_global_systems().await;
        }
        _ = terminate() => {
            info!("接收到终止信号，正在终止任务..");
            // 立即刷新日志以确保不丢失
            file_logger::flush_file_logger();
            token.cancel();
            tracker.wait().await;
            info!("所有任务均已终止，程序退出");
            finalize_global_systems().await;
        }
    }
}

/// 完成全局系统清理
async fn finalize_global_systems() {
    // SQLite会自动处理mmap的清理，不需要额外的finalize操作

    // 关闭文件日志系统
    file_logger::shutdown_file_logger();
}
