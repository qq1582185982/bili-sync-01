use std::path::Path;
use std::pin::Pin;

use crate::utils::time_format::{now_standard_string, parse_time_string};
use anyhow::{Context, Result};
use bili_sync_entity::*;
use chrono::Utc;
use futures::Stream;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{OnConflict, SimpleExpr};
use sea_orm::ActiveValue::Set;
use sea_orm::{DatabaseConnection, Unchanged};
use tracing::{debug, info, warn};

use crate::adapter::{VideoSource, VideoSourceEnum, _ActiveModel};
use crate::bilibili::{BiliClient, Submission, VideoInfo};

impl VideoSource for submission::Model {
    fn filter_expr(&self) -> SimpleExpr {
        video::Column::SubmissionId.eq(self.id)
    }

    fn set_relation_id(&self, video_model: &mut video::ActiveModel) {
        video_model.submission_id = Set(Some(self.id));
        // 同时设置source_submission_id，用于合作视频的归类
        video_model.source_submission_id = Set(Some(self.id));
    }

    fn path(&self) -> &Path {
        Path::new(self.path.as_str())
    }

    fn get_latest_row_at(&self) -> String {
        self.latest_row_at.clone()
    }

    fn update_latest_row_at(&self, datetime: String) -> _ActiveModel {
        _ActiveModel::Submission(submission::ActiveModel {
            id: Unchanged(self.id),
            latest_row_at: Set(datetime),
            ..Default::default()
        })
    }

    fn should_take(&self, release_datetime: &chrono::DateTime<Utc>, latest_row_at_string: &str) -> bool {
        // 检查是否存在断点恢复情况
        let upper_id_str = self.upper_id.to_string();
        let has_checkpoint = {
            let tracker = crate::bilibili::submission::SUBMISSION_PAGE_TRACKER.read().unwrap();
            tracker.contains_key(&upper_id_str)
        };

        if has_checkpoint {
            return true;
        }

        // 如果有选择的视频列表，检查是否是首次扫描
        if self.selected_videos.is_some() {
            // 检查 latest_row_at 是否为初始值（首次扫描）
            let is_first_scan = latest_row_at_string == "1970-01-01 00:00:00"
                || latest_row_at_string.is_empty();

            if is_first_scan {
                // 首次扫描：需要获取所有视频以找到选定的视频
                debug!(
                    "UP主「{}」选择性下载首次扫描：获取所有视频信息以便匹配选择列表",
                    self.upper_name
                );
                return true;
            }
            // 非首次扫描：使用增量逻辑，只获取新视频
            debug!(
                "UP主「{}」选择性下载增量扫描：只获取新发布的视频",
                self.upper_name
            );
        }

        // 增量扫描逻辑：只获取比上次扫描时间更新的视频
        let current_config = crate::config::reload_config();
        if current_config.submission_risk_control.enable_incremental_fetch || self.selected_videos.is_some() {
            // 将UTC时间转换为北京时间字符串，然后直接比较字符串
            let beijing_tz = crate::utils::time_format::beijing_timezone();
            let release_beijing = release_datetime.with_timezone(&beijing_tz);
            let release_beijing_str = release_beijing.format("%Y-%m-%d %H:%M:%S").to_string();

            let should_take = release_beijing_str.as_str() > latest_row_at_string;

            if should_take {
                debug!(
                    "UP主「{}」增量获取：视频发布时间 {} > 上次扫描最新视频发布时间 {}",
                    self.upper_name, release_beijing_str, latest_row_at_string
                );
            } else {
                debug!(
                    "UP主「{}」增量跳过：视频发布时间 {} <= 上次扫描最新视频发布时间 {}",
                    self.upper_name, release_beijing_str, latest_row_at_string
                );
            }

            should_take
        } else {
            // 全量模式：获取所有视频，但会在 create_videos 中过滤已存在的视频
            debug!(
                "UP主「{}」全量获取：增量获取已禁用，获取所有视频（将在数据库层面去重）",
                self.upper_name
            );
            true
        }
    }

    fn log_refresh_video_start(&self) {
        // 检查是否有断点恢复
        let upper_id_str = self.upper_id.to_string();
        let has_checkpoint = {
            let tracker = crate::bilibili::submission::SUBMISSION_PAGE_TRACKER.read().unwrap();
            tracker.contains_key(&upper_id_str)
        };

        if has_checkpoint {
            info!("开始断点恢复「{}」投稿扫描..", self.upper_name);
        } else if self.selected_videos.is_some() {
            // 选择性下载模式
            let is_first_scan = self.latest_row_at == "1970-01-01 00:00:00"
                || self.latest_row_at.is_empty();
            if is_first_scan {
                info!("开始全量扫描「{}」投稿（首次选择性下载，需匹配选择列表）..", self.upper_name);
            } else {
                info!("开始增量扫描「{}」投稿（选择性下载，仅获取新视频）..", self.upper_name);
            }
        } else {
            let current_config = crate::config::reload_config();
            if current_config.submission_risk_control.enable_incremental_fetch {
                info!("开始增量扫描「{}」投稿（仅获取新视频）..", self.upper_name);
            } else {
                info!(
                    "开始全量扫描「{}」投稿（获取所有视频，已存在视频将自动跳过）..",
                    self.upper_name
                );
            }
        }
    }

    fn log_refresh_video_end(&self, count: usize) {
        if count > 0 {
            info!("扫描「{}」投稿完成，获取到 {} 条新视频", self.upper_name, count);
        } else {
            info!("「{}」投稿无新视频", self.upper_name);
        }
    }

    fn log_fetch_video_start(&self) {
        debug!("开始填充「{}」投稿视频详情..", self.upper_name);
    }

    fn log_fetch_video_end(&self) {
        debug!("填充「{}」投稿视频详情完成", self.upper_name);
    }

    fn log_download_video_start(&self) {
        debug!("开始下载「{}」投稿视频..", self.upper_name);
    }

    fn log_download_video_end(&self) {
        debug!("下载「{}」投稿视频完成", self.upper_name);
    }

    fn scan_deleted_videos(&self) -> bool {
        self.scan_deleted_videos
    }

    fn get_selected_videos(&self) -> Option<Vec<String>> {
        self.selected_videos.as_ref().and_then(|json_str| {
            serde_json::from_str::<Vec<String>>(json_str)
                .map_err(|e| {
                    warn!("解析 selected_videos JSON 失败: {}", e);
                    e
                })
                .ok()
        })
    }

    fn get_created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        // 使用统一的时间解析函数
        parse_time_string(&self.created_at).map(|dt| dt.and_utc()).or_else(|| {
            warn!("解析 created_at 时间失败，原始值: {}", self.created_at);
            None
        })
    }

    fn source_type_display(&self) -> String {
        "UP主投稿".to_string()
    }

    fn source_name_display(&self) -> String {
        self.upper_name.clone()
    }

    fn get_keyword_filters(&self) -> Option<String> {
        self.keyword_filters.clone()
    }

    fn get_keyword_filter_mode(&self) -> Option<String> {
        self.keyword_filter_mode.clone()
    }

    fn get_blacklist_keywords(&self) -> Option<String> {
        self.blacklist_keywords.clone()
    }

    fn get_whitelist_keywords(&self) -> Option<String> {
        self.whitelist_keywords.clone()
    }

    fn get_keyword_case_sensitive(&self) -> bool {
        self.keyword_case_sensitive
    }

    fn audio_only(&self) -> bool {
        self.audio_only
    }

    fn audio_only_m4a_only(&self) -> bool {
        self.audio_only_m4a_only
    }

    fn flat_folder(&self) -> bool {
        self.flat_folder
    }

    fn download_danmaku(&self) -> bool {
        self.download_danmaku
    }

    fn download_subtitle(&self) -> bool {
        self.download_subtitle
    }

    fn ai_rename(&self) -> bool {
        self.ai_rename
    }

    fn ai_rename_video_prompt(&self) -> &str {
        &self.ai_rename_video_prompt
    }

    fn ai_rename_audio_prompt(&self) -> &str {
        &self.ai_rename_audio_prompt
    }

    fn ai_rename_enable_multi_page(&self) -> bool {
        self.ai_rename_enable_multi_page
    }

    fn ai_rename_enable_collection(&self) -> bool {
        self.ai_rename_enable_collection
    }

    fn ai_rename_enable_bangumi(&self) -> bool {
        self.ai_rename_enable_bangumi
    }

    fn source_key(&self) -> String {
        format!("submission_{}", self.id)
    }
}

#[allow(dead_code)]
pub async fn init_submission_sources(
    conn: &DatabaseConnection,
    submission_list: &std::collections::HashMap<String, std::path::PathBuf>,
) -> Result<()> {
    // 遍历配置中的UP主投稿列表
    for (upper_id, path) in submission_list {
        // 尝试将upper_id转换为i64
        let upper_id_i64 = match upper_id.parse::<i64>() {
            Ok(id) => id,
            Err(e) => {
                warn!("无效的UP主ID {}: {}, 跳过", upper_id, e);
                continue;
            }
        };

        // 检查数据库中是否已存在该UP主ID的记录
        let existing = submission::Entity::find()
            .filter(submission::Column::UpperId.eq(upper_id_i64))
            .one(conn)
            .await?;

        if existing.is_none() {
            // 如果不存在，尝试获取UP主信息并创建新记录
            let bili_client = crate::bilibili::BiliClient::new(String::new());
            let submission = Submission::new(&bili_client, upper_id.to_owned());

            // 尝试获取UP主信息
            match submission.get_info().await {
                Ok(upper) => {
                    // 在使用upper.name之前先克隆它
                    let upper_name = upper.name.clone();

                    let model = submission::ActiveModel {
                        id: Set(Default::default()),
                        upper_id: Set(upper.mid.parse()?),
                        upper_name: Set(upper.name),
                        path: Set(path.to_string_lossy().to_string()),
                        created_at: Set(now_standard_string()),
                        latest_row_at: Set("1970-01-01 00:00:00".to_string()),
                        enabled: Set(true),
                        scan_deleted_videos: Set(false),
                        selected_videos: Set(None),
                        keyword_filters: Set(None),
                        keyword_filter_mode: Set(None),
                        blacklist_keywords: Set(None),
                        whitelist_keywords: Set(None),
                        keyword_case_sensitive: Set(true),
                        audio_only: Set(false),
                        audio_only_m4a_only: Set(false),
                        flat_folder: Set(false),
                        download_danmaku: Set(true),
                        download_subtitle: Set(true),
                        ai_rename: Set(false),
                        ai_rename_video_prompt: Set(String::new()),
                        ai_rename_audio_prompt: Set(String::new()),
                        ai_rename_enable_multi_page: Set(false),
                        ai_rename_enable_collection: Set(false),
                        ai_rename_enable_bangumi: Set(false),
                    };

                    // 插入数据库
                    let result = submission::Entity::insert(model)
                        .exec(conn)
                        .await
                        .context("Failed to insert submission source")?;

                    info!("初始化UP主投稿源: {} (ID: {})", upper_name, result.last_insert_id);
                }
                Err(e) => {
                    // 如果获取失败，创建一个临时记录
                    warn!("获取UP主 {} 信息失败: {}, 创建临时记录", upper_id, e);
                    let model = submission::ActiveModel {
                        id: Set(Default::default()),
                        upper_id: Set(upper_id_i64),
                        upper_name: Set(format!("UP主 {}", upper_id)),
                        path: Set(path.to_string_lossy().to_string()),
                        created_at: Set(now_standard_string()),
                        latest_row_at: Set("1970-01-01 00:00:00".to_string()),
                        enabled: Set(true),
                        scan_deleted_videos: Set(false),
                        selected_videos: Set(None),
                        keyword_filters: Set(None),
                        keyword_filter_mode: Set(None),
                        blacklist_keywords: Set(None),
                        whitelist_keywords: Set(None),
                        keyword_case_sensitive: Set(true),
                        audio_only: Set(false),
                        audio_only_m4a_only: Set(false),
                        flat_folder: Set(false),
                        download_danmaku: Set(true),
                        download_subtitle: Set(true),
                        ai_rename: Set(false),
                        ai_rename_video_prompt: Set(String::new()),
                        ai_rename_audio_prompt: Set(String::new()),
                        ai_rename_enable_multi_page: Set(false),
                        ai_rename_enable_collection: Set(false),
                        ai_rename_enable_bangumi: Set(false),
                    };

                    let result = submission::Entity::insert(model)
                        .exec(conn)
                        .await
                        .context("Failed to insert temporary submission source")?;

                    info!("初始化临时UP主投稿源: {} (ID: {})", upper_id, result.last_insert_id);
                }
            }
        } else if let Some(existing) = existing {
            // 如果已存在，更新路径
            if existing.path != path.to_string_lossy() {
                let model = submission::ActiveModel {
                    id: Set(existing.id),
                    path: Set(path.to_string_lossy().to_string()),
                    ..Default::default()
                };

                // 更新数据库
                submission::Entity::update(model)
                    .exec(conn)
                    .await
                    .context("Failed to update submission source")?;

                info!("更新UP主投稿源: {} (ID: {})", existing.upper_name, existing.id);
            }
        }
    }

    Ok(())
}

pub(super) async fn submission_from<'a>(
    upper_id: &str,
    path: &Path,
    bili_client: &'a BiliClient,
    connection: &DatabaseConnection,
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
) -> Result<(
    VideoSourceEnum,
    Pin<Box<dyn Stream<Item = Result<VideoInfo>> + 'a + Send>>,
)> {
    let submission = Submission::new(bili_client, upper_id.to_owned());
    let upper = submission.get_info().await?;
    // 重新创建带有UP主名称的Submission实例，用于后续的视频流处理和日志显示
    let submission_with_name = Submission::with_name(bili_client, upper_id.to_owned(), upper.name.clone());
    submission::Entity::insert(submission::ActiveModel {
        upper_id: Set(upper.mid.parse()?),
        upper_name: Set(upper.name),
        path: Set(path.to_string_lossy().to_string()),
        created_at: Set(crate::utils::time_format::now_standard_string()),
        latest_row_at: Set("1970-01-01 00:00:00".to_string()),
        enabled: Set(true),
        scan_deleted_videos: Set(false),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(submission::Column::UpperId)
            .update_columns([submission::Column::UpperName, submission::Column::Path])
            .to_owned(),
    )
    .exec(connection)
    .await?;
    Ok((
        submission::Entity::find()
            .filter(submission::Column::UpperId.eq(upper.mid))
            .one(connection)
            .await?
            .context("submission not found")?
            .into(),
        Box::pin(submission_with_name.into_video_stream(cancellation_token.unwrap_or_default())),
    ))
}
