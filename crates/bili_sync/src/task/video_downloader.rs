use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tokio::time;

use crate::adapter::{
    init_collection_sources, init_favorite_sources, init_submission_sources, init_watch_later_source,
};
use crate::bilibili::{self, BiliClient};
use crate::config::{CONFIG, Config};
use crate::initialization;
use crate::workflow::process_video_source;
use crate::task::TASK_CONTROLLER;

/// 初始化所有视频源的辅助函数
async fn init_all_sources(
    config: &Config,
    connection: &DatabaseConnection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 初始化番剧源
    if let Err(e) = initialization::init_sources(config, connection).await {
        error!("初始化番剧源失败: {}", e);
        return Err(e.into());
    }

    // 初始化收藏夹源
    if let Err(e) = init_favorite_sources(connection, &config.favorite_list).await {
        error!("初始化收藏夹源失败: {:#}", e);
        return Err(e.into());
    }

    // 初始化合集源
    if let Err(e) = init_collection_sources(connection, &config.collection_list).await {
        error!("初始化合集源失败: {:#}", e);
        return Err(e.into());
    }

    // 初始化UP主投稿源
    if let Err(e) = init_submission_sources(connection, &config.submission_list).await {
        error!("初始化UP主投稿源失败: {:#}", e);
        return Err(e.into());
    }

    // 初始化稍后观看源
    if let Err(e) = init_watch_later_source(connection, &config.watch_later).await {
        error!("初始化稍后观看源失败: {:#}", e);
        return Err(e.into());
    }

    Ok(())
}

/// 启动周期下载视频的任务
pub async fn video_downloader(connection: Arc<DatabaseConnection>) {
    let bili_client = BiliClient::new(String::new());

    // 在启动时初始化所有视频源
    if let Err(e) = init_all_sources(&CONFIG, &connection).await {
        error!("启动时初始化视频源失败: {}", e);
    } else {
        info!("启动时视频源初始化成功");
    }

    loop {
        // 检查是否需要暂停扫描任务
        if TASK_CONTROLLER.is_paused() {
            debug!("定时扫描任务已暂停，等待恢复...");
            TASK_CONTROLLER.wait_if_paused().await;
            info!("定时扫描任务已恢复");
        }

        // 重新加载配置并初始化视频源
        // 注意：由于我们使用Lazy，全局CONFIG并不会自动更新
        // 但我们可以获取最新配置用于本轮任务
        let config = crate::config::reload_config();

        // 重新初始化所有视频源（确保源初始化是幂等的）
        if let Err(e) = init_all_sources(&config, &connection).await {
            error!("重新初始化视频源失败: {}", e);
            // 即使初始化失败，也继续使用现有配置进行下载
        }

        let video_sources = config.as_video_sources();

        info!("开始执行本轮视频下载任务..");

        'inner: {
            // 在开始扫描前再次检查是否暂停
            if TASK_CONTROLLER.is_paused() {
                debug!("扫描开始前检测到暂停信号，跳过本轮扫描");
                break 'inner;
            }

            match bili_client.wbi_img().await.map(|wbi_img| wbi_img.into()) {
                Ok(Some(mixin_key)) => bilibili::set_global_mixin_key(mixin_key),
                Ok(_) => {
                    error!("解析 mixin key 失败，等待下一轮执行");
                    break 'inner;
                }
                Err(e) => {
                    let error_msg = format!("{:#}", e);
                    // 检查是否是登录状态过期错误（-101错误码）
                    if error_msg.contains("status code: -101") || error_msg.contains("账号未登录") {
                        warn!("检测到登录状态过期，尝试刷新凭据...");
                        if let Err(refresh_err) = bili_client.check_refresh().await {
                            error!("刷新凭据失败：{:#}，等待下一轮执行", refresh_err);
                            break 'inner;
                        } else {
                            info!("凭据刷新成功，重新尝试获取 mixin key");
                            // 重新尝试获取 mixin key
                            match bili_client.wbi_img().await.map(|wbi_img| wbi_img.into()) {
                                Ok(Some(mixin_key)) => bilibili::set_global_mixin_key(mixin_key),
                                Ok(_) => {
                                    error!("刷新凭据后仍无法解析 mixin key，等待下一轮执行");
                                    break 'inner;
                                }
                                Err(retry_err) => {
                                    error!("刷新凭据后仍无法获取 mixin key：{:#}，等待下一轮执行", retry_err);
                                    break 'inner;
                                }
                            }
                        }
                    } else {
                        error!("获取 mixin key 遇到错误：{:#}，等待下一轮执行", e);
                        break 'inner;
                    }
                }
            };

            // 每轮任务都检查凭据是否需要刷新，而不是只在日期变化时检查
            if let Err(e) = bili_client.check_refresh().await {
                error!("检查刷新 Credential 遇到错误：{:#}，等待下一轮执行", e);
                break 'inner;
            }

            let mut processed_sources = 0;
            let mut sources_with_new_content = 0;
            for (args, path) in &video_sources {
                // 在处理每个视频源前检查是否暂停
                if TASK_CONTROLLER.is_paused() {
                    debug!("在处理视频源时检测到暂停信号，停止当前轮次扫描");
                    break;
                }

                match process_video_source(*args, &bili_client, path, &connection).await {
                    Ok(new_video_count) => {
                        processed_sources += 1;
                        if new_video_count > 0 {
                            sources_with_new_content += 1;
                        }
                    }
                    Err(e) => {
                        error!("处理过程遇到错误：{:#}", e);
                    }
                }
            }

            if processed_sources == video_sources.len() {
                if sources_with_new_content > 0 {
                    info!(
                        "本轮任务执行完毕，成功扫描 {} 个视频源，其中 {} 个源有新内容",
                        processed_sources, sources_with_new_content
                    );
                } else {
                    info!(
                        "本轮任务执行完毕，成功扫描 {} 个视频源（均无新内容）",
                        processed_sources
                    );
                }
            } else if processed_sources > 0 {
                if sources_with_new_content > 0 {
                    info!(
                        "本轮任务执行完毕，成功扫描 {} 个视频源（其中 {} 个有新内容），{} 个源处理失败",
                        processed_sources,
                        sources_with_new_content,
                        video_sources.len() - processed_sources
                    );
                } else {
                    info!(
                        "本轮任务执行完毕，成功扫描 {} 个视频源（均无新内容），{} 个源处理失败",
                        processed_sources,
                        video_sources.len() - processed_sources
                    );
                }
            } else {
                warn!("本轮任务执行完毕，所有 {} 个视频源均处理失败", video_sources.len());
            }
        }

        time::sleep(time::Duration::from_secs(config.interval)).await;
    }
}
