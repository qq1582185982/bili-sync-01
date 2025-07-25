pub mod bangumi;
mod collection;
mod favorite;
mod submission;
mod watch_later;

// 移除不再使用的init函数导出，因为现在视频源通过Web API管理
// pub use collection::init_collection_sources;
// pub use favorite::init_favorite_sources;
// pub use submission::init_submission_sources;
// pub use watch_later::init_watch_later_source;

pub use bangumi::BangumiSource;

use std::path::Path;
use std::pin::Pin;

use anyhow::Result;
use chrono::Utc;
use enum_dispatch::enum_dispatch;
use futures::Stream;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::DatabaseConnection;

#[rustfmt::skip]
use bili_sync_entity::collection::Model as Collection;
use bili_sync_entity::favorite::Model as Favorite;
use bili_sync_entity::submission::Model as Submission;
use bili_sync_entity::watch_later::Model as WatchLater;

use crate::adapter::collection::collection_from;
use crate::adapter::favorite::favorite_from;
use crate::adapter::submission::submission_from;
use crate::adapter::watch_later::watch_later_from;
use crate::bilibili::{BiliClient, CollectionItem, VideoInfo};

#[enum_dispatch]
pub enum VideoSourceEnum {
    Favorite,
    Collection,
    Submission,
    WatchLater,
    BangumiSource,
}

#[enum_dispatch(VideoSourceEnum)]
pub trait VideoSource {
    /// 获取特定视频列表的筛选条件
    fn filter_expr(&self) -> SimpleExpr;

    // 为 video_model 设置该视频列表的关联 id
    fn set_relation_id(&self, video_model: &mut bili_sync_entity::video::ActiveModel);

    /// 获取视频 model 中记录的最新时间
    fn get_latest_row_at(&self) -> String;

    /// 更新视频 model 中记录的最新时间，此处返回需要更新的 ActiveModel，接着调用 save 方法执行保存
    /// 不同 VideoSource 返回的类型不同，为了 VideoSource 的 object safety 不能使用 impl Trait
    /// Box<dyn ActiveModelTrait> 又提示 ActiveModelTrait 没有 object safety，因此手写一个 Enum 静态分发
    fn update_latest_row_at(&self, datetime: String) -> _ActiveModel;

    // 获取视频列表的保存路径
    fn path(&self) -> &Path;

    // 判断是否应该继续拉取视频
    fn should_take(&self, release_datetime: &chrono::DateTime<Utc>, latest_row_at: &chrono::DateTime<Utc>) -> bool {
        release_datetime > latest_row_at
    }

    /// 开始刷新视频
    fn log_refresh_video_start(&self);

    /// 结束刷新视频
    fn log_refresh_video_end(&self, count: usize);

    /// 开始填充视频
    fn log_fetch_video_start(&self);

    /// 结束填充视频
    fn log_fetch_video_end(&self);

    /// 开始下载视频
    fn log_download_video_start(&self);

    /// 结束下载视频
    fn log_download_video_end(&self);

    /// 获取是否扫描已删除视频的设置
    fn scan_deleted_videos(&self) -> bool;

    /// 获取选择的视频列表，仅对 submission 类型有效
    /// 返回 Some(Vec<String>) 表示有选择性下载列表，None 表示下载所有视频
    fn get_selected_videos(&self) -> Option<Vec<String>> {
        None // 默认实现：没有选择性下载
    }

    /// 获取创建时间，用于判断是否为新投稿
    fn get_created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        None // 默认实现：没有创建时间信息
    }

    /// 获取视频源类型的显示名称
    fn source_type_display(&self) -> String;

    /// 获取视频源的显示名称
    fn source_name_display(&self) -> String;
}

#[derive(Clone, Debug)]
pub enum Args {
    Favorite {
        fid: String,
    },
    Collection {
        collection_item: CollectionItem,
    },
    WatchLater,
    Submission {
        upper_id: String,
    },
    Bangumi {
        season_id: Option<String>,
        media_id: Option<String>,
        ep_id: Option<String>,
    },
}

pub async fn video_source_from<'a>(
    args: &'a Args,
    path: &'a Path,
    bili_client: &'a BiliClient,
    connection: &DatabaseConnection,
) -> Result<(
    VideoSourceEnum,
    Pin<Box<dyn Stream<Item = Result<VideoInfo>> + 'a + Send>>,
)> {
    match args {
        Args::Favorite { fid } => favorite_from(fid, path, bili_client, connection).await,
        Args::Collection { collection_item } => collection_from(collection_item, path, bili_client, connection).await,
        Args::WatchLater => watch_later_from(path, bili_client, connection).await,
        Args::Submission { upper_id } => submission_from(upper_id, path, bili_client, connection).await,
        Args::Bangumi {
            season_id,
            media_id,
            ep_id,
        } => bangumi_from(season_id, media_id, ep_id, path, bili_client, connection).await,
    }
}

pub enum _ActiveModel {
    Favorite(bili_sync_entity::favorite::ActiveModel),
    Collection(bili_sync_entity::collection::ActiveModel),
    Submission(bili_sync_entity::submission::ActiveModel),
    WatchLater(bili_sync_entity::watch_later::ActiveModel),
    Bangumi(bili_sync_entity::video_source::ActiveModel),
}

impl _ActiveModel {
    pub async fn save(self, connection: &DatabaseConnection) -> Result<()> {
        match self {
            _ActiveModel::Favorite(model) => {
                model.save(connection).await?;
            }
            _ActiveModel::Collection(model) => {
                model.save(connection).await?;
            }
            _ActiveModel::Submission(model) => {
                model.save(connection).await?;
            }
            _ActiveModel::WatchLater(model) => {
                model.save(connection).await?;
            }
            _ActiveModel::Bangumi(model) => {
                model.save(connection).await?;
            }
        }
        Ok(())
    }
}

pub async fn bangumi_from<'a>(
    season_id: &Option<String>,
    media_id: &Option<String>,
    ep_id: &Option<String>,
    path: &'a Path,
    bili_client: &'a BiliClient,
    connection: &DatabaseConnection,
) -> Result<(
    VideoSourceEnum,
    Pin<Box<dyn Stream<Item = Result<VideoInfo>> + 'a + Send>>,
)> {
    // 使用可用的ID构建查询条件
    let mut query =
        bili_sync_entity::video_source::Entity::find().filter(bili_sync_entity::video_source::Column::Type.eq(1));

    // 根据提供的标识符构建查询
    if let Some(season_id_value) = season_id {
        query = query.filter(bili_sync_entity::video_source::Column::SeasonId.eq(season_id_value));
    }

    if let Some(media_id_value) = media_id {
        query = query.filter(bili_sync_entity::video_source::Column::MediaId.eq(media_id_value));
    }

    if let Some(ep_id_value) = ep_id {
        query = query.filter(bili_sync_entity::video_source::Column::EpId.eq(ep_id_value));
    }

    // 从数据库中获取现有的番剧源
    let bangumi_model = query.one(connection).await?;

    // 如果数据库中存在，则使用数据库中的ID；否则使用默认ID
    let bangumi_source = if let Some(model) = bangumi_model {
        // 解析 selected_seasons JSON 字符串
        let selected_seasons = if let Some(json_str) = &model.selected_seasons {
            serde_json::from_str::<Vec<String>>(json_str).ok()
        } else {
            None
        };

        BangumiSource {
            id: model.id,
            name: model.name,
            latest_row_at: model.latest_row_at,
            season_id: model.season_id,
            media_id: model.media_id,
            ep_id: model.ep_id,
            path: path.to_path_buf(),
            download_all_seasons: model.download_all_seasons.unwrap_or(false),
            page_name_template: model.page_name_template,
            selected_seasons,
            scan_deleted_videos: model.scan_deleted_videos,
        }
    } else {
        // 如果数据库中不存在，使用默认值并发出警告
        let id_desc = match (season_id, media_id, ep_id) {
            (Some(s), _, _) => format!("season_id: {}", s),
            (_, Some(m), _) => format!("media_id: {}", m),
            (_, _, Some(e)) => format!("ep_id: {}", e),
            _ => "未提供ID".to_string(),
        };

        warn!("数据库中未找到番剧 {} 的记录，使用临时ID", id_desc);
        BangumiSource {
            id: 0, // 临时的 ID
            name: format!("番剧 {}", id_desc),
            latest_row_at: "1970-01-01 00:00:00".to_string(),
            season_id: season_id.clone(),
            media_id: media_id.clone(),
            ep_id: ep_id.clone(),
            path: path.to_path_buf(),
            download_all_seasons: false,
            page_name_template: None,
            selected_seasons: None,
            scan_deleted_videos: false,
        }
    };

    // 获取番剧的视频流
    let video_stream = bangumi_source.video_stream_from(bili_client, path, connection).await?;

    // 将 'static 生命周期的流转换为 'a 生命周期
    let video_stream = unsafe {
        std::mem::transmute::<
            Pin<Box<dyn Stream<Item = Result<VideoInfo>> + Send>>,
            Pin<Box<dyn Stream<Item = Result<VideoInfo>> + 'a + Send>>,
        >(video_stream)
    };

    Ok((VideoSourceEnum::BangumiSource(bangumi_source), video_stream))
}
