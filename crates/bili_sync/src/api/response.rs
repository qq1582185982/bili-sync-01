use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::utils::status::{PageStatus, VideoStatus};

#[derive(Debug, Serialize, ToSchema, Default)]
pub struct VideoSourcesResponse {
    #[serde(default)]
    pub collection: Vec<VideoSource>,
    #[serde(default)]
    pub favorite: Vec<VideoSource>,
    #[serde(default)]
    pub submission: Vec<VideoSource>,
    #[serde(default)]
    pub watch_later: Vec<VideoSource>,
    #[serde(default)]
    pub bangumi: Vec<VideoSource>,
}

#[derive(Serialize, ToSchema)]
pub struct VideosResponse {
    pub videos: Vec<VideoInfo>,
    pub total_count: u64,
}

#[derive(Serialize, ToSchema)]
pub struct VideoResponse {
    pub video: VideoInfo,
    pub pages: Vec<PageInfo>,
}

#[derive(Serialize, ToSchema)]
pub struct VideoBvidResponse {
    pub bvid: String,
    pub title: String,
    pub bilibili_url: String,
}

#[derive(Serialize, ToSchema)]
pub struct ResetVideoResponse {
    pub resetted: bool,
    pub video: VideoInfo,
    pub pages: Vec<PageInfo>,
}

#[derive(Serialize, ToSchema)]
pub struct UpdateVideoStatusResponse {
    pub success: bool,
    pub video: VideoInfo,
    pub pages: Vec<PageInfo>,
}

#[derive(Serialize, ToSchema)]
pub struct ResetAllVideosResponse {
    pub resetted: bool,
    pub resetted_videos_count: usize,
    pub resetted_pages_count: usize,
}

#[derive(Serialize, ToSchema)]
pub struct AddVideoSourceResponse {
    pub success: bool,
    pub source_id: i32,
    pub source_type: String,
    pub message: String,
}

#[derive(Serialize, ToSchema)]
pub struct SubmissionVideosResponse {
    pub videos: Vec<SubmissionVideoInfo>,
    pub total: i64,
    pub page: i32,
    pub page_size: i32,
}

#[derive(Serialize, ToSchema)]
pub struct SubmissionVideoInfo {
    pub bvid: String,
    pub title: String,
    pub cover: String,
    pub pubtime: String,
    pub duration: i32,
    pub view: i32,
    pub danmaku: i32,
    pub description: String,
}

#[derive(Serialize, ToSchema)]
pub struct DeleteVideoSourceResponse {
    pub success: bool,
    pub source_id: i32,
    pub source_type: String,
    pub message: String,
}

#[derive(Serialize, ToSchema)]
pub struct DeleteVideoResponse {
    pub success: bool,
    pub video_id: i32,
    pub message: String,
}

#[derive(Serialize, ToSchema)]
pub struct UpdateVideoSourceEnabledResponse {
    pub success: bool,
    pub source_id: i32,
    pub source_type: String,
    pub enabled: bool,
    pub message: String,
}

#[derive(Serialize, ToSchema)]
pub struct UpdateVideoSourceScanDeletedResponse {
    pub success: bool,
    pub source_id: i32,
    pub source_type: String,
    pub scan_deleted_videos: bool,
    pub message: String,
}

#[derive(Serialize, ToSchema)]
pub struct ResetVideoSourcePathResponse {
    pub success: bool,
    pub source_id: i32,
    pub source_type: String,
    pub old_path: String,
    pub new_path: String,
    pub moved_files_count: usize,
    pub updated_videos_count: usize,
    pub cleaned_folders_count: usize,
    pub message: String,
}

#[derive(FromQueryResult, Serialize, ToSchema, Debug)]
pub struct VideoSource {
    pub id: i32,
    pub name: String,
    pub enabled: bool,
    pub path: String,
    pub scan_deleted_videos: bool,
    // 类型特有的ID字段
    pub f_id: Option<i64>,         // 收藏夹ID
    pub s_id: Option<i64>,         // 合集ID
    pub m_id: Option<i64>,         // UP主ID (用于合集)
    pub upper_id: Option<i64>,     // UP主ID (用于投稿)
    pub season_id: Option<String>, // 番剧season_id
    pub media_id: Option<String>,  // 番剧media_id
}

#[derive(Serialize, ToSchema)]
pub struct PageInfo {
    pub id: i32,
    pub pid: i32,
    pub name: String,
    pub download_status: [u32; 5],
    pub path: Option<String>,
}

impl From<(i32, i32, String, u32)> for PageInfo {
    fn from((id, pid, name, download_status): (i32, i32, String, u32)) -> Self {
        Self {
            id,
            pid,
            name,
            download_status: PageStatus::from(download_status).into(),
            path: None,
        }
    }
}

impl From<(i32, i32, String, u32, Option<String>)> for PageInfo {
    fn from((id, pid, name, download_status, path): (i32, i32, String, u32, Option<String>)) -> Self {
        Self {
            id,
            pid,
            name,
            download_status: PageStatus::from(download_status).into(),
            path,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct VideoInfo {
    pub id: i32,
    pub name: String,
    pub upper_name: String,
    pub path: String,
    pub category: i32,
    pub download_status: [u32; 5],
    pub cover: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bangumi_title: Option<String>, // 番剧真实标题，用于番剧类型视频的显示
}

impl From<(i32, String, String, String, i32, u32, String)> for VideoInfo {
    fn from(
        (id, name, upper_name, path, category, download_status, cover): (i32, String, String, String, i32, u32, String),
    ) -> Self {
        Self {
            id,
            name,
            upper_name,
            path,
            category,
            download_status: VideoStatus::from(download_status).into(),
            cover,
            bangumi_title: None, // 默认为None，将在API层根据视频类型填充
        }
    }
}

// 获取配置的响应结构体
#[derive(Serialize, ToSchema)]
pub struct ConfigResponse {
    pub video_name: String,
    pub page_name: String,
    pub multi_page_name: String,
    pub bangumi_name: String,
    pub folder_structure: String,
    pub bangumi_folder_name: String,
    pub collection_folder_mode: String,
    pub time_format: String,
    pub interval: u64,
    pub nfo_time_type: String,
    // 多线程下载配置
    pub parallel_download_enabled: bool,
    pub parallel_download_threads: usize,
    // 视频质量设置
    pub video_max_quality: String,
    pub video_min_quality: String,
    pub audio_max_quality: String,
    pub audio_min_quality: String,
    pub codecs: Vec<String>,
    pub no_dolby_video: bool,
    pub no_dolby_audio: bool,
    pub no_hdr: bool,
    pub no_hires: bool,
    // 弹幕设置
    pub danmaku_duration: f64,
    pub danmaku_font: String,
    pub danmaku_font_size: u32,
    pub danmaku_width_ratio: f64,
    pub danmaku_horizontal_gap: f64,
    pub danmaku_lane_size: u32,
    pub danmaku_float_percentage: f64,
    pub danmaku_bottom_percentage: f64,
    pub danmaku_opacity: u8,
    pub danmaku_bold: bool,
    pub danmaku_outline: f64,
    pub danmaku_time_offset: f64,
    // 并发控制设置
    pub concurrent_video: usize,
    pub concurrent_page: usize,
    pub rate_limit: Option<usize>,
    pub rate_duration: Option<u64>,
    // 其他设置
    pub cdn_sorting: bool,
    // UP主投稿风控配置
    pub large_submission_threshold: usize,
    pub base_request_delay: u64,
    pub large_submission_delay_multiplier: u64,
    pub enable_progressive_delay: bool,
    pub max_delay_multiplier: u64,
    pub enable_incremental_fetch: bool,
    pub incremental_fallback_to_full: bool,
    pub enable_batch_processing: bool,
    pub batch_size: usize,
    pub batch_delay_seconds: u64,
    pub enable_auto_backoff: bool,
    pub auto_backoff_base_seconds: u64,
    pub auto_backoff_max_multiplier: u64,
    pub source_delay_seconds: u64,
    pub submission_source_delay_seconds: u64,
    // 系统设置
    pub scan_deleted_videos: bool,
    // aria2监控配置
    pub enable_aria2_health_check: bool,
    pub enable_aria2_auto_restart: bool,
    pub aria2_health_check_interval: u64,
    // 多P视频目录结构配置
    pub multi_page_use_season_structure: bool,
    // 合集目录结构配置
    pub collection_use_season_structure: bool,
    // 番剧目录结构配置
    pub bangumi_use_season_structure: bool,
    // UP主头像保存路径
    pub upper_path: String,
    // B站凭证信息
    pub credential: Option<CredentialInfo>,
    // 推送通知配置
    pub notification: NotificationConfigResponse,
}

// B站凭证信息结构体
#[derive(Serialize, ToSchema)]
pub struct CredentialInfo {
    pub sessdata: String,
    pub bili_jct: String,
    pub buvid3: String,
    pub dedeuserid: String,
    pub ac_time_value: String,
}

// 更新配置的响应结构体
#[derive(Serialize, ToSchema)]
pub struct UpdateConfigResponse {
    pub success: bool,
    pub message: String,
    pub updated_files: Option<u32>,               // 重命名的文件数量
    pub resetted_nfo_videos_count: Option<usize>, // 重置的视频NFO任务数量
    pub resetted_nfo_pages_count: Option<usize>,  // 重置的页面NFO任务数量
}

// 配置管理相关响应结构体

// 配置项响应
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigItemResponse {
    pub key: String,
    pub value: serde_json::Value,
    pub updated_at: String,
}

// 配置重载响应
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigReloadResponse {
    pub success: bool,
    pub message: String,
    pub reloaded_at: String,
}

// 配置变更历史响应
#[derive(Serialize, ToSchema)]
pub struct ConfigHistoryResponse {
    pub changes: Vec<ConfigChangeInfo>,
    pub total: usize,
}

#[derive(Serialize, ToSchema)]
pub struct ConfigChangeInfo {
    pub id: i32,
    pub key_name: String,
    pub old_value: Option<String>,
    pub new_value: String,
    pub changed_at: String,
}

// 配置验证响应
#[derive(Serialize, ToSchema)]
pub struct ConfigValidationResponse {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

// 热重载状态响应
#[derive(Serialize, ToSchema)]
pub struct HotReloadStatusResponse {
    pub enabled: bool,
    pub last_reload: Option<String>,
    pub pending_changes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BangumiSeasonInfo {
    pub season_id: String,
    pub season_title: String,
    pub full_title: Option<String>, // 完整的番剧标题
    pub media_id: Option<String>,
    pub cover: Option<String>,
    pub episode_count: Option<i32>,  // 集数
    pub description: Option<String>, // 简介
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BangumiSeasonsResponse {
    pub success: bool,
    pub data: Vec<BangumiSeasonInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SearchResult {
    pub result_type: String,       // video, bili_user, media_bangumi等
    pub title: String,             // 标题
    pub author: String,            // 作者/UP主
    pub bvid: Option<String>,      // 视频BV号
    pub aid: Option<i64>,          // 视频AV号
    pub mid: Option<i64>,          // UP主ID
    pub season_id: Option<String>, // 番剧season_id
    pub media_id: Option<String>,  // 番剧media_id
    pub cover: String,             // 封面图
    pub description: String,       // 描述
    pub duration: Option<String>,  // 视频时长
    pub pubdate: Option<i64>,      // 发布时间
    pub play: Option<i64>,         // 播放量
    pub danmaku: Option<i64>,      // 弹幕数
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SearchResponse {
    pub success: bool,
    pub results: Vec<SearchResult>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UserFavoriteFolder {
    /// 收藏夹完整ID（推荐使用）
    #[serde(serialize_with = "serialize_i64_as_string")]
    pub id: i64,
    /// 收藏夹短ID（可能截断，不推荐直接使用）
    #[serde(serialize_with = "serialize_i64_as_string")]
    pub fid: i64,
    /// 收藏夹标题
    pub title: String,
    /// 收藏夹内视频数量
    pub media_count: i32,
}

// 辅助函数：将 i64 序列化为字符串
fn serialize_i64_as_string<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&value.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UserCollection {
    /// 合集类型：season（合集）或 series（系列）
    pub collection_type: String,
    /// 合集/系列ID
    pub sid: String,
    /// 合集/系列名称
    pub name: String,
    /// 封面图片URL
    pub cover: String,
    /// 描述
    pub description: String,
    /// 视频总数
    pub total: i64,
    /// 发布时间
    pub ptime: Option<i64>,
    /// UP主ID
    pub mid: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UserCollectionInfo {
    /// 合集/系列ID
    pub sid: String,
    /// 合集/系列名称
    pub name: String,
    /// 封面图片URL
    pub cover: String,
    /// 描述
    pub description: String,
    /// 视频总数
    pub total: i32,
    /// 合集类型：season（合集）或 series（系列）
    pub collection_type: String,
    /// UP主名称
    pub up_name: String,
    /// UP主ID
    pub up_mid: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UserCollectionsResponse {
    pub success: bool,
    pub collections: Vec<UserCollection>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Serialize, ToSchema)]
pub struct UserFollowing {
    pub mid: i64,
    pub name: String,
    pub face: String,
    pub sign: String,
    pub official_verify: Option<OfficialVerify>,
}

#[derive(Serialize, ToSchema)]
pub struct OfficialVerify {
    #[serde(rename = "type")]
    pub type_: i32,
    pub desc: String,
}

// 初始设置相关响应

// 初始设置检查响应
#[derive(Serialize, ToSchema)]
pub struct InitialSetupCheckResponse {
    pub needs_setup: bool,
    pub has_auth_token: bool,
    pub has_credential: bool,
}

// 设置API Token响应
#[derive(Serialize, ToSchema)]
pub struct SetupAuthTokenResponse {
    pub success: bool,
    pub message: String,
}

// 更新凭证响应
#[derive(Serialize, ToSchema)]
pub struct UpdateCredentialResponse {
    pub success: bool,
    pub message: String,
}

// 扫码登录相关响应

// 生成二维码响应
#[derive(Serialize, ToSchema)]
pub struct QRGenerateResponse {
    pub session_id: String,
    pub qr_url: String,
    pub expires_in: u64, // 过期时间（秒）
}

// 轮询二维码状态响应
#[derive(Serialize, ToSchema)]
pub struct QRPollResponse {
    pub status: String, // "pending", "scanned", "confirmed", "expired"
    pub message: String,
    pub user_info: Option<QRUserInfo>,
}

// 扫码登录成功后的用户信息
#[derive(Serialize, ToSchema)]
pub struct QRUserInfo {
    pub user_id: String,
    pub username: String,
    pub avatar_url: String,
}

/// 任务控制响应
#[derive(Serialize, ToSchema)]
pub struct TaskControlResponse {
    pub success: bool,
    pub message: String,
    pub is_paused: bool,
}

/// 任务控制状态响应
#[derive(Serialize, ToSchema)]
pub struct TaskControlStatusResponse {
    pub is_paused: bool,
    pub is_scanning: bool,
    pub message: String,
}

/// 视频播放信息响应
#[derive(Serialize, ToSchema)]
pub struct VideoPlayInfoResponse {
    pub success: bool,
    pub video_streams: Vec<VideoStreamInfo>,
    pub audio_streams: Vec<AudioStreamInfo>,
    pub subtitle_streams: Vec<SubtitleStreamInfo>,
    pub video_title: String,
    pub video_duration: Option<u32>,
    pub video_quality_description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_bvid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bilibili_url: Option<String>,
}

/// 视频流信息
#[derive(Serialize, ToSchema)]
pub struct VideoStreamInfo {
    pub url: String,
    pub backup_urls: Vec<String>,
    pub quality: u32,
    pub quality_description: String,
    pub codecs: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// 音频流信息
#[derive(Serialize, ToSchema)]
pub struct AudioStreamInfo {
    pub url: String,
    pub backup_urls: Vec<String>,
    pub quality: u32,
    pub quality_description: String,
}

/// 字幕信息
#[derive(Serialize, ToSchema)]
pub struct SubtitleStreamInfo {
    pub language: String,
    pub language_doc: String,
    pub url: String,
}

/// 验证收藏夹响应
#[derive(Serialize, ToSchema)]
pub struct ValidateFavoriteResponse {
    pub valid: bool,
    pub fid: i64,
    pub title: String,
    pub message: String,
}

/// 仪表盘响应
#[derive(Serialize, ToSchema)]
pub struct DashBoardResponse {
    pub enabled_favorites: u64,
    pub enabled_collections: u64,
    pub enabled_submissions: u64,
    pub enabled_bangumi: u64,
    pub enable_watch_later: bool,
    pub total_favorites: u64,
    pub total_collections: u64,
    pub total_submissions: u64,
    pub total_bangumi: u64,
    pub total_watch_later: u64,
    pub videos_by_day: Vec<DayCountPair>,
    /// 当前监听状态
    pub monitoring_status: MonitoringStatus,
}

/// 监听状态信息
#[derive(Serialize, ToSchema)]
pub struct MonitoringStatus {
    pub total_sources: u64,
    pub active_sources: u64,
    pub inactive_sources: u64,
    pub last_scan_time: Option<String>,
    pub next_scan_time: Option<String>,
    pub is_scanning: bool,
}

/// 每日视频计数
#[derive(Serialize, ToSchema, FromQueryResult)]
pub struct DayCountPair {
    pub day: String,
    pub cnt: i64,
}

/// 系统信息
#[derive(Serialize, ToSchema)]
pub struct SysInfo {
    pub total_memory: u64,
    pub used_memory: u64,
    pub process_memory: u64,
    pub used_cpu: f32,
    pub process_cpu: f32,
    pub total_disk: u64,
    pub available_disk: u64,
}

// 推送配置响应
#[derive(Serialize, ToSchema)]
pub struct NotificationConfigResponse {
    pub serverchan_key: Option<String>,
    pub enable_scan_notifications: bool,
    pub notification_min_videos: usize,
    pub notification_timeout: u64,
    pub notification_retry_count: u8,
}

// 测试推送响应
#[derive(Serialize, ToSchema)]
pub struct TestNotificationResponse {
    pub success: bool,
    pub message: String,
}

// 推送状态响应
#[derive(Serialize, ToSchema)]
pub struct NotificationStatusResponse {
    pub configured: bool,
    pub enabled: bool,
    pub last_notification_time: Option<String>,
    pub total_notifications_sent: u64,
    pub last_error: Option<String>,
}

// 直播监控相关响应结构体

// 直播监控信息
#[derive(Debug, Serialize, ToSchema)]
pub struct LiveMonitorInfo {
    pub id: i32,
    pub upper_id: i64,
    pub upper_name: String,
    pub room_id: i64,
    pub short_room_id: Option<i64>,
    pub path: String,
    pub enabled: bool,
    pub check_interval: i32,
    pub quality: String,
    pub format: String,
    pub last_status: i32,
    pub last_check_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    // 新增字段：直播间信息
    pub cover_url: Option<String>,
    pub room_url: Option<String>,
    pub current_title: Option<String>,
}

// 录制记录信息
#[derive(Debug, Serialize, ToSchema)]
pub struct RecordingInfo {
    pub id: i32,
    pub monitor_id: i32,
    pub room_id: i64,
    pub title: Option<String>,
    pub start_time: String,
    pub end_time: Option<String>,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub status: i32,
    pub monitor: Option<LiveMonitorInfo>,
}

// 添加直播监控响应
#[derive(Debug, Serialize, ToSchema)]
pub struct AddLiveMonitorResponse {
    pub success: bool,
    pub message: String,
    pub monitor: Option<LiveMonitorInfo>,
}

// 直播监控列表响应
#[derive(Debug, Serialize, ToSchema)]
pub struct LiveMonitorsResponse {
    pub monitors: Vec<LiveMonitorInfo>,
    pub total_count: u64,
}

// 更新直播监控响应
#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateLiveMonitorResponse {
    pub success: bool,
    pub message: String,
    pub monitor: Option<LiveMonitorInfo>,
}

// 删除直播监控响应
#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteLiveMonitorResponse {
    pub success: bool,
    pub message: String,
}

// 录制记录列表响应
#[derive(Debug, Serialize, ToSchema)]
pub struct RecordingsResponse {
    pub recordings: Vec<RecordingInfo>,
    pub total_count: u64,
}

// 直播监控状态响应
#[derive(Debug, Serialize, ToSchema)]
pub struct LiveMonitorStatusResponse {
    pub running: bool,
    pub total_monitors: usize,
    pub enabled_monitors: usize,
    pub active_recordings: usize,
}

// 控制直播监控响应
#[derive(Debug, Serialize, ToSchema)]
pub struct LiveMonitorControlResponse {
    pub success: bool,
    pub message: String,
    pub status: LiveMonitorStatusResponse,
}
