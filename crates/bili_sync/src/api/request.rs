use serde::Deserialize;
use utoipa::IntoParams;
use utoipa::ToSchema;

#[derive(Deserialize, IntoParams, Default)]
pub struct VideosRequest {
    pub collection: Option<i32>,
    pub favorite: Option<i32>,
    pub submission: Option<i32>,
    pub watch_later: Option<i32>,
    pub bangumi: Option<i32>,
    pub query: Option<String>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub show_failed_only: Option<bool>,
    pub force: Option<bool>,
    pub sort_by: Option<String>,     // "id", "name", "upper_name", "created_at", "updated_at"
    pub sort_order: Option<String>,  // "asc", "desc"
}

#[derive(Deserialize, IntoParams)]
pub struct SubmissionVideosRequest {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

// 添加新视频源的请求结构体
#[derive(Deserialize, IntoParams, ToSchema)]
pub struct AddVideoSourceRequest {
    // 视频源类型: "collection", "favorite", "submission", "watch_later", "bangumi"
    pub source_type: String,
    // 视频源ID: 收藏夹ID、合集ID、UP主ID等
    pub source_id: String,
    // UP主ID: 仅当source_type为"collection"时需要
    pub up_id: Option<String>,
    // 视频源名称
    pub name: String,
    // 保存路径
    pub path: String,
    // 合集类型: "season"(视频合集) 或 "series"(视频列表)，仅当source_type为"collection"时有效
    pub collection_type: Option<String>,
    // 番剧特有字段
    pub media_id: Option<String>,
    pub ep_id: Option<String>,
    // 是否下载全部季度，仅当source_type为"bangumi"时有效
    pub download_all_seasons: Option<bool>,
    // 选中的季度ID列表，仅当source_type为"bangumi"且download_all_seasons为false时有效
    pub selected_seasons: Option<Vec<String>>,
    // 选中的视频ID列表，仅当source_type为"submission"时有效，用于选择性下载历史投稿
    pub selected_videos: Option<Vec<String>>,
}

// 删除视频源的请求结构体
#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteVideoSourceRequest {
    pub delete_local_files: bool,
}

// 更新视频源启用状态的请求结构体
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateVideoSourceEnabledRequest {
    pub enabled: bool,
}

// 更新视频源扫描已删除视频设置的请求结构体
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateVideoSourceScanDeletedRequest {
    pub scan_deleted_videos: bool,
}

// 重设视频源路径的请求结构体
#[derive(Debug, Deserialize, ToSchema)]
pub struct ResetVideoSourcePathRequest {
    /// 新的基础路径
    pub new_path: String,
    /// 是否应用四步重命名原则移动文件
    #[serde(default = "default_apply_rename_rules")]
    pub apply_rename_rules: bool,
    /// 是否删除空的原始文件夹
    #[serde(default = "default_clean_empty_folders")]
    pub clean_empty_folders: bool,
}

fn default_apply_rename_rules() -> bool {
    true
}

fn default_clean_empty_folders() -> bool {
    true
}

// 更新配置的请求结构体
#[derive(Deserialize, IntoParams, ToSchema)]
pub struct UpdateConfigRequest {
    // 视频命名模板
    pub video_name: Option<String>,
    // 分页命名模板
    pub page_name: Option<String>,
    // 多P视频分页命名模板
    pub multi_page_name: Option<String>,
    // 番剧分页命名模板
    pub bangumi_name: Option<String>,
    // 文件夹结构模板
    pub folder_structure: Option<String>,
    // 番剧文件夹命名模板
    pub bangumi_folder_name: Option<String>,
    // 合集文件夹模式
    pub collection_folder_mode: Option<String>,
    // 时间格式
    pub time_format: Option<String>,
    // 扫描间隔（秒）
    pub interval: Option<u64>,
    // NFO时间类型
    pub nfo_time_type: Option<String>,
    // 多线程下载配置
    pub parallel_download_enabled: Option<bool>,
    pub parallel_download_threads: Option<usize>,
    // 视频质量设置
    pub video_max_quality: Option<String>,
    pub video_min_quality: Option<String>,
    pub audio_max_quality: Option<String>,
    pub audio_min_quality: Option<String>,
    pub codecs: Option<Vec<String>>,
    pub no_dolby_video: Option<bool>,
    pub no_dolby_audio: Option<bool>,
    pub no_hdr: Option<bool>,
    pub no_hires: Option<bool>,
    // 弹幕设置
    pub danmaku_duration: Option<f64>,
    pub danmaku_font: Option<String>,
    pub danmaku_font_size: Option<u32>,
    pub danmaku_width_ratio: Option<f64>,
    pub danmaku_horizontal_gap: Option<f64>,
    pub danmaku_lane_size: Option<u32>,
    pub danmaku_float_percentage: Option<f64>,
    pub danmaku_bottom_percentage: Option<f64>,
    pub danmaku_opacity: Option<u8>,
    pub danmaku_bold: Option<bool>,
    pub danmaku_outline: Option<f64>,
    pub danmaku_time_offset: Option<f64>,
    // 并发控制设置
    pub concurrent_video: Option<usize>,
    pub concurrent_page: Option<usize>,
    pub rate_limit: Option<usize>,
    pub rate_duration: Option<u64>,
    // 其他设置
    pub cdn_sorting: Option<bool>,
    // UP主投稿风控配置
    pub large_submission_threshold: Option<usize>,
    pub base_request_delay: Option<u64>,
    pub large_submission_delay_multiplier: Option<u64>,
    pub enable_progressive_delay: Option<bool>,
    pub max_delay_multiplier: Option<u64>,
    pub enable_incremental_fetch: Option<bool>,
    pub incremental_fallback_to_full: Option<bool>,
    pub enable_batch_processing: Option<bool>,
    pub batch_size: Option<usize>,
    pub batch_delay_seconds: Option<u64>,
    pub enable_auto_backoff: Option<bool>,
    pub auto_backoff_base_seconds: Option<u64>,
    pub auto_backoff_max_multiplier: Option<u64>,
    pub source_delay_seconds: Option<u64>,
    pub submission_source_delay_seconds: Option<u64>,
    // 系统配置
    pub scan_deleted_videos: Option<bool>,
    // aria2监控配置
    pub enable_aria2_health_check: Option<bool>,
    pub enable_aria2_auto_restart: Option<bool>,
    pub aria2_health_check_interval: Option<u64>,
    // 多P视频目录结构配置
    pub multi_page_use_season_structure: Option<bool>,
    // 合集目录结构配置
    pub collection_use_season_structure: Option<bool>,
    // 番剧目录结构配置
    pub bangumi_use_season_structure: Option<bool>,
    // UP主头像保存路径
    pub upper_path: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchRequest {
    pub keyword: String,     // 搜索关键词
    pub search_type: String, // 搜索类型：video, bili_user, media_bangumi
    #[serde(default = "default_page")]
    pub page: u32, // 页码，默认1
    #[serde(default = "default_page_size")]
    pub page_size: u32, // 每页数量，默认20
}

fn default_page() -> u32 {
    1
}

fn default_page_size() -> u32 {
    20
}

// 状态更新结构
#[derive(Deserialize, ToSchema)]
pub struct StatusUpdate {
    pub status_index: usize, // 状态位索引 (0-4)
    pub status_value: u32,   // 状态值 (0, 1, 2, 3)
}

// 推送配置更新请求
#[derive(Deserialize, ToSchema)]
pub struct UpdateNotificationConfigRequest {
    pub serverchan_key: Option<String>,
    pub enable_scan_notifications: Option<bool>,
    pub notification_min_videos: Option<usize>,
    pub notification_timeout: Option<u64>,
    pub notification_retry_count: Option<u8>,
}

// 测试推送请求（可选消息内容）
#[derive(Deserialize, ToSchema)]
pub struct TestNotificationRequest {
    pub custom_message: Option<String>,
}

// 分页状态更新结构
#[derive(Deserialize, ToSchema)]
pub struct PageStatusUpdate {
    pub page_id: i32,
    pub updates: Vec<StatusUpdate>,
}

// 更新视频状态请求
#[derive(Deserialize, ToSchema)]
pub struct UpdateVideoStatusRequest {
    #[serde(default)]
    pub video_updates: Vec<StatusUpdate>,
    #[serde(default)]
    pub page_updates: Vec<PageStatusUpdate>,
}

// 选择性重置任务请求
#[derive(Deserialize, ToSchema)]
pub struct ResetSpecificTasksRequest {
    pub task_indexes: Vec<usize>, // 要重置的任务索引列表 (0-4)
    pub collection: Option<i32>,
    pub favorite: Option<i32>,
    pub submission: Option<i32>,
    pub watch_later: Option<i32>,
    pub bangumi: Option<i32>,
}

// 配置管理相关请求结构体

// 更新单个配置项请求
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateConfigItemRequest {
    pub value: serde_json::Value,
}

// 批量更新配置请求
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct BatchUpdateConfigRequest {
    pub items: std::collections::HashMap<String, serde_json::Value>,
}

// 配置历史查询请求
#[derive(Deserialize, IntoParams)]
#[allow(dead_code)]
pub struct ConfigHistoryRequest {
    pub key: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

// 配置导出请求
#[derive(Deserialize, ToSchema)]
#[allow(dead_code)]
pub struct ConfigExportRequest {
    pub format: String,            // "json" 或 "toml"
    pub keys: Option<Vec<String>>, // 指定要导出的配置键，None表示导出全部
}

// 配置导入请求
#[derive(Deserialize, ToSchema)]
#[allow(dead_code)]
pub struct ConfigImportRequest {
    pub format: String, // "json" 或 "toml"
    pub data: String,   // 配置数据
    pub merge: bool,    // 是否合并现有配置，false表示覆盖
}

// 初始设置相关请求

// 设置API Token请求
#[derive(Deserialize, ToSchema)]
pub struct SetupAuthTokenRequest {
    pub auth_token: String,
}

// 更新凭证请求
#[derive(Deserialize, ToSchema)]
pub struct UpdateCredentialRequest {
    pub sessdata: String,
    pub bili_jct: String,
    pub buvid3: String,
    pub dedeuserid: String,
    pub ac_time_value: Option<String>,
}

// 扫码登录相关请求

// 生成二维码请求
#[derive(Deserialize, ToSchema)]
pub struct QRGenerateRequest {
    #[allow(dead_code)]
    pub client_type: Option<String>, // "web" 或 "tv"，默认为 "web"
}

// 轮询二维码状态请求
#[derive(Deserialize, IntoParams)]
pub struct QRPollRequest {
    pub session_id: String,
}

// 直播监控相关请求结构体

// 添加直播监控请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddLiveMonitorRequest {
    /// UP主ID
    pub upper_id: i64,
    /// UP主名称
    pub upper_name: String,
    /// 房间ID
    pub room_id: i64,
    /// 短房间ID
    pub short_room_id: Option<i64>,
    /// 保存路径
    pub path: String,
    /// 检查间隔（秒）
    #[serde(default = "default_check_interval")]
    pub check_interval: u64,
    /// 录制质量（可选，使用全局配置）
    pub quality: Option<String>,
    /// 录制格式（可选，使用全局配置）
    pub format: Option<String>,
    /// 是否启用
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 最大文件大小（字节），0表示无限制
    #[serde(default = "default_max_file_size")]
    pub max_file_size: i64,
}

// 更新直播监控请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateLiveMonitorRequest {
    pub upper_name: Option<String>,
    pub path: Option<String>,
    pub check_interval: Option<u64>,
    pub quality: Option<String>,
    pub format: Option<String>,
    pub enabled: Option<bool>,
    pub max_file_size: Option<i64>,
}

// 直播监控列表查询请求
#[derive(Deserialize, IntoParams)]
pub struct LiveMonitorsRequest {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub enabled: Option<bool>,
    pub upper_name: Option<String>,
}

// 录制记录查询请求
#[derive(Deserialize, IntoParams)]
pub struct RecordingsRequest {
    pub monitor_id: Option<i32>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub status: Option<i32>,
}

fn default_check_interval() -> u64 {
    60 // 默认60秒检查一次
}

fn default_quality() -> String {
    "high".to_string() // 默认高清
}

fn default_format() -> String {
    "flv".to_string() // 默认FLV格式
}

fn default_enabled() -> bool {
    true // 默认启用
}

fn default_max_file_size() -> i64 {
    0 // 默认无限制
}
