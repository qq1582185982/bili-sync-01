use anyhow::{Context, Result};
use bili_sync_entity::*;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{OnConflict, SimpleExpr};
use sea_orm::DatabaseTransaction;
use tracing::{debug, info};
use std::collections::HashSet;

use crate::adapter::{VideoSource, VideoSourceEnum};
use crate::bilibili::{PageInfo, VideoInfo};
use crate::utils::status::STATUS_COMPLETED;

/// 从 VideoInfo 中提取 BVID
fn extract_bvid(video_info: &VideoInfo) -> String {
    match video_info {
        VideoInfo::Submission { bvid, .. } => bvid.clone(),
        VideoInfo::Detail { bvid, .. } => bvid.clone(),
        VideoInfo::Favorite { bvid, .. } => bvid.clone(),
        VideoInfo::WatchLater { bvid, .. } => bvid.clone(),
        VideoInfo::Collection { bvid, .. } => bvid.clone(),
        VideoInfo::Bangumi { bvid, .. } => bvid.clone(),
    }
}

/// 根据show_season_type和其他字段重新计算番剧的智能命名
fn recalculate_bangumi_name(
    title: &str,
    share_copy: Option<&str>,
    show_title: Option<&str>,
    show_season_type: Option<i32>,
) -> String {
    // 参考convert.rs中的智能命名逻辑
    if show_season_type == Some(2) {
        // 番剧影视类型，使用简化命名
        show_title.unwrap_or(title).to_string()
    } else {
        // 常规番剧类型，使用详细命名
        share_copy
            .filter(|s| !s.is_empty() && s.len() > title.len()) // 只有当share_copy更详细时才使用
            .or(show_title)
            .unwrap_or(title)
            .to_string()
    }
}

/// 筛选未填充的视频
pub async fn filter_unfilled_videos(
    additional_expr: SimpleExpr,
    conn: &DatabaseConnection,
) -> Result<Vec<video::Model>> {
    video::Entity::find()
        .filter(
            video::Column::Valid
                .eq(true)
                .and(video::Column::DownloadStatus.eq(0))
                .and(video::Column::Category.is_in([1, 2]))
                .and(video::Column::SinglePage.is_null())
                .and(video::Column::Deleted.eq(0))
                .and(video::Column::AutoDownload.eq(true))  // 只处理设置为自动下载的视频
                .and(additional_expr),
        )
        .all(conn)
        .await
        .context("filter unfilled videos failed")
}

/// 筛选未处理完成的视频和视频页
pub async fn filter_unhandled_video_pages(
    additional_expr: SimpleExpr,
    connection: &DatabaseConnection,
) -> Result<Vec<(video::Model, Vec<page::Model>)>> {
    video::Entity::find()
        .filter(
            video::Column::Valid
                .eq(true)
                .and(video::Column::DownloadStatus.lt(STATUS_COMPLETED))
                .and(video::Column::Category.is_in([1, 2]))
                .and(video::Column::SinglePage.is_not_null())
                .and(video::Column::Deleted.eq(0))
                .and(video::Column::AutoDownload.eq(true))  // 只处理设置为自动下载的视频
                .and(additional_expr),
        )
        .find_with_related(page::Entity)
        .all(connection)
        .await
        .context("filter unhandled video pages failed")
}

/// 筛选在当前循环中失败但可重试的视频（不包括已达到最大重试次数的视频）
pub async fn get_failed_videos_in_current_cycle(
    additional_expr: SimpleExpr,
    connection: &DatabaseConnection,
) -> Result<Vec<(video::Model, Vec<page::Model>)>> {
    use crate::utils::status::STATUS_COMPLETED;

    let all_videos = video::Entity::find()
        .filter(
            video::Column::Valid
                .eq(true)
                .and(video::Column::DownloadStatus.lt(STATUS_COMPLETED))
                .and(video::Column::DownloadStatus.gt(0)) // 排除未开始的视频 (状态为0)
                .and(video::Column::Category.is_in([1, 2]))
                .and(video::Column::SinglePage.is_not_null())
                .and(video::Column::Deleted.eq(0))
                .and(video::Column::AutoDownload.eq(true))  // 只处理设置为自动下载的视频
                .and(additional_expr),
        )
        .find_with_related(page::Entity)
        .all(connection)
        .await?;

    // 获取所有待处理的删除任务中的视频ID
    use crate::task::DeleteVideoTask;
    use bili_sync_entity::task_queue::{self, TaskStatus, TaskType};

    let pending_delete_tasks = task_queue::Entity::find()
        .filter(task_queue::Column::TaskType.eq(TaskType::DeleteVideo))
        .filter(task_queue::Column::Status.eq(TaskStatus::Pending))
        .all(connection)
        .await?;

    let mut videos_in_delete_queue = std::collections::HashSet::new();
    for task_record in pending_delete_tasks {
        if let Ok(task_data) = serde_json::from_str::<DeleteVideoTask>(&task_record.task_data) {
            videos_in_delete_queue.insert(task_data.video_id);
        }
    }

    let result = all_videos
        .into_iter()
        .filter(|(video_model, pages_model)| {
            // 如果视频已经在删除队列中，跳过重试
            if videos_in_delete_queue.contains(&video_model.id) {
                return false;
            }

            // 检查视频和分页是否有可重试的失败
            let video_status = crate::utils::status::VideoStatus::from(video_model.download_status);
            let video_should_retry = video_status.should_run().iter().any(|&should_run| should_run);

            let pages_should_retry = pages_model.iter().any(|page_model| {
                let page_status = crate::utils::status::PageStatus::from(page_model.download_status);
                page_status.should_run().iter().any(|&should_run| should_run)
            });

            video_should_retry || pages_should_retry
        })
        .collect::<Vec<_>>();

    Ok(result)
}

/// 尝试创建 Video Model，如果发生冲突则忽略
/// 如果视频源启用了扫描已删除视频设置，则会恢复已删除的视频
/// 对于选择性下载模式，只存储选中的视频到数据库
pub async fn create_videos(
    videos_info: Vec<VideoInfo>,
    video_source: &VideoSourceEnum,
    connection: &DatabaseConnection,
) -> Result<()> {
    use sea_orm::{Set, Unchanged};

    // 新增：在全量模式下进行去重检查，防止重复处理已存在的视频
    let current_config = crate::config::reload_config();
    let is_full_mode = !current_config.submission_risk_control.enable_incremental_fetch;
    
    let final_videos_info = if is_full_mode && matches!(video_source, VideoSourceEnum::Submission(_)) {
        // 全量模式下的 UP主投稿，检查哪些视频已存在
        let all_bvids: Vec<String> = videos_info.iter().map(extract_bvid).collect();
        
        // 批量查询已存在的视频
        let existing_videos = video::Entity::find()
            .filter(video::Column::Bvid.is_in(all_bvids.clone()))
            .filter(video_source.filter_expr())
            .all(connection)
            .await?;
        
        let existing_bvids: HashSet<String> = existing_videos
            .into_iter()
            .map(|v| v.bvid)
            .collect();
        
        // 过滤出真正的新视频
        let new_videos: Vec<VideoInfo> = videos_info
            .into_iter()
            .filter(|info| !existing_bvids.contains(&extract_bvid(info)))
            .collect();
        
        let total_count = all_bvids.len();
        let existing_count = existing_bvids.len();
        let new_count = new_videos.len();
        
        if existing_count > 0 {
            info!(
                "全量模式去重检查完成：总视频 {} 个，已存在 {} 个，新视频 {} 个",
                total_count, existing_count, new_count
            );
        } else {
            debug!("全量模式：所有 {} 个视频都是新视频", new_count);
        }
        
        new_videos
    } else {
        // 增量模式或其他类型的视频源，使用原有逻辑
        videos_info
    };

    // 如果没有新视频需要处理，直接返回
    if final_videos_info.is_empty() {
        debug!("没有新视频需要创建，跳过处理");
        return Ok(());
    }

    // 检查是否启用了扫描已删除视频
    let scan_deleted = video_source.scan_deleted_videos();

    if scan_deleted {
        // 启用扫描已删除视频：需要特别处理已删除的视频
        for video_info in final_videos_info {
            // 选择性下载逻辑：针对 submission 类型视频源 - 需要在 into_simple_model() 之前获取信息
            let should_store_video = if let Some(selected_videos) = video_source.get_selected_videos() {
                // 获取创建时间来判断是否为新投稿
                let is_new_submission = if let Some(created_at) = video_source.get_created_at() {
                    // 如果视频发布时间晚于订阅创建时间，则为新投稿，自动下载
                    video_info.release_datetime() > &created_at
                } else {
                    // 如果无法获取创建时间，保守地认为不是新投稿
                    false
                };

                // 获取视频的 BVID（从 VideoInfo 获取）
                let video_bvid = extract_bvid(&video_info);

                let should_store = if is_new_submission {
                    // 新投稿：存储到数据库并设置自动下载
                    true
                } else {
                    // 历史投稿：只有在选择列表中的才存储到数据库
                    selected_videos.contains(&video_bvid)
                };

                debug!(
                    "选择性下载检查(已删除扫描): BVID={}, 是否新投稿={}, 是否在选择列表中={}, 是否存储={}",
                    video_bvid,
                    is_new_submission,
                    selected_videos.contains(&video_bvid),
                    should_store
                );

                should_store
            } else {
                // 没有选择性下载，存储所有视频
                true
            };

            // 如果不应该存储此视频，则跳过
            if !should_store_video {
                continue;
            }

            let mut model = video_info.into_simple_model();
            video_source.set_relation_id(&mut model);

            // 对于需要存储的视频，设置 auto_download 为 true
            model.auto_download = Set(true);

            // 查找是否存在已删除的同一视频
            let existing_video = video::Entity::find()
                .filter(video::Column::Bvid.eq(model.bvid.as_ref()))
                .filter(video_source.filter_expr())
                .one(connection)
                .await?;

            if let Some(existing) = existing_video {
                if existing.deleted == 1 {
                    // 存在已删除的视频，恢复它并重置下载状态以强制重新下载
                    let update_model = video::ActiveModel {
                        id: Unchanged(existing.id),
                        deleted: Set(0),
                        download_status: Set(0),   // 重置下载状态为未开始，强制重新下载
                        path: Set("".to_string()), // 清空原有路径，因为文件可能已经不存在
                        single_page: Set(None),    // 设为NULL，让filter_unfilled_videos识别并重新获取完整信息
                        // 更新其他可能变化的字段
                        name: model.name.clone(),
                        intro: model.intro.clone(),
                        cover: model.cover.clone(),
                        tags: model.tags.clone(),
                        ..Default::default()
                    };
                    update_model.save(connection).await?;

                    // 删除该视频的所有旧page记录（如果存在的话）
                    // 因为视频信息可能已经变化，旧的page记录可能不准确
                    page::Entity::delete_many()
                        .filter(page::Column::VideoId.eq(existing.id))
                        .exec(connection)
                        .await?;

                    info!("恢复已删除的视频，将重新获取详细信息: {}", existing.name);
                } else {
                    // 视频存在且未删除，检查是否需要更新字段
                    let mut needs_update = false;
                    let mut should_recalculate_name = false;

                    // 检查 share_copy 字段更新
                    let share_copy_changed = if let Some(new_share_copy) = model.share_copy.as_ref() {
                        existing.share_copy.as_ref() != Some(new_share_copy)
                    } else {
                        false
                    };

                    // 检查 show_season_type 字段更新
                    let show_season_type_changed = if let Some(new_show_season_type) = model.show_season_type.as_ref() {
                        existing.show_season_type != Some(*new_show_season_type)
                    } else {
                        false
                    };

                    // 检查 actors 字段更新
                    let actors_changed = match (&existing.actors, model.actors.as_ref()) {
                        (None, Some(new_actors)) => {
                            // 数据库为空，API有数据，需要更新
                            tracing::info!("检测到actors字段从空值更新为有值: {:?}", new_actors);
                            true
                        }
                        (Some(existing_actors), Some(new_actors)) => {
                            // 两者都有值，比较是否不同
                            let changed = existing_actors != new_actors;
                            if changed {
                                tracing::info!(
                                    "检测到actors字段值发生变化: 原值={:?}, 新值={:?}",
                                    existing_actors,
                                    new_actors
                                );
                            }
                            changed
                        }
                        (Some(_), None) => {
                            // 数据库有值，API返回空，保持原值不变
                            tracing::debug!("API未返回actors数据，保持数据库现有值");
                            false
                        }
                        (None, None) => {
                            // 两者都为空，无需更新
                            false
                        }
                    };

                    if share_copy_changed || show_season_type_changed || actors_changed {
                        needs_update = true;
                        should_recalculate_name = true;

                        if share_copy_changed {
                            info!(
                                "检测到需要更新share_copy: 视频={}, 原值={:?}, 新值={:?}",
                                existing.name, existing.share_copy, model.share_copy
                            );
                        }
                        if show_season_type_changed {
                            info!(
                                "检测到需要更新show_season_type: 视频={}, 原值={:?}, 新值={:?}",
                                existing.name, existing.show_season_type, model.show_season_type
                            );
                        }
                        if actors_changed {
                            info!(
                                "检测到需要更新actors: 视频={}, 原值={:?}, 新值={:?}",
                                existing.name, existing.actors, model.actors
                            );
                        }
                    }

                    if needs_update {
                        // 如果需要重新计算name，并且这是番剧类型（category=1）
                        // 但对于番剧影视类型（show_season_type=2），不重新计算name，保持原有的简洁格式
                        let new_name = if should_recalculate_name && existing.category == 1 {
                            let new_show_season_type = match &model.show_season_type {
                                Set(opt) => *opt,
                                _ => existing.show_season_type,
                            };

                            // 如果是番剧影视类型，不重新计算name，保持现有的简洁name
                            if new_show_season_type == Some(2) {
                                sea_orm::ActiveValue::NotSet // 保持现有name不变
                            } else {
                                // 对于常规番剧类型，进行重新计算
                                let title = existing.name.as_str();
                                let share_copy = match &model.share_copy {
                                    Set(Some(s)) => Some(s.as_str()),
                                    Set(None) => None,
                                    _ => existing.share_copy.as_deref(),
                                };

                                let recalculated_name =
                                    recalculate_bangumi_name(title, share_copy, None, new_show_season_type);
                                info!(
                                    "重新计算常规番剧name: 视频={}, 原name={}, 新name={}",
                                    existing.name, existing.name, recalculated_name
                                );
                                Set(recalculated_name)
                            }
                        } else {
                            model.name.clone()
                        };

                        let update_model = video::ActiveModel {
                            id: Unchanged(existing.id),
                            share_copy: model.share_copy.clone(),
                            show_season_type: model.show_season_type.clone(),
                            actors: model.actors.clone(),
                            name: new_name,
                            intro: model.intro.clone(),
                            cover: model.cover.clone(),
                            ..Default::default()
                        };

                        // 详细的数据库更新调试日志
                        tracing::info!(
                            "即将执行数据库更新(启用扫描删除): 视频={}, actors字段={:?}, share_copy={:?}, show_season_type={:?}",
                            existing.name, update_model.actors, update_model.share_copy, update_model.show_season_type
                        );

                        update_model.save(connection).await?;
                        info!("更新视频 {} 的字段完成", existing.name);
                    } else {
                        tracing::debug!(
                            "字段无需更新: 视频={}, share_copy={:?}, show_season_type={:?}, actors={:?}",
                            existing.name,
                            existing.share_copy,
                            existing.show_season_type,
                            existing.actors
                        );
                    }
                    continue;
                }
            } else {
                // 视频不存在，正常插入
                video::Entity::insert(model)
                    .on_conflict(OnConflict::new().do_nothing().to_owned())
                    .do_nothing()
                    .exec(connection)
                    .await?;
            }
        }
    } else {
        // 未启用扫描已删除视频：使用原有逻辑，但增加 share_copy 更新检查
        for video_info in final_videos_info {
            // 选择性下载逻辑：针对 submission 类型视频源 - 需要在 into_simple_model() 之前获取信息
            let should_store_video = if let Some(selected_videos) = video_source.get_selected_videos() {
                // 获取创建时间来判断是否为新投稿
                let is_new_submission = if let Some(created_at) = video_source.get_created_at() {
                    // 如果视频发布时间晚于订阅创建时间，则为新投稿，自动下载
                    video_info.release_datetime() > &created_at
                } else {
                    // 如果无法获取创建时间，保守地认为不是新投稿
                    false
                };

                // 获取视频的 BVID（从 VideoInfo 获取）
                let video_bvid = extract_bvid(&video_info);

                let should_store = if is_new_submission {
                    // 新投稿：存储到数据库并设置自动下载
                    true
                } else {
                    // 历史投稿：只有在选择列表中的才存储到数据库
                    selected_videos.contains(&video_bvid)
                };

                debug!(
                    "选择性下载检查(常规模式): BVID={}, 是否新投稿={}, 是否在选择列表中={}, 是否存储={}",
                    video_bvid,
                    is_new_submission,
                    selected_videos.contains(&video_bvid),
                    should_store
                );

                should_store
            } else {
                // 没有选择性下载，存储所有视频
                true
            };

            // 如果不应该存储此视频，则跳过
            if !should_store_video {
                continue;
            }

            let mut model = video_info.into_simple_model();
            video_source.set_relation_id(&mut model);

            // 对于需要存储的视频，设置 auto_download 为 true
            model.auto_download = Set(true);

            // 先尝试插入，如果失败说明记录已存在
            let insert_result = video::Entity::insert(model.clone())
                .on_conflict(OnConflict::new().do_nothing().to_owned())
                .do_nothing()
                .exec(connection)
                .await;

            // 如果插入没有影响任何行（即记录已存在），检查是否需要更新 share_copy
            if let Ok(insert_res) = insert_result {
                // 检查插入是否真的生效，如果没有生效说明记录已存在
                let insert_success = match &insert_res {
                    sea_orm::TryInsertResult::Inserted(_) => true,
                    sea_orm::TryInsertResult::Conflicted => false,
                    sea_orm::TryInsertResult::Empty => true, // 空插入视为成功
                };
                if !insert_success {
                    // 记录已存在，检查是否需要更新字段
                    let existing_video = video::Entity::find()
                        .filter(video::Column::Bvid.eq(model.bvid.as_ref()))
                        .filter(video_source.filter_expr())
                        .one(connection)
                        .await?;

                    if let Some(existing) = existing_video {
                        let mut needs_update = false;
                        let mut should_recalculate_name = false;

                        // 检查 share_copy 字段更新
                        let share_copy_changed = if let Some(new_share_copy) = model.share_copy.as_ref() {
                            existing.share_copy.as_ref() != Some(new_share_copy)
                        } else {
                            false
                        };

                        // 检查 show_season_type 字段更新
                        let show_season_type_changed =
                            if let Some(new_show_season_type) = model.show_season_type.as_ref() {
                                existing.show_season_type != Some(*new_show_season_type)
                            } else {
                                false
                            };

                        // 检查 actors 字段更新
                        let actors_changed = match (&existing.actors, model.actors.as_ref()) {
                            (None, Some(new_actors)) => {
                                // 数据库为空，API有数据，需要更新
                                tracing::info!("检测到actors字段从空值更新为有值(未启用扫描删除): {:?}", new_actors);
                                true
                            }
                            (Some(existing_actors), Some(new_actors)) => {
                                // 两者都有值，比较是否不同
                                let changed = existing_actors != new_actors;
                                if changed {
                                    tracing::info!(
                                        "检测到actors字段值发生变化(未启用扫描删除): 原值={:?}, 新值={:?}",
                                        existing_actors,
                                        new_actors
                                    );
                                }
                                changed
                            }
                            (Some(_), None) => {
                                // 数据库有值，API返回空，保持原值不变
                                tracing::debug!("API未返回actors数据，保持数据库现有值(未启用扫描删除)");
                                false
                            }
                            (None, None) => {
                                // 两者都为空，无需更新
                                false
                            }
                        };

                        if share_copy_changed || show_season_type_changed || actors_changed {
                            needs_update = true;
                            should_recalculate_name = true;

                            if share_copy_changed {
                                info!(
                                    "检测到需要更新share_copy(未启用扫描删除): 视频={}, 原值={:?}, 新值={:?}",
                                    existing.name, existing.share_copy, model.share_copy
                                );
                            }
                            if show_season_type_changed {
                                info!(
                                    "检测到需要更新show_season_type(未启用扫描删除): 视频={}, 原值={:?}, 新值={:?}",
                                    existing.name, existing.show_season_type, model.show_season_type
                                );
                            }
                            if actors_changed {
                                info!(
                                    "检测到需要更新actors(未启用扫描删除): 视频={}, 原值={:?}, 新值={:?}",
                                    existing.name, existing.actors, model.actors
                                );
                            }
                        }

                        if needs_update {
                            // 如果需要重新计算name，并且这是番剧类型（category=1）
                            // 但对于番剧影视类型（show_season_type=2），不重新计算name，保持原有的简洁格式
                            let new_name = if should_recalculate_name && existing.category == 1 {
                                let new_show_season_type = match &model.show_season_type {
                                    Set(opt) => *opt,
                                    _ => existing.show_season_type,
                                };

                                // 如果是番剧影视类型，不重新计算name，保持现有的简洁name
                                if new_show_season_type == Some(2) {
                                    sea_orm::ActiveValue::NotSet // 保持现有name不变
                                } else {
                                    // 对于常规番剧类型，进行重新计算
                                    let title = existing.name.as_str();
                                    let share_copy = match &model.share_copy {
                                        Set(Some(s)) => Some(s.as_str()),
                                        Set(None) => None,
                                        _ => existing.share_copy.as_deref(),
                                    };

                                    let recalculated_name =
                                        recalculate_bangumi_name(title, share_copy, None, new_show_season_type);
                                    info!(
                                        "重新计算常规番剧name(未启用扫描删除): 视频={}, 原name={}, 新name={}",
                                        existing.name, existing.name, recalculated_name
                                    );
                                    Set(recalculated_name)
                                }
                            } else {
                                model.name.clone()
                            };

                            let update_model = video::ActiveModel {
                                id: Unchanged(existing.id),
                                share_copy: model.share_copy.clone(),
                                show_season_type: model.show_season_type.clone(),
                                actors: model.actors.clone(),
                                name: new_name,
                                intro: model.intro.clone(),
                                cover: model.cover.clone(),
                                ..Default::default()
                            };

                            // 详细的数据库更新调试日志
                            tracing::info!(
                                "即将执行数据库更新(未启用扫描删除): 视频={}, actors字段={:?}, share_copy={:?}, show_season_type={:?}",
                                existing.name, update_model.actors, update_model.share_copy, update_model.show_season_type
                            );

                            update_model.save(connection).await?;
                            info!("更新视频 {} 的字段完成(未启用扫描删除)", existing.name);
                        } else {
                            tracing::debug!(
                                "字段无需更新(未启用扫描删除): 视频={}, share_copy={:?}, show_season_type={:?}, actors={:?}",
                                existing.name,
                                existing.share_copy,
                                existing.show_season_type,
                                existing.actors
                            );
                        }
                    }
                }
            }
        }
    }
    
    Ok(())
}

/// 尝试创建 Page Model，如果发生冲突则忽略
pub async fn create_pages(
    mut pages_info: Vec<PageInfo>,
    video_model: &bili_sync_entity::video::Model,
    connection: &DatabaseTransaction,
) -> Result<()> {
    // 对于单P视频，统一使用视频标题作为页面名称
    if pages_info.len() == 1 && pages_info[0].page == 1 && pages_info[0].name != video_model.name {
        debug!(
            "单P视频页面名称标准化: 视频 {} ({}), 原名称='{}' -> 使用视频标题='{}'",
            video_model.bvid, video_model.id, pages_info[0].name, video_model.name
        );
        pages_info[0].name = video_model.name.clone();
    }
    
    let page_models = pages_info
        .into_iter()
        .map(|p| p.into_active_model(video_model))
        .collect::<Vec<page::ActiveModel>>();
    for page_chunk in page_models.chunks(50) {
        page::Entity::insert_many(page_chunk.to_vec())
            .on_conflict(
                OnConflict::columns([page::Column::VideoId, page::Column::Pid])
                    .do_nothing()
                    .to_owned(),
            )
            .do_nothing()
            .exec(connection)
            .await?;
    }
    
    Ok(())
}

/// 更新视频 model 的下载状态
pub async fn update_videos_model(videos: Vec<video::ActiveModel>, connection: &DatabaseConnection) -> Result<()> {
    video::Entity::insert_many(videos)
        .on_conflict(
            OnConflict::column(video::Column::Id)
                .update_columns([video::Column::DownloadStatus, video::Column::Path])
                .to_owned(),
        )
        .exec(connection)
        .await?;
    
    Ok(())
}

/// 更新视频页 model 的下载状态
pub async fn update_pages_model(pages: Vec<page::ActiveModel>, connection: &DatabaseConnection) -> Result<()> {
    let query = page::Entity::insert_many(pages).on_conflict(
        OnConflict::column(page::Column::Id)
            .update_columns([page::Column::DownloadStatus, page::Column::Path])
            .to_owned(),
    );
    query.exec(connection).await?;
    
    Ok(())
}
