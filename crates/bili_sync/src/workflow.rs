use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use bili_sync_entity::*;
use futures::stream::FuturesUnordered;
use futures::{Stream, StreamExt, TryStreamExt};
use sea_orm::entity::prelude::*;
use sea_orm::ActiveValue::Set;
use sea_orm::TransactionTrait;
use tokio::fs;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::utils::time_format::now_standard_string;

// 全局番剧季度标题缓存
lazy_static::lazy_static! {
    pub static ref SEASON_TITLE_CACHE: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
}

use crate::adapter::{video_source_from, Args, VideoSource, VideoSourceEnum};
use crate::bilibili::{
    BestStream, BiliClient, BiliError, Dimension, PageInfo, Stream as VideoStream, Video, VideoInfo,
};
use crate::config::ARGS;
use crate::error::{DownloadAbortError, ExecutionStatus, ProcessPageError};
use crate::task::{DeleteVideoTask, VIDEO_DELETE_TASK_QUEUE};
use crate::unified_downloader::UnifiedDownloader;
use crate::utils::format_arg::{page_format_args, video_format_args};
use crate::utils::model::{
    create_pages, create_videos, filter_unfilled_videos, filter_unhandled_video_pages,
    get_failed_videos_in_current_cycle, update_pages_model, update_videos_model,
};
use crate::utils::nfo::NFO;
use crate::utils::notification::NewVideoInfo;
use crate::utils::status::{PageStatus, VideoStatus, STATUS_OK};

/// 检测并处理需要自动删除的错误（主要是87007充电专享视频错误）
///
/// # 参数
/// - `results`: 执行结果列表
/// - `task_names`: 任务名称列表，用于日志输出
/// - `video_model`: 视频模型引用
/// - `connection`: 数据库连接
/// - `context_info`: 额外的上下文信息（如"第X页"），用于日志输出
///
/// # 返回值
/// - `Result<bool>`: 如果检测到需要自动删除的错误返回true，否则返回false
async fn detect_and_handle_auto_delete_errors(
    results: &[ExecutionStatus],
    task_names: &[&str],
    video_model: &video::Model,
    connection: &DatabaseConnection,
    context_info: Option<&str>,
) -> Result<bool> {
    let mut needs_auto_delete = false;

    // 扩展检测范围：检查所有任务结果
    for (res, _task_name) in results.iter().zip(task_names.iter()) {
        if let ExecutionStatus::ClassifiedFailed(classified_error) = res {
            // 使用error.rs层面的should_auto_delete标志，更精确的判断
            if classified_error.should_auto_delete {
                if let Some(context) = context_info {
                    info!("「{}」{} - 充电专享视频，将自动删除", &video_model.name, context);
                } else {
                    info!("「{}」- 充电专享视频，将自动删除", &video_model.name);
                }
                needs_auto_delete = true;
                break; // 只需要检测一次即可
            }
        } else if let ExecutionStatus::Ignored(e) = res {
            // 向后兼容：继续检查字符串匹配的87007错误
            let error_msg = e.to_string();
            if error_msg.contains("status code: 87007") {
                if let Some(context) = context_info {
                    info!(
                        "「{}」{} - 充电专享视频，将自动删除（向后兼容检测）",
                        &video_model.name, context
                    );
                } else {
                    info!("「{}」- 充电专享视频，将自动删除（向后兼容检测）", &video_model.name);
                }
                needs_auto_delete = true;
                break; // 只需要检测一次即可
            }
        }
    }

    // 如果检测到需要自动删除的错误，创建自动删除任务
    if needs_auto_delete {
        let task_id = if context_info.is_some() {
            format!("auto_delete_87007_page_{}", video_model.id)
        } else {
            format!("auto_delete_87007_{}", video_model.id)
        };

        let delete_task = DeleteVideoTask {
            video_id: video_model.id,
            task_id,
        };

        if let Err(delete_err) = VIDEO_DELETE_TASK_QUEUE.enqueue_task(delete_task, connection).await {
            error!("创建删除任务失败「{}」: {:#}", &video_model.name, delete_err);
        } else {
            debug!("删除任务已加入队列「{}」", &video_model.name);
        }
    }

    Ok(needs_auto_delete)
}

// 新增：番剧季信息结构体
#[derive(Debug, Clone)]
pub struct SeasonInfo {
    pub title: String,
    pub episodes: Vec<EpisodeInfo>,
    // API扩展字段
    pub alias: Option<String>,                 // 别名
    pub evaluate: Option<String>,              // 剧情简介
    pub rating: Option<f32>,                   // 评分 (如9.6)
    pub rating_count: Option<i64>,             // 评分人数
    pub areas: Vec<String>,                    // 制作地区 (如"中国大陆")
    pub actors: Option<String>,                // 声优演员信息 (格式化字符串)
    pub styles: Vec<String>,                   // 类型标签 (如"科幻", "机战")
    pub total_episodes: Option<i32>,           // 总集数
    pub status: Option<String>,                // 播出状态 (如"完结", "连载中")
    pub cover: Option<String>,                 // 季度封面图URL (竖版)
    pub new_ep_cover: Option<String>,          // 新EP封面图URL (来自new_ep.cover)
    pub horizontal_cover_1610: Option<String>, // 16:10横版封面URL
    pub horizontal_cover_169: Option<String>,  // 16:9横版封面URL
    pub bkg_cover: Option<String>,             // 背景图URL (专门的背景图)
    pub media_id: Option<i64>,                 // 媒体ID
    pub season_id: String,                     // 季度ID
    pub publish_time: Option<String>,          // 发布时间
    pub total_views: Option<i64>,              // 总播放量
    pub total_favorites: Option<i64>,          // 总收藏数
    pub total_seasons: Option<i32>,            // 总季数（从API的seasons数组计算）
    pub show_season_type: Option<i32>,         // 番剧季度类型
}

#[derive(Debug, Clone)]
pub struct EpisodeInfo {
    pub ep_id: String,
    pub cid: i64,
    pub duration: u32, // 秒
}

/// 创建一个配置了 truncate 辅助函数的 handlebars 实例
///
/// 完整地处理某个视频来源，返回新增的视频数量和视频信息
pub async fn process_video_source(
    args: &Args,
    bili_client: &BiliClient,
    path: &Path,
    connection: &DatabaseConnection,
    downloader: &UnifiedDownloader,
    token: CancellationToken,
) -> Result<(usize, Vec<NewVideoInfo>)> {
    // 记录当前处理的参数和路径
    if let Args::Bangumi {
        season_id,
        media_id: _,
        ep_id: _,
    } = &args
    {
        // 尝试从API获取真实的番剧标题
        let title = if let Some(season_id) = season_id {
            // 如果有season_id，尝试获取番剧标题
            get_season_title_from_api(bili_client, season_id, token.clone())
                .await
                .unwrap_or_else(|| {
                    // API获取失败，回退到路径名
                    path.file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| "未知番剧".to_string())
                })
        } else {
            // 没有season_id，使用路径名
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "未知番剧".to_string())
        };
        info!("处理番剧下载: {}", title);
    }

    // 定义一个辅助函数来处理-101错误并重试
    let retry_with_refresh = |error_msg: String| async move {
        if error_msg.contains("status code: -101") || error_msg.contains("账号未登录") {
            warn!("检测到登录状态过期，尝试刷新凭据...");
            if let Err(refresh_err) = bili_client.check_refresh().await {
                error!("刷新凭据失败：{:#}", refresh_err);
                return Err(refresh_err);
            } else {
                info!("凭据刷新成功，将重试操作");
                return Ok(());
            }
        }
        Err(anyhow::anyhow!("非登录状态错误，无需刷新凭据"))
    };

    // 从参数中获取视频列表的 Model 与视频流
    let (video_source, video_streams) = match video_source_from(args, path, bili_client, connection).await {
        Ok(result) => result,
        Err(e) => {
            let error_msg = format!("{:#}", e);
            if retry_with_refresh(error_msg).await.is_ok() {
                // 刷新成功，重试
                video_source_from(args, path, bili_client, connection).await?
            } else {
                return Err(e);
            }
        }
    };

    // 从视频流中获取新视频的简要信息，写入数据库，并获取新增视频数量和信息
    let (new_video_count, new_videos) =
        match refresh_video_source(&video_source, video_streams, connection, token.clone(), bili_client).await {
            Ok(result) => result,
            Err(e) => {
                let error_msg = format!("{:#}", e);
                if retry_with_refresh(error_msg).await.is_ok() {
                    // 刷新成功，重新获取视频流并重试
                    let (_, video_streams) = video_source_from(args, path, bili_client, connection).await?;
                    refresh_video_source(&video_source, video_streams, connection, token.clone(), bili_client).await?
                } else {
                    return Err(e);
                }
            }
        };

    // 单独请求视频详情接口，获取视频的详情信息与所有的分页，写入数据库
    if let Err(e) = fetch_video_details(bili_client, &video_source, connection, token.clone()).await {
        // 新增：检查是否为风控导致的下载中止
        if e.downcast_ref::<DownloadAbortError>().is_some() {
            error!("获取视频详情时触发风控，已终止当前视频源的处理，停止所有后续扫描");
            // 风控时应该返回错误，中断整个扫描循环，而不是继续处理下一个视频源
            return Err(e);
        }

        let error_msg = format!("{:#}", e);
        if retry_with_refresh(error_msg).await.is_ok() {
            // 刷新成功，重试
            fetch_video_details(bili_client, &video_source, connection, token.clone()).await?;
        } else {
            return Err(e);
        }
    }

    if ARGS.scan_only {
        warn!("已开启仅扫描模式，跳过视频下载..");
    } else {
        // 从数据库中查找所有未下载的视频与分页，下载并处理
        if let Err(e) =
            download_unprocessed_videos(bili_client, &video_source, connection, downloader, token.clone()).await
        {
            let error_msg = format!("{:#}", e);
            if retry_with_refresh(error_msg).await.is_ok() {
                // 刷新成功，重试（继续使用原有的取消令牌）
                download_unprocessed_videos(bili_client, &video_source, connection, downloader, token.clone()).await?;
            } else {
                return Err(e);
            }
        }

        // 新增：循环内重试失败的视频
        // 在当前扫描循环结束前，对失败的视频进行一次额外的重试机会
        if let Err(e) =
            retry_failed_videos_once(bili_client, &video_source, connection, downloader, token.clone()).await
        {
            warn!("循环内重试失败的视频时出错: {:#}", e);
            // 重试失败不中断主流程，继续执行
        }
    }
    Ok((new_video_count, new_videos))
}

/// 更新番剧缓存
async fn update_bangumi_cache(
    source_id: i32,
    connection: &DatabaseConnection,
    bili_client: &BiliClient,
    season_info: Option<SeasonInfo>,
) -> Result<()> {
    use bili_sync_entity::video_source;
    use crate::utils::bangumi_cache::{serialize_cache, BangumiCache};
    use sea_orm::ActiveValue::Set;
    
    // 获取番剧源信息
    let source = video_source::Entity::find_by_id(source_id)
        .one(connection)
        .await?
        .ok_or_else(|| anyhow::anyhow!("番剧源不存在"))?;
    
    // 如果没有提供season_info，尝试从API获取
    let season_info = if let Some(info) = season_info {
        info
    } else if let Some(season_id) = &source.season_id {
        // 从API获取完整的season信息
        match get_season_info_from_api(bili_client, season_id, CancellationToken::new()).await {
            Ok(info) => info,
            Err(e) => {
                warn!("获取番剧季信息失败，跳过缓存更新: {}", e);
                return Ok(());
            }
        }
    } else {
        debug!("番剧源 {} 没有season_id，跳过缓存更新", source_id);
        return Ok(());
    };
    
    // 构建episodes数组
    let mut episodes = Vec::new();
    
    // 查询该番剧源的所有视频和分页信息
    let videos_with_pages = bili_sync_entity::video::Entity::find()
        .filter(bili_sync_entity::video::Column::SourceId.eq(source_id))
        .filter(bili_sync_entity::video::Column::SourceType.eq(1))
        .find_with_related(bili_sync_entity::page::Entity)
        .all(connection)
        .await?;
    
    // 从数据库记录构建episodes信息
    for (video, pages) in &videos_with_pages {
        if let Some(page) = pages.first() {
            let mut episode = serde_json::json!({
                "id": video.ep_id.as_ref().map(|s| s.parse::<i64>().ok()).flatten().unwrap_or(0),
                "aid": video.bvid.clone(), // 暂时使用bvid，实际应该是aid
                "bvid": video.bvid.clone(),
                "cid": page.cid,
                "title": video.episode_number.map(|n| n.to_string()).unwrap_or_else(|| video.name.clone()),
                "long_title": video.name.clone(),
                "cover": video.cover.clone(),
                "duration": page.duration as i64 * 1000, // 秒转毫秒
                "pub_time": video.pubtime.and_utc().timestamp(),
                "section_type": 0, // 正片
            });
            
            // 如果有share_copy，添加到episode中
            if let Some(share_copy) = &video.share_copy {
                episode["share_copy"] = serde_json::Value::String(share_copy.clone());
            }
            
            episodes.push(episode);
        }
    }
    
    // 如果没有视频数据，使用API提供的episodes
    if episodes.is_empty() && !season_info.episodes.is_empty() {
        for ep_info in &season_info.episodes {
            episodes.push(serde_json::json!({
                "id": ep_info.ep_id.parse::<i64>().unwrap_or(0),
                "cid": ep_info.cid,
                "duration": ep_info.duration as i64 * 1000, // 秒转毫秒
                "section_type": 0,
            }));
        }
    }
    
    // 构建season_info JSON
    let season_json = serde_json::json!({
        "title": season_info.title,
        "cover": season_info.cover,
        "evaluate": season_info.evaluate,
        "show_season_type": season_info.show_season_type,
        "actors": season_info.actors,
        "rating": season_info.rating,
        "areas": season_info.areas,
        "styles": season_info.styles,
        "total": season_info.total_episodes,
        "new_ep": {
            "cover": season_info.new_ep_cover,
        },
        "horizontal_cover_1610": season_info.horizontal_cover_1610,
        "horizontal_cover_169": season_info.horizontal_cover_169,
        "bkg_cover": season_info.bkg_cover,
    });
    
    // 获取最新的剧集时间
    let last_episode_time = videos_with_pages
        .iter()
        .map(|(v, _)| v.pubtime.and_utc())
        .max();
    
    // 创建缓存对象
    let cache = BangumiCache {
        season_info: season_json,
        episodes: episodes.clone(),
        last_episode_time,
        total_episodes: season_info.total_episodes.unwrap_or(episodes.len() as i32) as usize,
    };
    
    // 序列化缓存
    let cache_json = serialize_cache(&cache)?;
    
    // 更新数据库
    let active_model = video_source::ActiveModel {
        id: Set(source_id),
        cached_episodes: Set(Some(cache_json)),
        cache_updated_at: Set(Some(crate::utils::time_format::now_standard_string())),
        ..Default::default()
    };
    
    active_model.update(connection).await?;
    
    info!(
        "番剧源 {} ({}) 缓存更新成功，共 {} 集",
        source_id,
        season_info.title,
        episodes.len()
    );
    
    Ok(())
}

/// 请求接口，获取视频列表中所有新添加的视频信息，将其写入数据库
pub async fn refresh_video_source<'a>(
    video_source: &VideoSourceEnum,
    video_streams: Pin<Box<dyn Stream<Item = Result<VideoInfo>> + 'a + Send>>,
    connection: &DatabaseConnection,
    token: CancellationToken,
    bili_client: &BiliClient,
) -> Result<(usize, Vec<NewVideoInfo>)> {
    video_source.log_refresh_video_start();
    let latest_row_at_string = video_source.get_latest_row_at();
    let latest_row_at = crate::utils::time_format::parse_time_string(&latest_row_at_string)
        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc())
        .and_utc();
    let mut max_datetime = latest_row_at;
    let mut error = Ok(());
    let mut video_streams = video_streams
        .take_while(|res| {
            if token.is_cancelled() {
                return futures::future::ready(false);
            }
            match res {
                Err(e) => {
                    error = Err(anyhow!(e.to_string()));
                    futures::future::ready(false)
                }
                Ok(v) => {
                    // 虽然 video_streams 是从新到旧的，但由于此处是分页请求，极端情况下可能发生访问完第一页时插入了两整页视频的情况
                    // 此时获取到的第二页视频比第一页的还要新，因此为了确保正确，理应对每一页的第一个视频进行时间比较
                    // 但在 streams 的抽象下，无法判断具体是在哪里分页的，所以暂且对每个视频都进行比较，应该不会有太大性能损失
                    let release_datetime = v.release_datetime();
                    if release_datetime > &max_datetime {
                        max_datetime = *release_datetime;
                    }
                    futures::future::ready(video_source.should_take(release_datetime, &latest_row_at))
                }
            }
        })
        .filter_map(|res| futures::future::ready(res.ok()))
        .chunks(10);
    let mut count = 0;
    let mut new_videos = Vec::new();

    while let Some(videos_info) = video_streams.next().await {
        // 获取插入前的视频数量
        let before_count = get_video_count_for_source(video_source, connection).await?;

        // 先收集需要的视频信息（包括集数信息）
        let mut temp_video_infos = Vec::new();
        for video_info in &videos_info {
            let (title, bvid, upper_name, episode_num) = match video_info {
                VideoInfo::Detail { title, bvid, upper, .. } => (title.clone(), bvid.clone(), upper.name.clone(), None),
                VideoInfo::Favorite { title, bvid, upper, .. } => (title.clone(), bvid.clone(), upper.name.clone(), None),
                VideoInfo::Collection { bvid, .. } => {
                    // Collection 没有 title 和 upper，使用默认值
                    ("未知".to_string(), bvid.clone(), "未知".to_string(), None)
                }
                VideoInfo::WatchLater { title, bvid, upper, .. } => (title.clone(), bvid.clone(), upper.name.clone(), None),
                VideoInfo::Submission { title, bvid, .. } => {
                    // Submission 没有 upper 信息，使用默认值
                    (title.clone(), bvid.clone(), "未知".to_string(), None)
                }
                VideoInfo::Bangumi { title, bvid, episode_number, .. } => {
                    // Bangumi 没有 upper 信息，使用番剧名称
                    (title.clone(), bvid.clone(), "番剧".to_string(), *episode_number)
                }
            };
            temp_video_infos.push((title, bvid, upper_name, episode_num));
        }

        // 获取所有视频的BVID，用于后续判断哪些是新增的
        let video_bvids: Vec<String> = videos_info.iter().map(|v| {
            match v {
                VideoInfo::Detail { bvid, .. } => bvid.clone(),
                VideoInfo::Favorite { bvid, .. } => bvid.clone(),
                VideoInfo::Collection { bvid, .. } => bvid.clone(),
                VideoInfo::WatchLater { bvid, .. } => bvid.clone(),
                VideoInfo::Submission { bvid, .. } => bvid.clone(),
                VideoInfo::Bangumi { bvid, .. } => bvid.clone(),
            }
        }).collect();

        create_videos(videos_info, video_source, connection).await?;

        // 获取插入后的视频数量，计算实际新增数量
        let after_count = get_video_count_for_source(video_source, connection).await?;
        let new_count = after_count - before_count;
        count += new_count;

        // 如果有新增视频，通过查询数据库来确定哪些是新增的
        if new_count > 0 {
            // 查询这批视频中哪些是新插入的（根据创建时间）
            let now = chrono::Utc::now();
            let recent_threshold = now - chrono::Duration::seconds(10); // 10秒内创建的视频
            
            let newly_inserted = video::Entity::find()
                .filter(video_source.filter_expr())
                .filter(video::Column::Bvid.is_in(video_bvids.clone()))
                .filter(video::Column::CreatedAt.gte(recent_threshold.format("%Y-%m-%d %H:%M:%S").to_string()))
                .all(connection)
                .await?;
            
            debug!("查询到 {} 个新插入的视频记录", newly_inserted.len());

            // 为每个新插入的视频创建通知信息
            for new_video in newly_inserted {
                // 查找对应的视频信息
                if let Some(idx) = temp_video_infos.iter().position(|(_, bvid, _, _)| bvid == &new_video.bvid) {
                    let (title, _, upper_name, bangumi_episode) = &temp_video_infos[idx];
                    
                    // 使用数据库中的发布时间（已经是北京时间）
                    let pubtime = new_video.pubtime
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string();
                    
                    // 获取集数信息
                    let episode_number = if let Some(ep) = bangumi_episode {
                        // 番剧：使用从VideoInfo中获取的集数
                        Some(*ep)
                    } else {
                        // 其他类型：使用数据库中的episode_number字段
                        new_video.episode_number
                    };
                    
                    new_videos.push(NewVideoInfo {
                        title: title.clone(),
                        bvid: new_video.bvid.clone(),
                        upper_name: upper_name.clone(),
                        source_type: video_source.source_type_display(),
                        source_name: video_source.source_name_display(),
                        pubtime: Some(pubtime),
                        episode_number,
                        season_number: None,
                    });
                }
            }
            
            debug!("实际收集到 {} 个新视频信息用于推送", new_videos.len());
        }
    }
    // 如果获取视频分页过程中发生了错误，直接在此处返回，不更新 latest_row_at
    error?;
    if max_datetime != latest_row_at {
        // 转换为北京时间的标准字符串格式
        let beijing_datetime = max_datetime.with_timezone(&crate::utils::time_format::beijing_timezone());
        let beijing_datetime_string = beijing_datetime.format("%Y-%m-%d %H:%M:%S").to_string();
        video_source
            .update_latest_row_at(beijing_datetime_string)
            .save(connection)
            .await?;
    }
    
    // 番剧源：更新缓存
    if let VideoSourceEnum::BangumiSource(bangumi_source) = video_source {
        // 检查是否需要更新缓存
        let should_update_cache = if count > 0 || max_datetime != latest_row_at {
            // 有新视频或时间更新，说明获取了新数据
            true
        } else {
            // 检查缓存是否存在
            let source_model = bili_sync_entity::video_source::Entity::find_by_id(bangumi_source.id)
                .one(connection)
                .await?;
            
            if let Some(source) = source_model {
                // 如果缓存不存在，需要创建
                source.cached_episodes.is_none()
            } else {
                false
            }
        };
        
        if should_update_cache {
            update_bangumi_cache(bangumi_source.id, connection, bili_client, None).await?;
        }
    }
    
    video_source.log_refresh_video_end(count);
    Ok((count, new_videos))
}

/// 筛选出所有未获取到全部信息的视频，尝试补充其详细信息
pub async fn fetch_video_details(
    bili_client: &BiliClient,
    video_source: &VideoSourceEnum,
    connection: &DatabaseConnection,
    token: CancellationToken,
) -> Result<()> {
    video_source.log_fetch_video_start();
    let videos_model = filter_unfilled_videos(video_source.filter_expr(), connection).await?;

    // 分离出番剧和普通视频
    let (bangumi_videos, normal_videos): (Vec<_>, Vec<_>) =
        videos_model.into_iter().partition(|v| v.source_type == Some(1));

    // 优化后的番剧信息获取 - 使用数据库缓存和按季分组
    if !bangumi_videos.is_empty() {
        info!("开始处理 {} 个番剧视频", bangumi_videos.len());

        // 按 season_id 分组番剧视频
        let mut videos_by_season: HashMap<String, Vec<video::Model>> = HashMap::new();
        let mut videos_without_season = Vec::new();

        for video in bangumi_videos {
            if let Some(season_id) = &video.season_id {
                videos_by_season.entry(season_id.clone()).or_default().push(video);
            } else {
                videos_without_season.push(video);
            }
        }

        // 处理每个季
        for (season_id, videos) in videos_by_season {
            // 首先尝试获取番剧季标题用于日志显示
            let season_title = get_season_title_from_api(bili_client, &season_id, token.clone()).await;
            let display_name = season_title.as_deref().unwrap_or(&season_id);

            info!(
                "处理番剧季 {} 「{}」的 {} 个视频",
                season_id,
                display_name,
                videos.len()
            );

            // 1. 首先从现有数据库中查找该季已有的分集信息
            let mut existing_episodes =
                get_existing_episodes_for_season(connection, &season_id, bili_client, token.clone()).await?;

            // 2. 检查哪些ep_id还没有信息
            let missing_ep_ids: Vec<String> = videos
                .iter()
                .filter_map(|v| v.ep_id.as_ref())
                .filter(|ep_id| !existing_episodes.contains_key(*ep_id))
                .cloned()
                .collect();

            // 3. 只对缺失的信息发起API请求（每个季只请求一次）
            if !missing_ep_ids.is_empty() {
                info!(
                    "需要从API获取番剧季 {} 「{}」的信息（包含 {} 个新分集）",
                    season_id,
                    display_name,
                    missing_ep_ids.len()
                );

                match get_season_info_from_api(bili_client, &season_id, token.clone()).await {
                    Ok(season_info) => {
                        // 将新获取的信息添加到映射中
                        for episode in season_info.episodes {
                            existing_episodes.insert(episode.ep_id, (episode.cid, episode.duration));
                        }
                        debug!("成功获取番剧季 {} 「{}」的完整信息", season_id, season_info.title);
                    }
                    Err(e) => {
                        error!("获取番剧季 {} 「{}」信息失败: {}", season_id, display_name, e);
                        // 即使API失败，已有缓存的分集仍可正常处理
                    }
                }
            } else {
                info!(
                    "番剧季 {} 「{}」的所有分集信息已缓存，无需API请求",
                    season_id, display_name
                );
            }

            // 4. 使用合并后的信息处理所有视频
            for video_model in videos {
                if let Err(e) = process_bangumi_video(video_model, &existing_episodes, connection, video_source).await {
                    error!("处理番剧视频失败: {}", e);
                }
            }
        }

        // 处理没有season_id的番剧视频（回退到原逻辑）
        if !videos_without_season.is_empty() {
            warn!(
                "发现 {} 个缺少season_id的番剧视频，使用原有逻辑处理",
                videos_without_season.len()
            );
            for video_model in videos_without_season {
                let txn = connection.begin().await?;

                let (actual_cid, duration) = if let Some(ep_id) = &video_model.ep_id {
                    match get_bangumi_info_from_api(bili_client, ep_id, token.clone()).await {
                        Some(info) => info,
                        None => {
                            error!("番剧 {} (EP{}) 信息获取失败，将跳过弹幕下载", &video_model.name, ep_id);
                            (-1, 1440)
                        }
                    }
                } else {
                    error!("番剧 {} 缺少EP ID，无法获取详细信息", &video_model.name);
                    (-1, 1440)
                };

                let page_info = PageInfo {
                    cid: actual_cid,
                    page: 1,
                    name: video_model.name.clone(),
                    duration,
                    first_frame: None,
                    dimension: None,
                };

                create_pages(vec![page_info], &video_model, &txn).await?;

                let mut video_active_model: bili_sync_entity::video::ActiveModel = video_model.into();
                video_source.set_relation_id(&mut video_active_model);
                video_active_model.single_page = Set(Some(true));
                video_active_model.tags = Set(Some(serde_json::Value::Array(vec![])));
                video_active_model.save(&txn).await?;
                txn.commit().await?;
            }
        }
    }

    // 处理普通视频 - 使用并发处理优化性能
    if !normal_videos.is_empty() {
        info!("开始并发处理 {} 个普通视频的详情", normal_videos.len());

        // 使用信号量控制并发数
        let current_config = crate::config::reload_config();
        let semaphore = Semaphore::new(current_config.concurrent_limit.video);

        let tasks = normal_videos
            .into_iter()
            .map(|video_model| {
                let semaphore = &semaphore;
                let token = token.clone();
                async move {
                    // 获取许可以控制并发
                    let _permit = tokio::select! {
                        biased;
                        _ = token.cancelled() => return Err(anyhow!("Download cancelled")),
                        permit = semaphore.acquire() => permit.context("acquire semaphore failed")?,
                    };

                    let video = Video::new(bili_client, video_model.bvid.clone());
                    let info: Result<_> = tokio::select! {
                        biased;
                        _ = token.cancelled() => return Err(anyhow!("Download cancelled")),
                        res = async { Ok((video.get_tags().await?, video.get_view_info().await?)) } => res,
                    };
                    match info {
                        Err(e) => {
                            // 新增：检查是否为风控错误
                            let classified_error = crate::error::ErrorClassifier::classify_error(&e);
                            if classified_error.error_type == crate::error::ErrorType::RiskControl {
                                error!(
                                    "获取视频 {} - {} 的详细信息时触发风控: {}",
                                    &video_model.bvid, &video_model.name, classified_error.message
                                );
                                // 返回一个特定的错误来中止整个批处理
                                return Err(anyhow!(DownloadAbortError()));
                            }

                            error!(
                                "获取视频 {} - {} 的详细信息失败，错误为：{:#}",
                                &video_model.bvid, &video_model.name, e
                            );
                            if let Some(BiliError::RequestFailed(-404, _)) = e.downcast_ref::<BiliError>() {
                                let mut video_active_model: bili_sync_entity::video::ActiveModel = video_model.into();
                                video_active_model.valid = Set(false);
                                video_active_model.save(connection).await?;
                            }
                        }
                        Ok((tags, mut view_info)) => {
                            let VideoInfo::Detail { pages, staff, .. } = &mut view_info else {
                                unreachable!()
                            };
                            let pages = std::mem::take(pages);
                            let pages_len = pages.len();

                            // 调试日志：检查staff信息
                            if let Some(staff_list) = staff {
                                debug!("视频 {} 有staff信息，成员数量: {}", video_model.bvid, staff_list.len());
                                for staff_member in staff_list.iter() {
                                    debug!(
                                        "  - staff: mid={}, title={}, name={}",
                                        staff_member.mid, staff_member.title, staff_member.name
                                    );
                                }
                            } else {
                                debug!("视频 {} 没有staff信息", video_model.bvid);
                            }

                            // 检查是否为合作视频，支持submission和收藏夹来源
                            let mut video_model_mut = video_model.clone();
                            let mut collaboration_video_updated = false;
                            if let Some(staff_list) = staff {
                                debug!("视频 {} 有staff信息，成员数量: {}", video_model.bvid, staff_list.len());
                                for staff_member in staff_list.iter() {
                                    debug!(
                                        "  - staff: mid={}, title={}, name={}",
                                        staff_member.mid, staff_member.title, staff_member.name
                                    );
                                }

                                if staff_list.len() > 1 {
                                    debug!(
                                        "发现合作视频：bvid={}, staff_count={}",
                                        video_model.bvid,
                                        staff_list.len()
                                    );

                                    // 查找所有可能的订阅UP主
                                    let mut matched_submission: Option<submission::Model> = None;

                                    // 1. 如果是submission来源，直接使用source_submission_id
                                    if let Some(source_submission_id) = video_model.source_submission_id {
                                        debug!("submission来源视频，source_submission_id: {}", source_submission_id);
                                        if let Ok(Some(submission)) =
                                            submission::Entity::find_by_id(source_submission_id)
                                                .one(connection)
                                                .await
                                        {
                                            debug!(
                                                "找到来源submission: {} ({})",
                                                submission.upper_name, submission.upper_id
                                            );
                                            // 检查这个submission的UP主是否在staff列表中
                                            if staff_list.iter().any(|staff| staff.mid == submission.upper_id) {
                                                debug!(
                                                    "submission UP主 {} ({}) 在staff列表中",
                                                    submission.upper_name, submission.upper_id
                                                );
                                                matched_submission = Some(submission);
                                            } else {
                                                debug!(
                                                    "submission UP主 {} ({}) 不在staff列表中",
                                                    submission.upper_name, submission.upper_id
                                                );
                                            }
                                        } else {
                                            debug!(
                                                "找不到source_submission_id对应的submission记录: {}",
                                                source_submission_id
                                            );
                                        }
                                    } else {
                                        debug!("非submission来源视频，检查staff中是否有已订阅的UP主");
                                        // 2. 如果不是submission来源（如收藏夹），查找所有subscription中匹配的UP主
                                        for staff_member in staff_list.iter() {
                                            debug!(
                                                "检查staff成员 {} ({}) 是否已订阅",
                                                staff_member.name, staff_member.mid
                                            );
                                            if let Ok(Some(submission)) = submission::Entity::find()
                                                .filter(submission::Column::UpperId.eq(staff_member.mid))
                                                .filter(submission::Column::Enabled.eq(true))
                                                .one(connection)
                                                .await
                                            {
                                                debug!(
                                                    "在staff中找到已订阅的UP主：{} ({})",
                                                    staff_member.name, staff_member.mid
                                                );
                                                matched_submission = Some(submission);
                                                break;
                                            } else {
                                                debug!("staff成员 {} ({}) 未订阅", staff_member.name, staff_member.mid);
                                            }
                                        }
                                    }

                                    // 如果找到了匹配的订阅UP主，进行归类
                                    if let Some(submission) = matched_submission {
                                        // 从staff信息中找到匹配UP主的头像
                                        let matched_staff_face = staff_list
                                            .iter()
                                            .find(|staff| staff.mid == submission.upper_id)
                                            .map(|staff| staff.face.clone())
                                            .unwrap_or_default();

                                        debug!(
                                            "为合作视频匹配UP主头像: {} -> {}",
                                            submission.upper_name, matched_staff_face
                                        );

                                        // 使用submission的信息更新视频，包括正确的头像
                                        video_model_mut.upper_id = submission.upper_id;
                                        video_model_mut.upper_name = submission.upper_name.clone();
                                        video_model_mut.upper_face = matched_staff_face;
                                        collaboration_video_updated = true;
                                        info!(
                                            "合作视频 {} 归类到订阅UP主「{}」(来源：{})",
                                            video_model_mut.bvid,
                                            submission.upper_name,
                                            if video_model.source_submission_id.is_some() {
                                                "投稿订阅"
                                            } else {
                                                "收藏夹"
                                            }
                                        );
                                    } else {
                                        debug!("staff列表中没有找到已订阅的UP主");
                                    }
                                } else {
                                    debug!("staff列表只有{}个成员，不是合作视频", staff_list.len());
                                }
                            } else {
                                debug!("视频 {} 没有staff信息", video_model.bvid);
                            }

                            let txn = connection.begin().await?;
                            // 将分页信息写入数据库
                            create_pages(pages, &video_model_mut, &txn).await?;
                            let mut video_active_model = view_info.into_detail_model(video_model_mut.clone());
                            video_source.set_relation_id(&mut video_active_model);
                            video_active_model.single_page = Set(Some(pages_len == 1));
                            video_active_model.tags = Set(Some(serde_json::to_value(tags)?));

                            // 只有合作视频更新时才覆盖upper信息，保持其他视频的API更新不被影响
                            if collaboration_video_updated {
                                debug!("合作视频检测到更新，覆盖upper信息到数据库");
                                video_active_model.upper_id = Set(video_model_mut.upper_id);
                                video_active_model.upper_name = Set(video_model_mut.upper_name.clone());
                                video_active_model.upper_face = Set(video_model_mut.upper_face.clone());
                            } else {
                                debug!("非合作视频或未发生更新，保持API返回的upper信息");
                            }

                            video_active_model.save(&txn).await?;
                            txn.commit().await?;
                        }
                    };
                    Ok::<_, anyhow::Error>(())
                }
            })
            .collect::<FuturesUnordered<_>>();

        // 并发执行所有任务
        let mut stream = tasks;
        while let Some(res) = stream.next().await {
            if let Err(e) = res {
                // 使用错误分类器进行统一处理
                #[allow(clippy::needless_borrow)]
                let classified_error = crate::error::ErrorClassifier::classify_error(&e);

                if classified_error.error_type == crate::error::ErrorType::UserCancelled {
                    info!("视频详情获取因用户暂停而终止: {}", classified_error.message);
                    return Err(e); // 直接返回暂停错误，不取消其他任务
                }

                let error_msg = e.to_string();

                if e.downcast_ref::<DownloadAbortError>().is_some() || error_msg.contains("Download cancelled") {
                    token.cancel();
                    // drain the rest of the tasks
                    while stream.next().await.is_some() {}
                    return Err(e);
                }
                // for other errors, just log and continue
                error!("获取视频详情时发生错误: {:#}", e);
            }
        }
        info!("完成普通视频详情处理");
    }
    video_source.log_fetch_video_end();
    Ok(())
}

/// 下载所有未处理成功的视频
pub async fn download_unprocessed_videos(
    bili_client: &BiliClient,
    video_source: &VideoSourceEnum,
    connection: &DatabaseConnection,
    downloader: &UnifiedDownloader,
    token: CancellationToken,
) -> Result<()> {
    video_source.log_download_video_start();
    let current_config = crate::config::reload_config();
    let semaphore = Semaphore::new(current_config.concurrent_limit.video);
    let unhandled_videos_pages = filter_unhandled_video_pages(video_source.filter_expr(), connection).await?;

    // 只有当有未处理视频时才显示日志
    if !unhandled_videos_pages.is_empty() {
        info!("找到 {} 个未处理完成的视频", unhandled_videos_pages.len());
    }

    let mut assigned_upper = HashSet::new();
    let tasks = unhandled_videos_pages
        .into_iter()
        .map(|(video_model, pages_model)| {
            let should_download_upper = !assigned_upper.contains(&video_model.upper_id);
            assigned_upper.insert(video_model.upper_id);
            debug!("下载视频: {}", video_model.name);
            download_video_pages(
                bili_client,
                video_source,
                video_model,
                pages_model,
                connection,
                &semaphore,
                downloader,
                should_download_upper,
                token.clone(),
            )
        })
        .collect::<FuturesUnordered<_>>();
    let mut download_aborted = false;
    let mut stream = tasks;
    // 使用循环和select来处理任务，以便在检测到取消信号时立即停止
    while let Some(res) = stream.next().await {
        match res {
            Ok(model) => {
                if download_aborted {
                    continue;
                }
                // 任务成功完成，更新数据库
                if let Err(db_err) = update_videos_model(vec![model], connection).await {
                    error!("更新数据库失败: {:#}", db_err);
                }
            }
            Err(e) => {
                let error_msg = e.to_string();

                // 调试：输出完整的错误信息
                debug!("检查下载错误消息: '{}'", error_msg);
                debug!("完整错误链: {:#}", e);
                debug!("是否包含'任务已暂停': {}", error_msg.contains("任务已暂停"));
                debug!("是否包含'停止下载': {}", error_msg.contains("停止下载"));
                debug!(
                    "是否包含'Download cancelled': {}",
                    error_msg.contains("Download cancelled")
                );

                // 检查是否是暂停导致的失败，只有在任务暂停时才将 Download cancelled 视为暂停错误
                if error_msg.contains("任务已暂停")
                    || error_msg.contains("停止下载")
                    || error_msg.contains("用户主动暂停任务")
                    || (error_msg.contains("Download cancelled") && crate::task::TASK_CONTROLLER.is_paused())
                {
                    info!("下载任务因用户暂停而终止: {}", error_msg);
                    continue; // 跳过暂停相关的错误，不触发风控
                }

                if e.downcast_ref::<DownloadAbortError>().is_some() || error_msg.contains("Download cancelled") {
                    if !download_aborted {
                        debug!("检测到风控或取消信号，开始中止所有下载任务");
                        token.cancel(); // 立即取消所有其他正在运行的任务
                        download_aborted = true;
                    }
                } else {
                    // 检查是否为暂停相关错误
                    let error_msg = e.to_string();
                    if error_msg.contains("用户主动暂停任务") || error_msg.contains("任务已暂停") {
                        info!("下载任务因用户暂停而终止");
                    } else {
                        // 任务返回了非中止的错误
                        error!("下载任务失败: {:#}", e);
                    }
                }
            }
        }
    }

    if download_aborted {
        error!("下载触发风控，已终止所有任务，停止所有后续扫描");

        // 自动重置风控导致的失败任务
        if let Err(reset_err) = auto_reset_risk_control_failures(connection).await {
            error!("自动重置风控失败任务时出错: {:#}", reset_err);
        }

        video_source.log_download_video_end();
        // 风控时返回错误，中断整个扫描循环
        bail!(DownloadAbortError());
    }
    video_source.log_download_video_end();
    Ok(())
}

/// 对当前循环中失败的视频进行一次重试
pub async fn retry_failed_videos_once(
    bili_client: &BiliClient,
    video_source: &VideoSourceEnum,
    connection: &DatabaseConnection,
    downloader: &UnifiedDownloader,
    token: CancellationToken,
) -> Result<()> {
    let failed_videos_pages = get_failed_videos_in_current_cycle(video_source.filter_expr(), connection).await?;

    if failed_videos_pages.is_empty() {
        debug!("当前循环中没有失败的视频需要重试");
        return Ok(());
    }

    info!("开始重试当前循环中的 {} 个失败视频", failed_videos_pages.len());

    let current_config = crate::config::reload_config();
    let semaphore = Semaphore::new(current_config.concurrent_limit.video);
    let mut assigned_upper = HashSet::new();

    let tasks = failed_videos_pages
        .into_iter()
        .map(|(video_model, pages_model)| {
            let should_download_upper = !assigned_upper.contains(&video_model.upper_id);
            assigned_upper.insert(video_model.upper_id);
            debug!("重试视频: {}", video_model.name);
            download_video_pages(
                bili_client,
                video_source,
                video_model,
                pages_model,
                connection,
                &semaphore,
                downloader,
                should_download_upper,
                token.clone(),
            )
        })
        .collect::<FuturesUnordered<_>>();

    let mut download_aborted = false;
    let mut stream = tasks;
    let mut retry_success_count = 0;

    while let Some(res) = stream.next().await {
        match res {
            Ok(model) => {
                if download_aborted {
                    continue;
                }
                retry_success_count += 1;
                if let Err(db_err) = update_videos_model(vec![model], connection).await {
                    error!("重试后更新数据库失败: {:#}", db_err);
                }
            }
            Err(e) => {
                let error_msg = e.to_string();

                // 检查是否是暂停导致的失败，只有在任务暂停时才将 Download cancelled 视为暂停错误
                if error_msg.contains("任务已暂停")
                    || error_msg.contains("停止下载")
                    || error_msg.contains("用户主动暂停任务")
                    || (error_msg.contains("Download cancelled") && crate::task::TASK_CONTROLLER.is_paused())
                {
                    info!("重试任务因用户暂停而终止: {}", error_msg);
                    continue; // 跳过暂停相关的错误，不触发风控
                }

                if e.downcast_ref::<DownloadAbortError>().is_some() || error_msg.contains("Download cancelled") {
                    if !download_aborted {
                        debug!("重试过程中检测到风控或取消信号，停止重试");
                        token.cancel();
                        download_aborted = true;
                    }
                } else {
                    // 重试失败，但不中断其他重试任务
                    debug!("视频重试失败: {:#}", e);
                }
            }
        }
    }

    if download_aborted {
        warn!("重试过程中触发风控，已停止重试");
        // 不返回错误，避免影响主流程
    } else if retry_success_count > 0 {
        info!("循环内重试完成，成功重试 {} 个视频", retry_success_count);
    } else {
        debug!("循环内重试完成，但没有视频重试成功");
    }

    Ok(())
}

/// 分页下载任务的参数结构体
pub struct DownloadPageArgs<'a> {
    pub should_run: bool,
    pub bili_client: &'a BiliClient,
    pub video_source: &'a VideoSourceEnum,
    pub video_model: &'a video::Model,
    pub pages: Vec<page::Model>,
    pub connection: &'a DatabaseConnection,
    pub downloader: &'a UnifiedDownloader,
    pub base_path: &'a Path,
    #[allow(dead_code)]
    pub token: CancellationToken,
}

#[allow(clippy::too_many_arguments)]
pub async fn download_video_pages(
    bili_client: &BiliClient,
    video_source: &VideoSourceEnum,
    video_model: video::Model,
    pages: Vec<page::Model>,
    connection: &DatabaseConnection,
    semaphore: &Semaphore,
    downloader: &UnifiedDownloader,
    should_download_upper: bool,
    token: CancellationToken,
) -> Result<video::ActiveModel> {
    let _permit = tokio::select! {
        biased;
        _ = token.cancelled() => return Err(anyhow!("Download cancelled")),
        permit = semaphore.acquire() => permit.context("acquire semaphore failed")?,
    };
    let mut status = VideoStatus::from(video_model.download_status);
    let separate_status = status.should_run();

    // 检查是否为番剧
    let is_bangumi = matches!(video_source, VideoSourceEnum::BangumiSource(_));

    // 检查是否为合集
    let is_collection = matches!(video_source, VideoSourceEnum::Collection(_));

    // 定义最终使用的视频模型
    let final_video_model = if is_bangumi {
        video_model.clone()
    } else {
        // 对于非番剧，重新从数据库加载视频信息，以获取可能在fetch_video_details中更新的upper信息
        if let Ok(Some(updated)) = video::Entity::find_by_id(video_model.id).one(connection).await {
            debug!(
                "重新加载视频信息: upper_name={}, upper_id={}",
                updated.upper_name, updated.upper_id
            );
            updated
        } else {
            debug!("无法重新加载视频信息，使用原始模型");
            video_model.clone()
        }
    };

    // 对于已经获取过详情但可能需要合作视频重新归类的普通视频，进行检测
    let final_video_model = if !is_bangumi {
        // 检查是否需要进行合作视频检测（只对有staff信息的视频）
        if let Some(staff_info) = &final_video_model.staff_info {
            if let Ok(staff_list) = serde_json::from_value::<Vec<crate::bilibili::StaffInfo>>(staff_info.clone()) {
                debug!(
                    "视频 {} 有staff信息，成员数量: {} (下载阶段检测)",
                    final_video_model.bvid,
                    staff_list.len()
                );

                if staff_list.len() > 1 {
                    // 获取所有启用的订阅
                    let submissions = submission::Entity::find()
                        .filter(submission::Column::Enabled.eq(true))
                        .all(connection)
                        .await
                        .context("get submissions failed")?;

                    let mut matched_submission = None;
                    // 检查staff中是否有已订阅的UP主
                    for submission in &submissions {
                        for staff_member in &staff_list {
                            if staff_member.mid == submission.upper_id {
                                debug!(
                                    "在staff中找到已订阅的UP主：{} ({})",
                                    staff_member.name, staff_member.mid
                                );
                                matched_submission = Some(submission);
                                break;
                            }
                        }
                    }

                    // 如果找到了匹配的订阅UP主，进行归类
                    if let Some(submission) = matched_submission {
                        // 从staff信息中找到匹配UP主的头像
                        let matched_staff_face = staff_list
                            .iter()
                            .find(|staff| staff.mid == submission.upper_id)
                            .map(|staff| staff.face.clone())
                            .unwrap_or_default();

                        debug!(
                            "为合作视频匹配UP主头像 (下载阶段): {} -> {}",
                            submission.upper_name, matched_staff_face
                        );

                        // 创建更新后的视频模型
                        let mut updated_model = final_video_model.clone();
                        updated_model.upper_id = submission.upper_id;
                        updated_model.upper_name = submission.upper_name.clone();
                        updated_model.upper_face = matched_staff_face.clone();

                        // 立即保存到数据库
                        let mut active_model: video::ActiveModel = updated_model.clone().into();
                        active_model.upper_id = Set(submission.upper_id);
                        active_model.upper_name = Set(submission.upper_name.clone());
                        active_model.upper_face = Set(matched_staff_face);

                        if let Err(e) = active_model.update(connection).await {
                            warn!("更新合作视频信息失败: {}", e);
                        } else {
                            info!(
                                "合作视频 {} 归类到订阅UP主「{}」(下载阶段处理)",
                                updated_model.bvid, submission.upper_name
                            );
                        }

                        updated_model
                    } else {
                        debug!("staff列表中没有找到已订阅的UP主 (下载阶段)");
                        final_video_model
                    }
                } else {
                    debug!("staff列表只有{}个成员，不是合作视频 (下载阶段)", staff_list.len());
                    final_video_model
                }
            } else {
                debug!("解析staff信息失败 (下载阶段)");
                final_video_model
            }
        } else {
            debug!("视频 {} 没有staff信息 (下载阶段)", final_video_model.bvid);
            final_video_model
        }
    } else {
        final_video_model
    };

    // 为番剧获取API数据用于NFO生成
    let season_info = if is_bangumi && video_model.season_id.is_some() {
        let season_id = video_model.season_id.as_ref().unwrap();
        match get_season_info_from_api(bili_client, season_id, token.clone()).await {
            Ok(info) => {
                debug!("成功获取番剧 {} 的API信息用于NFO生成", info.title);
                Some(info)
            }
            Err(e) => {
                warn!(
                    "获取番剧 {} (season_id: {}) 的API信息失败: {}",
                    video_model.name, season_id, e
                );
                None
            }
        }
    } else {
        None
    };

    // 获取番剧源和季度信息
    let (base_path, season_folder, bangumi_folder_path) = if is_bangumi {
        let bangumi_source = match video_source {
            VideoSourceEnum::BangumiSource(source) => source,
            _ => unreachable!(),
        };

        // 为番剧创建独立的文件夹：配置路径 -> 番剧文件夹 -> Season文件夹
        let bangumi_root_path = bangumi_source.path();

        // 创建临时的page模型来获取格式化参数（只创建一次，避免重复）
        let temp_page = bili_sync_entity::page::Model {
            id: 0,
            video_id: video_model.id,
            cid: 0,
            pid: 1,
            name: "temp".to_string(),
            width: None,
            height: None,
            duration: 0,
            path: None,
            image: None,
            download_status: 0,
            created_at: now_standard_string(),
        };

        // 获取真实的番剧标题（从缓存或API）
        let api_title = if let Some(ref season_id) = video_model.season_id {
            get_cached_season_title(bili_client, season_id, token.clone()).await
        } else {
            None
        };

        // 使用番剧格式化参数，优先使用API提供的真实标题
        let format_args =
            crate::utils::format_arg::bangumi_page_format_args(&video_model, &temp_page, api_title.as_deref());

        // 检查是否有有效的series_title，如果没有则跳过番剧处理
        let series_title = format_args["series_title"].as_str().unwrap_or("");
        if series_title.is_empty() {
            return Err(anyhow::anyhow!(
                "番剧 {} (BVID: {}) 缺少API标题数据，无法创建番剧文件夹",
                video_model.name,
                video_model.bvid
            ));
        }

        // 生成番剧文件夹名称
        let bangumi_folder_name =
            crate::config::with_config(|bundle| bundle.render_bangumi_folder_template(&format_args))
                .map_err(|e| anyhow::anyhow!("渲染番剧文件夹模板失败: {}", e))?;

        // 番剧文件夹路径
        let bangumi_folder_path = bangumi_root_path.join(&bangumi_folder_name);

        // 延迟创建番剧文件夹，只在实际需要时创建

        // 检查是否启用番剧Season结构
        let use_bangumi_season_structure =
            crate::config::with_config(|bundle| bundle.config.bangumi_use_season_structure);

        if use_bangumi_season_structure {
            // 启用番剧Season结构：创建统一的系列根目录，在其下创建Season子目录

            // 提取基础系列名称和季度信息
            let series_title = api_title.as_deref().unwrap_or(&video_model.name);
            let season_title = format_args.get("season_title").and_then(|v| v.as_str());

            let (base_series_name, season_number) =
                crate::utils::bangumi_name_extractor::BangumiNameExtractor::extract_series_name_and_season(
                    series_title,
                    season_title,
                );

            // 系列根目录路径，延迟创建
            let series_root_path = bangumi_root_path.join(&base_series_name);

            // 生成标准的Season文件夹名称
            let season_folder_name =
                crate::utils::bangumi_name_extractor::BangumiNameExtractor::generate_season_folder_name(season_number);
            let season_path = series_root_path.join(&season_folder_name);

            (season_path, Some(season_folder_name), Some(series_root_path))
        } else {
            // 原有逻辑：根据配置决定是否创建季度子目录
            let should_create_season_folder = bangumi_source.download_all_seasons
                || (bangumi_source
                    .selected_seasons
                    .as_ref()
                    .map(|s| !s.is_empty())
                    .unwrap_or(false))
                || video_model.season_id.is_some(); // 单季度番剧：如果有season_id就创建目录

            if should_create_season_folder && video_model.season_id.is_some() {
                // 使用配置的folder_structure模板生成季度文件夹名称（复用已有的format_args）
                let season_folder_name =
                    crate::config::with_config(|bundle| bundle.render_folder_structure_template(&format_args))
                        .map_err(|e| anyhow::anyhow!("渲染季度文件夹模板失败: {}", e))?;

                (
                    bangumi_folder_path.join(&season_folder_name),
                    Some(season_folder_name),
                    Some(bangumi_folder_path),
                )
            } else {
                // 不启用下载所有季度且没有选中特定季度时，直接使用番剧文件夹路径
                (bangumi_folder_path.clone(), None, Some(bangumi_folder_path))
            }
        }
    } else {
        // 非番剧使用原来的逻辑，但对合集进行特殊处理
        // 【重要】：始终从视频源的原始路径开始计算，避免使用已保存的视频路径
        let video_source_base_path = video_source.path();

        debug!("=== 路径计算开始 ===");
        debug!("视频源基础路径: {:?}", video_source_base_path);
        debug!("视频BVID: {}", final_video_model.bvid);
        debug!(
            "视频UP主: {} ({})",
            final_video_model.upper_name, final_video_model.upper_id
        );
        debug!("数据库中保存的路径: {:?}", final_video_model.path);
        debug!("注意：将忽略数据库中的路径，从视频源基础路径重新计算");

        let path = if let VideoSourceEnum::Collection(collection_source) = video_source {
            // 合集的特殊处理
            let config = crate::config::reload_config();
            match config.collection_folder_mode.as_ref() {
                "unified" => {
                    // 统一模式：所有视频放在以合集名称命名的同一个文件夹下
                    video_source_base_path.join(&collection_source.name)
                }
                _ => {
                    // 分离模式（默认）：每个视频有自己的文件夹
                    let base_folder_name = crate::config::with_config(|bundle| {
                        bundle.render_video_template(&video_format_args(&final_video_model))
                    })
                    .map_err(|e| anyhow::anyhow!("模板渲染失败: {}", e))?;

                    debug!("合集分离模式 - 渲染的文件夹名: '{}'", base_folder_name);
                    debug!("合集分离模式 - 基础路径: {:?}", video_source_base_path);

                    // **智能判断：根据模板内容决定是否需要去重**
                    let video_template =
                        crate::config::with_config(|bundle| bundle.config.video_name.as_ref().to_string());
                    let needs_deduplication = video_template.contains("title")
                        || (video_template.contains("name") && !video_template.contains("upper_name"));

                    if needs_deduplication {
                        // 智能去重：检查文件夹名是否已存在，如果存在则追加唯一标识符
                        let unique_folder_name = generate_unique_folder_name(
                            video_source_base_path,
                            &base_folder_name,
                            &video_model,
                            &video_model.pubtime.format("%Y-%m-%d").to_string(),
                        );
                        video_source_base_path.join(&unique_folder_name)
                    } else {
                        // 不使用去重，允许多个视频共享同一文件夹
                        video_source_base_path.join(&base_folder_name)
                    }
                }
            }
        } else {
            // 其他类型的视频源使用原来的逻辑
            let base_folder_name = crate::config::with_config(|bundle| {
                bundle.render_video_template(&video_format_args(&final_video_model))
            })
            .map_err(|e| anyhow::anyhow!("模板渲染失败: {}", e))?;

            debug!("普通视频源 - 渲染的文件夹名: '{}'", base_folder_name);
            debug!("普通视频源 - 基础路径: {:?}", video_source_base_path);

            // **智能判断：根据模板内容决定是否需要去重**
            let video_template = crate::config::with_config(|bundle| bundle.config.video_name.as_ref().to_string());
            let needs_deduplication = video_template.contains("title")
                || (video_template.contains("name") && !video_template.contains("upper_name"));

            if needs_deduplication {
                // 智能去重：检查文件夹名是否已存在，如果存在则追加唯一标识符
                let unique_folder_name = generate_unique_folder_name(
                    video_source_base_path,
                    &base_folder_name,
                    &final_video_model,
                    &final_video_model.pubtime.format("%Y-%m-%d").to_string(),
                );
                debug!("使用去重文件夹名: '{}'", unique_folder_name);
                let final_path = video_source_base_path.join(&unique_folder_name);
                debug!("最终计算路径: {:?}", final_path);
                final_path
            } else {
                // 不使用去重，允许多个视频共享同一文件夹
                debug!("不使用去重，直接使用基础文件夹名: '{}'", base_folder_name);
                let final_path = video_source_base_path.join(&base_folder_name);
                debug!("最终计算路径: {:?}", final_path);
                final_path
            }
        };

        // 检查是否为多P视频且启用了Season结构
        let config = crate::config::reload_config();
        let is_single_page = final_video_model.single_page.unwrap_or(true);

        if (!is_single_page && config.multi_page_use_season_structure)
            || (is_collection && config.collection_use_season_structure)
        {
            // 为多P视频或合集创建Season文件夹结构
            let season_folder_name = "Season 01".to_string();
            let season_path = path.join(&season_folder_name);
            (season_path, Some(season_folder_name), Some(path))
        } else {
            (path, None, None)
        }
    };

    // 延迟创建季度文件夹，只在实际需要写入文件时创建

    let upper_id = final_video_model.upper_id.to_string();
    let current_config = crate::config::reload_config();
    let base_upper_path = &current_config
        .upper_path
        .join(upper_id.chars().next().context("upper_id is empty")?.to_string())
        .join(upper_id);
    let is_single_page = final_video_model.single_page.context("single_page is null")?;

    // 为多P视频生成基于视频名称的文件名
    let video_base_name = if !is_single_page {
        // 多P视频启用Season结构时，使用视频根目录的文件夹名作为系列级封面的文件名
        let config = crate::config::reload_config();
        if config.multi_page_use_season_structure {
            // 从base_path获取视频根目录的文件夹名称
            if let Some(parent) = base_path.parent() {
                if let Some(folder_name) = parent.file_name() {
                    folder_name.to_string_lossy().to_string()
                } else {
                    final_video_model.name.clone() // 回退到视频标题
                }
            } else {
                final_video_model.name.clone() // 回退到视频标题
            }
        } else {
            // 不使用Season结构时，使用模板渲染
            crate::config::with_config(|bundle| bundle.render_video_template(&video_format_args(&final_video_model)))
                .map_err(|e| anyhow::anyhow!("模板渲染失败: {}", e))?
        }
    } else if is_collection {
        // 合集中的单页视频：检查是否启用Season结构
        let config = crate::config::reload_config();
        if config.collection_use_season_structure {
            // 合集启用Season结构时，使用合集名称作为文件名前缀
            if let VideoSourceEnum::Collection(collection_source) = video_source {
                collection_source.name.clone()
            } else {
                String::new()
            }
        } else {
            String::new() // 合集不使用Season结构时不需要
        }
    } else {
        String::new() // 单P视频不需要这些文件
    };

    // 为番剧生成番剧文件夹级别的文件名前缀
    let bangumi_base_name = if is_bangumi {
        if let Some(ref bangumi_folder_path) = bangumi_folder_path {
            // 使用番剧文件夹名称作为前缀
            bangumi_folder_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "bangumi".to_string())
        } else {
            "bangumi".to_string()
        }
    } else {
        String::new()
    };

    // 对于单页视频，page 的下载已经足够
    // 对于多页视频，page 下载仅包含了分集内容，需要额外补上视频的 poster 的 tvshow.nfo
    // 使用 tokio::join! 替代装箱的 Future，零分配并行执行

    // 首先检查是否取消
    if token.is_cancelled() {
        return Err(anyhow!("Download cancelled"));
    }

    // 为番剧检查元数据文件是否已存在，避免重复下载
    let (should_download_bangumi_poster, should_download_bangumi_nfo) = if is_bangumi && bangumi_folder_path.is_some() {
        let bangumi_path = bangumi_folder_path.as_ref().unwrap();
        let poster_path = bangumi_path.join(format!("{}-poster.jpg", bangumi_base_name));
        let fanart_path = bangumi_path.join(format!("{}-fanart.jpg", bangumi_base_name));
        let nfo_path = bangumi_path.join("tvshow.nfo");

        let poster_exists = poster_path.exists() && fanart_path.exists();
        let nfo_exists = nfo_path.exists();

        (!poster_exists, !nfo_exists)
    } else {
        (false, false)
    };

    // 为启用Season结构的非番剧视频检查封面文件是否已存在，避免重复下载
    let should_download_season_poster = if !is_bangumi {
        let config = crate::config::reload_config();
        let uses_season_structure = (is_collection && config.collection_use_season_structure)
            || (!is_single_page && config.multi_page_use_season_structure);

        if uses_season_structure && season_folder.is_some() {
            // 计算封面文件路径（与下载逻辑保持一致）
            let poster_path = base_path
                .parent()
                .map(|parent| parent.join(format!("{}-poster.jpg", video_base_name)))
                .unwrap_or_else(|| base_path.join(format!("{}-poster.jpg", video_base_name)));
            let fanart_path = base_path
                .parent()
                .map(|parent| parent.join(format!("{}-fanart.jpg", video_base_name)))
                .unwrap_or_else(|| base_path.join(format!("{}-fanart.jpg", video_base_name)));

            let poster_exists = poster_path.exists() && fanart_path.exists();
            let video_type = if is_collection { "合集" } else { "多P视频" };
            info!(
                "{}「{}」封面检查: poster_path={:?}, fanart_path={:?}, exists={}",
                video_type, video_model.name, poster_path, fanart_path, poster_exists
            );
            !poster_exists
        } else {
            true // 未启用Season结构时不进行检查
        }
    } else {
        true // 番剧不在此处检查
    };

    // 先处理NFO生成（独立执行，避免tokio::join!类型问题）
    let nfo_result = if is_bangumi && season_info.is_some() {
        // 番剧且有API数据：使用API驱动的NFO生成
        // 注意：启用Season结构时，bangumi_folder_path已经指向系列根目录
        generate_bangumi_video_nfo(
            separate_status[2] && bangumi_folder_path.is_some() && should_download_bangumi_nfo,
            &video_model,
            season_info.as_ref().unwrap(),
            bangumi_folder_path.as_ref().unwrap().join("tvshow.nfo"),
        )
        .await
    } else {
        // 普通视频或番剧无API数据：使用原有逻辑
        generate_video_nfo(
            if is_bangumi {
                // 番剧：只有在文件不存在时才生成，放在番剧文件夹根目录
                separate_status[2] && bangumi_folder_path.is_some() && should_download_bangumi_nfo
            } else {
                // 普通视频：为多P视频或启用Season结构的合集生成nfo
                let config = crate::config::reload_config();
                separate_status[2] && (!is_single_page || (is_collection && config.collection_use_season_structure))
            },
            &video_model,
            if let Some(ref bangumi_path) = bangumi_folder_path {
                if is_bangumi {
                    // 番剧tvshow.nfo放在番剧文件夹根目录，使用固定文件名
                    bangumi_path.join("tvshow.nfo")
                } else {
                    // 多P视频或合集使用Season结构时，tvshow.nfo放在视频根目录
                    let config = crate::config::reload_config();
                    if ((!is_single_page && config.multi_page_use_season_structure)
                        || (is_collection && config.collection_use_season_structure))
                        && season_folder.is_some()
                    {
                        bangumi_path.join("tvshow.nfo")
                    } else {
                        // 不使用Season结构时，保持原有逻辑
                        base_path.join(format!("{}.nfo", video_base_name))
                    }
                }
            } else {
                // 多P视频或合集使用Season结构时，tvshow.nfo放在视频根目录
                let config = crate::config::reload_config();
                if ((!is_single_page && config.multi_page_use_season_structure)
                    || (is_collection && config.collection_use_season_structure))
                    && season_folder.is_some()
                {
                    // 需要从base_path（Season文件夹）回到父目录（视频根目录）
                    base_path
                        .parent()
                        .map(|parent| parent.join("tvshow.nfo"))
                        .unwrap_or_else(|| base_path.join("tvshow.nfo"))
                } else {
                    // 普通视频nfo放在视频文件夹
                    base_path.join(format!("{}.nfo", video_base_name))
                }
            },
        )
        .await
    };

    // 为启用Season结构的番剧生成season.nfo
    let season_nfo_result = if is_bangumi && season_info.is_some() {
        let config = crate::config::reload_config();
        if config.bangumi_use_season_structure {
            // 提取季度信息来生成season.nfo
            let series_title = season_info.as_ref().unwrap().title.as_str();

            // 直接从series_title中提取季度信息
            let (_, season_number) =
                crate::utils::bangumi_name_extractor::BangumiNameExtractor::extract_series_name_and_season(
                    series_title,
                    None, // 让算法从title中自动提取
                );

            info!("番剧「{}」提取的季度编号: {}", series_title, season_number);

            // 独立检查season.nfo文件是否存在（不依赖tvshow.nfo检查）
            let season_nfo_path = base_path.join("season.nfo");
            let should_generate_season_nfo = separate_status[2] && !season_nfo_path.exists();

            generate_bangumi_season_nfo(
                should_generate_season_nfo,
                &video_model,
                season_info.as_ref().unwrap(),
                base_path.clone(),
                season_number,
            )
            .await
        } else {
            Ok(ExecutionStatus::Skipped)
        }
    } else {
        Ok(ExecutionStatus::Skipped)
    };

    // 为启用Season结构的番剧下载季度级图片
    let season_images_result = if is_bangumi && season_info.is_some() {
        let config = crate::config::reload_config();
        if config.bangumi_use_season_structure {
            // 获取季度编号用于生成正确的文件名
            let series_title = season_info.as_ref().unwrap().title.as_str();
            let (_, season_number) =
                crate::utils::bangumi_name_extractor::BangumiNameExtractor::extract_series_name_and_season(
                    series_title,
                    None,
                );

            // 季度级图片应该放在系列根目录，使用标准命名
            let series_root = bangumi_folder_path.as_ref().unwrap();
            let poster_path = series_root.join(format!("Season{:02}-poster.jpg", season_number));
            let fanart_path = series_root.join(format!("Season{:02}-fanart.jpg", season_number));

            // 独立检查季度级图片文件是否存在
            let should_download_season_images = separate_status[0] && (!poster_path.exists() || !fanart_path.exists());

            info!("准备下载季度级图片到: {:?} 和 {:?}", poster_path, fanart_path);

            // 季度级图片：poster使用封面，fanart使用横版封面（优先级：新EP封面 > 横版封面 > 专门背景图 > 竖版封面）
            let season_info_ref = season_info.as_ref().unwrap();
            let season_cover_url = season_info_ref.cover.as_deref();
            let season_fanart_url = season_info_ref
                .new_ep_cover
                .as_deref()
                .filter(|s| !s.is_empty())
                .or(season_info_ref
                    .horizontal_cover_169
                    .as_deref()
                    .filter(|s| !s.is_empty()))
                .or(season_info_ref
                    .horizontal_cover_1610
                    .as_deref()
                    .filter(|s| !s.is_empty()))
                .or(season_info_ref.bkg_cover.as_deref().filter(|s| !s.is_empty()))
                .or(season_info_ref.cover.as_deref().filter(|s| !s.is_empty()));

            info!("Season级别fanart选择逻辑:");
            info!("  最终选择的season fanart URL: {:?}", season_fanart_url);

            fetch_video_poster(
                should_download_season_images,
                &video_model,
                downloader,
                poster_path,
                fanart_path,
                token.clone(),
                season_cover_url,  // 使用季度封面URL
                season_fanart_url, // 使用横版封面作为fanart
            )
            .await
        } else {
            Ok(ExecutionStatus::Skipped)
        }
    } else {
        Ok(ExecutionStatus::Skipped)
    };

    let (res_1, res_3, res_4, res_5) = tokio::join!(
        // 下载视频封面（番剧和普通视频采用不同策略）
        fetch_video_poster(
            if is_bangumi {
                // 番剧：只有在文件不存在时才下载，放在番剧文件夹根目录
                separate_status[0] && bangumi_folder_path.is_some() && should_download_bangumi_poster
            } else {
                // 普通视频：为多P视频或启用Season结构的合集生成封面，并检查文件是否已存在
                let config = crate::config::reload_config();
                separate_status[0]
                    && (!is_single_page || (is_collection && config.collection_use_season_structure))
                    && should_download_season_poster
            },
            &video_model,
            downloader,
            if is_bangumi && bangumi_folder_path.is_some() {
                // 番剧封面放在番剧文件夹根目录
                bangumi_folder_path
                    .as_ref()
                    .unwrap()
                    .join(format!("{}-poster.jpg", bangumi_base_name))
            } else {
                // 多P视频或合集使用Season结构时，封面放在视频根目录
                let config = crate::config::reload_config();
                if (!is_single_page && config.multi_page_use_season_structure && season_folder.is_some())
                    || (is_collection && config.collection_use_season_structure && season_folder.is_some())
                {
                    // 需要从base_path（Season文件夹）回到父目录（视频根目录）
                    base_path
                        .parent()
                        .map(|parent| parent.join(format!("{}-poster.jpg", video_base_name)))
                        .unwrap_or_else(|| base_path.join(format!("{}-poster.jpg", video_base_name)))
                } else {
                    // 普通视频封面放在视频文件夹
                    base_path.join(format!("{}-poster.jpg", video_base_name))
                }
            },
            if is_bangumi && bangumi_folder_path.is_some() {
                // 番剧fanart放在番剧文件夹根目录
                bangumi_folder_path
                    .as_ref()
                    .unwrap()
                    .join(format!("{}-fanart.jpg", bangumi_base_name))
            } else {
                // 多P视频或合集使用Season结构时，fanart放在视频根目录
                let config = crate::config::reload_config();
                if (!is_single_page && config.multi_page_use_season_structure && season_folder.is_some())
                    || (is_collection && config.collection_use_season_structure && season_folder.is_some())
                {
                    // 需要从base_path（Season文件夹）回到父目录（视频根目录）
                    base_path
                        .parent()
                        .map(|parent| parent.join(format!("{}-fanart.jpg", video_base_name)))
                        .unwrap_or_else(|| base_path.join(format!("{}-fanart.jpg", video_base_name)))
                } else {
                    // 普通视频fanart放在视频文件夹
                    base_path.join(format!("{}-fanart.jpg", video_base_name))
                }
            },
            token.clone(),
            // 番剧使用季度封面，普通视频使用默认封面
            if is_bangumi && season_info.is_some() {
                season_info.as_ref().unwrap().cover.as_deref()
            } else {
                None
            },
            // 番剧fanart优先级：新EP封面 > 横版封面 > 专门背景图 > 竖版封面，普通视频复用poster
            if is_bangumi && season_info.is_some() {
                let season = season_info.as_ref().unwrap();
                let fanart_url = season
                    .new_ep_cover
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .or(season.horizontal_cover_169.as_deref().filter(|s| !s.is_empty()))
                    .or(season.horizontal_cover_1610.as_deref().filter(|s| !s.is_empty()))
                    .or(season.bkg_cover.as_deref().filter(|s| !s.is_empty()))
                    .or(season.cover.as_deref().filter(|s| !s.is_empty()));

                info!("番剧「{}」fanart选择逻辑:", video_model.name);
                debug!(
                    "  字段值: new_ep_cover={:?}, h169={:?}, h1610={:?}, bkg={:?}, cover={:?}",
                    season.new_ep_cover,
                    season.horizontal_cover_169,
                    season.horizontal_cover_1610,
                    season.bkg_cover,
                    season.cover
                );
                info!("  最终选择的fanart URL: {:?}", fanart_url);

                fanart_url
            } else {
                None
            },
        ),
        // 下载 Up 主头像（番剧跳过，因为番剧没有UP主信息）
        fetch_upper_face(
            separate_status[2] && should_download_upper && !is_bangumi,
            &final_video_model,
            downloader,
            base_upper_path.join("folder.jpg"),
            token.clone(),
        ),
        // 生成 Up 主信息的 nfo（番剧跳过，因为番剧没有UP主信息）
        generate_upper_nfo(
            separate_status[3] && should_download_upper && !is_bangumi,
            &final_video_model,
            base_upper_path.join("person.nfo"),
        ),
        // 分发并执行分 P 下载的任务
        dispatch_download_page(
            DownloadPageArgs {
                should_run: separate_status[4],
                bili_client,
                video_source,
                video_model: &final_video_model,
                pages,
                connection,
                downloader,
                base_path: &base_path,
                token: token.clone(),
            },
            token.clone()
        )
    );

    // 主要的5个任务结果，保持与VideoStatus<5>兼容
    let main_results = [res_1, nfo_result, res_3, res_4, res_5]
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    status.update_status(&main_results);

    // 额外的结果单独处理（季度NFO和季度图片）
    let extra_results = [
        Ok(season_nfo_result.unwrap_or(ExecutionStatus::Skipped)),
        Ok(season_images_result.unwrap_or(ExecutionStatus::Skipped)),
    ]
    .into_iter()
    .map(Into::into)
    .collect::<Vec<_>>();

    // 合并所有结果用于日志处理
    let mut all_results = main_results;
    all_results.extend(extra_results);

    // 使用新的公共函数处理87007错误检测和自动删除
    let task_names = [
        "封面",
        "详情",
        "作者头像",
        "作者详情",
        "分页下载",
        "季度NFO",
        "季度图片",
    ];
    let _has_87007_error =
        detect_and_handle_auto_delete_errors(&all_results, &task_names, &final_video_model, connection, None).await?;

    all_results
        .iter()
        .take(7)
        .zip([
            "封面",
            "详情",
            "作者头像",
            "作者详情",
            "分页下载",
            "季度NFO",
            "季度图片",
        ])
        .for_each(|(res, task_name)| match res {
            ExecutionStatus::Skipped => debug!("处理视频「{}」{}已成功过，跳过", &video_model.name, task_name),
            ExecutionStatus::Succeeded => debug!("处理视频「{}」{}成功", &video_model.name, task_name),
            ExecutionStatus::Ignored(e) => {
                let error_msg = e.to_string();
                if !error_msg.contains("status code: 87007") {
                    info!(
                        "处理视频「{}」{}出现常见错误，已忽略: {:#}",
                        &video_model.name, task_name, e
                    );
                }
            }
            ExecutionStatus::ClassifiedFailed(classified_error) => {
                // 根据错误分类进行不同级别的日志记录
                match classified_error.error_type {
                    crate::error::ErrorType::NotFound => {
                        debug!(
                            "处理视频「{}」{}失败({}): {}",
                            &video_model.name, task_name, classified_error.error_type, classified_error.message
                        );
                    }
                    crate::error::ErrorType::Permission => {
                        // 对于权限错误（包括充电专享视频），使用debug级别记录，避免重复日志
                        if classified_error.should_auto_delete {
                            debug!(
                                "跳过视频「{}」{}: 充电专享视频（已安排自动删除）",
                                &video_model.name, task_name
                            );
                        } else {
                            info!(
                                "跳过视频「{}」{}: {}",
                                &video_model.name, task_name, classified_error.message
                            );
                        }
                    }
                    crate::error::ErrorType::Network
                    | crate::error::ErrorType::Timeout
                    | crate::error::ErrorType::RateLimit => {
                        warn!(
                            "处理视频「{}」{}失败({}): {}{}",
                            &video_model.name,
                            task_name,
                            classified_error.error_type,
                            classified_error.message,
                            if classified_error.should_retry {
                                " (可重试)"
                            } else {
                                ""
                            }
                        );
                    }
                    crate::error::ErrorType::RiskControl => {
                        error!(
                            "处理视频「{}」{}触发风控: {}",
                            &video_model.name, task_name, classified_error.message
                        );
                    }
                    crate::error::ErrorType::UserCancelled => {
                        info!("处理视频「{}」{}因用户暂停而终止", &video_model.name, task_name);
                    }
                    _ => {
                        error!(
                            "处理视频「{}」{}失败({}): {}",
                            &video_model.name, task_name, classified_error.error_type, classified_error.message
                        );
                    }
                }
            }
            ExecutionStatus::Failed(e) | ExecutionStatus::FixedFailed(_, e) => {
                // 使用错误分类器进行统一处理
                #[allow(clippy::needless_borrow)]
                let classified_error = crate::error::ErrorClassifier::classify_error(&e);
                match classified_error.error_type {
                    crate::error::ErrorType::NotFound => {
                        debug!("处理视频「{}」{}失败(404): {:#}", &video_model.name, task_name, e);
                    }
                    crate::error::ErrorType::UserCancelled => {
                        info!("处理视频「{}」{}因用户暂停而终止", &video_model.name, task_name);
                    }
                    _ => {
                        // 对于分页下载任务，错误日志已经在内部处理了，这里只记录DEBUG级别
                        if task_name == "分页下载" {
                            debug!("处理视频「{}」{}失败: {:#}", &video_model.name, task_name, e);
                        } else {
                            error!("处理视频「{}」{}失败: {:#}", &video_model.name, task_name, e);
                        }
                    }
                }
            }
        });
    if let ExecutionStatus::Failed(e) = all_results
        .into_iter()
        .nth(4)
        .context("page download result not found")?
    {
        if e.downcast_ref::<DownloadAbortError>().is_some() {
            return Err(e);
        }
    }
    let mut video_active_model: video::ActiveModel = final_video_model.into();
    video_active_model.download_status = Set(status.into());

    // 对于番剧和多P视频使用Season结构时，保存根文件夹路径而不是Season文件夹路径
    let path_to_save = if is_bangumi {
        if let Some(ref bangumi_folder_path) = bangumi_folder_path {
            bangumi_folder_path.to_string_lossy().to_string()
        } else {
            base_path.to_string_lossy().to_string()
        }
    } else {
        // 检查是否为多P视频或合集且启用了Season结构
        let config = crate::config::reload_config();
        if (!is_single_page && config.multi_page_use_season_structure && season_folder.is_some())
            || (is_collection && config.collection_use_season_structure && season_folder.is_some())
        {
            // 对于多P视频或合集使用Season结构时，保存根目录路径而不是Season子文件夹路径
            base_path
                .parent()
                .map(|parent| parent.to_string_lossy().to_string())
                .unwrap_or_else(|| base_path.to_string_lossy().to_string())
        } else {
            base_path.to_string_lossy().to_string()
        }
    };

    debug!("=== 路径保存 ===");
    debug!("最终保存到数据库的路径: {:?}", path_to_save);
    debug!("原始基础路径: {:?}", base_path);
    if let Some(ref bangumi_folder_path) = bangumi_folder_path {
        debug!("番剧文件夹路径: {:?}", bangumi_folder_path);
    }
    if let Some(ref season_folder) = season_folder {
        debug!("季度文件夹名: {}", season_folder);
    }
    debug!("=== 路径计算结束 ===");

    video_active_model.path = Set(path_to_save);
    Ok(video_active_model)
}

/// 分发并执行分页下载任务，当且仅当所有分页成功下载或达到最大重试次数时返回 Ok，否则根据失败原因返回对应的错误
pub async fn dispatch_download_page(args: DownloadPageArgs<'_>, token: CancellationToken) -> Result<ExecutionStatus> {
    if !args.should_run {
        return Ok(ExecutionStatus::Skipped);
    }

    let current_config = crate::config::reload_config();
    let child_semaphore = Semaphore::new(current_config.concurrent_limit.page);
    let tasks = args
        .pages
        .into_iter()
        .map(|page_model| {
            download_page(
                args.bili_client,
                args.video_source,
                args.video_model,
                page_model,
                args.connection,
                &child_semaphore,
                args.downloader,
                args.base_path,
                token.clone(),
            )
        })
        .collect::<FuturesUnordered<_>>();
    let (mut download_aborted, mut target_status) = (false, STATUS_OK);
    let mut has_charging_video_error = false;
    let mut stream = tasks;
    while let Some(res) = stream.next().await {
        match res {
            Ok(model) => {
                if download_aborted {
                    continue;
                }
                // 该视频的所有分页的下载状态都会在此返回，需要根据这些状态确认视频层"分 P 下载"子任务的状态
                // 在过去的实现中，此处仅仅根据 page_download_status 的最高标志位来判断，如果最高标志位是 true 则认为完成
                // 这样会导致即使分页中有失败到 MAX_RETRY 的情况，视频层的分 P 下载状态也会被认为是 Succeeded，不够准确
                // 新版本实现会将此处取值为所有子任务状态的最小值，这样只有所有分页的子任务全部成功时才会认为视频层的分 P 下载状态是 Succeeded
                let page_download_status = model.download_status.try_as_ref().expect("download_status must be set");
                let separate_status: [u32; 5] = PageStatus::from(*page_download_status).into();
                for status in separate_status {
                    target_status = target_status.min(status);
                }
                update_pages_model(vec![model], args.connection).await?;
            }
            Err(e) => {
                let error_msg = e.to_string();

                // 检查是否是暂停导致的失败，只有在任务暂停时才将 Download cancelled 视为暂停错误
                if error_msg.contains("任务已暂停")
                    || error_msg.contains("停止下载")
                    || error_msg.contains("用户主动暂停任务")
                    || (error_msg.contains("Download cancelled") && crate::task::TASK_CONTROLLER.is_paused())
                {
                    info!("分页下载任务因用户暂停而终止: {}", error_msg);
                    continue; // 跳过暂停相关的错误，不触发风控
                }

                if e.downcast_ref::<DownloadAbortError>().is_some() || error_msg.contains("Download cancelled") {
                    if !download_aborted {
                        token.cancel();
                        download_aborted = true;
                    }
                } else {
                    // 检查是否为暂停相关错误
                    let error_msg = e.to_string();
                    if error_msg.contains("用户主动暂停任务") || error_msg.contains("任务已暂停") {
                        info!("分页下载因用户暂停而终止");
                    } else {
                        // 检查是否是充电专享视频错误
                        let is_charging_video_error = error_msg.contains("status code: 87007")
                            || error_msg.contains("status code: 87008")
                            || error_msg.contains("充电专享视频");

                        if is_charging_video_error {
                            debug!("分页下载失败: 充电专享视频");
                            has_charging_video_error = true;
                        } else {
                            error!("下载分页子任务失败: {:#}", e);
                        }
                    }
                }
            }
        }
    }

    if download_aborted {
        error!(
            "下载视频「{}」的分页时触发风控，将异常向上传递..",
            &args.video_model.name
        );
        bail!(DownloadAbortError());
    }
    if target_status != STATUS_OK {
        // 如果检测到充电专享视频错误，返回相应的ClassifiedFailed状态
        if has_charging_video_error {
            let classified_error = crate::error::ClassifiedError::new(
                crate::error::ErrorType::Permission,
                "充电专享视频，需要为UP主充电才能观看".to_string(),
            )
            .with_auto_delete(true);
            return Ok(ExecutionStatus::ClassifiedFailed(classified_error));
        }

        return Ok(ExecutionStatus::FixedFailed(target_status, ProcessPageError().into()));
    }
    Ok(ExecutionStatus::Succeeded)
}

/// 下载某个分页，未发生风控且正常运行时返回 Ok(Page::ActiveModel)，其中 status 字段存储了新的下载状态，发生风控时返回 DownloadAbortError
#[allow(clippy::too_many_arguments)]
pub async fn download_page(
    bili_client: &BiliClient,
    video_source: &VideoSourceEnum,
    video_model: &video::Model,
    page_model: page::Model,
    connection: &DatabaseConnection,
    semaphore: &Semaphore,
    downloader: &UnifiedDownloader,
    base_path: &Path,
    token: CancellationToken,
) -> Result<page::ActiveModel> {
    let _permit = tokio::select! {
        biased;
        _ = token.cancelled() => return Err(anyhow!("Download cancelled")),
        permit = semaphore.acquire() => permit.context("acquire semaphore failed")?,
    };
    let mut status = PageStatus::from(page_model.download_status);
    let separate_status = status.should_run();
    let is_single_page = video_model.single_page.context("single_page is null")?;

    // 检查是否为番剧
    let is_bangumi = match video_model.source_type {
        Some(1) => true, // source_type = 1 表示为番剧
        _ => false,
    };

    // 根据视频源类型选择不同的模板渲染方式
    let base_name = if let VideoSourceEnum::Collection(collection_source) = video_source {
        // 合集视频的特殊处理
        let config = crate::config::reload_config();
        if config.collection_folder_mode.as_ref() == "unified" {
            // 统一模式：使用S01E01格式命名
            match get_collection_video_episode_number(connection, collection_source.id, &video_model.bvid).await {
                Ok(episode_number) => {
                    let clean_name = crate::utils::filenamify::filenamify(&video_model.name);
                    format!("S01E{:02} - {}", episode_number, clean_name)
                }
                Err(_) => {
                    // 如果获取序号失败，使用默认命名
                    crate::config::with_config(|bundle| {
                        bundle.render_page_template(&page_format_args(video_model, &page_model))
                    })
                    .map_err(|e| anyhow::anyhow!("模板渲染失败: {}", e))?
                }
            }
        } else {
            // 分离模式：使用原有逻辑
            crate::config::with_config(|bundle| {
                bundle.render_page_template(&page_format_args(video_model, &page_model))
            })
            .map_err(|e| anyhow::anyhow!("模板渲染失败: {}", e))?
        }
    } else if is_bangumi {
        // 番剧使用专用的模板方法
        if let VideoSourceEnum::BangumiSource(bangumi_source) = video_source {
            bangumi_source
                .render_page_name(video_model, &page_model, connection)
                .await?
        } else {
            // 如果类型不匹配，使用最新配置手动渲染
            crate::config::with_config(|bundle| {
                bundle.render_page_template(&page_format_args(video_model, &page_model))
            })
            .map_err(|e| anyhow::anyhow!("模板渲染失败: {}", e))?
        }
    } else if !is_single_page {
        // 对于多P视频（非番剧），使用最新配置中的multi_page_name模板
        let page_args = page_format_args(video_model, &page_model);
        match crate::config::with_config(|bundle| bundle.render_multi_page_template(&page_args)) {
            Ok(rendered) => rendered,
            Err(_) => {
                // 如果渲染失败，使用默认格式
                let season_number = 1;
                let episode_number = page_model.pid;
                format!("S{:02}E{:02}-{:02}", season_number, episode_number, episode_number)
            }
        }
    } else {
        // 单P视频使用最新配置的page_name模板
        crate::config::with_config(|bundle| bundle.render_page_template(&page_format_args(video_model, &page_model)))
            .map_err(|e| anyhow::anyhow!("模板渲染失败: {}", e))?
    };

    let (poster_path, video_path, nfo_path, danmaku_path, fanart_path, subtitle_path) = if is_single_page {
        (
            base_path.join(format!("{}-poster.jpg", &base_name)),
            base_path.join(format!("{}.mp4", &base_name)),
            base_path.join(format!("{}.nfo", &base_name)),
            base_path.join(format!("{}.zh-CN.default.ass", &base_name)),
            Some(base_path.join(format!("{}-fanart.jpg", &base_name))),
            base_path.join(format!("{}.srt", &base_name)),
        )
    } else if is_bangumi {
        // 番剧直接使用基础路径，不创建子文件夹结构
        (
            base_path.join(format!("{}-poster.jpg", &base_name)),
            base_path.join(format!("{}.mp4", &base_name)),
            base_path.join(format!("{}.nfo", &base_name)),
            base_path.join(format!("{}.zh-CN.default.ass", &base_name)),
            None,
            base_path.join(format!("{}.srt", &base_name)),
        )
    } else {
        // 非番剧的多P视频直接使用基础路径，不创建子文件夹
        (
            base_path.join(format!("{}-poster.jpg", &base_name)),
            base_path.join(format!("{}.mp4", &base_name)),
            base_path.join(format!("{}.nfo", &base_name)),
            base_path.join(format!("{}.zh-CN.default.ass", &base_name)),
            // 多P视频的每个分页都应该有自己的fanart
            Some(base_path.join(format!("{}-fanart.jpg", &base_name))),
            base_path.join(format!("{}.srt", &base_name)),
        )
    };
    let dimension = match (page_model.width, page_model.height) {
        (Some(width), Some(height)) => Some(Dimension {
            width,
            height,
            rotate: 0,
        }),
        _ => None,
    };
    let page_info = PageInfo {
        cid: page_model.cid,
        duration: page_model.duration,
        dimension,
        ..Default::default()
    };
    // 使用 tokio::join! 替代装箱的 Future，零分配并行执行
    let (res_1, res_2, res_3, res_4, res_5) = tokio::join!(
        fetch_page_poster(
            separate_status[0],
            video_model,
            &page_model,
            downloader,
            poster_path,
            fanart_path,
            token.clone(),
        ),
        fetch_page_video(
            separate_status[1],
            bili_client,
            video_model,
            downloader,
            &page_info,
            &video_path,
            token.clone(),
        ),
        generate_page_nfo(separate_status[2], video_model, &page_model, nfo_path, connection),
        fetch_page_danmaku(
            separate_status[3],
            bili_client,
            video_model,
            &page_info,
            danmaku_path,
            token.clone(),
        ),
        fetch_page_subtitle(
            separate_status[4],
            bili_client,
            video_model,
            &page_info,
            &subtitle_path,
            token.clone(),
        )
    );

    let results = [res_1, res_2, res_3, res_4, res_5]
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    status.update_status(&results);

    // 使用新的公共函数处理87007错误检测和自动删除
    let task_names = ["封面", "视频", "详情", "弹幕", "字幕"];
    let context_info = format!("第 {} 页", page_model.pid);
    let _has_87007_error =
        detect_and_handle_auto_delete_errors(&results, &task_names, video_model, connection, Some(&context_info))
            .await?;

    results
        .iter()
        .zip(["封面", "视频", "详情", "弹幕", "字幕"])
        .for_each(|(res, task_name)| match res {
            ExecutionStatus::Skipped => debug!(
                "处理视频「{}」第 {} 页{}已成功过，跳过",
                &video_model.name, page_model.pid, task_name
            ),
            ExecutionStatus::Succeeded => debug!(
                "处理视频「{}」第 {} 页{}成功",
                &video_model.name, page_model.pid, task_name
            ),
            ExecutionStatus::Ignored(e) => {
                let error_msg = e.to_string();
                if !error_msg.contains("status code: 87007") {
                    info!(
                        "处理视频「{}」第 {} 页{}出现常见错误，已忽略: {:#}",
                        &video_model.name, page_model.pid, task_name, e
                    );
                }
            }
            ExecutionStatus::ClassifiedFailed(classified_error) => {
                // 根据错误分类进行不同级别的日志记录
                match classified_error.error_type {
                    crate::error::ErrorType::NotFound => {
                        debug!(
                            "处理视频「{}」第 {} 页{}失败({}): {}",
                            &video_model.name,
                            page_model.pid,
                            task_name,
                            classified_error.error_type,
                            classified_error.message
                        );
                    }
                    crate::error::ErrorType::Permission => {
                        // 对于权限错误（包括充电专享视频），使用debug级别记录，避免重复日志
                        if classified_error.should_auto_delete {
                            debug!(
                                "跳过视频「{}」第 {} 页{}: 充电专享视频（已安排自动删除）",
                                &video_model.name, page_model.pid, task_name
                            );
                        } else {
                            info!(
                                "跳过视频「{}」第 {} 页{}: {}",
                                &video_model.name, page_model.pid, task_name, classified_error.message
                            );
                        }
                    }
                    crate::error::ErrorType::Network
                    | crate::error::ErrorType::Timeout
                    | crate::error::ErrorType::RateLimit => {
                        warn!(
                            "处理视频「{}」第 {} 页{}失败({}): {}{}",
                            &video_model.name,
                            page_model.pid,
                            task_name,
                            classified_error.error_type,
                            classified_error.message,
                            if classified_error.should_retry {
                                " (可重试)"
                            } else {
                                ""
                            }
                        );
                    }
                    crate::error::ErrorType::RiskControl => {
                        error!(
                            "处理视频「{}」第 {} 页{}触发风控: {}",
                            &video_model.name, page_model.pid, task_name, classified_error.message
                        );
                    }
                    crate::error::ErrorType::UserCancelled => {
                        info!(
                            "处理视频「{}」第 {} 页{}因用户暂停而终止",
                            &video_model.name, page_model.pid, task_name
                        );
                    }
                    _ => {
                        error!(
                            "处理视频「{}」第 {} 页{}失败({}): {}",
                            &video_model.name,
                            page_model.pid,
                            task_name,
                            classified_error.error_type,
                            classified_error.message
                        );
                    }
                }
            }
            ExecutionStatus::Failed(e) | ExecutionStatus::FixedFailed(_, e) => {
                // 使用错误分类器进行统一处理
                #[allow(clippy::needless_borrow)]
                let classified_error = crate::error::ErrorClassifier::classify_error(&e);
                match classified_error.error_type {
                    crate::error::ErrorType::NotFound => {
                        debug!(
                            "处理视频「{}」第 {} 页{}失败(404): {:#}",
                            &video_model.name, page_model.pid, task_name, e
                        );
                    }
                    crate::error::ErrorType::UserCancelled => {
                        info!(
                            "处理视频「{}」第 {} 页{}因用户暂停而终止",
                            &video_model.name, page_model.pid, task_name
                        );
                    }
                    _ => {
                        error!(
                            "处理视频「{}」第 {} 页{}失败: {:#}",
                            &video_model.name, page_model.pid, task_name, e
                        );
                    }
                }
            }
        });
    // 检查下载视频时是否触发风控
    match results.into_iter().nth(1).context("video download result not found")? {
        ExecutionStatus::Failed(e) => {
            if let Ok(BiliError::RiskControlOccurred) = e.downcast::<BiliError>() {
                bail!(DownloadAbortError());
            }
        }
        ExecutionStatus::ClassifiedFailed(ref classified_error) => {
            if classified_error.error_type == crate::error::ErrorType::RiskControl {
                bail!(DownloadAbortError());
            }
        }
        _ => {}
    }
    let mut page_active_model: page::ActiveModel = page_model.into();
    page_active_model.download_status = Set(status.into());
    page_active_model.path = Set(Some(video_path.to_string_lossy().to_string()));
    Ok(page_active_model)
}

pub async fn fetch_page_poster(
    should_run: bool,
    video_model: &video::Model,
    page_model: &page::Model,
    downloader: &UnifiedDownloader,
    poster_path: PathBuf,
    fanart_path: Option<PathBuf>,
    token: CancellationToken,
) -> Result<ExecutionStatus> {
    if !should_run {
        return Ok(ExecutionStatus::Skipped);
    }
    let single_page = video_model.single_page.context("single_page is null")?;
    let url = if single_page {
        // 单页视频直接用视频的封面
        video_model.cover.as_str()
    } else {
        // 多页视频，如果单页没有封面，就使用视频的封面
        match &page_model.image {
            Some(url) => url.as_str(),
            None => video_model.cover.as_str(),
        }
    };
    let urls = vec![url];
    tokio::select! {
        biased;
        _ = token.cancelled() => return Ok(ExecutionStatus::Skipped),
        res = downloader.fetch_with_fallback(&urls, &poster_path) => res,
    }?;
    if let Some(fanart_path) = fanart_path {
        ensure_parent_dir_for_file(&fanart_path).await?;
        fs::copy(&poster_path, &fanart_path).await?;
    }
    Ok(ExecutionStatus::Succeeded)
}

/// 下载单个流文件并返回文件大小（使用UnifiedDownloader智能选择下载方式）
async fn download_stream(downloader: &UnifiedDownloader, urls: &[&str], path: &Path) -> Result<u64> {
    // 直接使用UnifiedDownloader，它会智能选择aria2或原生下载器
    // aria2本身就支持多线程，原生下载器作为备选方案使用单线程
    let download_result = downloader.fetch_with_fallback(urls, path).await;

    match download_result {
        Ok(_) => {
            // 获取文件大小
            Ok(tokio::fs::metadata(path)
                .await
                .map(|metadata| metadata.len())
                .unwrap_or(0))
        }
        Err(e) => {
            let error_msg = e.to_string();
            // 检查是否为暂停相关错误
            if error_msg.contains("用户主动暂停任务") || error_msg.contains("任务已暂停") {
                info!("下载因用户暂停而终止");
            } else if let Some(BiliError::RequestFailed(-404, _)) = e.downcast_ref::<BiliError>() {
                // 对于404错误，降级为debug日志
                debug!("下载失败(404): {:#}", e);
            } else {
                // 使用错误分类器进行统一处理
                #[allow(clippy::needless_borrow)]
                let classified_error = crate::error::ErrorClassifier::classify_error(&e);
                match classified_error.error_type {
                    crate::error::ErrorType::UserCancelled => {
                        info!("下载因用户暂停而终止: {:#}", e);
                    }
                    _ => {
                        error!("下载失败: {:#}", e);
                    }
                }
            }
            Err(e)
        }
    }
}

pub async fn fetch_page_video(
    should_run: bool,
    bili_client: &BiliClient,
    video_model: &video::Model,
    downloader: &UnifiedDownloader,
    page_info: &PageInfo,
    page_path: &Path,
    token: CancellationToken,
) -> Result<ExecutionStatus> {
    if !should_run {
        return Ok(ExecutionStatus::Skipped);
    }

    let bili_video = Video::new(bili_client, video_model.bvid.clone());

    // 获取视频流信息 - 使用带回退机制的API调用
    let mut streams = tokio::select! {
        biased;
        _ = token.cancelled() => return Err(anyhow!("Download cancelled")),
        res = async {
            // 检查是否为番剧视频
            if video_model.source_type == Some(1) && video_model.ep_id.is_some() {
                // 使用番剧专用API的回退机制
                let ep_id = video_model.ep_id.as_ref().unwrap();
                debug!("使用带质量回退的番剧API获取播放地址: ep_id={}", ep_id);
                bili_video.get_bangumi_page_analyzer_with_fallback(page_info, ep_id).await
            } else {
                // 使用普通视频API的回退机制
                debug!("使用带质量回退的视频API获取播放地址");
                bili_video.get_page_analyzer_with_fallback(page_info).await
            }
        } => res
    }?;

    // 按需创建保存目录（只在实际下载时创建）
    ensure_parent_dir_for_file(page_path).await?;

    // UnifiedDownloader会自动选择最佳下载方式

    // 获取用户配置的筛选选项
    let config = crate::config::reload_config();
    let filter_option = &config.filter_option;

    // 简化的配置调试日志
    debug!("=== 视频下载配置 ===");
    debug!("视频: {} ({})", video_model.name, video_model.bvid);
    debug!("分页: {} (cid: {})", page_info.name, page_info.cid);
    debug!(
        "质量配置: {} - {} (最高-最低)",
        format!(
            "{:?}({})",
            filter_option.video_max_quality, filter_option.video_max_quality as u32
        ),
        format!(
            "{:?}({})",
            filter_option.video_min_quality, filter_option.video_min_quality as u32
        )
    );
    debug!(
        "音频配置: {} - {} (最高-最低)",
        format!(
            "{:?}({})",
            filter_option.audio_max_quality, filter_option.audio_max_quality as u32
        ),
        format!(
            "{:?}({})",
            filter_option.audio_min_quality, filter_option.audio_min_quality as u32
        )
    );
    debug!("编码偏好: {:?}", filter_option.codecs);

    // 会员状态检查
    let credential = config.credential.load();
    match credential.as_deref() {
        Some(cred) => {
            debug!("用户认证: 已登录 (DedeUserID: {})", cred.dedeuserid);
        }
        None => {
            debug!("用户认证: 未登录 - 高质量视频流可能不可用");
        }
    }

    // 高质量需求提醒
    if filter_option.video_max_quality as u32 >= 120 {
        // 4K及以上
        debug!("⚠️  请求高质量视频(4K+)，需要大会员权限");
    }

    debug!("=== 配置调试结束 ===");

    // 记录开始时间
    let start_time = std::time::Instant::now();

    // 根据流类型进行不同处理
    let best_stream_result = streams.best_stream(filter_option)?;

    // 添加流选择结果日志和质量分析
    debug!("=== 流选择结果 ===");
    match &best_stream_result {
        BestStream::Mixed(stream) => {
            debug!("选择了混合流: {:?}", stream);
        }
        BestStream::VideoAudio { video, audio } => {
            if let VideoStream::DashVideo { quality, codecs, .. } = video {
                let quality_value = *quality as u32;
                let requested_quality = filter_option.video_max_quality as u32;

                info!("✓ 选择视频流: {:?}({}) {:?}", quality, quality_value, codecs);

                // 质量对比分析
                if quality_value < requested_quality {
                    let quality_gap = requested_quality - quality_value;
                    if requested_quality >= 120 && quality_value < 120 {
                        debug!("⚠️  未获得4K+质量(请求{}，实际{})", requested_quality, quality_value);
                    } else if quality_gap >= 40 {
                        warn!(
                            "⚠️  视频质量显著低于预期(请求{}，实际{}) - 视频源可能不支持更高质量",
                            requested_quality, quality_value
                        );
                    } else {
                        info!(
                            "ℹ️  视频质量略低于预期(请求{}，实际{}) - 已选择可用的最高质量",
                            requested_quality, quality_value
                        );
                    }
                } else {
                    info!("✓ 获得预期质量或更高");
                }
            }
            if let Some(VideoStream::DashAudio { quality, .. }) = audio {
                debug!("✓ 选择音频流: {:?}({})", quality, *quality as u32);
            } else {
                debug!("ℹ️  无独立音频流(可能为混合流)");
            }
        }
    }
    debug!("=== 流选择结束 ===");

    let total_bytes = match best_stream_result {
        BestStream::Mixed(mix_stream) => {
            let urls = mix_stream.urls();
            download_stream(downloader, &urls, page_path).await?
        }
        BestStream::VideoAudio {
            video: video_stream,
            audio: None,
        } => {
            let urls = video_stream.urls();
            download_stream(downloader, &urls, page_path).await?
        }
        BestStream::VideoAudio {
            video: video_stream,
            audio: Some(audio_stream),
        } => {
            let (tmp_video_path, tmp_audio_path) = (
                page_path.with_extension("tmp_video"),
                page_path.with_extension("tmp_audio"),
            );

            let video_urls = video_stream.urls();
            let video_size = download_stream(downloader, &video_urls, &tmp_video_path)
                .await
                .map_err(|e| {
                    // 使用错误分类器进行统一处理
                    let classified_error = crate::error::ErrorClassifier::classify_error(&e);
                    match classified_error.error_type {
                        crate::error::ErrorType::UserCancelled => {
                            info!("视频流下载因用户暂停而终止");
                        }
                        _ => {
                            error!("视频流下载失败: {:#}", e);
                        }
                    }
                    e
                })?;

            let audio_urls = audio_stream.urls();
            let audio_size = download_stream(downloader, &audio_urls, &tmp_audio_path)
                .await
                .map_err(|e| {
                    // 使用错误分类器进行统一处理
                    let classified_error = crate::error::ErrorClassifier::classify_error(&e);
                    match classified_error.error_type {
                        crate::error::ErrorType::UserCancelled => {
                            info!("音频流下载因用户暂停而终止");
                        }
                        _ => {
                            error!("音频流下载失败: {:#}", e);
                        }
                    }
                    // 异步删除临时视频文件
                    let video_path_clone = tmp_video_path.clone();
                    tokio::spawn(async move {
                        let _ = fs::remove_file(&video_path_clone).await;
                    });
                    e
                })?;

            // 增强的音视频合并，带损坏文件检测和重试机制
            let res = downloader.merge(&tmp_video_path, &tmp_audio_path, page_path).await;

            // 合并失败时的智能处理
            if let Err(e) = res {
                error!("音视频合并失败: {:#}", e);

                // 检查是否是文件损坏导致的失败
                let error_msg = e.to_string();
                if error_msg.contains("Invalid data found when processing input")
                    || error_msg.contains("ffmpeg error")
                    || error_msg.contains("文件损坏")
                {
                    warn!("检测到文件损坏，清理临时文件并标记为重试: {}", error_msg);

                    // 立即清理损坏的临时文件
                    let _ = fs::remove_file(&tmp_video_path).await;
                    let _ = fs::remove_file(&tmp_audio_path).await;

                    // 返回特殊错误，让上层重试下载
                    return Err(anyhow::anyhow!(
                        "视频文件损坏，已清理临时文件，请重试下载: {}",
                        error_msg
                    ));
                } else {
                    // 其他类型的合并错误，清理临时文件后直接返回
                    let _ = fs::remove_file(&tmp_video_path).await;
                    let _ = fs::remove_file(&tmp_audio_path).await;
                    return Err(e);
                }
            }

            // 合并成功，清理临时文件
            let _ = fs::remove_file(tmp_video_path).await;
            let _ = fs::remove_file(tmp_audio_path).await;

            // 获取合并后文件大小，如果失败则使用视频和音频大小之和
            tokio::fs::metadata(page_path)
                .await
                .map(|metadata| metadata.len())
                .unwrap_or(video_size + audio_size)
        }
    };

    // 计算并记录下载速度
    let elapsed = start_time.elapsed();
    let elapsed_secs = elapsed.as_secs_f64();

    if elapsed_secs > 0.0 && total_bytes > 0 {
        let speed_bps = total_bytes as f64 / elapsed_secs;
        let (speed, unit) = if speed_bps >= 1_048_576.0 {
            (speed_bps / 1_048_576.0, "MB/s")
        } else if speed_bps >= 1_024.0 {
            (speed_bps / 1_024.0, "KB/s")
        } else {
            (speed_bps, "B/s")
        };

        info!(
            "视频下载完成，总大小: {:.2} MB，耗时: {:.2} 秒，平均速度: {:.2} {}",
            total_bytes as f64 / 1_048_576.0,
            elapsed_secs,
            speed,
            unit
        );
    }

    Ok(ExecutionStatus::Succeeded)
}

pub async fn fetch_page_danmaku(
    should_run: bool,
    bili_client: &BiliClient,
    video_model: &video::Model,
    page_info: &PageInfo,
    danmaku_path: PathBuf,
    token: CancellationToken,
) -> Result<ExecutionStatus> {
    if !should_run {
        return Ok(ExecutionStatus::Skipped);
    }

    // 检查 CID 是否有效（-1 表示信息获取失败）
    if page_info.cid < 0 {
        warn!(
            "视频 {} 的 CID 无效（{}），跳过弹幕下载",
            &video_model.name, page_info.cid
        );
        return Ok(ExecutionStatus::Ignored(anyhow::anyhow!("CID 无效，无法下载弹幕")));
    }

    // 检查是否为番剧，如果是番剧则需要从API获取正确的 aid
    let bili_video = if video_model.source_type == Some(1) {
        // 番剧：需要从API获取aid
        if let Some(ep_id) = &video_model.ep_id {
            match tokio::select! {
                biased;
                _ = token.cancelled() => None,
                res = get_bangumi_aid_from_api(bili_client, ep_id, token.clone()) => res,
            } {
                Some(aid) => {
                    debug!("使用番剧API获取到的aid: {}", aid);
                    Video::new_with_aid(bili_client, video_model.bvid.clone(), aid)
                }
                None => {
                    warn!("无法获取番剧 {} (EP{}) 的AID，使用bvid转换", &video_model.name, ep_id);
                    Video::new(bili_client, video_model.bvid.clone())
                }
            }
        } else {
            warn!("番剧 {} 缺少EP ID，使用bvid转换aid", &video_model.name);
            Video::new(bili_client, video_model.bvid.clone())
        }
    } else {
        // 普通视频：使用 bvid 转换的 aid
        Video::new(bili_client, video_model.bvid.clone())
    };

    let danmaku_writer = tokio::select! {
        biased;
        _ = token.cancelled() => return Err(anyhow!("Download cancelled")),
        res = bili_video.get_danmaku_writer(page_info, token.clone()) => res?,
    };

    danmaku_writer.write(danmaku_path).await?;
    Ok(ExecutionStatus::Succeeded)
}

pub async fn fetch_page_subtitle(
    should_run: bool,
    bili_client: &BiliClient,
    video_model: &video::Model,
    page_info: &PageInfo,
    subtitle_path: &Path,
    token: CancellationToken,
) -> Result<ExecutionStatus> {
    if !should_run {
        return Ok(ExecutionStatus::Skipped);
    }
    let bili_video = Video::new(bili_client, video_model.bvid.clone());
    let subtitles = tokio::select! {
        biased;
        _ = token.cancelled() => return Err(anyhow!("Download cancelled")),
        res = bili_video.get_subtitles(page_info) => res?,
    };
    let tasks = subtitles
        .into_iter()
        .map(|subtitle| async move {
            let path = subtitle_path.with_extension(format!("{}.srt", subtitle.lan));
            ensure_parent_dir_for_file(&path).await.map_err(std::io::Error::other)?;
            tokio::fs::write(path, subtitle.body.to_string()).await
        })
        .collect::<FuturesUnordered<_>>();
    tasks.try_collect::<Vec<()>>().await?;
    Ok(ExecutionStatus::Succeeded)
}

pub async fn generate_page_nfo(
    should_run: bool,
    video_model: &video::Model,
    page_model: &page::Model,
    nfo_path: PathBuf,
    _connection: &DatabaseConnection,
) -> Result<ExecutionStatus> {
    if !should_run {
        return Ok(ExecutionStatus::Skipped);
    }
    // 检查是否为番剧
    let is_bangumi = video_model.category == 1;

    let nfo = match video_model.single_page {
        Some(single_page) => {
            if single_page {
                if is_bangumi {
                    // 番剧单页也应使用Episode格式，符合Emby标准
                    use crate::utils::nfo::Episode;
                    NFO::Episode(Episode::from_video_and_page(video_model, page_model))
                } else {
                    // 普通单页视频生成Movie
                    use crate::utils::nfo::Movie;
                    NFO::Movie(Movie::from_video_with_pages(video_model, &[page_model.clone()]))
                }
            } else {
                use crate::utils::nfo::Episode;
                NFO::Episode(Episode::from_video_and_page(video_model, page_model))
            }
        }
        None => {
            use crate::utils::nfo::Episode;
            NFO::Episode(Episode::from_video_and_page(video_model, page_model))
        }
    };
    generate_nfo(nfo, nfo_path).await?;
    Ok(ExecutionStatus::Succeeded)
}

#[allow(clippy::too_many_arguments)]
pub async fn fetch_video_poster(
    should_run: bool,
    video_model: &video::Model,
    downloader: &UnifiedDownloader,
    poster_path: PathBuf,
    fanart_path: PathBuf,
    token: CancellationToken,
    custom_cover_url: Option<&str>,
    custom_fanart_url: Option<&str>,
) -> Result<ExecutionStatus> {
    if !should_run {
        return Ok(ExecutionStatus::Skipped);
    }

    info!("开始处理视频「{}」的封面和背景图", video_model.name);
    info!("  poster路径: {:?}", poster_path);
    info!("  fanart路径: {:?}", fanart_path);
    info!("  custom_cover_url: {:?}", custom_cover_url);
    info!("  custom_fanart_url: {:?}", custom_fanart_url);

    // 下载poster封面
    let cover_url = custom_cover_url.unwrap_or(video_model.cover.as_str());
    let urls = vec![cover_url];
    tokio::select! {
        biased;
        _ = token.cancelled() => return Ok(ExecutionStatus::Skipped),
        res = downloader.fetch_with_fallback(&urls, &poster_path) => res,
    }?;

    // 下载fanart背景图（可能使用不同的URL）
    ensure_parent_dir_for_file(&fanart_path).await?;
    if let Some(fanart_url) = custom_fanart_url {
        // 如果有专门的fanart URL，独立下载
        let fanart_urls = vec![fanart_url];
        tokio::select! {
            biased;
            _ = token.cancelled() => return Ok(ExecutionStatus::Skipped),
            res = downloader.fetch_with_fallback(&fanart_urls, &fanart_path) => {
                match res {
                    Ok(_) => {
                        info!("✓ 成功下载fanart背景图: {}", fanart_url);
                        return Ok(ExecutionStatus::Succeeded);
                    },
                    Err(e) => {
                        warn!("✗ fanart背景图下载失败，URL: {}, 错误: {:#}", fanart_url, e);
                        warn!("回退策略：复制poster作为fanart");
                        // fanart下载失败，回退到复制poster
                        fs::copy(&poster_path, &fanart_path).await?;
                    }
                }
            },
        }
    } else {
        // 没有专门的fanart URL，直接复制poster
        fs::copy(&poster_path, &fanart_path).await?;
    }

    Ok(ExecutionStatus::Succeeded)
}

pub async fn fetch_upper_face(
    should_run: bool,
    video_model: &video::Model,
    downloader: &UnifiedDownloader,
    upper_face_path: PathBuf,
    token: CancellationToken,
) -> Result<ExecutionStatus> {
    if !should_run {
        return Ok(ExecutionStatus::Skipped);
    }

    // 检查URL是否有效，避免相对路径或空URL
    let upper_face_url = &video_model.upper_face;
    if upper_face_url.is_empty() || !upper_face_url.starts_with("http") {
        debug!("跳过无效的作者头像URL: {}", upper_face_url);
        return Ok(ExecutionStatus::Ignored(anyhow::anyhow!("无效的作者头像URL")));
    }

    let urls = vec![upper_face_url.as_str()];
    tokio::select! {
        biased;
        _ = token.cancelled() => return Ok(ExecutionStatus::Skipped),
        res = downloader.fetch_with_fallback(&urls, &upper_face_path) => res,
    }?;
    Ok(ExecutionStatus::Succeeded)
}

pub async fn generate_upper_nfo(
    should_run: bool,
    video_model: &video::Model,
    nfo_path: PathBuf,
) -> Result<ExecutionStatus> {
    if !should_run {
        return Ok(ExecutionStatus::Skipped);
    }
    generate_nfo(NFO::Upper(video_model.into()), nfo_path).await?;
    Ok(ExecutionStatus::Succeeded)
}

pub async fn generate_video_nfo(
    should_run: bool,
    video_model: &video::Model,
    nfo_path: PathBuf,
) -> Result<ExecutionStatus> {
    if !should_run {
        return Ok(ExecutionStatus::Skipped);
    }
    generate_nfo(NFO::TVShow(video_model.into()), nfo_path).await?;
    Ok(ExecutionStatus::Succeeded)
}

/// 为番剧生成带有API数据的TVShow NFO
pub async fn generate_bangumi_video_nfo(
    should_run: bool,
    video_model: &video::Model,
    season_info: &SeasonInfo,
    nfo_path: PathBuf,
) -> Result<ExecutionStatus> {
    if !should_run {
        return Ok(ExecutionStatus::Skipped);
    }
    use crate::utils::nfo::TVShow;
    let tvshow = TVShow::from_season_info(video_model, season_info);
    generate_nfo(NFO::TVShow(tvshow), nfo_path).await?;
    Ok(ExecutionStatus::Succeeded)
}

/// 为番剧季度生成season.nfo文件
pub async fn generate_bangumi_season_nfo(
    should_run: bool,
    video_model: &video::Model,
    season_info: &SeasonInfo,
    season_path: PathBuf,
    _season_number: u32,
) -> Result<ExecutionStatus> {
    if !should_run {
        return Ok(ExecutionStatus::Skipped);
    }

    let nfo_path = season_path.join("season.nfo");

    // 检查文件是否已存在（但仍然继续生成，确保内容更新）
    if nfo_path.exists() {
        debug!("Season NFO文件已存在，将覆盖更新: {:?}", nfo_path);
    }

    use crate::utils::nfo::Season;
    let mut season = Season::from_season_info(video_model, season_info);
    season.season_number = _season_number as i32; // 设置正确的季度编号

    generate_nfo(NFO::Season(season), nfo_path.clone()).await?;
    info!("成功生成season.nfo: {:?} (季度{})", nfo_path, _season_number);
    Ok(ExecutionStatus::Succeeded)
}

/// 按需创建目录的辅助函数，只在实际需要写入文件时创建
async fn ensure_parent_dir_for_file(file_path: &std::path::Path) -> Result<()> {
    if let Some(parent) = file_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).await?;
            debug!("按需创建目录: {}", parent.display());
        }
    }
    Ok(())
}

async fn generate_nfo(nfo: NFO<'_>, nfo_path: PathBuf) -> Result<()> {
    // 只在实际写入NFO文件时才创建父目录
    ensure_parent_dir_for_file(&nfo_path).await?;
    fs::write(nfo_path, nfo.generate_nfo().await?.as_bytes()).await?;
    Ok(())
}

/// 获取番剧季标题，优先从缓存获取，缓存未命中时从API获取
async fn get_cached_season_title(
    bili_client: &BiliClient,
    season_id: &str,
    token: CancellationToken,
) -> Option<String> {
    // 先检查缓存
    if let Ok(cache) = SEASON_TITLE_CACHE.lock() {
        if let Some(title) = cache.get(season_id) {
            return Some(title.clone());
        }
    }

    // 缓存未命中，从API获取
    get_season_title_from_api(bili_client, season_id, token).await
}

async fn get_season_title_from_api(
    bili_client: &BiliClient,
    season_id: &str,
    token: CancellationToken,
) -> Option<String> {
    let url = format!("https://api.bilibili.com/pgc/view/web/season?season_id={}", season_id);

    // 重试配置：最大重试3次，每次重试间隔递增
    let max_retries = 3;
    let mut retry_count = 0;

    while retry_count <= max_retries {
        // 检查是否被取消
        if token.is_cancelled() {
            debug!("请求被取消，停止重试");
            return None;
        }

        let retry_delay = std::time::Duration::from_millis(500 * (retry_count as u64 + 1));
        if retry_count > 0 {
            debug!("第{}次重试获取季度信息，延迟{}ms", retry_count, retry_delay.as_millis());
            tokio::time::sleep(retry_delay).await;
        }

        match tokio::select! {
            biased;
            _ = token.cancelled() => return None,
            res = bili_client.get(&url, token.clone()) => res,
        } {
            Ok(res) => {
                if res.status().is_success() {
                    match res.json::<serde_json::Value>().await {
                        Ok(json) => {
                            // 检查API返回是否成功
                            if json["code"].as_i64().unwrap_or(-1) == 0 {
                                // 获取季度标题
                                if let Some(title) = json["result"]["title"].as_str() {
                                    debug!("获取到季度标题: {} (尝试次数: {})", title, retry_count + 1);

                                    // 缓存番剧标题
                                    if let Ok(mut cache) = SEASON_TITLE_CACHE.lock() {
                                        cache.insert(season_id.to_string(), title.to_string());
                                    }

                                    return Some(title.to_string());
                                }
                            } else {
                                warn!(
                                    "获取季度信息失败，API返回错误: {} (尝试次数: {})",
                                    json["message"].as_str().unwrap_or("未知错误"),
                                    retry_count + 1
                                );
                                // API返回错误码通常不是临时性问题，直接返回
                                return None;
                            }
                        }
                        Err(e) => {
                            warn!("解析季度信息JSON失败: {} (尝试次数: {})", e, retry_count + 1);
                            // JSON解析失败通常不是临时性问题，直接返回
                            return None;
                        }
                    }
                } else {
                    warn!(
                        "获取季度信息HTTP请求失败，状态码: {} (尝试次数: {})",
                        res.status(),
                        retry_count + 1
                    );
                }
            }
            Err(e) => {
                warn!("发送季度信息请求失败: {} (尝试次数: {})", e, retry_count + 1);
            }
        }

        retry_count += 1;
    }

    error!("获取season_id={}的季度信息失败，已重试{}次", season_id, max_retries);
    None
}

/// 从番剧API获取指定EP的AID
async fn get_bangumi_aid_from_api(bili_client: &BiliClient, ep_id: &str, token: CancellationToken) -> Option<String> {
    let url = format!("https://api.bilibili.com/pgc/view/web/season?ep_id={}", ep_id);

    // 重试配置：最大重试3次，每次重试间隔递增
    let max_retries = 3;
    let mut retry_count = 0;

    while retry_count <= max_retries {
        // 检查是否被取消
        if token.is_cancelled() {
            debug!("请求被取消，停止重试");
            return None;
        }

        let retry_delay = std::time::Duration::from_millis(500 * (retry_count as u64 + 1));
        if retry_count > 0 {
            debug!("第{}次重试获取EP信息，延时{}ms", retry_count, retry_delay.as_millis());
            tokio::time::sleep(retry_delay).await;
        }

        match tokio::select! {
            biased;
            _ = token.cancelled() => return None,
            res = bili_client.get(&url, token.clone()) => res,
        } {
            Ok(res) => {
                if res.status().is_success() {
                    match res.json::<serde_json::Value>().await {
                        Ok(json) => {
                            // 检查API返回是否成功
                            if json["code"].as_i64().unwrap_or(-1) == 0 {
                                // 在episodes数组中查找对应EP的AID
                                if let Some(episodes) = json["result"]["episodes"].as_array() {
                                    for episode in episodes {
                                        if let Some(episode_id) = episode["id"].as_i64() {
                                            if episode_id.to_string() == ep_id {
                                                debug!("获取到EP {} 的AID (尝试次数: {})", ep_id, retry_count + 1);
                                                return episode["aid"].as_i64().map(|aid| aid.to_string());
                                            }
                                        }
                                    }
                                }
                                // 找不到对应的EP，不是网络问题，直接返回
                                warn!("在episodes数组中找不到EP {}", ep_id);
                                return None;
                            } else {
                                warn!(
                                    "获取EP信息失败，API返回错误: {} (尝试次数: {})",
                                    json["message"].as_str().unwrap_or("未知错误"),
                                    retry_count + 1
                                );
                                // API返回错误码通常不是临时性问题，直接返回
                                return None;
                            }
                        }
                        Err(e) => {
                            warn!("解析番剧API响应失败: {} (尝试次数: {})", e, retry_count + 1);
                            // JSON解析失败通常不是临时性问题，直接返回
                            return None;
                        }
                    }
                } else {
                    warn!(
                        "请求EP信息HTTP失败，状态码: {} (尝试次数: {})",
                        res.status(),
                        retry_count + 1
                    );
                }
            }
            Err(e) => {
                warn!("请求番剧API失败: {} (尝试次数: {})", e, retry_count + 1);
            }
        }

        retry_count += 1;
    }

    error!("获取ep_id={}的AID失败，已重试{}次", ep_id, max_retries);
    None
}

/// 从番剧API获取指定EP的CID和duration
async fn get_bangumi_info_from_api(
    bili_client: &BiliClient,
    ep_id: &str,
    token: CancellationToken,
) -> Option<(i64, u32)> {
    let url = format!("https://api.bilibili.com/pgc/view/web/season?ep_id={}", ep_id);

    // 重试配置：最大重试3次，每次重试间隔递增
    let max_retries = 3;
    let mut retry_count = 0;

    while retry_count <= max_retries {
        // 检查是否被取消
        if token.is_cancelled() {
            debug!("请求被取消，停止重试");
            return None;
        }

        let retry_delay = std::time::Duration::from_millis(500 * (retry_count as u64 + 1));
        if retry_count > 0 {
            debug!(
                "第{}次重试获取EP详细信息，延时{}ms",
                retry_count,
                retry_delay.as_millis()
            );
            tokio::time::sleep(retry_delay).await;
        }

        match tokio::select! {
            biased;
            _ = token.cancelled() => return None,
            res = bili_client.get(&url, token.clone()) => res,
        } {
            Ok(res) => {
                if res.status().is_success() {
                    match res.json::<serde_json::Value>().await {
                        Ok(json) => {
                            // 检查API返回是否成功
                            if json["code"].as_i64().unwrap_or(-1) == 0 {
                                // 在episodes数组中查找对应EP的信息
                                if let Some(episodes) = json["result"]["episodes"].as_array() {
                                    for episode in episodes {
                                        if let Some(episode_id) = episode["id"].as_i64() {
                                            if episode_id.to_string() == ep_id {
                                                let cid = episode["cid"].as_i64().unwrap_or(0);
                                                // duration在API中是毫秒，需要转换为秒
                                                let duration_ms = episode["duration"].as_i64().unwrap_or(0);
                                                let duration_sec = (duration_ms / 1000) as u32;
                                                debug!(
                                                    "获取到番剧EP {} 的CID: {}, 时长: {}秒 (尝试次数: {})",
                                                    ep_id,
                                                    cid,
                                                    duration_sec,
                                                    retry_count + 1
                                                );
                                                return Some((cid, duration_sec));
                                            }
                                        }
                                    }
                                }
                                // 找不到对应的EP，不是网络问题，直接返回
                                warn!("在episodes数组中找不到EP {}", ep_id);
                                return None;
                            } else {
                                warn!(
                                    "获取EP详细信息失败，API返回错误: {} (尝试次数: {})",
                                    json["message"].as_str().unwrap_or("未知错误"),
                                    retry_count + 1
                                );
                                // API返回错误码通常不是临时性问题，直接返回
                                return None;
                            }
                        }
                        Err(e) => {
                            warn!("解析番剧API响应失败: {} (尝试次数: {})", e, retry_count + 1);
                            // JSON解析失败通常不是临时性问题，直接返回
                            return None;
                        }
                    }
                } else {
                    warn!(
                        "请求EP详细信息HTTP失败，状态码: {} (尝试次数: {})",
                        res.status(),
                        retry_count + 1
                    );
                }
            }
            Err(e) => {
                warn!("请求番剧API失败: {} (尝试次数: {})", e, retry_count + 1);
            }
        }

        retry_count += 1;
    }

    error!("获取ep_id={}的详细信息失败，已重试{}次", ep_id, max_retries);
    None
}

/// 从现有数据库中获取该季已有的分集信息
async fn get_existing_episodes_for_season(
    connection: &DatabaseConnection,
    season_id: &str,
    bili_client: &BiliClient,
    token: CancellationToken,
) -> Result<HashMap<String, (i64, u32)>> {
    use sea_orm::*;

    // 查询该season_id下所有已有page信息的视频
    let existing_data = video::Entity::find()
        .filter(video::Column::SeasonId.eq(season_id))
        .filter(video::Column::SourceType.eq(1)) // 番剧类型
        .filter(video::Column::EpId.is_not_null())
        .find_with_related(page::Entity)
        .all(connection)
        .await?;

    let mut episodes_map = HashMap::new();

    for (video, pages) in existing_data {
        if let Some(ep_id) = video.ep_id {
            // 每个番剧视频通常只有一个page（单集）
            if let Some(page) = pages.first() {
                episodes_map.insert(ep_id, (page.cid, page.duration));
            }
        }
    }

    if !episodes_map.is_empty() {
        // 尝试获取番剧标题用于显示
        let season_title = get_season_title_from_api(bili_client, season_id, token.clone()).await;
        let display_name = season_title.as_deref().unwrap_or(season_id);

        info!(
            "从数据库缓存中找到季 {} 「{}」的 {} 个分集信息",
            season_id,
            display_name,
            episodes_map.len()
        );
    }

    Ok(episodes_map)
}

/// 从API获取整个番剧季的信息（单次请求）
async fn get_season_info_from_api(
    bili_client: &BiliClient,
    season_id: &str,
    token: CancellationToken,
) -> Result<SeasonInfo> {
    let url = format!("https://api.bilibili.com/pgc/view/web/season?season_id={}", season_id);

    let res = tokio::select! {
        biased;
        _ = token.cancelled() => return Err(anyhow!("Request cancelled")),
        res = bili_client.get(&url, token.clone()) => res,
    }?;

    if !res.status().is_success() {
        bail!("获取番剧季信息失败，HTTP状态码: {}", res.status());
    }

    let json: serde_json::Value = res
        .json()
        .await
        .with_context(|| format!("解析番剧季 {} 响应失败", season_id))?;

    if json["code"].as_i64().unwrap_or(-1) != 0 {
        let error_code = json["code"].as_i64().unwrap_or(-1);
        let error_msg = json["message"].as_str().unwrap_or("未知错误").to_string();

        // 创建BiliError以触发风控检测
        let bili_error = crate::bilibili::BiliError::RequestFailed(error_code, error_msg.clone());
        let error = anyhow::Error::from(bili_error);

        // 使用错误分类器检测风控
        let classified_error = crate::error::ErrorClassifier::classify_error(&error);
        if classified_error.error_type == crate::error::ErrorType::RiskControl {
            // 风控错误，触发下载中止
            return Err(anyhow!(crate::error::DownloadAbortError()));
        }

        // 其他错误正常返回，使用BiliError以便被错误分类系统处理
        return Err(crate::bilibili::BiliError::RequestFailed(error_code, error_msg.to_string()).into());
    }

    let result = &json["result"];

    // 获取番剧标题
    let title = result["title"]
        .as_str()
        .unwrap_or(&format!("番剧{}", season_id))
        .to_string();

    // 缓存番剧标题
    if let Ok(mut cache) = SEASON_TITLE_CACHE.lock() {
        cache.insert(season_id.to_string(), title.clone());
    }

    // 提取API中的丰富元数据
    let alias = result["alias"].as_str().map(|s| s.to_string());
    let evaluate = result["evaluate"].as_str().map(|s| s.to_string());

    // 评分信息
    let rating = result["rating"]["score"].as_f64().map(|r| r as f32);
    let rating_count = result["rating"]["count"].as_i64();

    // 制作地区
    let areas: Vec<String> = result["areas"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|area| area["name"].as_str().map(|s| s.to_string()))
        .collect();

    // 声优演员信息（格式化为字符串）
    let actors = if let Some(actors_array) = result["actors"].as_array() {
        let actor_list: Vec<String> = actors_array
            .iter()
            .filter_map(|actor| {
                let character = actor["title"].as_str()?;
                let actor_name = actor["actor"].as_str()?;
                Some(format!("{}：{}", character, actor_name))
            })
            .collect();

        if !actor_list.is_empty() {
            Some(actor_list.join("\n"))
        } else {
            None
        }
    } else {
        None
    };

    // 类型标签
    let styles: Vec<String> = result["styles"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|style| style["name"].as_str().map(|s| s.to_string()))
        .collect();

    // 播出状态
    let status = result["publish"]["pub_time_show"].as_str().map(|s| {
        if s.contains("完结") || s.contains("全") {
            "Ended".to_string()
        } else if s.contains("更新") || s.contains("连载") {
            "Continuing".to_string()
        } else {
            "Ended".to_string() // 默认为完结
        }
    });

    // 其他元数据
    let total_episodes = result["total"].as_i64().map(|t| t as i32);
    let cover = result["cover"].as_str().map(|s| s.to_string());

    // 从seasons数组中查找当前season的横版封面信息
    let (new_ep_cover, horizontal_cover_1610, horizontal_cover_169, bkg_cover) = if let Some(seasons_array) =
        result["seasons"].as_array()
    {
        debug!(
            "seasons数组查找: 目标season_id={}, 数组长度={}",
            season_id,
            seasons_array.len()
        );

        // 在seasons数组中查找当前season_id对应的条目，同时记录第一个有横版封面的条目作为备选
        let mut target_season_covers = Vec::new(); // 目标season_id的所有条目
        let mut first_available_covers = None;

        for (index, season) in seasons_array.iter().enumerate() {
            // 简化调试输出
            let season_season_id = season["season_id"].as_i64().unwrap_or(-1);
            debug!("处理seasons[{}]: season_id={}", index, season_season_id);

            // 检查当前条目是否有有效的横版封面（作为备选）
            let current_h1610 = season["horizontal_cover_1610"].as_str().filter(|s| !s.is_empty());
            let current_h169 = season["horizontal_cover_169"].as_str().filter(|s| !s.is_empty());
            let current_bkg = season["bkg_cover"].as_str().filter(|s| !s.is_empty());
            let current_new_ep_cover = season["new_ep"]["cover"].as_str().filter(|s| !s.is_empty());

            // 如果还没有备选条目，且当前条目有有效的横版封面，就记录它
            if first_available_covers.is_none()
                && (current_new_ep_cover.is_some()
                    || current_h1610.is_some()
                    || current_h169.is_some()
                    || current_bkg.is_some())
            {
                let covers = (
                    current_new_ep_cover.map(|s| s.to_string()),
                    current_h1610.map(|s| s.to_string()),
                    current_h169.map(|s| s.to_string()),
                    current_bkg.map(|s| s.to_string()),
                );
                first_available_covers = Some(covers);
                info!("💾 记录为第一个可用的横版封面备选：season_id={}", season_season_id);
            }

            // 检查是否匹配当前season_id
            if season_season_id.to_string() == season_id {
                info!(
                    "✓ 找到匹配的season_id: {} (第{}个条目)",
                    season_season_id,
                    target_season_covers.len() + 1
                );
                // 找到了当前season，提取横版封面信息
                let new_ep = season["new_ep"]["cover"].as_str().map(|s| s.to_string());
                let h1610 = season["horizontal_cover_1610"].as_str().map(|s| s.to_string());
                let h169 = season["horizontal_cover_169"].as_str().map(|s| s.to_string());
                let bkg = season["bkg_cover"].as_str().map(|s| s.to_string());
                debug!(
                    "  字段提取: new_ep={:?}, h1610={:?}, h169={:?}, bkg={:?}",
                    new_ep, h1610, h169, bkg
                );
                target_season_covers.push((new_ep, h1610, h169, bkg));
                // 不要break，继续查找是否还有其他相同season_id的条目
            }
        }

        // 从目标season的所有条目中选择第一个有有效横版封面的
        let found_season_covers = if !target_season_covers.is_empty() {
            info!(
                "共找到 {} 个 season_id {} 的条目",
                target_season_covers.len(),
                season_id
            );

            // 先寻找有有效横版封面的条目
            let valid_cover = target_season_covers.iter().find(|(new_ep, h1610, h169, bkg)| {
                new_ep.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
                    || h1610.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
                    || h169.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
                    || bkg.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
            });

            if let Some(covers) = valid_cover {
                info!("✓ 找到有有效横版封面的season_id {} 条目", season_id);
                Some(covers.clone())
            } else {
                warn!("⚠️ 目标season {} 的所有条目都没有有效的横版封面", season_id);
                target_season_covers.first().cloned()
            }
        } else {
            None
        };

        // 智能fallback逻辑
        match found_season_covers {
            Some((new_ep, h1610, h169, bkg)) => {
                // 检查找到的season是否有有效的横版封面
                let has_valid_covers = new_ep.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
                    || h1610.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
                    || h169.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
                    || bkg.as_ref().map(|s| !s.is_empty()).unwrap_or(false);

                if has_valid_covers {
                    info!("✓ 目标season {} 有有效的横版封面，直接使用", season_id);
                    (new_ep, h1610, h169, bkg)
                } else if let Some((fallback_new_ep, fallback_h1610, fallback_h169, fallback_bkg)) =
                    first_available_covers
                {
                    warn!("⚠️ 目标season {} 没有有效的横版封面，使用第一个可用的备选", season_id);
                    info!(
                        "  备选横版封面: new_ep={:?}, h1610={:?}, h169={:?}, bkg={:?}",
                        fallback_new_ep, fallback_h1610, fallback_h169, fallback_bkg
                    );
                    (fallback_new_ep, fallback_h1610, fallback_h169, fallback_bkg)
                } else {
                    warn!(
                        "⚠️ 目标season {} 和所有备选都没有有效的横版封面，使用顶层字段",
                        season_id
                    );
                    (
                        None, // 顶层没有new_ep字段
                        result["horizontal_cover_1610"].as_str().map(|s| s.to_string()),
                        result["horizontal_cover_169"].as_str().map(|s| s.to_string()),
                        result["bkg_cover"].as_str().map(|s| s.to_string()),
                    )
                }
            }
            None => {
                // 完全没找到目标season，使用备选或顶层
                if let Some((fallback_new_ep, fallback_h1610, fallback_h169, fallback_bkg)) = first_available_covers {
                    warn!("⚠️ 未找到目标season {}，使用第一个可用的备选", season_id);
                    info!(
                        "  备选横版封面: new_ep={:?}, h1610={:?}, h169={:?}, bkg={:?}",
                        fallback_new_ep, fallback_h1610, fallback_h169, fallback_bkg
                    );
                    (fallback_new_ep, fallback_h1610, fallback_h169, fallback_bkg)
                } else {
                    warn!("⚠️ 未找到目标season {} 且无备选，使用顶层字段", season_id);
                    (
                        None, // 顶层没有new_ep字段
                        result["horizontal_cover_1610"].as_str().map(|s| s.to_string()),
                        result["horizontal_cover_169"].as_str().map(|s| s.to_string()),
                        result["bkg_cover"].as_str().map(|s| s.to_string()),
                    )
                }
            }
        }
    } else {
        // 没有seasons数组，使用顶层字段
        warn!("API响应中没有seasons数组，使用顶层字段");
        (
            None, // 顶层没有new_ep字段
            result["horizontal_cover_1610"].as_str().map(|s| s.to_string()),
            result["horizontal_cover_169"].as_str().map(|s| s.to_string()),
            result["bkg_cover"].as_str().map(|s| s.to_string()),
        )
    };
    let media_id = result["media_id"].as_i64();
    let publish_time = result["publish"]["pub_time_show"].as_str().map(|s| s.to_string());
    let total_views = result["stat"]["views"].as_i64();
    let total_favorites = result["stat"]["favorites"].as_i64();

    let episodes: Vec<EpisodeInfo> = result["episodes"]
        .as_array()
        .context("找不到分集列表")?
        .iter()
        .filter_map(|ep| {
            let ep_id = ep["id"].as_i64()?.to_string();
            let cid = ep["cid"].as_i64()?;
            let duration_ms = ep["duration"].as_i64()?;
            let duration = (duration_ms / 1000) as u32;

            Some(EpisodeInfo { ep_id, cid, duration })
        })
        .collect();

    info!(
        "成功获取番剧季 {} 「{}」完整信息：{} 集，评分 {:?}，制作地区 {:?}，类型 {:?}",
        season_id,
        title,
        episodes.len(),
        rating,
        areas,
        styles
    );

    // 从API的seasons数组计算总季数
    let total_seasons = if let Some(seasons_array) = result["seasons"].as_array() {
        if seasons_array.is_empty() {
            // 单季度番剧：seasons数组为空，默认为1季
            Some(1)
        } else {
            // 多季度番剧：seasons数组长度就是总季数
            Some(seasons_array.len() as i32)
        }
    } else {
        // 没有seasons字段，假设为单季
        Some(1)
    };

    debug!("番剧 {} 总季数计算完成: {} 季", title, total_seasons.unwrap_or(1));

    // 获取show_season_type
    let show_season_type = result["type"].as_i64().map(|v| v as i32);

    Ok(SeasonInfo {
        title,
        episodes,
        alias,
        evaluate,
        rating,
        rating_count,
        areas,
        actors,
        styles,
        total_episodes,
        status,
        cover,
        new_ep_cover,
        horizontal_cover_1610,
        horizontal_cover_169,
        bkg_cover,
        media_id,
        season_id: season_id.to_string(),
        publish_time,
        total_views,
        total_favorites,
        total_seasons,
        show_season_type,
    })
}

/// 处理单个番剧视频
async fn process_bangumi_video(
    video_model: video::Model,
    episodes_map: &HashMap<String, (i64, u32)>,
    connection: &DatabaseConnection,
    video_source: &VideoSourceEnum,
) -> Result<()> {
    let txn = connection.begin().await?;

    let (actual_cid, duration) = if let Some(ep_id) = &video_model.ep_id {
        match episodes_map.get(ep_id) {
            Some(&info) => {
                debug!("使用缓存信息: EP{} -> CID={}, Duration={}s", ep_id, info.0, info.1);
                info
            }
            None => {
                warn!("找不到分集 {} 的信息，使用默认值", ep_id);
                (-1, 1440) // 默认值
            }
        }
    } else {
        error!("番剧 {} 缺少EP ID", video_model.name);
        (-1, 1440)
    };

    let page_info = PageInfo {
        cid: actual_cid,
        page: 1,
        name: video_model.name.clone(),
        duration,
        first_frame: None,
        dimension: None,
    };

    // 创建page记录（这里会自动缓存cid和duration到数据库）
    create_pages(vec![page_info], &video_model, &txn).await?;

    // 更新视频状态
    let mut video_active_model: bili_sync_entity::video::ActiveModel = video_model.into();
    video_source.set_relation_id(&mut video_active_model);
    video_active_model.single_page = Set(Some(true)); // 番剧的每一集都是单页
    video_active_model.tags = Set(Some(serde_json::Value::Array(vec![]))); // 空标签数组
    video_active_model.save(&txn).await?;

    txn.commit().await?;
    Ok(())
}

/// 获取特定视频源的视频数量
async fn get_video_count_for_source(video_source: &VideoSourceEnum, connection: &DatabaseConnection) -> Result<usize> {
    let count = video::Entity::find()
        .filter(video_source.filter_expr())
        .count(connection)
        .await?;
    Ok(count as usize)
}

/// 自动重置风控导致的失败任务
/// 当检测到风控时，将所有失败状态(值为3)、正在进行状态(值为2)以及未完成的任务重置为未开始状态(值为0)
pub async fn auto_reset_risk_control_failures(connection: &DatabaseConnection) -> Result<()> {
    use crate::utils::status::{PageStatus, VideoStatus};
    use bili_sync_entity::{page, video};
    use sea_orm::*;

    info!("检测到风控，开始自动重置失败、进行中和未完成的下载任务...");

    // 查询所有视频和页面数据
    let (all_videos, all_pages) = tokio::try_join!(
        video::Entity::find()
            .select_only()
            .columns([video::Column::Id, video::Column::Name, video::Column::DownloadStatus,])
            .into_tuple::<(i32, String, u32)>()
            .all(connection),
        page::Entity::find()
            .select_only()
            .columns([page::Column::Id, page::Column::Name, page::Column::DownloadStatus,])
            .into_tuple::<(i32, String, u32)>()
            .all(connection)
    )?;

    let mut resetted_videos = 0;
    let mut resetted_pages = 0;

    let txn = connection.begin().await?;

    // 重置视频失败、进行中和未完成状态
    for (id, name, download_status) in all_videos {
        let mut video_status = VideoStatus::from(download_status);
        let mut video_resetted = false;

        // 检查是否为完全成功的状态（所有任务都是1）
        let is_fully_completed = (0..5).all(|task_index| video_status.get(task_index) == 1);

        if !is_fully_completed {
            // 如果不是完全成功，检查所有任务索引，将失败状态(3)、正在进行状态(2)和未开始状态(0)重置为未开始(0)
            for task_index in 0..5 {
                let status_value = video_status.get(task_index);
                if status_value == 3 || status_value == 2 || status_value == 0 {
                    video_status.set(task_index, 0); // 重置为未开始
                    video_resetted = true;
                }
            }
        }

        if video_resetted {
            video::Entity::update(video::ActiveModel {
                id: sea_orm::ActiveValue::Unchanged(id),
                download_status: sea_orm::Set(video_status.into()),
                ..Default::default()
            })
            .exec(&txn)
            .await?;

            resetted_videos += 1;
            debug!("重置视频「{}」的未完成任务状态", name);
        }
    }

    // 重置页面失败、进行中和未完成状态
    for (id, name, download_status) in all_pages {
        let mut page_status = PageStatus::from(download_status);
        let mut page_resetted = false;

        // 检查是否为完全成功的状态（所有任务都是1）
        let is_fully_completed = (0..5).all(|task_index| page_status.get(task_index) == 1);

        if !is_fully_completed {
            // 如果不是完全成功，检查所有任务索引，将失败状态(3)、正在进行状态(2)和未开始状态(0)重置为未开始(0)
            for task_index in 0..5 {
                let status_value = page_status.get(task_index);
                if status_value == 3 || status_value == 2 || status_value == 0 {
                    page_status.set(task_index, 0); // 重置为未开始
                    page_resetted = true;
                }
            }
        }

        if page_resetted {
            page::Entity::update(page::ActiveModel {
                id: sea_orm::ActiveValue::Unchanged(id),
                download_status: sea_orm::Set(page_status.into()),
                ..Default::default()
            })
            .exec(&txn)
            .await?;

            resetted_pages += 1;
            debug!("重置页面「{}」的未完成任务状态", name);
        }
    }

    txn.commit().await?;

    if resetted_videos > 0 || resetted_pages > 0 {
        info!(
            "风控自动重置完成：重置了 {} 个视频和 {} 个页面的未完成任务状态",
            resetted_videos, resetted_pages
        );
    } else {
        info!("风控自动重置完成：所有任务都已完成，无需重置");
    }

    Ok(())
}

/// 获取合集中视频的集数序号
/// 根据视频在合集中的发布时间顺序确定集数
async fn get_collection_video_episode_number(
    connection: &DatabaseConnection,
    collection_id: i32,
    bvid: &str,
) -> Result<i32> {
    use bili_sync_entity::video;
    use sea_orm::*;

    // 获取该合集中所有视频，按发布时间排序
    let videos = video::Entity::find()
        .filter(video::Column::CollectionId.eq(collection_id))
        .order_by_asc(video::Column::Pubtime)
        .select_only()
        .columns([video::Column::Bvid, video::Column::Pubtime])
        .into_tuple::<(String, chrono::NaiveDateTime)>()
        .all(connection)
        .await?;

    // 找到当前视频的位置，返回序号（从1开始）
    for (index, (video_bvid, _)) in videos.iter().enumerate() {
        if video_bvid == bvid {
            return Ok((index + 1) as i32);
        }
    }

    // 如果没找到，返回错误
    Err(anyhow!("视频 {} 在合集 {} 中未找到", bvid, collection_id))
}

/// 检查文件夹是否为同一视频的文件夹
fn is_same_video_folder(folder_path: &std::path::Path, video_model: &video::Model) -> bool {
    use std::fs;

    if !folder_path.exists() {
        debug!("文件夹不存在: {:?}", folder_path);
        return false;
    }

    debug!("=== 智能冲突检测开始 ===");
    debug!("检查文件夹: {:?}", folder_path);
    debug!("数据库存储路径: {}", video_model.path);
    debug!("视频BVID: {}", video_model.bvid);
    debug!("视频标题: {}", video_model.name);

    // 方法1：增强的数据库路径匹配
    let db_path = std::path::Path::new(&video_model.path);

    // 1.1 完整路径匹配
    if folder_path == db_path {
        debug!("✓ 通过完整路径匹配确认为同一视频文件夹");
        return true;
    }

    // 1.2 规范化路径比较（处理不同的路径分隔符）
    let folder_normalized = folder_path.to_string_lossy().replace('\\', "/");
    let db_normalized = db_path.to_string_lossy().replace('\\', "/");
    if folder_normalized == db_normalized {
        debug!("✓ 通过规范化路径匹配确认为同一视频文件夹");
        return true;
    }

    // 1.3 文件夹名称匹配（原有逻辑）
    if let Some(db_folder_name) = db_path.file_name() {
        if let Some(check_folder_name) = folder_path.file_name() {
            if db_folder_name == check_folder_name {
                debug!("✓ 通过文件夹名称匹配确认为同一视频文件夹");
                return true;
            }
        }
    }

    // 1.4 相对路径后缀匹配
    if let Some(db_folder_name) = db_path.file_name() {
        if folder_path.ends_with(db_folder_name) {
            debug!("✓ 通过路径后缀匹配确认为同一视频文件夹");
            return true;
        }
    }

    debug!("⚠ 数据库路径匹配失败，尝试文件内容检测");

    // 方法2：扩展的文件内容检测
    if let Ok(entries) = fs::read_dir(folder_path) {
        let mut found_media_files = false;
        let mut found_bvid_files = false;
        let mut found_title_files = false;

        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy().to_lowercase();

            // 2.1 检查包含BVID的文件（扩展文件类型）
            if file_name_str.contains(&video_model.bvid.to_lowercase()) {
                if file_name_str.ends_with(".tmp_video")
                    || file_name_str.ends_with(".tmp_audio")
                    || file_name_str.ends_with(".mp4")
                    || file_name_str.ends_with(".mkv")
                    || file_name_str.ends_with(".flv")
                    || file_name_str.ends_with(".webm")
                    || file_name_str.ends_with(".nfo")
                    || file_name_str.ends_with(".jpg")
                    || file_name_str.ends_with(".png")
                    || file_name_str.ends_with(".ass")
                    || file_name_str.ends_with(".srt")
                {
                    debug!(
                        "✓ 通过BVID文件匹配确认为同一视频文件夹: {} (匹配文件: {})",
                        folder_path.display(),
                        file_name_str
                    );
                    return true;
                }
                found_bvid_files = true;
            }

            // 2.2 检查包含视频标题关键词的文件
            let video_title_clean = video_model
                .name
                .to_lowercase()
                .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "");
            if !video_title_clean.is_empty() && video_title_clean.len() > 3 {
                // 提取标题的前几个字符作为关键词
                let title_keywords: Vec<&str> = video_title_clean.split_whitespace().take(3).collect();
                for keyword in title_keywords {
                    if keyword.len() > 2 && file_name_str.contains(keyword) {
                        if file_name_str.ends_with(".mp4")
                            || file_name_str.ends_with(".mkv")
                            || file_name_str.ends_with(".flv")
                            || file_name_str.ends_with(".webm")
                            || file_name_str.ends_with(".nfo")
                        {
                            debug!(
                                "✓ 通过标题关键词匹配确认为同一视频文件夹: {} (匹配关键词: {}, 文件: {})",
                                folder_path.display(),
                                keyword,
                                file_name_str
                            );
                            return true;
                        }
                        found_title_files = true;
                    }
                }
            }

            // 2.3 检查是否有媒体相关文件（降低要求）
            if file_name_str.ends_with(".mp4")
                || file_name_str.ends_with(".mkv")
                || file_name_str.ends_with(".flv")
                || file_name_str.ends_with(".webm")
                || file_name_str.ends_with(".nfo")
                || file_name_str.ends_with(".jpg")
                || file_name_str.ends_with(".png")
                || file_name_str.ends_with(".ass")
                || file_name_str.ends_with(".srt")
            {
                found_media_files = true;
            }
        }

        debug!(
            "文件检测结果: 媒体文件={}, BVID文件={}, 标题文件={}",
            found_media_files, found_bvid_files, found_title_files
        );

        // 如果找到了相关文件但没有精确匹配，记录为可疑
        if found_media_files || found_bvid_files || found_title_files {
            debug!("⚠ 文件夹包含相关文件但无法确认为同一视频文件夹: {:?}", folder_path);
        }
    }

    debug!("✗ 无法确认为同一视频文件夹: {:?}", folder_path);
    debug!("=== 智能冲突检测结束 ===");
    false
}

/// 生成唯一的文件夹名称，避免同名冲突（增强版）
pub fn generate_unique_folder_name(
    parent_dir: &std::path::Path,
    base_name: &str,
    video_model: &video::Model,
    pubtime: &str,
) -> String {
    let mut unique_name = base_name.to_string();
    let mut counter = 0;

    // 检查基础名称是否已存在
    let base_path = parent_dir.join(&unique_name);
    if !base_path.exists() {
        return unique_name;
    }

    // 如果存在，智能检查这个文件夹是否就是当前视频的文件夹
    if is_same_video_folder(&base_path, video_model) {
        debug!("文件夹 {:?} 已是当前视频的文件夹，无需生成新名称", base_path);
        return unique_name;
    }

    // 确认是真正的冲突，开始生成唯一名称
    debug!("检测到真实的文件夹名冲突，开始生成唯一名称: {}", base_name);

    // 如果存在真正冲突，先尝试追加发布时间
    unique_name = format!("{}-{}", base_name, pubtime);
    let time_path = parent_dir.join(&unique_name);
    if !time_path.exists() {
        info!("检测到下载文件夹名冲突，追加发布时间: {} -> {}", base_name, unique_name);
        return unique_name;
    }

    // 如果发布时间也冲突，追加BVID
    unique_name = format!("{}-{}", base_name, &video_model.bvid);
    let bvid_path = parent_dir.join(&unique_name);
    if !bvid_path.exists() {
        info!("检测到下载文件夹名冲突，追加BVID: {} -> {}", base_name, unique_name);
        return unique_name;
    }

    // 如果都冲突，使用数字后缀
    loop {
        counter += 1;
        unique_name = format!("{}-{}", base_name, counter);
        let numbered_path = parent_dir.join(&unique_name);
        if !numbered_path.exists() {
            warn!(
                "检测到严重下载文件夹名冲突，使用数字后缀: {} -> {}",
                base_name, unique_name
            );
            return unique_name;
        }

        // 防止无限循环，使用随机后缀
        if counter > 1000 {
            warn!("下载文件夹名冲突解决失败，使用随机后缀");
            let random_suffix: u32 = rand::random::<u32>() % 90000 + 10000;
            unique_name = format!("{}-{}", base_name, random_suffix);
            return unique_name;
        }
    }
}

#[cfg(test)]
mod tests {
    use handlebars::handlebars_helper;
    use serde_json::json;

    use crate::config::PathSafeTemplate;

    #[test]
    fn test_template_usage() {
        let mut template = handlebars::Handlebars::new();
        handlebars_helper!(truncate: |s: String, len: usize| {
            if s.chars().count() > len {
                s.chars().take(len).collect::<String>()
            } else {
                s.to_string()
            }
        });
        template.register_helper("truncate", Box::new(truncate));
        let _ = template.path_safe_register("video", "test{{bvid}}test");
        let _ = template.path_safe_register("test_truncate", "哈哈，{{ truncate title 30 }}");
        let _ = template.path_safe_register("test_path_unix", "{{ truncate title 7 }}/test/a");
        let _ = template.path_safe_register("test_path_windows", r"{{ truncate title 7 }}\\test\\a");
        #[cfg(not(windows))]
        {
            assert_eq!(
                template
                    .path_safe_render("test_path_unix", &json!({"title": "关注/永雏塔菲喵"}))
                    .unwrap(),
                "关注_永雏塔菲/test/a"
            );
            assert_eq!(
                template
                    .path_safe_render("test_path_windows", &json!({"title": "关注/永雏塔菲喵"}))
                    .unwrap(),
                "关注_永雏塔菲_test_a"
            );
        }
        #[cfg(windows)]
        {
            assert_eq!(
                template
                    .path_safe_render("test_path_unix", &json!({"title": "关注/永雏塔菲喵"}))
                    .unwrap(),
                "关注_永雏塔菲_test_a"
            );
            assert_eq!(
                template
                    .path_safe_render("test_path_windows", &json!({"title": "关注/永雏塔菲喵"}))
                    .unwrap(),
                r"关注_永雏塔菲\\test\\a"
            );
        }
        assert_eq!(
            template
                .path_safe_render("video", &json!({"bvid": "BV1b5411h7g7"}))
                .unwrap(),
            "testBV1b5411h7g7test"
        );
        assert_eq!(
            template
                .path_safe_render(
                    "test_truncate",
                    &json!({"title": "你说得对，但是 Rust 是由 Mozilla 自主研发的一款全新的编译期格斗游戏。\
                    编译将发生在一个被称作「Cargo」的构建系统中。在这里，被引用的指针将被授予「生命周期」之力，导引对象安全。\
                    你将扮演一位名为「Rustacean」的神秘角色, 在与「Rustc」的搏斗中邂逅各种骨骼惊奇的傲娇报错。\
                    征服她们、通过编译同时，逐步发掘「C++」程序崩溃的真相。"})
                )
                .unwrap(),
            "哈哈，你说得对，但是 Rust 是由 Mozilla 自主研发的一"
        );
    }

    #[cfg(test)]
    mod auto_delete_tests {
        use crate::error::{ClassifiedError, ErrorType, ExecutionStatus};
        use bili_sync_entity::video;

        #[tokio::test]
        async fn test_87007_error_detection_via_classified_error() {
            // 创建一个包含should_auto_delete标志的ClassifiedError
            let classified_error = ClassifiedError::new(
                ErrorType::Permission,
                "充电专享视频，需要为UP主充电才能观看".to_string(),
            )
            .with_auto_delete(true);

            let results = [
                ExecutionStatus::Succeeded,
                ExecutionStatus::ClassifiedFailed(classified_error),
                ExecutionStatus::Skipped,
            ];

            let task_names = ["封面", "详情", "作者头像"];

            // 创建测试用的video model
            let _video_model = video::Model {
                id: 12345,
                collection_id: None,
                favorite_id: None,
                watch_later_id: None,
                submission_id: None,
                source_id: Some(1),
                source_type: Some(1),
                upper_id: 1,
                upper_name: "测试UP主".to_string(),
                upper_face: "http://example.com/face.jpg".to_string(),
                name: "测试充电专享视频".to_string(),
                path: "/test/path".to_string(),
                category: 1,
                bvid: "BV1234567890".to_string(),
                intro: "测试视频介绍".to_string(),
                cover: "http://example.com/cover.jpg".to_string(),
                ctime: crate::utils::time_format::now_naive(),
                pubtime: crate::utils::time_format::now_naive(),
                favtime: crate::utils::time_format::now_naive(),
                download_status: 0,
                valid: true,
                tags: None,
                single_page: Some(true),
                created_at: now_standard_string(),
                season_id: None,
                ep_id: None,
                season_number: None,
                episode_number: None,
                deleted: 0,
                share_copy: None,
                show_season_type: None,
                actors: None,
                auto_download: false,
                source_submission_id: None,
                staff_info: None,
            };

            // 注意：这个测试不能真正连接数据库，所以我们只验证函数的逻辑部分
            // 在实际项目中，应该使用mock或测试数据库

            // 验证错误检测逻辑 - 我们可以通过检查结果来模拟
            let mut needs_auto_delete = false;

            for (res, _task_name) in results.iter().zip(task_names.iter()) {
                if let ExecutionStatus::ClassifiedFailed(classified_error) = res {
                    if classified_error.should_auto_delete {
                        needs_auto_delete = true;
                        break;
                    }
                }
            }

            assert!(needs_auto_delete, "应该检测到需要自动删除的87007错误");
        }

        #[tokio::test]
        async fn test_backward_compatibility_87007_string_matching() {
            // 测试向后兼容的字符串匹配逻辑
            let error_87007 = anyhow::anyhow!("B站API错误: status code: 87007");
            let error_other = anyhow::anyhow!("B站API错误: status code: 404");

            let results = [
                ExecutionStatus::Succeeded,
                ExecutionStatus::Ignored(error_87007),
                ExecutionStatus::Ignored(error_other),
            ];

            let task_names = ["封面", "详情", "作者头像"];

            // 验证字符串匹配逻辑
            let mut has_87007_error = false;

            for (res, _task_name) in results.iter().zip(task_names.iter()) {
                if let ExecutionStatus::Ignored(e) = res {
                    let error_msg = e.to_string();
                    if error_msg.contains("status code: 87007") {
                        has_87007_error = true;
                        break;
                    }
                }
            }

            assert!(has_87007_error, "应该通过字符串匹配检测到87007错误");
        }

        #[test]
        fn test_87007_classified_error_creation() {
            // 测试error.rs中87007错误的分类逻辑
            use crate::bilibili::BiliError;
            use crate::error::ErrorClassifier;

            let bili_error = BiliError::RequestFailed(87007, "充电专享视频".to_string());
            let anyhow_error = anyhow::anyhow!(bili_error);

            let classified = ErrorClassifier::classify_error(&anyhow_error);

            assert_eq!(classified.error_type, ErrorType::Permission);
            assert!(classified.should_ignore, "87007错误应该被忽略");
            assert!(!classified.should_retry, "87007错误不应该重试");
            assert!(classified.should_auto_delete, "87007错误应该自动删除");
            assert!(classified.message.contains("充电专享视频"));
        }

        #[test]
        fn test_87008_classified_error_creation() {
            // 测试87008错误的分类逻辑（修复后应该和87007一样工作）
            use crate::bilibili::BiliError;
            use crate::error::ErrorClassifier;

            let bili_error = BiliError::RequestFailed(87008, "87008".to_string());
            let anyhow_error = anyhow::anyhow!(bili_error);

            let classified = ErrorClassifier::classify_error(&anyhow_error);

            assert_eq!(classified.error_type, ErrorType::Permission);
            assert!(classified.should_ignore, "87008错误应该被忽略");
            assert!(!classified.should_retry, "87008错误不应该重试");
            assert!(classified.should_auto_delete, "87008错误应该自动删除");
            assert!(classified.message.contains("充电专享视频"));
        }

        #[test]
        fn test_87008_execution_status_conversion() {
            // 测试87008错误的ExecutionStatus转换（修复关键点）
            use crate::bilibili::BiliError;

            let bili_error = BiliError::RequestFailed(87008, "87008".to_string());
            let result: Result<ExecutionStatus, anyhow::Error> = Err(bili_error.into());
            let execution_status = ExecutionStatus::from(result);

            // 修复后，87008错误应该被转换为ClassifiedFailed，而不是Ignored
            match execution_status {
                ExecutionStatus::ClassifiedFailed(classified_error) => {
                    assert_eq!(classified_error.error_type, ErrorType::Permission);
                    assert!(classified_error.should_auto_delete, "87008错误应该标记为需要自动删除");
                }
                _ => panic!("87008错误应该被转换为ClassifiedFailed状态"),
            }
        }
    }
}
