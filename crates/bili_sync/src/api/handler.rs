use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use axum::extract::{Extension, Path, Query};
use bili_sync_entity::*;
use bili_sync_migration::{Expr, OnConflict};
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, TransactionTrait, Unchanged,
};
use tracing::{debug, error, info, warn};
use utoipa::OpenApi;

use crate::api::auth::OpenAPIAuth;
use crate::api::error::InnerApiError;
use crate::api::request::{AddVideoSourceRequest, UpdateConfigRequest, VideosRequest};
use crate::api::response::{
    AddVideoSourceResponse, ConfigResponse, DeleteVideoSourceResponse, PageInfo, ResetVideoResponse,
    UpdateConfigResponse, VideoInfo, VideoResponse, VideoSource, VideoSourcesResponse, VideosResponse,
};
use crate::api::wrapper::{ApiError, ApiResponse};
use crate::utils::status::{PageStatus, VideoStatus};

use std::fs;

#[derive(OpenApi)]
#[openapi(
    paths(get_video_sources, get_videos, get_video, reset_video, add_video_source, delete_video_source, reload_config, get_config, update_config),
    modifiers(&OpenAPIAuth),
    security(
        ("Token" = []),
    )
)]
pub struct ApiDoc;

/// 获取配置文件路径，提供统一的错误处理
fn get_config_path() -> Result<PathBuf> {
    dirs::config_dir()
        .context("无法获取配置目录")
        .map(|dir| dir.join("bili-sync").join("config.toml"))
}

/// 列出所有视频来源
#[utoipa::path(
    get,
    path = "/api/video-sources",
    responses(
        (status = 200, body = ApiResponse<VideoSourcesResponse>),
    )
)]
pub async fn get_video_sources(
    Extension(db): Extension<Arc<DatabaseConnection>>,
) -> Result<ApiResponse<VideoSourcesResponse>, ApiError> {
    // 获取各类视频源
    let collection_sources = collection::Entity::find()
        .select_only()
        .columns([collection::Column::Id, collection::Column::Name])
        .into_model::<VideoSource>()
        .all(db.as_ref())
        .await?;

    let favorite_sources = favorite::Entity::find()
        .select_only()
        .columns([favorite::Column::Id, favorite::Column::Name])
        .into_model::<VideoSource>()
        .all(db.as_ref())
        .await?;

    let submission_sources = submission::Entity::find()
        .select_only()
        .column(submission::Column::Id)
        .column_as(submission::Column::UpperName, "name")
        .into_model::<VideoSource>()
        .all(db.as_ref())
        .await?;

    let watch_later_sources = watch_later::Entity::find()
        .select_only()
        .column(watch_later::Column::Id)
        .column_as(Expr::value("稍后再看"), "name")
        .into_model::<VideoSource>()
        .all(db.as_ref())
        .await?;

    // 确保bangumi_sources是一个数组，即使为空
    let bangumi_sources = video_source::Entity::find()
        .filter(video_source::Column::Type.eq(1))
        .select_only()
        .columns([video_source::Column::Id, video_source::Column::Name])
        .into_model::<VideoSource>()
        .all(db.as_ref())
        .await?;

    // 返回响应，确保每个分类都是一个数组
    Ok(ApiResponse::ok(VideoSourcesResponse {
        collection: collection_sources,
        favorite: favorite_sources,
        submission: submission_sources,
        watch_later: watch_later_sources,
        bangumi: bangumi_sources,
    }))
}

/// 列出视频的基本信息，支持根据视频来源筛选、名称查找和分页
#[utoipa::path(
    get,
    path = "/api/videos",
    params(
        VideosRequest,
    ),
    responses(
        (status = 200, body = ApiResponse<VideosResponse>),
    )
)]
pub async fn get_videos(
    Extension(db): Extension<Arc<DatabaseConnection>>,
    Query(params): Query<VideosRequest>,
) -> Result<ApiResponse<VideosResponse>, ApiError> {
    let mut query = video::Entity::find();

    // 直接检查是否存在bangumi参数，单独处理
    if let Some(id) = params.bangumi {
        query = query.filter(video::Column::SourceId.eq(id).and(video::Column::SourceType.eq(1)));
    } else {
        // 处理其他常规类型
        for (field, column) in [
            (params.collection, video::Column::CollectionId),
            (params.favorite, video::Column::FavoriteId),
            (params.submission, video::Column::SubmissionId),
            (params.watch_later, video::Column::WatchLaterId),
        ] {
            if let Some(id) = field {
                query = query.filter(column.eq(id));
            }
        }
    }
    if let Some(query_word) = params.query {
        query = query.filter(video::Column::Name.contains(query_word));
    }
    let total_count = query.clone().count(db.as_ref()).await?;
    let (page, page_size) = if let (Some(page), Some(page_size)) = (params.page, params.page_size) {
        (page, page_size)
    } else {
        (1, 10)
    };
    Ok(ApiResponse::ok(VideosResponse {
        videos: query
            .order_by_desc(video::Column::Id)
            .select_only()
            .columns([
                video::Column::Id,
                video::Column::Name,
                video::Column::UpperName,
                video::Column::DownloadStatus,
            ])
            .into_tuple::<(i32, String, String, u32)>()
            .paginate(db.as_ref(), page_size)
            .fetch_page(page)
            .await?
            .into_iter()
            .map(VideoInfo::from)
            .collect(),
        total_count,
    }))
}

/// 获取视频详细信息，包括关联的所有 page
#[utoipa::path(
    get,
    path = "/api/videos/{id}",
    responses(
        (status = 200, body = ApiResponse<VideoResponse>),
    )
)]
pub async fn get_video(
    Path(id): Path<i32>,
    Extension(db): Extension<Arc<DatabaseConnection>>,
) -> Result<ApiResponse<VideoResponse>, ApiError> {
    let video_info = video::Entity::find_by_id(id)
        .select_only()
        .columns([
            video::Column::Id,
            video::Column::Name,
            video::Column::UpperName,
            video::Column::DownloadStatus,
        ])
        .into_tuple::<(i32, String, String, u32)>()
        .one(db.as_ref())
        .await?
        .map(VideoInfo::from);
    let Some(video_info) = video_info else {
        return Err(InnerApiError::NotFound(id).into());
    };
    let pages = page::Entity::find()
        .filter(page::Column::VideoId.eq(id))
        .order_by_asc(page::Column::Pid)
        .select_only()
        .columns([
            page::Column::Id,
            page::Column::Pid,
            page::Column::Name,
            page::Column::DownloadStatus,
        ])
        .into_tuple::<(i32, i32, String, u32)>()
        .all(db.as_ref())
        .await?
        .into_iter()
        .map(PageInfo::from)
        .collect();
    Ok(ApiResponse::ok(VideoResponse {
        video: video_info,
        pages,
    }))
}

/// 将某个视频与其所有分页的失败状态清空为未下载状态，这样在下次下载任务中会触发重试
#[utoipa::path(
    post,
    path = "/api/videos/{id}/reset",
    responses(
        (status = 200, body = ApiResponse<ResetVideoResponse> ),
    )
)]
pub async fn reset_video(
    Path(id): Path<i32>,
    Extension(db): Extension<Arc<DatabaseConnection>>,
) -> Result<ApiResponse<ResetVideoResponse>, ApiError> {
    let txn = db.begin().await?;
    let video_status: Option<u32> = video::Entity::find_by_id(id)
        .select_only()
        .column(video::Column::DownloadStatus)
        .into_tuple()
        .one(&txn)
        .await?;
    let Some(video_status) = video_status else {
        return Err(anyhow!(InnerApiError::NotFound(id)).into());
    };
    let resetted_pages_model: Vec<_> = page::Entity::find()
        .filter(page::Column::VideoId.eq(id))
        .all(&txn)
        .await?
        .into_iter()
        .filter_map(|mut model| {
            let mut page_status = PageStatus::from(model.download_status);
            if page_status.reset_failed() {
                model.download_status = page_status.into();
                Some(model)
            } else {
                None
            }
        })
        .collect();
    let mut video_status = VideoStatus::from(video_status);
    let mut should_update_video = video_status.reset_failed();
    if !resetted_pages_model.is_empty() {
        // 视频状态标志的第 5 位表示是否有分 P 下载失败，如果有需要重置的分页，需要同时重置视频的该状态
        video_status.set(4, 0);
        should_update_video = true;
    }
    if should_update_video {
        video::Entity::update(video::ActiveModel {
            id: Unchanged(id),
            download_status: Set(video_status.into()),
            ..Default::default()
        })
        .exec(&txn)
        .await?;
    }
    let resetted_pages_id: Vec<_> = resetted_pages_model.iter().map(|model| model.id).collect();
    let resetted_pages_model: Vec<page::ActiveModel> = resetted_pages_model
        .into_iter()
        .map(|model| model.into_active_model())
        .collect();
    for page_trunk in resetted_pages_model.chunks(50) {
        page::Entity::insert_many(page_trunk.to_vec())
            .on_conflict(
                OnConflict::column(page::Column::Id)
                    .update_column(page::Column::DownloadStatus)
                    .to_owned(),
            )
            .exec(&txn)
            .await?;
    }
    txn.commit().await?;
    Ok(ApiResponse::ok(ResetVideoResponse {
        resetted: should_update_video,
        video: id,
        pages: resetted_pages_id,
    }))
}

/// 添加新的视频源
#[utoipa::path(
    post,
    path = "/api/video-sources",
    request_body = AddVideoSourceRequest,
    responses(
        (status = 200, body = ApiResponse<AddVideoSourceResponse>),
    )
)]
pub async fn add_video_source(
    Extension(db): Extension<Arc<DatabaseConnection>>,
    axum::Json(params): axum::Json<AddVideoSourceRequest>,
) -> Result<ApiResponse<AddVideoSourceResponse>, ApiError> {
    let txn = db.begin().await?;

    let result = match params.source_type.as_str() {
        "collection" => {
            // 验证合集必需的参数
            let up_id_str = params
                .up_id
                .as_ref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("合集类型需要提供UP主ID"))?;

            let up_id = up_id_str.parse::<i64>().map_err(|_| anyhow!("无效的UP主ID"))?;

            // 添加合集
            let collection_type_value = params.collection_type.as_deref().unwrap_or("season");
            let collection_type = match collection_type_value {
                "season" => 2, // 视频合集
                "series" => 1, // 视频列表
                _ => 2,        // 默认使用season类型
            };

            let collection = collection::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                s_id: sea_orm::Set(params.source_id.parse::<i64>().map_err(|_| anyhow!("无效的合集ID"))?),
                m_id: sea_orm::Set(up_id), // 使用用户提供的UP主ID
                name: sea_orm::Set(params.name),
                r#type: sea_orm::Set(collection_type), // 使用用户选择的类型
                path: sea_orm::Set(params.path.clone()),
                created_at: sea_orm::Set(chrono::Utc::now().to_string()),
                latest_row_at: sea_orm::Set(chrono::Utc::now().naive_utc()),
            };

            let insert_result = collection::Entity::insert(collection).exec(&txn).await?;

            // 更新配置文件 - 直接修改文件
            let config_path = get_config_path()?;
            let config_content = std::fs::read_to_string(&config_path)?;
            let mut config: toml::Value = toml::from_str(&config_content)?;

            // 确保collection_list存在
            if config.get("collection_list").is_none() {
                config["collection_list"] = toml::Value::Table(toml::value::Table::new());
            }

            // 添加新项 - 使用正确的格式 type:mid:sid，使用用户提供的UP主ID
            if let Some(table) = config["collection_list"].as_table_mut() {
                let key = format!("{}:{}:{}", collection_type_value, up_id, params.source_id);
                table.insert(key, toml::Value::String(params.path.clone()));
            }

            // 写回文件
            let config_str = toml::to_string_pretty(&config)?;
            std::fs::write(&config_path, config_str)?;

            // 重新加载配置，使修改立即生效
            reload_config_file()?;

            AddVideoSourceResponse {
                success: true,
                source_id: insert_result.last_insert_id,
                source_type: "collection".to_string(),
                message: "合集添加成功".to_string(),
            }
        }
        "favorite" => {
            // 添加收藏夹
            let favorite = favorite::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                f_id: sea_orm::Set(params.source_id.parse::<i64>().map_err(|_| anyhow!("无效的收藏夹ID"))?),
                name: sea_orm::Set(params.name),
                path: sea_orm::Set(params.path.clone()),
                created_at: sea_orm::Set(chrono::Utc::now().to_string()),
                latest_row_at: sea_orm::Set(chrono::Utc::now().naive_utc()),
            };

            let insert_result = favorite::Entity::insert(favorite).exec(&txn).await?;

            // 更新配置文件 - 直接修改文件
            let config_path = get_config_path()?;
            let config_content = std::fs::read_to_string(&config_path)?;
            let mut config: toml::Value = toml::from_str(&config_content)?;

            // 确保favorite_list存在
            if config.get("favorite_list").is_none() {
                config["favorite_list"] = toml::Value::Table(toml::value::Table::new());
            }

            // 添加新项
            if let Some(table) = config["favorite_list"].as_table_mut() {
                table.insert(params.source_id.clone(), toml::Value::String(params.path.clone()));
            }

            // 写回文件
            let config_str = toml::to_string_pretty(&config)?;
            std::fs::write(&config_path, config_str)?;

            // 重新加载配置，使修改立即生效
            reload_config_file()?;

            AddVideoSourceResponse {
                success: true,
                source_id: insert_result.last_insert_id,
                source_type: "favorite".to_string(),
                message: "收藏夹添加成功".to_string(),
            }
        }
        "submission" => {
            // 添加UP主投稿
            let submission = submission::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                upper_id: sea_orm::Set(params.source_id.parse::<i64>().map_err(|_| anyhow!("无效的UP主ID"))?),
                upper_name: sea_orm::Set(params.name),
                path: sea_orm::Set(params.path.clone()),
                created_at: sea_orm::Set(chrono::Utc::now().to_string()),
                latest_row_at: sea_orm::Set(chrono::Utc::now().naive_utc()),
            };

            let insert_result = submission::Entity::insert(submission).exec(&txn).await?;

            // 更新配置文件 - 直接修改文件
            let config_path = get_config_path()?;
            let config_content = std::fs::read_to_string(&config_path)?;
            let mut config: toml::Value = toml::from_str(&config_content)?;

            // 确保submission_list存在
            if config.get("submission_list").is_none() {
                config["submission_list"] = toml::Value::Table(toml::value::Table::new());
            }

            // 添加新项
            if let Some(table) = config["submission_list"].as_table_mut() {
                table.insert(params.source_id.clone(), toml::Value::String(params.path.clone()));
            }

            // 写回文件
            let config_str = toml::to_string_pretty(&config)?;
            std::fs::write(&config_path, config_str)?;

            // 重新加载配置，使修改立即生效
            reload_config_file()?;

            AddVideoSourceResponse {
                success: true,
                source_id: insert_result.last_insert_id,
                source_type: "submission".to_string(),
                message: "UP主投稿添加成功".to_string(),
            }
        }
        "bangumi" => {
            // 添加番剧
            let media_id_clone = params.media_id.clone();
            let ep_id_clone = params.ep_id.clone();
            let download_all_seasons = params.download_all_seasons.unwrap_or(false);

            // 验证至少有一个ID不为空
            if params.source_id.is_empty() && params.media_id.is_none() && params.ep_id.is_none() {
                return Err(anyhow!("番剧标识不能全部为空，请至少提供 season_id、media_id 或 ep_id 中的一个").into());
            }

            let bangumi = video_source::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                name: sea_orm::Set(params.name),
                path: sea_orm::Set(params.path.clone()),
                r#type: sea_orm::Set(1), // 1表示番剧类型
                latest_row_at: sea_orm::Set(chrono::Utc::now().naive_utc()),
                season_id: sea_orm::Set(Some(params.source_id.clone())),
                media_id: sea_orm::Set(params.media_id),
                ep_id: sea_orm::Set(params.ep_id),
                download_all_seasons: sea_orm::Set(Some(download_all_seasons)),
                ..Default::default()
            };

            let insert_result = video_source::Entity::insert(bangumi).exec(&txn).await?;

            // 更新配置文件 - 直接修改文件
            let config_path = get_config_path()?;
            let config_content = std::fs::read_to_string(&config_path)?;
            let mut config: toml::Value = toml::from_str(&config_content)?;

            // 创建新的bangumi配置
            let mut bangumi_item = toml::value::Table::new();
            if !params.source_id.is_empty() {
                bangumi_item.insert("season_id".to_string(), toml::Value::String(params.source_id.clone()));
            }
            if let Some(media_id) = &media_id_clone {
                bangumi_item.insert("media_id".to_string(), toml::Value::String(media_id.clone()));
            }
            if let Some(ep_id) = &ep_id_clone {
                bangumi_item.insert("ep_id".to_string(), toml::Value::String(ep_id.clone()));
            }
            bangumi_item.insert("path".to_string(), toml::Value::String(params.path.clone()));
            bangumi_item.insert(
                "download_all_seasons".to_string(),
                toml::Value::Boolean(download_all_seasons),
            );

            // 安全地添加到bangumi数组
            match config.get_mut("bangumi") {
                Some(toml::Value::Array(array)) => {
                    // 如果bangumi存在且是数组类型，直接添加
                    array.push(toml::Value::Table(bangumi_item));
                }
                _ => {
                    // 如果bangumi不存在或不是数组类型，创建新数组
                    let mut new_array = Vec::new();
                    new_array.push(toml::Value::Table(bangumi_item));

                    // 使用insert方法而不是索引操作，避免可能的panic
                    if let Some(table) = config.as_table_mut() {
                        table.insert("bangumi".to_string(), toml::Value::Array(new_array));
                    } else {
                        // 如果config不是table类型，这是一个严重错误
                        return Err(anyhow!("配置文件格式错误，无法添加番剧配置").into());
                    }
                }
            }

            // 写回文件
            let config_str = toml::to_string_pretty(&config)?;
            std::fs::write(&config_path, config_str)?;

            // 重新加载配置，使修改立即生效
            reload_config_file()?;

            AddVideoSourceResponse {
                success: true,
                source_id: insert_result.last_insert_id,
                source_type: "bangumi".to_string(),
                message: "番剧添加成功".to_string(),
            }
        }
        "watch_later" => {
            // 稍后观看只能有一个
            let existing = watch_later::Entity::find().count(&txn).await?;

            if existing > 0 {
                return Err(anyhow!("已存在稍后观看配置，无法添加多个").into());
            }

            let watch_later = watch_later::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                path: sea_orm::Set(params.path.clone()),
                latest_row_at: sea_orm::Set(chrono::Utc::now().naive_utc()),
                ..Default::default()
            };

            let insert_result = watch_later::Entity::insert(watch_later).exec(&txn).await?;

            // 更新配置文件 - 直接修改文件
            let config_path = get_config_path()?;
            let config_content = std::fs::read_to_string(&config_path)?;
            let mut config: toml::Value = toml::from_str(&config_content)?;

            // 确保watch_later存在
            if config.get("watch_later").is_none() {
                config["watch_later"] = toml::Value::Table(toml::value::Table::new());
            }

            // 设置稍后观看配置
            config["watch_later"]["enabled"] = toml::Value::Boolean(true);
            config["watch_later"]["path"] = toml::Value::String(params.path.clone());

            // 写回文件
            let config_str = toml::to_string_pretty(&config)?;
            std::fs::write(&config_path, config_str)?;

            // 重新加载配置，使修改立即生效
            reload_config_file()?;

            AddVideoSourceResponse {
                success: true,
                source_id: insert_result.last_insert_id,
                source_type: "watch_later".to_string(),
                message: "稍后观看添加成功".to_string(),
            }
        }
        _ => return Err(anyhow!("不支持的视频源类型: {}", params.source_type).into()),
    };

    // 确保目标路径存在
    fs::create_dir_all(&params.path).map_err(|e| anyhow!("创建目录失败: {}", e))?;

    txn.commit().await?;
    Ok(ApiResponse::ok(result))
}

/// 重新加载配置
#[utoipa::path(
    post,
    path = "/api/reload-config",
    responses(
        (status = 200, body = ApiResponse<bool>),
    )
)]
pub async fn reload_config() -> Result<ApiResponse<bool>, ApiError> {
    // 调用config中的reload_config函数获取新配置
    let _new_config = crate::config::reload_config();

    // 将配置应用到数据库或其他状态管理中
    // 这里我们可以执行额外的初始化操作，如果需要的话
    info!("配置已重新加载");

    // 返回成功响应
    Ok(ApiResponse::ok(true))
}

/// 删除视频源
#[utoipa::path(
    delete,
    path = "/api/video-sources/{source_type}/{id}",
    params(
        ("source_type" = String, Path, description = "视频源类型"),
        ("id" = i32, Path, description = "视频源ID"),
        ("delete_local_files" = bool, Query, description = "是否删除本地文件")
    ),
    responses(
        (status = 200, body = ApiResponse<DeleteVideoSourceResponse>),
    )
)]
pub async fn delete_video_source(
    Extension(db): Extension<Arc<DatabaseConnection>>,
    Path((source_type, id)): Path<(String, i32)>,
    Query(params): Query<crate::api::request::DeleteVideoSourceRequest>,
) -> Result<ApiResponse<crate::api::response::DeleteVideoSourceResponse>, ApiError> {
    let txn = db.begin().await?;

    let delete_local_files = params.delete_local_files;

    // 根据不同类型的视频源执行不同的删除操作
    let result = match source_type.as_str() {
        "collection" => {
            // 查找要删除的合集
            let collection = collection::Entity::find_by_id(id)
                .one(&txn)
                .await?
                .ok_or_else(|| anyhow!("未找到指定的合集"))?;

            // 删除相关的视频和页面数据
            let videos = video::Entity::find()
                .filter(video::Column::CollectionId.eq(id))
                .all(&txn)
                .await?;

            for video in &videos {
                // 删除页面数据
                page::Entity::delete_many()
                    .filter(page::Column::VideoId.eq(video.id))
                    .exec(&txn)
                    .await?;
            }

            // 删除视频数据
            video::Entity::delete_many()
                .filter(video::Column::CollectionId.eq(id))
                .exec(&txn)
                .await?;

            // 如果需要删除本地文件
            if delete_local_files {
                // 尝试删除本地文件夹
                let path = &collection.path;
                if let Err(e) = std::fs::remove_dir_all(path) {
                    warn!("删除合集文件夹失败: {}", e);
                }
            }

            // 删除数据库中的记录
            collection::Entity::delete_by_id(id).exec(&txn).await?;

            // 更新配置文件 - 从 collection_list HashMap 中删除
            update_config_file(|config| {
                // 查找并删除对应的合集配置
                config.collection_list.retain(|collection_item, _| {
                    !(collection_item.mid == collection.m_id.to_string()
                        && collection_item.sid == collection.s_id.to_string())
                });
                Ok(())
            })?;

            crate::api::response::DeleteVideoSourceResponse {
                success: true,
                source_id: id,
                source_type: "collection".to_string(),
                message: format!("合集 {} 已成功删除", collection.name),
            }
        }
        "favorite" => {
            // 查找要删除的收藏夹
            let favorite = favorite::Entity::find_by_id(id)
                .one(&txn)
                .await?
                .ok_or_else(|| anyhow!("未找到指定的收藏夹"))?;

            // 删除相关的视频和页面数据
            let videos = video::Entity::find()
                .filter(video::Column::FavoriteId.eq(id))
                .all(&txn)
                .await?;

            for video in &videos {
                // 删除页面数据
                page::Entity::delete_many()
                    .filter(page::Column::VideoId.eq(video.id))
                    .exec(&txn)
                    .await?;
            }

            // 删除视频数据
            video::Entity::delete_many()
                .filter(video::Column::FavoriteId.eq(id))
                .exec(&txn)
                .await?;

            // 如果需要删除本地文件
            if delete_local_files {
                // 尝试删除本地文件夹
                let path = &favorite.path;
                if let Err(e) = std::fs::remove_dir_all(path) {
                    warn!("删除收藏夹文件夹失败: {}", e);
                }
            }

            // 删除数据库中的记录
            favorite::Entity::delete_by_id(id).exec(&txn).await?;

            // 更新配置文件 - 从 favorite_list HashMap 中删除
            update_config_file(|config| {
                config.favorite_list.retain(|fid, _| fid != &favorite.f_id.to_string());
                Ok(())
            })?;

            crate::api::response::DeleteVideoSourceResponse {
                success: true,
                source_id: id,
                source_type: "favorite".to_string(),
                message: format!("收藏夹 {} 已成功删除", favorite.name),
            }
        }
        "submission" => {
            // 查找要删除的UP主投稿
            let submission = submission::Entity::find_by_id(id)
                .one(&txn)
                .await?
                .ok_or_else(|| anyhow!("未找到指定的UP主投稿"))?;

            // 删除相关的视频和页面数据
            let videos = video::Entity::find()
                .filter(video::Column::SubmissionId.eq(id))
                .all(&txn)
                .await?;

            for video in &videos {
                // 删除页面数据
                page::Entity::delete_many()
                    .filter(page::Column::VideoId.eq(video.id))
                    .exec(&txn)
                    .await?;
            }

            // 删除视频数据
            video::Entity::delete_many()
                .filter(video::Column::SubmissionId.eq(id))
                .exec(&txn)
                .await?;

            // 如果需要删除本地文件
            if delete_local_files {
                // 尝试删除本地文件夹
                let path = &submission.path;
                if let Err(e) = std::fs::remove_dir_all(path) {
                    warn!("删除UP主投稿文件夹失败: {}", e);
                }
            }

            // 删除数据库中的记录
            submission::Entity::delete_by_id(id).exec(&txn).await?;

            // 更新配置文件 - 从 submission_list HashMap 中删除
            update_config_file(|config| {
                config
                    .submission_list
                    .retain(|upper_id, _| upper_id != &submission.upper_id.to_string());
                Ok(())
            })?;

            crate::api::response::DeleteVideoSourceResponse {
                success: true,
                source_id: id,
                source_type: "submission".to_string(),
                message: format!("UP主 {} 的投稿已成功删除", submission.upper_name),
            }
        }
        "watch_later" => {
            // 查找要删除的稍后再看
            let watch_later = watch_later::Entity::find_by_id(id)
                .one(&txn)
                .await?
                .ok_or_else(|| anyhow!("未找到指定的稍后再看"))?;

            // 删除相关的视频和页面数据
            let videos = video::Entity::find()
                .filter(video::Column::WatchLaterId.eq(id))
                .all(&txn)
                .await?;

            for video in &videos {
                // 删除页面数据
                page::Entity::delete_many()
                    .filter(page::Column::VideoId.eq(video.id))
                    .exec(&txn)
                    .await?;
            }

            // 删除视频数据
            video::Entity::delete_many()
                .filter(video::Column::WatchLaterId.eq(id))
                .exec(&txn)
                .await?;

            // 如果需要删除本地文件
            if delete_local_files {
                // 尝试删除本地文件夹
                let path = &watch_later.path;
                if let Err(e) = std::fs::remove_dir_all(path) {
                    warn!("删除稍后再看文件夹失败: {}", e);
                }
            }

            // 删除数据库中的记录
            watch_later::Entity::delete_by_id(id).exec(&txn).await?;

            // 更新配置文件 - 禁用 watch_later
            update_config_file(|config| {
                config.watch_later.enabled = false;
                Ok(())
            })?;

            crate::api::response::DeleteVideoSourceResponse {
                success: true,
                source_id: id,
                source_type: "watch_later".to_string(),
                message: "稍后再看已成功删除".to_string(),
            }
        }
        "bangumi" => {
            // 查找要删除的番剧
            let bangumi = video_source::Entity::find_by_id(id)
                .one(&txn)
                .await?
                .ok_or_else(|| anyhow!("未找到指定的番剧"))?;

            // 删除相关的视频和页面数据
            let videos = video::Entity::find()
                .filter(video::Column::SourceId.eq(id))
                .filter(video::Column::SourceType.eq(1)) // 番剧类型
                .all(&txn)
                .await?;

            for video in &videos {
                // 删除页面数据
                page::Entity::delete_many()
                    .filter(page::Column::VideoId.eq(video.id))
                    .exec(&txn)
                    .await?;
            }

            // 删除视频数据
            video::Entity::delete_many()
                .filter(video::Column::SourceId.eq(id))
                .filter(video::Column::SourceType.eq(1))
                .exec(&txn)
                .await?;

            // 如果需要删除本地文件
            if delete_local_files {
                // 尝试删除本地文件夹
                let path = &bangumi.path;
                if let Err(e) = std::fs::remove_dir_all(path) {
                    warn!("删除番剧文件夹失败: {}", e);
                }
            }

            // 删除数据库中的记录
            video_source::Entity::delete_by_id(id).exec(&txn).await?;

            // 更新配置文件 - 从 bangumi Vec 中删除
            update_config_file(|config| {
                config.bangumi.retain(|bangumi_config| {
                    // 根据 season_id 匹配删除
                    bangumi_config.season_id.as_ref() != bangumi.season_id.as_ref()
                });
                Ok(())
            })?;

            crate::api::response::DeleteVideoSourceResponse {
                success: true,
                source_id: id,
                source_type: "bangumi".to_string(),
                message: format!("番剧 {} 已成功删除", bangumi.name),
            }
        }
        _ => return Err(anyhow!("不支持的视频源类型: {}", source_type).into()),
    };

    txn.commit().await?;
    Ok(ApiResponse::ok(result))
}

/// 更新配置文件的辅助函数
fn update_config_file<F>(update_fn: F) -> Result<()>
where
    F: FnOnce(&mut crate::config::Config) -> Result<()>,
{
    // 重新加载当前配置
    let mut config = crate::config::reload_config();

    // 应用更新函数
    update_fn(&mut config)?;

    // 保存更新后的配置
    config.save()?;

    // 重新加载全局配置
    crate::config::reload_config();

    info!("配置文件已更新，视频源删除完成");
    Ok(())
}

// 在添加视频源成功后调用此函数获取新配置
fn reload_config_file() -> Result<()> {
    // 使用公共的 reload_config 函数重新加载配置
    let new_config = crate::config::reload_config();

    // 保存新配置以确保格式正确和一致性
    if let Err(e) = new_config.save() {
        warn!("保存重载的配置时出错: {}", e);
    }

    info!("配置文件已成功重新加载，新添加的视频源将在下一轮下载任务中生效");
    Ok(())
}

/// 获取当前配置
#[utoipa::path(
    get,
    path = "/api/config",
    responses(
        (status = 200, description = "获取配置成功", body = ConfigResponse),
        (status = 500, description = "服务器内部错误", body = String)
    )
)]
pub async fn get_config() -> Result<ApiResponse<crate::api::response::ConfigResponse>, ApiError> {
    // 使用reload_config获取最新配置，而不是使用全局CONFIG
    let config = crate::config::reload_config();

    let nfo_time_type = match config.nfo_time_type {
        crate::config::NFOTimeType::FavTime => "favtime",
        crate::config::NFOTimeType::PubTime => "pubtime",
    };

    Ok(ApiResponse::ok(crate::api::response::ConfigResponse {
        video_name: config.video_name.to_string(),
        page_name: config.page_name.to_string(),
        multi_page_name: config.multi_page_name.to_string(),
        bangumi_name: config.bangumi_name.to_string(),
        folder_structure: config.folder_structure.to_string(),
        time_format: config.time_format.clone(),
        interval: config.interval,
        nfo_time_type: nfo_time_type.to_string(),
    }))
}

/// 更新配置
#[utoipa::path(
    put,
    path = "/api/config",
    request_body = UpdateConfigRequest,
    responses(
        (status = 200, description = "配置更新成功", body = UpdateConfigResponse),
        (status = 400, description = "请求参数错误", body = String),
        (status = 500, description = "服务器内部错误", body = String)
    )
)]
pub async fn update_config(
    Extension(db): Extension<Arc<DatabaseConnection>>,
    axum::Json(params): axum::Json<crate::api::request::UpdateConfigRequest>,
) -> Result<ApiResponse<crate::api::response::UpdateConfigResponse>, ApiError> {
    use std::borrow::Cow;

    // 获取当前配置的副本
    let mut config = crate::config::reload_config();
    let mut updated_fields = Vec::new();

    // 记录原始的NFO时间类型，用于比较是否真正发生了变化
    let original_nfo_time_type = config.nfo_time_type.clone();

    // 记录原始的命名相关配置，用于比较是否真正发生了变化
    let original_video_name = config.video_name.clone();
    let original_page_name = config.page_name.clone();
    let original_multi_page_name = config.multi_page_name.clone();
    let original_bangumi_name = config.bangumi_name.clone();
    let original_folder_structure = config.folder_structure.clone();

    // 更新配置字段
    if let Some(video_name) = params.video_name {
        if !video_name.trim().is_empty() && video_name != original_video_name.as_ref() {
            config.video_name = Cow::Owned(video_name);
            updated_fields.push("video_name");
        }
    }

    if let Some(page_name) = params.page_name {
        if !page_name.trim().is_empty() && page_name != original_page_name.as_ref() {
            config.page_name = Cow::Owned(page_name);
            updated_fields.push("page_name");
        }
    }

    if let Some(multi_page_name) = params.multi_page_name {
        if !multi_page_name.trim().is_empty() && multi_page_name != original_multi_page_name.as_ref() {
            config.multi_page_name = Cow::Owned(multi_page_name);
            updated_fields.push("multi_page_name");
        }
    }

    if let Some(folder_structure) = params.folder_structure {
        if !folder_structure.trim().is_empty() && folder_structure != original_folder_structure.as_ref() {
            config.folder_structure = Cow::Owned(folder_structure);
            updated_fields.push("folder_structure");
        }
    }

    if let Some(time_format) = params.time_format {
        if !time_format.trim().is_empty() && time_format != config.time_format {
            config.time_format = time_format;
            updated_fields.push("time_format");
        }
    }

    if let Some(interval) = params.interval {
        if interval > 0 && interval != config.interval {
            config.interval = interval;
            updated_fields.push("interval");
        }
    }

    if let Some(nfo_time_type) = params.nfo_time_type {
        let new_nfo_time_type = match nfo_time_type.as_str() {
            "favtime" => crate::config::NFOTimeType::FavTime,
            "pubtime" => crate::config::NFOTimeType::PubTime,
            _ => return Err(anyhow!("无效的NFO时间类型，只支持 'favtime' 或 'pubtime'").into()),
        };

        // 只有当NFO时间类型真正发生变化时才标记为需要更新
        if original_nfo_time_type != new_nfo_time_type {
            config.nfo_time_type = new_nfo_time_type;
            updated_fields.push("nfo_time_type");
        }
    }

    if let Some(bangumi_name) = params.bangumi_name {
        if !bangumi_name.trim().is_empty() && bangumi_name != original_bangumi_name.as_ref() {
            config.bangumi_name = Cow::Owned(bangumi_name);
            updated_fields.push("bangumi_name");
        }
    }

    if updated_fields.is_empty() {
        return Ok(ApiResponse::ok(crate::api::response::UpdateConfigResponse {
            success: false,
            message: "没有提供有效的配置更新".to_string(),
            updated_files: None,
        }));
    }

    // 保存配置到文件
    config.save()?;

    // 重新加载全局配置
    crate::config::reload_config();

    // 如果更新了命名相关的配置，重命名已下载的文件
    let mut updated_files = 0u32;
    let naming_fields = [
        "video_name",
        "page_name",
        "multi_page_name",
        "bangumi_name",
        "folder_structure",
    ];
    let should_rename = updated_fields.iter().any(|field| naming_fields.contains(field));

    if should_rename {
        // 根据更新的字段类型来决定重命名哪些文件
        let rename_single_page = updated_fields.contains(&"page_name") || updated_fields.contains(&"video_name");
        let rename_multi_page = updated_fields.contains(&"multi_page_name") || updated_fields.contains(&"video_name");
        let rename_bangumi = updated_fields.contains(&"bangumi_name") || updated_fields.contains(&"video_name");
        let rename_folder_structure = updated_fields.contains(&"folder_structure");

        // 执行文件重命名并等待完成
        match rename_existing_files(
            db.clone(),
            &config,
            rename_single_page,
            rename_multi_page,
            rename_bangumi,
            rename_folder_structure,
        )
        .await
        {
            Ok(count) => {
                updated_files = count;
            }
            Err(e) => {
                error!("重命名已下载文件时出错: {}", e);
                // 即使重命名失败，配置更新仍然成功
            }
        }
    }

    // 检查是否需要重新生成NFO文件
    let should_regenerate_nfo = updated_fields.contains(&"nfo_time_type");

    if should_regenerate_nfo {
        // 重新生成NFO文件
        match regenerate_nfo_files(db.clone(), &config).await {
            Ok(count) => {
                if !should_rename {
                    updated_files = count;
                }
            }
            Err(e) => {
                error!("重新生成NFO文件时出错: {}", e);
                // 即使重新生成失败，配置更新仍然成功
            }
        }
    }

    Ok(ApiResponse::ok(crate::api::response::UpdateConfigResponse {
        success: true,
        message: if should_rename && should_regenerate_nfo {
            format!(
                "配置更新成功，已更新字段: {}，重命名了文件并重新生成了NFO文件",
                updated_fields.join(", ")
            )
        } else if should_rename {
            format!(
                "配置更新成功，已更新字段: {}，重命名了 {} 个文件/文件夹",
                updated_fields.join(", "),
                updated_files
            )
        } else if should_regenerate_nfo {
            format!(
                "配置更新成功，已更新字段: {}，重新生成了 {} 个NFO文件",
                updated_fields.join(", "),
                updated_files
            )
        } else {
            format!("配置更新成功，已更新字段: {}", updated_fields.join(", "))
        },
        updated_files: if should_rename || should_regenerate_nfo {
            Some(updated_files)
        } else {
            None
        },
    }))
}

/// 查找分页文件的原始命名模式
fn find_page_file_pattern(video_path: &std::path::Path, page: &bili_sync_entity::page::Model) -> Result<String> {
    if !video_path.exists() {
        return Ok(String::new());
    }

    if let Ok(entries) = std::fs::read_dir(video_path) {
        for entry in entries.flatten() {
            let file_path = entry.path();
            let file_name = file_path.file_name().unwrap_or_default().to_string_lossy();

            // 尝试通过文件名中的分页编号来匹配主文件（MP4）
            if file_name.ends_with(".mp4")
                && (file_name.contains(&format!("{:02}", page.pid))
                    || file_name.contains(&format!("{:03}", page.pid))
                    || file_name.contains(&page.name))
            {
                // 找到MP4文件，提取文件名（不包括扩展名）
                if let Some(file_stem) = file_path.file_stem() {
                    return Ok(file_stem.to_string_lossy().to_string());
                }
            }
        }
    }

    Ok(String::new())
}

/// 重命名已下载的文件以匹配新的命名规则
#[allow(unused_variables)] // rename_folder_structure 参数表示是否更新了 folder_structure 配置，虽然当前未使用但保留以备将来扩展
async fn rename_existing_files(
    db: Arc<DatabaseConnection>,
    config: &crate::config::Config,
    rename_single_page: bool,
    rename_multi_page: bool,
    rename_bangumi: bool,
    rename_folder_structure: bool,
) -> Result<u32> {
    use handlebars::Handlebars;
    use sea_orm::*;
    use std::path::Path;

    info!("开始重命名已下载的文件以匹配新的配置...");

    let mut updated_count = 0u32;

    // 创建模板引擎
    let mut handlebars = Handlebars::new();

    // 使用register_template_string而不是path_safe_register来避免生命周期问题
    let video_template = config.video_name.replace(std::path::MAIN_SEPARATOR_STR, "__SEP__");
    let page_template = config.page_name.replace(std::path::MAIN_SEPARATOR_STR, "__SEP__");

    handlebars.register_template_string("video", video_template)?;
    handlebars.register_template_string("page", page_template)?;

    // 分别处理不同类型的视频
    let mut all_videos = Vec::new();

    // 1. 处理非番剧类型的视频（原有逻辑）
    if rename_single_page || rename_multi_page {
        let regular_videos = bili_sync_entity::video::Entity::find()
            .filter(bili_sync_entity::video::Column::DownloadStatus.gt(0))
            .filter(
                // 排除番剧类型（source_type=1），包含其他所有类型
                bili_sync_entity::video::Column::SourceType
                    .is_null()
                    .or(bili_sync_entity::video::Column::SourceType.ne(1)),
            )
            .all(db.as_ref())
            .await?;
        all_videos.extend(regular_videos);
    }

    // 2. 处理番剧类型的视频
    if rename_bangumi {
        let bangumi_videos = bili_sync_entity::video::Entity::find()
            .filter(bili_sync_entity::video::Column::DownloadStatus.gt(0))
            .filter(bili_sync_entity::video::Column::SourceType.eq(1)) // 番剧类型
            .all(db.as_ref())
            .await?;
        all_videos.extend(bangumi_videos);
    }

    info!("找到 {} 个需要检查的视频", all_videos.len());

    for video in all_videos {
        // 检查视频类型，决定是否需要重命名
        let is_single_page = video.single_page.unwrap_or(true);
        let is_bangumi = video.source_type == Some(1);

        // 根据视频类型和配置更新情况决定是否跳过
        let should_process_video = if is_bangumi {
            rename_bangumi // 番剧视频只在bangumi_name或video_name更新时处理
        } else if is_single_page {
            rename_single_page // 单P视频只在page_name或video_name更新时处理
        } else {
            rename_multi_page // 多P视频只在multi_page_name或video_name更新时处理
        };

        if !should_process_video {
            let video_type = if is_bangumi {
                "番剧"
            } else if is_single_page {
                "单P"
            } else {
                "多P"
            };
            debug!("跳过视频重命名: {} (类型: {})", video.name, video_type);
            continue;
        }

        // 构建模板数据
        let mut template_data = std::collections::HashMap::new();
        template_data.insert("title".to_string(), serde_json::Value::String(video.name.clone()));
        template_data.insert("show_title".to_string(), serde_json::Value::String(video.name.clone()));
        template_data.insert("bvid".to_string(), serde_json::Value::String(video.bvid.clone()));
        template_data.insert(
            "upper_name".to_string(),
            serde_json::Value::String(video.upper_name.clone()),
        );
        template_data.insert(
            "upper_mid".to_string(),
            serde_json::Value::String(video.upper_id.to_string()),
        );

        // 格式化时间
        let formatted_pubtime = video.pubtime.format(&config.time_format).to_string();
        template_data.insert("pubtime".to_string(), serde_json::Value::String(formatted_pubtime));

        let formatted_favtime = video.favtime.format(&config.time_format).to_string();
        template_data.insert("fav_time".to_string(), serde_json::Value::String(formatted_favtime));

        let formatted_ctime = video.ctime.format(&config.time_format).to_string();
        template_data.insert("ctime".to_string(), serde_json::Value::String(formatted_ctime));

        // 确定最终的视频文件夹路径
        let final_video_path = if is_bangumi {
            // 番剧不重命名视频文件夹，直接使用现有路径
            let video_path = Path::new(&video.path);
            if video_path.exists() {
                video_path.to_path_buf()
            } else {
                // 如果路径不存在，尝试智能查找
                if let Some(parent_dir) = video_path.parent() {
                    if let Ok(entries) = std::fs::read_dir(parent_dir) {
                        let mut found_path = None;
                        for entry in entries.flatten() {
                            let entry_path = entry.path();
                            if entry_path.is_dir() {
                                let dir_name = entry_path.file_name().unwrap_or_default().to_string_lossy();
                                // 检查是否包含视频的bvid或标题
                                if dir_name.contains(&video.bvid) || dir_name.contains(&video.name) {
                                    found_path = Some(entry_path);
                                    break;
                                }
                            }
                        }
                        found_path.unwrap_or_else(|| video_path.to_path_buf())
                    } else {
                        video_path.to_path_buf()
                    }
                } else {
                    video_path.to_path_buf()
                }
            }
        } else {
            // 非番剧视频的重命名逻辑（原有逻辑）
            // 渲染新的视频文件夹名称（使用video_name模板）
            let template_value = serde_json::Value::Object(template_data.clone().into_iter().collect());
            let rendered_name = handlebars
                .render("video", &template_value)
                .unwrap_or_else(|_| video.name.clone());
            let new_video_name =
                crate::utils::filenamify::filenamify(&rendered_name).replace("__SEP__", std::path::MAIN_SEPARATOR_STR);

            // 使用视频记录中的路径信息
            let video_path = Path::new(&video.path);
            if let Some(parent_dir) = video_path.parent() {
                let old_path = video_path;
                let new_path = parent_dir.join(&new_video_name);

                // 智能查找实际的文件夹路径
                let actual_old_path = if old_path.exists() {
                    old_path.to_path_buf()
                } else {
                    // 尝试在父目录中查找可能的文件夹
                    if let Ok(entries) = std::fs::read_dir(parent_dir) {
                        let mut found_path = None;
                        for entry in entries.flatten() {
                            let entry_path = entry.path();
                            if entry_path.is_dir() {
                                let dir_name = entry_path.file_name().unwrap_or_default().to_string_lossy();
                                // 检查是否包含视频的bvid或标题
                                if dir_name.contains(&video.bvid) || dir_name.contains(&video.name) {
                                    found_path = Some(entry_path);
                                    break;
                                }
                            }
                        }
                        found_path.unwrap_or_else(|| old_path.to_path_buf())
                    } else {
                        old_path.to_path_buf()
                    }
                };

                // 确定最终的视频文件夹路径
                if actual_old_path.exists() && actual_old_path != new_path {
                    // 需要重命名视频文件夹
                    match std::fs::rename(&actual_old_path, &new_path) {
                        Ok(_) => {
                            debug!("重命名视频文件夹成功: {:?} -> {:?}", actual_old_path, new_path);
                            updated_count += 1;
                            new_path.clone()
                        }
                        Err(e) => {
                            warn!(
                                "重命名视频文件夹失败: {:?} -> {:?}, 错误: {}",
                                actual_old_path, new_path, e
                            );
                            actual_old_path.clone()
                        }
                    }
                } else if actual_old_path.exists() {
                    // 文件夹已经是正确的名称，不需要重命名
                    actual_old_path.clone()
                } else {
                    // 文件夹不存在，使用新路径
                    new_path.clone()
                }
            } else {
                video_path.to_path_buf()
            }
        };

        // **关键修复：始终更新数据库中的路径记录**
        // 不管文件夹是否重命名，都要确保数据库路径与实际文件系统路径一致
        let final_path_str = final_video_path.to_string_lossy().to_string();
        if video.path != final_path_str {
            let mut video_update: bili_sync_entity::video::ActiveModel = video.clone().into();
            video_update.path = Set(final_path_str.clone());
            if let Err(e) = video_update.update(db.as_ref()).await {
                warn!("更新数据库中的视频路径失败: {}", e);
            } else {
                debug!("更新数据库视频路径: {} -> {}", video.path, final_path_str);
            }
        }

        // **新增：处理视频级别的文件重命名（poster、fanart、nfo）**
        // 只对非番剧的多P视频进行视频级别文件重命名
        if !is_single_page && !is_bangumi {
            // 多P视频需要重命名视频级别的文件
            let old_video_name = Path::new(&video.path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| video.name.clone());

            let new_video_name = final_video_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| video.name.clone());

            if old_video_name != new_video_name {
                // 重命名视频级别的文件
                let video_level_files = [
                    (
                        format!("{}-poster.jpg", old_video_name),
                        format!("{}-poster.jpg", new_video_name),
                    ),
                    (
                        format!("{}-fanart.jpg", old_video_name),
                        format!("{}-fanart.jpg", new_video_name),
                    ),
                    (format!("{}.nfo", old_video_name), format!("{}.nfo", new_video_name)),
                    // 兼容旧的硬编码文件名
                    ("poster.jpg".to_string(), format!("{}-poster.jpg", new_video_name)),
                    ("fanart.jpg".to_string(), format!("{}-fanart.jpg", new_video_name)),
                    ("tvshow.nfo".to_string(), format!("{}.nfo", new_video_name)),
                ];

                for (old_file_name, new_file_name) in video_level_files {
                    let old_file_path = final_video_path.join(&old_file_name);
                    let new_file_path = final_video_path.join(&new_file_name);

                    if old_file_path.exists() && old_file_path != new_file_path {
                        match std::fs::rename(&old_file_path, &new_file_path) {
                            Ok(_) => {
                                debug!("重命名视频级别文件成功: {:?} -> {:?}", old_file_path, new_file_path);
                                updated_count += 1;
                            }
                            Err(e) => {
                                warn!(
                                    "重命名视频级别文件失败: {:?} -> {:?}, 错误: {}",
                                    old_file_path, new_file_path, e
                                );
                            }
                        }
                    }
                }
            }
        }

        // 处理分页视频的重命名
        let pages = bili_sync_entity::page::Entity::find()
            .filter(bili_sync_entity::page::Column::VideoId.eq(video.id))
            .filter(bili_sync_entity::page::Column::DownloadStatus.gt(0))
            .all(db.as_ref())
            .await?;

        for page in pages {
            // 为分页添加额外的模板数据
            let mut page_template_data = template_data.clone();
            page_template_data.insert("ptitle".to_string(), serde_json::Value::String(page.name.clone()));
            page_template_data.insert("pid".to_string(), serde_json::Value::String(page.pid.to_string()));
            page_template_data.insert(
                "pid_pad".to_string(),
                serde_json::Value::String(format!("{:02}", page.pid)),
            );

            // 为多P视频和番剧添加season相关变量
            if !is_single_page || is_bangumi {
                let season_number = if is_bangumi {
                    video.season_number.unwrap_or(1)
                } else {
                    1
                };
                let episode_number = if is_bangumi {
                    video.episode_number.unwrap_or(page.pid)
                } else {
                    page.pid
                };

                page_template_data.insert(
                    "season".to_string(),
                    serde_json::Value::String(season_number.to_string()),
                );
                page_template_data.insert(
                    "season_pad".to_string(),
                    serde_json::Value::String(format!("{:02}", season_number)),
                );
                page_template_data.insert("pid".to_string(), serde_json::Value::String(episode_number.to_string()));
                page_template_data.insert(
                    "pid_pad".to_string(),
                    serde_json::Value::String(format!("{:02}", episode_number)),
                );
            }

            page_template_data.insert(
                "duration".to_string(),
                serde_json::Value::String(page.duration.to_string()),
            );

            if let Some(width) = page.width {
                page_template_data.insert("width".to_string(), serde_json::Value::String(width.to_string()));
            }

            if let Some(height) = page.height {
                page_template_data.insert("height".to_string(), serde_json::Value::String(height.to_string()));
            }

            // 根据视频类型选择不同的模板
            let page_template_value = serde_json::Value::Object(page_template_data.into_iter().collect());
            let rendered_page_name = if is_bangumi {
                // 番剧使用bangumi_name模板
                match handlebars.render_template(&config.bangumi_name, &page_template_value) {
                    Ok(rendered) => rendered,
                    Err(_) => {
                        // 如果渲染失败，使用默认番剧格式
                        let season_number = video.season_number.unwrap_or(1);
                        let episode_number = video.episode_number.unwrap_or(page.pid);
                        format!("S{:02}E{:02}-{:02}", season_number, episode_number, episode_number)
                    }
                }
            } else if is_single_page {
                // 单P视频使用page_name模板
                handlebars
                    .render("page", &page_template_value)
                    .unwrap_or_else(|_| page.name.clone())
            } else {
                // 多P视频使用multi_page_name模板
                match handlebars.render_template(&config.multi_page_name, &page_template_value) {
                    Ok(rendered) => rendered,
                    Err(_) => {
                        // 如果渲染失败，使用默认格式
                        format!("S01E{:02}-{:02}", page.pid, page.pid)
                    }
                }
            };

            let new_page_name = crate::utils::filenamify::filenamify(&rendered_page_name)
                .replace("__SEP__", std::path::MAIN_SEPARATOR_STR);

            // **关键修复：重命名分页的所有相关文件**
            // 从数据库存储的路径或智能查找中获取原始文件名模式（去掉扩展名）
            let old_page_name = if let Some(stored_path) = &page.path {
                let stored_file_path = Path::new(stored_path);
                if let Some(file_stem) = stored_file_path.file_stem() {
                    file_stem.to_string_lossy().to_string()
                } else {
                    // 如果无法从存储路径提取，尝试智能查找
                    find_page_file_pattern(&final_video_path, &page)?
                }
            } else {
                // 数据库中没有存储路径，尝试智能查找
                find_page_file_pattern(&final_video_path, &page)?
            };

            // 如果找到了原始文件名模式，重命名所有相关文件
            if !old_page_name.is_empty() && old_page_name != new_page_name {
                debug!(
                    "准备重命名分页 {} 的文件：{} -> {}",
                    page.pid, old_page_name, new_page_name
                );

                if final_video_path.exists() {
                    if let Ok(entries) = std::fs::read_dir(&final_video_path) {
                        for entry in entries.flatten() {
                            let file_path = entry.path();
                            let file_name = file_path.file_name().unwrap_or_default().to_string_lossy();

                            // 检查文件是否属于当前分页（使用原始文件名模式匹配）
                            // 匹配规则：文件名以原始模式开头，后面可以跟扩展名或其他后缀
                            if file_name.starts_with(&old_page_name) {
                                // 提取原始文件名后面的部分（扩展名和其他后缀）
                                let suffix = file_name.strip_prefix(&old_page_name).unwrap_or("");

                                // 构建新的文件名：新模式 + 原有的后缀
                                let new_file_name = format!("{}{}", new_page_name, suffix);
                                let new_file_path = final_video_path.join(new_file_name);

                                // 只有当新旧路径不同时才进行重命名
                                if file_path != new_file_path {
                                    match std::fs::rename(&file_path, &new_file_path) {
                                        Ok(_) => {
                                            debug!("重命名分页相关文件成功: {:?} -> {:?}", file_path, new_file_path);
                                            updated_count += 1;

                                            // 如果这是主文件（MP4），更新数据库中的路径记录
                                            if file_name.ends_with(".mp4") {
                                                let new_path_str = new_file_path.to_string_lossy().to_string();
                                                let mut page_update: bili_sync_entity::page::ActiveModel =
                                                    page.clone().into();
                                                page_update.path = Set(Some(new_path_str));
                                                if let Err(e) = page_update.update(db.as_ref()).await {
                                                    warn!("更新数据库中的分页路径失败: {}", e);
                                                } else {
                                                    debug!("更新数据库分页路径成功: {:?}", new_file_path);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            warn!(
                                                "重命名分页相关文件失败: {:?} -> {:?}, 错误: {}",
                                                file_path, new_file_path, e
                                            );
                                        }
                                    }
                                } else {
                                    debug!("文件路径已经正确，无需重命名: {:?}", file_path);
                                }
                            }
                        }
                    }
                }
            } else {
                debug!(
                    "分页 {} 的文件名已经是正确格式或未找到文件，原始模式: '{}', 新模式: '{}'",
                    page.pid, old_page_name, new_page_name
                );
            }
        }
    }

    info!("文件重命名完成，共处理了 {} 个文件/文件夹", updated_count);
    Ok(updated_count)
}

/// 重新生成已下载视频的NFO文件以使用新的时间类型配置
async fn regenerate_nfo_files(db: Arc<DatabaseConnection>, config: &crate::config::Config) -> Result<u32> {
    use crate::utils::nfo::{ModelWrapper, NFOMode, NFOSerializer};
    use handlebars::Handlebars;
    use sea_orm::*;
    use std::path::Path;

    info!("开始重新生成NFO文件以使用新的时间类型配置...");

    let mut updated_count = 0u32;

    // 获取所有已下载的视频（下载状态大于0表示已下载）
    let videos = bili_sync_entity::video::Entity::find()
        .filter(bili_sync_entity::video::Column::DownloadStatus.gt(0))
        .all(db.as_ref())
        .await?;

    info!("找到 {} 个已下载的视频需要重新生成NFO文件", videos.len());

    for video in videos {
        let video_path = Path::new(&video.path);

        if !video_path.exists() {
            warn!("视频路径不存在，跳过NFO重新生成: {:?}", video_path);
            continue;
        }

        // 获取视频的实际文件名（不包括扩展名）
        let video_base_name = if let Ok(entries) = std::fs::read_dir(video_path) {
            let mut found_name = None;
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                if file_name_str.ends_with(".mp4") {
                    // 找到MP4文件，获取其基础名称（不包括.mp4扩展名）
                    found_name = Some(file_name_str.trim_end_matches(".mp4").to_string());
                    break;
                }
            }
            found_name.unwrap_or_else(|| video.name.clone())
        } else {
            video.name.clone()
        };

        // 重新生成视频NFO文件
        let video_nfo_path = if video.single_page.unwrap_or(false) {
            // 单P视频：使用与MP4文件相同的基础名称
            video_path.join(format!("{}.nfo", video_base_name))
        } else {
            // 多P视频：使用视频名称作为NFO文件名
            let video_name = if let Some(video_dir_name) = video_path.file_name() {
                video_dir_name.to_string_lossy().to_string()
            } else {
                crate::utils::filenamify::filenamify(&video.name)
            };
            video_path.join(format!("{}.nfo", video_name))
        };

        let nfo_mode = if video.single_page.unwrap_or(false) {
            NFOMode::MOVIE
        } else {
            NFOMode::TVSHOW
        };

        let nfo_serializer = NFOSerializer(ModelWrapper::Video(&video), nfo_mode);

        match nfo_serializer.generate_nfo(&config.nfo_time_type).await {
            Ok(nfo_content) => {
                if let Some(parent) = video_nfo_path.parent() {
                    if let Err(e) = tokio::fs::create_dir_all(parent).await {
                        warn!("创建NFO文件目录失败: {:?}, 错误: {}", parent, e);
                        continue;
                    }
                }

                match tokio::fs::write(&video_nfo_path, nfo_content.as_bytes()).await {
                    Ok(_) => {
                        debug!("重新生成视频NFO文件成功: {:?}", video_nfo_path);
                        updated_count += 1;
                    }
                    Err(e) => {
                        warn!("写入视频NFO文件失败: {:?}, 错误: {}", video_nfo_path, e);
                    }
                }
            }
            Err(e) => {
                warn!("生成视频NFO内容失败: {}, 错误: {}", video.name, e);
            }
        }

        // 重新生成UP主NFO文件
        let upper_id = video.upper_id.to_string();
        let base_upper_path = &config
            .upper_path
            .join(upper_id.chars().next().unwrap_or('0').to_string())
            .join(&upper_id);
        let upper_nfo_path = base_upper_path.join("person.nfo");
        let upper_nfo_serializer = NFOSerializer(ModelWrapper::Video(&video), NFOMode::UPPER);

        match upper_nfo_serializer.generate_nfo(&config.nfo_time_type).await {
            Ok(nfo_content) => {
                // 确保UP主目录存在
                if let Some(parent) = upper_nfo_path.parent() {
                    if let Err(e) = tokio::fs::create_dir_all(parent).await {
                        warn!("创建UP主NFO文件目录失败: {:?}, 错误: {}", parent, e);
                        continue;
                    }
                }

                match tokio::fs::write(&upper_nfo_path, nfo_content.as_bytes()).await {
                    Ok(_) => {
                        debug!("重新生成UP主NFO文件成功: {:?}", upper_nfo_path);
                        updated_count += 1;
                    }
                    Err(e) => {
                        warn!("写入UP主NFO文件失败: {:?}, 错误: {}", upper_nfo_path, e);
                    }
                }
            }
            Err(e) => {
                warn!("生成UP主NFO内容失败: {}, 错误: {}", video.name, e);
            }
        }

        // 重新生成分页NFO文件（对多P视频或番剧类型）
        let is_bangumi = video.source_type == Some(1);
        if !video.single_page.unwrap_or(false) || is_bangumi {
            if is_bangumi {
                // 对于番剧，需要获取番剧源的模板配置
                let bangumi_source = bili_sync_entity::video_source::Entity::find()
                    .filter(bili_sync_entity::video_source::Column::Id.eq(video.source_id.unwrap_or(0)))
                    .filter(bili_sync_entity::video_source::Column::Type.eq(1))
                    .one(db.as_ref())
                    .await?;

                let pages = bili_sync_entity::page::Entity::find()
                    .filter(bili_sync_entity::page::Column::VideoId.eq(video.id))
                    .filter(bili_sync_entity::page::Column::DownloadStatus.gt(0))
                    .all(db.as_ref())
                    .await?;

                for page in pages {
                    // 使用番剧的page_name_template来生成正确的文件名
                    let page_name = if let Some(bangumi_source) = &bangumi_source {
                        let template = bangumi_source
                            .page_name_template
                            .as_deref()
                            .unwrap_or("S{{season_pad}}E{{pid_pad}}-{{pid_pad}}");

                        // 构建番剧专用的模板数据
                        let episode_number = video.episode_number.unwrap_or(page.pid);
                        let season_number = video.season_number.unwrap_or(1);

                        let mut template_data = std::collections::HashMap::new();
                        template_data.insert("bvid".to_string(), serde_json::Value::String(video.bvid.clone()));
                        template_data.insert("title".to_string(), serde_json::Value::String(video.name.clone()));
                        template_data.insert(
                            "upper_name".to_string(),
                            serde_json::Value::String(video.upper_name.clone()),
                        );
                        template_data.insert(
                            "upper_mid".to_string(),
                            serde_json::Value::String(video.upper_id.to_string()),
                        );
                        template_data.insert("ptitle".to_string(), serde_json::Value::String(page.name.clone()));
                        template_data.insert("pid".to_string(), serde_json::Value::String(episode_number.to_string()));
                        template_data.insert(
                            "pid_pad".to_string(),
                            serde_json::Value::String(format!("{:02}", episode_number)),
                        );
                        template_data.insert(
                            "season".to_string(),
                            serde_json::Value::String(season_number.to_string()),
                        );
                        template_data.insert(
                            "season_pad".to_string(),
                            serde_json::Value::String(format!("{:02}", season_number)),
                        );

                        let handlebars = Handlebars::new();
                        let template_value = serde_json::Value::Object(template_data.into_iter().collect());

                        match handlebars.render_template(template, &template_value) {
                            Ok(rendered) => crate::utils::filenamify::filenamify(&rendered),
                            Err(e) => {
                                warn!("渲染番剧模板失败: {}, 使用默认格式", e);
                                format!("S{:02}E{:02}-{:02}", season_number, episode_number, episode_number)
                            }
                        }
                    } else {
                        // 如果找不到番剧源配置，使用默认格式
                        let season_number = video.season_number.unwrap_or(1);
                        let episode_number = video.episode_number.unwrap_or(page.pid);
                        format!("S{:02}E{:02}-{:02}", season_number, episode_number, episode_number)
                    };

                    let page_nfo_path = video_path.join(format!("{}.nfo", page_name));
                    let page_nfo_serializer = NFOSerializer(ModelWrapper::Page(&page), NFOMode::EPOSODE);

                    match page_nfo_serializer.generate_nfo(&config.nfo_time_type).await {
                        Ok(nfo_content) => match tokio::fs::write(&page_nfo_path, nfo_content.as_bytes()).await {
                            Ok(_) => {
                                debug!("重新生成番剧分页NFO文件成功: {:?}", page_nfo_path);
                                updated_count += 1;
                            }
                            Err(e) => {
                                warn!("写入番剧分页NFO文件失败: {:?}, 错误: {}", page_nfo_path, e);
                            }
                        },
                        Err(e) => {
                            warn!("生成分页NFO内容失败: {}", e);
                        }
                    }
                }
            } else {
                // 对于普通多P视频，需要使用与实际文件相同的命名模式
                let pages = bili_sync_entity::page::Entity::find()
                    .filter(bili_sync_entity::page::Column::VideoId.eq(video.id))
                    .filter(bili_sync_entity::page::Column::DownloadStatus.gt(0))
                    .all(db.as_ref())
                    .await?;

                for page in pages {
                    // 使用multi_page_name模板来生成正确的文件名
                    let mut template_data = std::collections::HashMap::new();
                    template_data.insert("title".to_string(), serde_json::Value::String(video.name.clone()));
                    template_data.insert("show_title".to_string(), serde_json::Value::String(video.name.clone()));
                    template_data.insert("bvid".to_string(), serde_json::Value::String(video.bvid.clone()));
                    template_data.insert(
                        "upper_name".to_string(),
                        serde_json::Value::String(video.upper_name.clone()),
                    );
                    template_data.insert(
                        "upper_mid".to_string(),
                        serde_json::Value::String(video.upper_id.to_string()),
                    );
                    template_data.insert("ptitle".to_string(), serde_json::Value::String(page.name.clone()));
                    template_data.insert("pid".to_string(), serde_json::Value::String(page.pid.to_string()));
                    template_data.insert(
                        "pid_pad".to_string(),
                        serde_json::Value::String(format!("{:02}", page.pid)),
                    );
                    template_data.insert("season".to_string(), serde_json::Value::String("1".to_string()));
                    template_data.insert("season_pad".to_string(), serde_json::Value::String("01".to_string()));
                    template_data.insert(
                        "duration".to_string(),
                        serde_json::Value::String(page.duration.to_string()),
                    );

                    if let Some(width) = page.width {
                        template_data.insert("width".to_string(), serde_json::Value::String(width.to_string()));
                    }

                    if let Some(height) = page.height {
                        template_data.insert("height".to_string(), serde_json::Value::String(height.to_string()));
                    }

                    // 格式化时间
                    let formatted_pubtime = video.pubtime.format(&config.time_format).to_string();
                    template_data.insert("pubtime".to_string(), serde_json::Value::String(formatted_pubtime));

                    let formatted_favtime = video.favtime.format(&config.time_format).to_string();
                    template_data.insert("fav_time".to_string(), serde_json::Value::String(formatted_favtime));

                    let formatted_ctime = video.ctime.format(&config.time_format).to_string();
                    template_data.insert("ctime".to_string(), serde_json::Value::String(formatted_ctime));

                    // 使用multi_page_name模板渲染文件名
                    let handlebars = Handlebars::new();
                    let template_value = serde_json::Value::Object(template_data.into_iter().collect());

                    let page_name = match handlebars.render_template(&config.multi_page_name, &template_value) {
                        Ok(rendered) => crate::utils::filenamify::filenamify(&rendered),
                        Err(e) => {
                            warn!("渲染多P视频模板失败: {}, 使用默认格式", e);
                            format!("S01E{:02}-{:02}", page.pid, page.pid)
                        }
                    };

                    let page_nfo_path = video_path.join(format!("{}.nfo", page_name));
                    let page_nfo_serializer = NFOSerializer(ModelWrapper::Page(&page), NFOMode::EPOSODE);

                    match page_nfo_serializer.generate_nfo(&config.nfo_time_type).await {
                        Ok(nfo_content) => match tokio::fs::write(&page_nfo_path, nfo_content.as_bytes()).await {
                            Ok(_) => {
                                debug!("重新生成分页NFO文件成功: {:?}", page_nfo_path);
                                updated_count += 1;
                            }
                            Err(e) => {
                                warn!("写入分页NFO文件失败: {:?}, 错误: {}", page_nfo_path, e);
                            }
                        },
                        Err(e) => {
                            warn!("生成分页NFO内容失败: {}", e);
                        }
                    }
                }
            }
        }
    }

    info!("NFO文件重新生成完成，共重新生成了 {} 个NFO文件", updated_count);
    Ok(updated_count)
}
