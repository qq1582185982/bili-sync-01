use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use bili_sync_entity::ai_conversation_history;
use super::deepseek_web::{deepseek_web_generate, DeepSeekSession};

/// AI 重命名上下文（从 API 获取的视频信息）
#[derive(Clone, Debug, Default)]
pub struct AiRenameContext {
    /// 视频标题
    pub title: String,
    /// 视频简介
    pub desc: String,
    /// UP主名称
    pub owner: String,
    /// 分区名称
    pub tname: String,
    /// 时长（秒）
    pub duration: u32,
    /// 发布日期（如 "2023-12-29"）
    pub pubdate: String,
    /// 分辨率（如 "1920x1080"）
    pub dimension: String,
    /// 当前分P名称
    pub part_name: String,
    /// 合集名称（如果属于合集）
    pub ugc_season: Option<String>,
    /// 版权类型（"自制" 或 "转载"）
    pub copyright: String,
    /// 播放量
    pub view: u64,
    /// 当前是第几P
    pub pid: i32,
    /// 合集中第几集
    pub episode_number: Option<i32>,
    /// 来源类型（收藏夹/合集/投稿等）
    pub source_type: String,
    /// 是否为音频模式
    pub is_audio: bool,
    /// 在视频源中的排序位置（按发布时间排序后的顺序，从1开始）
    pub sort_index: Option<i32>,
    /// B站视频ID（BV号）
    pub bvid: String,
}

impl AiRenameContext {
    /// 构建发送给 AI 的 JSON 信息
    pub fn to_json_string(&self) -> String {
        let mut info = serde_json::json!({
            "标题": self.title,
            "UP主": self.owner,
            "来源": self.source_type,
        });

        // 添加BV号（非空时）
        if !self.bvid.is_empty() {
            info["BV号"] = serde_json::json!(self.bvid);
        }

        // 只添加非空字段
        if !self.tname.is_empty() {
            info["分区"] = serde_json::json!(self.tname);
        }
        if !self.dimension.is_empty() {
            info["清晰度"] = serde_json::json!(self.dimension);
        }
        if self.duration > 0 {
            let dur_str = if self.duration >= 3600 {
                format!("{}:{:02}:{:02}", self.duration / 3600, (self.duration % 3600) / 60, self.duration % 60)
            } else {
                format!("{}:{:02}", self.duration / 60, self.duration % 60)
            };
            info["时长"] = serde_json::json!(dur_str);
        }
        if !self.pubdate.is_empty() {
            info["发布日期"] = serde_json::json!(self.pubdate);
        }
        if !self.copyright.is_empty() {
            info["版权"] = serde_json::json!(self.copyright);
        }
        if self.view > 0 {
            info["播放量"] = serde_json::json!(self.view);
        }
        if let Some(ref season) = self.ugc_season {
            info["合集"] = serde_json::json!(season);
        }
        if let Some(ep) = self.episode_number {
            info["集数"] = serde_json::json!(format!("第{}集", ep));
        }
        if let Some(idx) = self.sort_index {
            info["排序位置"] = serde_json::json!(idx);
        }
        if self.pid > 1 {
            info["分P"] = serde_json::json!(format!("P{}", self.pid));
        }
        if !self.part_name.is_empty() && self.part_name != self.title {
            info["分P名称"] = serde_json::json!(self.part_name);
        }
        if !self.desc.is_empty() {
            // 简介截取前200字符
            let desc_short = if self.desc.chars().count() > 200 {
                format!("{}...", self.desc.chars().take(200).collect::<String>())
            } else {
                self.desc.clone()
            };
            info["简介"] = serde_json::json!(desc_short);
        }
        if self.is_audio {
            info["模式"] = serde_json::json!("仅音频");
        }

        let json_str = serde_json::to_string_pretty(&info).unwrap_or_default();

        // 添加 API 参考链接，让 AI 可以参考更多信息
        if !self.bvid.is_empty() {
            format!("{}\nAPI参考: https://api.bilibili.com/x/web-interface/view?bvid={}", json_str, self.bvid)
        } else {
            json_str
        }
    }
}

/// DeepSeek Web 会话缓存（按 source_key 存储）
/// 同一个视频源复用同一个会话，避免创建过多会话
/// 使用 tokio::sync::Mutex 确保异步安全
static DEEPSEEK_SESSION_CACHE: Lazy<Mutex<HashMap<String, DeepSeekSession>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// AI 重命名全局锁（确保同一时间只有一个 AI 重命名请求）
/// 防止并发请求导致创建多个会话
static AI_RENAME_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// 对话消息（用于存储历史）
#[derive(Clone, Debug)]
struct ConversationMessage {
    role: String,
    content: String,
}

/// 清除指定视频源的对话历史（数据库持久化版本）
pub async fn clear_naming_cache(source_key: &str) -> Result<()> {
    let db = crate::database::get_global_db()
        .ok_or_else(|| anyhow!("数据库连接不可用"))?;

    let result = ai_conversation_history::Entity::delete_many()
        .filter(ai_conversation_history::Column::SourceKey.eq(source_key))
        .exec(db.as_ref())
        .await?;

    // 同时清除 DeepSeek Web 会话缓存
    {
        let mut cache = DEEPSEEK_SESSION_CACHE.lock().await;
        cache.remove(source_key);
    }

    info!("已清除视频源 {} 的AI对话历史，删除 {} 条记录", source_key, result.rows_affected);
    Ok(())
}

/// 清除所有对话历史（数据库持久化版本）
pub async fn clear_all_naming_cache() -> Result<()> {
    let db = crate::database::get_global_db()
        .ok_or_else(|| anyhow!("数据库连接不可用"))?;

    let result = ai_conversation_history::Entity::delete_many()
        .exec(db.as_ref())
        .await?;

    // 同时清除所有 DeepSeek Web 会话缓存
    {
        let mut cache = DEEPSEEK_SESSION_CACHE.lock().await;
        cache.clear();
    }

    info!("已清除所有AI对话历史，删除 {} 条记录", result.rows_affected);
    Ok(())
}

/// 添加对话消息到历史（数据库持久化版本）
async fn add_conversation_message(db: &DatabaseConnection, source_key: &str, role: &str, content: &str) -> Result<()> {
    // 获取当前最大的order_index
    let max_order = ai_conversation_history::Entity::find()
        .filter(ai_conversation_history::Column::SourceKey.eq(source_key))
        .order_by_desc(ai_conversation_history::Column::OrderIndex)
        .one(db)
        .await?
        .map(|m| m.order_index)
        .unwrap_or(-1);

    let new_order = max_order + 1;

    // 检查消息数量，如果超过10条（5轮对话）则删除最早的2条
    let count = ai_conversation_history::Entity::find()
        .filter(ai_conversation_history::Column::SourceKey.eq(source_key))
        .count(db)
        .await?;

    if count >= 10 {
        // 获取最早的2条记录的ID
        let oldest = ai_conversation_history::Entity::find()
            .filter(ai_conversation_history::Column::SourceKey.eq(source_key))
            .order_by_asc(ai_conversation_history::Column::OrderIndex)
            .limit(2)
            .all(db)
            .await?;

        for record in oldest {
            ai_conversation_history::Entity::delete_by_id(record.id)
                .exec(db)
                .await?;
        }
        debug!("清理 {} 的旧对话记录，保留最近8条", source_key);
    }

    // 插入新消息
    let new_message = ai_conversation_history::ActiveModel {
        source_key: Set(source_key.to_string()),
        role: Set(role.to_string()),
        content: Set(content.to_string()),
        order_index: Set(new_order),
        created_at: Set(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
        ..Default::default()
    };

    new_message.insert(db).await?;
    debug!("保存对话消息到数据库: source_key={}, role={}, order={}", source_key, role, new_order);

    Ok(())
}

/// 获取对话历史（数据库持久化版本）
async fn get_conversation_history(db: &DatabaseConnection, source_key: &str) -> Vec<ConversationMessage> {
    match ai_conversation_history::Entity::find()
        .filter(ai_conversation_history::Column::SourceKey.eq(source_key))
        .order_by_asc(ai_conversation_history::Column::OrderIndex)
        .all(db)
        .await
    {
        Ok(records) => {
            records
                .into_iter()
                .map(|r| ConversationMessage {
                    role: r.role,
                    content: r.content,
                })
                .collect()
        }
        Err(e) => {
            warn!("获取对话历史失败: {}", e);
            Vec::new()
        }
    }
}

/// 保存 DeepSeek 会话到数据库
/// 使用 role = "deepseek_session" 标识，content 存储 JSON
async fn save_deepseek_session(db: &DatabaseConnection, source_key: &str, session: &DeepSeekSession) -> Result<()> {
    // 序列化会话信息
    let content = serde_json::to_string(session)?;

    // 先删除旧的会话记录
    ai_conversation_history::Entity::delete_many()
        .filter(ai_conversation_history::Column::SourceKey.eq(source_key))
        .filter(ai_conversation_history::Column::Role.eq("deepseek_session"))
        .exec(db)
        .await?;

    // 插入新记录
    let new_record = ai_conversation_history::ActiveModel {
        source_key: Set(source_key.to_string()),
        role: Set("deepseek_session".to_string()),
        content: Set(content),
        order_index: Set(-1), // 特殊标记
        created_at: Set(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
        ..Default::default()
    };
    new_record.insert(db).await?;

    debug!("保存 DeepSeek 会话到数据库: source_key={}, session_id={}", source_key, session.session_id);
    Ok(())
}

/// 从数据库加载 DeepSeek 会话
async fn load_deepseek_session(db: &DatabaseConnection, source_key: &str) -> Option<DeepSeekSession> {
    match ai_conversation_history::Entity::find()
        .filter(ai_conversation_history::Column::SourceKey.eq(source_key))
        .filter(ai_conversation_history::Column::Role.eq("deepseek_session"))
        .one(db)
        .await
    {
        Ok(Some(record)) => {
            match serde_json::from_str::<DeepSeekSession>(&record.content) {
                Ok(session) => {
                    debug!("从数据库加载 DeepSeek 会话: source_key={}, session_id={}", source_key, session.session_id);
                    Some(session)
                }
                Err(e) => {
                    warn!("解析 DeepSeek 会话失败: {}", e);
                    None
                }
            }
        }
        Ok(None) => None,
        Err(e) => {
            warn!("加载 DeepSeek 会话失败: {}", e);
            None
        }
    }
}

/// AI 重命名全局配置（存储在 Config 中）
///
/// 说明：这里走 **OpenAI 兼容** 的 chat/completions 接口（DeepSeek / OpenAI / 其它兼容服务都可）。
/// 如果 api_key 为空，会直接返回错误，由调用方决定是否跳过。
///
/// 当 provider 为 "deepseek-web" 时，使用 chat.deepseek.com 免费 Web API。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiRenameConfig {
    /// 是否启用 AI 重命名（全局开关）
    pub enabled: bool,
    /// Provider 类型（openai / deepseek / deepseek-web / custom）
    /// - openai/deepseek/custom: 使用 OpenAI 兼容 API
    /// - deepseek-web: 使用 chat.deepseek.com 免费 Web API
    pub provider: String,
    /// OpenAI 兼容接口 base url，例如：
    /// - https://api.openai.com/v1
    /// - https://api.deepseek.com/v1
    pub base_url: String,
    /// API Key（用户自备）- 用于 OpenAI 兼容 API
    pub api_key: Option<String>,
    /// DeepSeek Web Token - 用于 deepseek-web provider
    /// 从浏览器开发者工具中获取
    #[serde(default)]
    pub deepseek_web_token: Option<String>,
    /// 模型名，例如 gpt-4o-mini / deepseek-chat
    pub model: String,
    /// 请求超时（秒）
    pub timeout_seconds: u64,
    /// 视频提示词（不含扩展名）
    pub video_prompt_hint: String,
    /// 音频提示词（不含扩展名）
    pub audio_prompt_hint: String,
    /// 是否启用多P视频AI重命名（默认关闭）
    #[serde(default)]
    pub enable_multi_page: bool,
    /// 是否启用合集视频AI重命名（默认关闭）
    #[serde(default)]
    pub enable_collection: bool,
    /// 是否启用番剧AI重命名（默认关闭）
    #[serde(default)]
    pub enable_bangumi: bool,
}

impl Default for AiRenameConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "custom".to_string(),
            base_url: "https://api.deepseek.com/v1".to_string(),
            api_key: None,
            deepseek_web_token: None,
            model: "deepseek-chat".to_string(),
            timeout_seconds: 20,
            // 视频命名规则
            video_prompt_hint: "【命名结构】精简标题-作者-时间(YYYYMMDD)；\
【标题规则】仅保留核心主题，去除修饰性/情绪性/营销性词语，不使用表情；\
【符号规则】仅用英文连字符-，禁止其他特殊符号".to_string(),
            // 音频命名规则
            audio_prompt_hint: "【命名结构】歌手-歌名-版本(如录音棚/现场)-时间(YYYYMMDD)；\
【规则】去除表情/情绪文案，仅用英文连字符-连接".to_string(),
            // 特殊类型默认关闭
            enable_multi_page: false,
            enable_collection: false,
            enable_bangumi: false,
        }
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
}

#[derive(Serialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessageResponse,
}

#[derive(Deserialize)]
struct ChatMessageResponse {
    content: String,
}

/// 调用 AI 接口生成新文件名（仅返回 stem，不含扩展名）
///
/// 根据 provider 自动选择：
/// - "deepseek-web": 使用 chat.deepseek.com 免费 Web API
/// - 其他 (openai/deepseek/custom): 使用 OpenAI 兼容 API
///
/// # 参数
/// - `cfg`: 全局AI重命名配置
/// - `source_key`: 视频源唯一标识（如 "collection_123"），用于保持同一源的对话上下文
/// - `ctx`: AI 重命名上下文（从 API 获取的视频信息）
/// - `current_filename`: 当前文件名（不含扩展名，可能包含剧集编号等信息）
/// - `video_prompt_override`: 视频源自定义视频提示词（非空时覆盖全局配置）
/// - `audio_prompt_override`: 视频源自定义音频提示词（非空时覆盖全局配置）
#[allow(dead_code)]
pub async fn ai_generate_filename(
    cfg: &AiRenameConfig,
    source_key: &str,
    ctx: &AiRenameContext,
    current_filename: &str,
    video_prompt_override: &str,
    audio_prompt_override: &str,
) -> Result<String> {
    // 优先使用视频源自定义提示词，如果为空则使用全局配置
    let prompt = if ctx.is_audio {
        if !audio_prompt_override.is_empty() {
            audio_prompt_override
        } else {
            &cfg.audio_prompt_hint
        }
    } else {
        if !video_prompt_override.is_empty() {
            video_prompt_override
        } else {
            &cfg.video_prompt_hint
        }
    };

    // 构建完整的用户提示（使用 JSON 格式发送视频信息）
    let video_info = ctx.to_json_string();
    let full_prompt = format!(
        "根据以下视频信息生成新的文件名。\n\
【重要】严格按照用户指定的命名格式生成，不要添加格式中未要求的任何信息！\n\
只输出文件名，不含扩展名，不要解释，不要引号。\n\n\
当前文件名：{}\n\n\
视频信息（仅供参考，只提取格式中需要的字段；如信息缺失可访问API参考链接获取）：\n{}\n\n\
用户指定的命名格式：{}\n",
        current_filename, video_info, prompt
    );

    // 根据 provider 选择实现
    if cfg.provider == "deepseek-web" {
        ai_generate_filename_deepseek_web(cfg, source_key, &full_prompt, current_filename).await
    } else {
        ai_generate_filename_openai_compatible(cfg, source_key, &full_prompt, current_filename).await
    }
}

/// 使用 DeepSeek Web API (chat.deepseek.com) 生成文件名
#[allow(dead_code)]
async fn ai_generate_filename_deepseek_web(
    cfg: &AiRenameConfig,
    source_key: &str,
    user_prompt: &str,
    current_filename: &str,
) -> Result<String> {
    // 获取全局锁，确保 AI 重命名请求串行执行
    // 防止并发请求导致创建多个会话
    let _lock = AI_RENAME_LOCK.lock().await;

    let token = cfg.deepseek_web_token.clone()
        .ok_or_else(|| anyhow!("DeepSeek Web Token 未配置"))?;

    // 获取数据库连接
    let db = crate::database::get_global_db()
        .ok_or_else(|| anyhow!("数据库连接不可用"))?;

    // 获取对话历史（从数据库）
    let history = get_conversation_history(db.as_ref(), source_key).await;
    let history_len = history.len();

    // 调试日志
    debug!(
        "AI重命名调用(DeepSeek Web) - source_key: '{}', 当前文件名: '{}', 对话历史: {}条消息",
        source_key, current_filename, history_len
    );

    // 构建完整提示（包含历史上下文）
    let full_prompt = if history.is_empty() {
        format!(
            "你是一个负责优化文件命名的助手，只输出文件名本身。这是同一视频源的第一个文件，请建立命名风格。\n\n{}",
            user_prompt
        )
    } else {
        // 构建包含历史的提示
        let mut context = String::from("你是一个负责优化文件命名的助手，只输出文件名本身。注意：这是同一视频源的后续文件，必须严格遵循之前已建立的命名风格。\n\n之前的命名示例：\n");
        for msg in &history {
            if msg.role == "assistant" {
                context.push_str(&format!("- {}\n", msg.content));
            }
        }
        context.push_str(&format!("\n现在请为以下内容生成一致的文件名：\n{}", user_prompt));
        context
    };

    // 从缓存获取会话（优先内存缓存，其次数据库）
    let cached_session = {
        let cache = DEEPSEEK_SESSION_CACHE.lock().await;
        if let Some(session) = cache.get(source_key).cloned() {
            info!("会话缓存命中（内存）: source_key='{}', session_id='{}'", source_key, session.session_id);
            Some(session)
        } else {
            // 内存缓存未命中，尝试从数据库加载
            drop(cache); // 释放锁
            if let Some(session) = load_deepseek_session(db.as_ref(), source_key).await {
                info!("会话缓存命中（数据库）: source_key='{}', session_id='{}'", source_key, session.session_id);
                // 加载到内存缓存
                let mut cache = DEEPSEEK_SESSION_CACHE.lock().await;
                cache.insert(source_key.to_string(), session.clone());
                Some(session)
            } else {
                info!("会话缓存未命中: source_key='{}'，将创建新会话", source_key);
                None
            }
        }
    };

    // 调用 DeepSeek Web API
    let result = deepseek_web_generate(
        &token,
        cached_session,
        &full_prompt,
        cfg.timeout_seconds,
    ).await;

    // 检查是否是会话长度上限错误，需要重建会话
    let (name, new_session) = match result {
        Ok(res) => res,
        Err(e) if e.to_string().contains("SESSION_LIMIT_REACHED") => {
            warn!("[{}] DeepSeek 会话达到长度上限，正在重建会话并带上历史...", source_key);

            // 清除旧会话缓存
            {
                let mut cache = DEEPSEEK_SESSION_CACHE.lock().await;
                cache.remove(source_key);
            }

            // 获取历史记录作为上下文（重新获取，因为之前的变量已被使用）
            let history_for_retry = get_conversation_history(db.as_ref(), source_key).await;
            let history_context = if !history_for_retry.is_empty() {
                let mut ctx = String::from("【之前的命名风格参考】\n");
                for msg in &history_for_retry {
                    if msg.role == "assistant" {
                        ctx.push_str(&format!("{}\n", msg.content));
                    }
                }
                ctx.push_str("\n请严格遵循以上命名风格。\n\n");
                ctx
            } else {
                String::new()
            };

            // 构建带历史上下文的新 prompt
            let new_prompt = format!("{}{}", history_context, full_prompt);

            // 用新会话重试（session = None 会创建新会话）
            info!("[{}] 使用新会话重试，带上 {} 条历史记录", source_key, history_for_retry.len());
            deepseek_web_generate(
                &token,
                None, // 创建新会话
                &new_prompt,
                cfg.timeout_seconds,
            ).await?
        }
        Err(e) if e.to_string().contains("读取响应体失败") || e.to_string().contains("error decoding response body") => {
            warn!("[{}] DeepSeek 响应体解码失败，正在重建会话并带上历史...", source_key);

            // 清除旧会话缓存
            {
                let mut cache = DEEPSEEK_SESSION_CACHE.lock().await;
                cache.remove(source_key);
            }

            // 获取历史记录作为上下文
            let history_for_retry = get_conversation_history(db.as_ref(), source_key).await;
            let history_context = if !history_for_retry.is_empty() {
                let mut ctx = String::from("【之前的命名风格参考】\n");
                for msg in &history_for_retry {
                    if msg.role == "assistant" {
                        ctx.push_str(&format!("{}\n", msg.content));
                    }
                }
                ctx.push_str("\n请严格遵循以上命名风格。\n\n");
                ctx
            } else {
                String::new()
            };

            // 构建带历史上下文的新 prompt
            let new_prompt = format!("{}{}", history_context, full_prompt);

            // 用新会话重试
            info!("[{}] 使用新会话重试（响应体错误），带上 {} 条历史记录", source_key, history_for_retry.len());
            deepseek_web_generate(
                &token,
                None,
                &new_prompt,
                cfg.timeout_seconds,
            ).await?
        }
        Err(e) => return Err(e),
    };

    // 更新会话缓存（内存 + 数据库）
    {
        let mut cache = DEEPSEEK_SESSION_CACHE.lock().await;
        info!("更新会话缓存: source_key='{}', session_id='{}'", source_key, new_session.session_id);
        cache.insert(source_key.to_string(), new_session.clone());
    }
    // 保存到数据库
    if let Err(e) = save_deepseek_session(db.as_ref(), source_key, &new_session).await {
        warn!("保存 DeepSeek 会话到数据库失败: {}", e);
    }

    // 将用户消息和助手回复添加到对话历史（保存到数据库）
    if let Err(e) = add_conversation_message(db.as_ref(), source_key, "user", user_prompt).await {
        warn!("保存用户消息到数据库失败: {}", e);
    }
    if let Err(e) = add_conversation_message(db.as_ref(), source_key, "assistant", &name).await {
        warn!("保存助手回复到数据库失败: {}", e);
    }

    info!(
        "AI重命名成功(DeepSeek Web) [{}]: {} → {}",
        source_key, current_filename, name
    );

    Ok(name)
}

/// 使用 OpenAI 兼容 API 生成文件名
#[allow(dead_code)]
async fn ai_generate_filename_openai_compatible(
    cfg: &AiRenameConfig,
    source_key: &str,
    user_prompt: &str,
    current_filename: &str,
) -> Result<String> {
    let api_key = cfg.api_key.clone().ok_or_else(|| anyhow!("API key missing"))?;

    // 获取数据库连接
    let db = crate::database::get_global_db()
        .ok_or_else(|| anyhow!("数据库连接不可用"))?;

    // 获取对话历史（从数据库）
    let history = get_conversation_history(db.as_ref(), source_key).await;
    let history_len = history.len();

    // 调试日志
    debug!(
        "AI重命名调用(OpenAI) - source_key: '{}', 当前文件名: '{}', 对话历史: {}条消息",
        source_key, current_filename, history_len
    );

    // 构建系统提示词
    let system_prompt = if history.is_empty() {
        "你是一个负责优化文件命名的助手，只输出文件名本身。这是同一视频源的第一个文件，请建立命名风格。".to_string()
    } else {
        "你是一个负责优化文件命名的助手，只输出文件名本身。注意：这是同一视频源的后续文件，必须严格遵循之前已建立的命名风格，保持完全一致的格式。".to_string()
    };

    // 构建消息列表：system + 历史对话 + 当前用户消息
    let mut messages = Vec::with_capacity(2 + history_len);

    // 添加系统消息
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt,
    });

    // 添加历史对话消息
    for msg in &history {
        messages.push(ChatMessage {
            role: msg.role.clone(),
            content: msg.content.clone(),
        });
    }

    // 添加当前用户消息
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_prompt.to_string(),
    });

    let req_body = ChatRequest {
        model: cfg.model.clone(),
        messages,
        max_tokens: Some(96),
        temperature: Some(0.1), // 降低温度以提高一致性
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(cfg.timeout_seconds.max(3)))
        .build()?;

    // 兼容 base_url 末尾带 / 或不带 /
    let base = cfg.base_url.trim_end_matches('/');
    let res = client
        .post(format!("{}/chat/completions", base))
        .bearer_auth(api_key)
        .json(&req_body)
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("AI rename request failed: {} {}", status, body));
    }

    let resp: ChatResponse = res.json().await?;
    let raw = resp
        .choices
        .first()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| anyhow!("No response"))?;

    // 清洗：去引号/换行，空格替换为 '-'，并做文件名安全化
    let mut name = raw.replace(['"', '\n', '\r'], "");
    name = name.replace(' ', "-");
    name = crate::utils::filenamify::filenamify(&name);

    // 避免过长（多数文件系统限制 255 bytes，这里留余量）
    if name.chars().count() > 180 {
        name = name.chars().take(180).collect();
    }

    if name.is_empty() {
        return Err(anyhow!("Empty filename"));
    }

    // 将用户消息和助手回复添加到对话历史（保存到数据库）
    if let Err(e) = add_conversation_message(db.as_ref(), source_key, "user", user_prompt).await {
        warn!("保存用户消息到数据库失败: {}", e);
    }
    if let Err(e) = add_conversation_message(db.as_ref(), source_key, "assistant", &name).await {
        warn!("保存助手回复到数据库失败: {}", e);
    }

    info!(
        "AI重命名成功(OpenAI) [{}]: {} → {}",
        source_key, current_filename, name
    );

    Ok(name)
}

/// 重命名同目录下的侧车文件（nfo/xml/srt/jpg/jpeg/png/ass等）
/// 支持复杂后缀如 -fanart.jpg, -thumb.jpg, .zh-CN.default.ass 等
/// 重命名侧车文件（NFO、字幕、封面等）
///
/// # 参数
/// - `old`: 原始文件路径（用于获取目录和原文件名基）
/// - `new_stem`: 新的文件名基（不含扩展名）
/// - `new_ext`: 新的主文件扩展名（用于排除已重命名的主文件）
pub fn rename_sidecars(old: &Path, new_stem: &str, new_ext: &str) -> Result<()> {
    let parent = old.parent().ok_or_else(|| anyhow!("Invalid path"))?;
    let stem = old
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("Invalid stem"))?;

    // 新的主文件名（需要跳过）
    let new_main_filename = format!("{}.{}", new_stem, new_ext);

    // 扫描目录中所有以旧文件名stem开头的文件
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let filename = match path.file_name().and_then(|s| s.to_str()) {
                Some(f) => f,
                None => continue,
            };

            // 跳过刚重命名的主文件
            if filename == new_main_filename {
                continue;
            }

            // 检查文件名是否以旧stem开头
            if !filename.starts_with(stem) {
                continue;
            }

            // 获取stem之后的后缀部分（如 "-fanart.jpg", ".nfo", ".zh-CN.default.ass"）
            let suffix = &filename[stem.len()..];

            // 跳过原始主视频/音频文件本身（理论上已经被重命名了，但以防万一）
            if suffix.starts_with('.') {
                let ext_lower = suffix.to_lowercase();
                if ext_lower == ".mp4" || ext_lower == ".mkv" || ext_lower == ".m4a"
                   || ext_lower == ".flv" || ext_lower == ".webm" || ext_lower == ".avi" {
                    continue;
                }
            }

            // 构建新文件名
            let new_filename = format!("{}{}", new_stem, suffix);
            let new_path = parent.join(&new_filename);

            // 如果新路径已存在则跳过
            if new_path.exists() {
                warn!("侧车文件目标已存在，跳过: {} -> {}", filename, new_filename);
                continue;
            }

            // 执行重命名
            if let Err(e) = fs::rename(&path, &new_path) {
                warn!("重命名侧车文件失败 {} -> {}: {}", filename, new_filename, e);
            } else {
                info!("重命名侧车文件: {} -> {}", filename, new_filename);
            }
        }
    }

    Ok(())
}

/// 更新NFO文件中的标题标签
///
/// # 参数
/// - `nfo_path`: NFO文件路径
/// - `new_title`: 新的标题（文件名stem）
///
/// # 说明
/// 更新NFO文件中的 <title>、<originaltitle>、<sorttitle> 标签
/// 如果新标题为空，则跳过不修改
pub fn update_nfo_content(nfo_path: &Path, new_title: &str) -> Result<()> {
    // 如果新标题为空，跳过不修改
    if new_title.trim().is_empty() {
        debug!("跳过NFO更新: 新标题为空");
        return Ok(());
    }

    // 检查NFO文件是否存在
    if !nfo_path.exists() {
        debug!("跳过NFO更新: 文件不存在 {:?}", nfo_path);
        return Ok(());
    }

    // 读取NFO文件内容
    let content = match fs::read_to_string(nfo_path) {
        Ok(c) => c,
        Err(e) => {
            warn!("读取NFO文件失败 {:?}: {}", nfo_path, e);
            return Err(anyhow!("读取NFO文件失败: {}", e));
        }
    };

    // 使用正则表达式更新标签内容
    // 匹配 <title>...</title>、<originaltitle>...</originaltitle>、<sorttitle>...</sorttitle>
    let title_re = Regex::new(r"<title>([^<]*)</title>").unwrap();
    let originaltitle_re = Regex::new(r"<originaltitle>([^<]*)</originaltitle>").unwrap();
    let sorttitle_re = Regex::new(r"<sorttitle>([^<]*)</sorttitle>").unwrap();

    let mut updated_content = content.clone();
    let mut updated = false;

    // 更新 <title> 标签（仅当原内容非空时）
    if let Some(caps) = title_re.captures(&content) {
        if !caps.get(1).map_or(true, |m| m.as_str().trim().is_empty()) {
            updated_content = title_re.replace(&updated_content, format!("<title>{}</title>", new_title)).to_string();
            updated = true;
        }
    }

    // 更新 <originaltitle> 标签（仅当原内容非空时）
    if let Some(caps) = originaltitle_re.captures(&updated_content) {
        if !caps.get(1).map_or(true, |m| m.as_str().trim().is_empty()) {
            updated_content = originaltitle_re.replace(&updated_content, format!("<originaltitle>{}</originaltitle>", new_title)).to_string();
            updated = true;
        }
    }

    // 更新 <sorttitle> 标签（仅当原内容非空时）
    if let Some(caps) = sorttitle_re.captures(&updated_content) {
        if !caps.get(1).map_or(true, |m| m.as_str().trim().is_empty()) {
            updated_content = sorttitle_re.replace(&updated_content, format!("<sorttitle>{}</sorttitle>", new_title)).to_string();
            updated = true;
        }
    }

    // 如果有更新，写回文件
    if updated {
        if let Err(e) = fs::write(nfo_path, &updated_content) {
            warn!("写入NFO文件失败 {:?}: {}", nfo_path, e);
            return Err(anyhow!("写入NFO文件失败: {}", e));
        }
        info!("更新NFO文件标题: {:?} -> {}", nfo_path, new_title);
    } else {
        debug!("NFO文件无需更新（标签为空或不存在）: {:?}", nfo_path);
    }

    Ok(())
}

/// 检测命名不一致的文件
///
/// 返回：需要重命名的文件 stem 列表
/// 注意：此功能仅支持 OpenAI 兼容 API，需要配置 api_key
/// DeepSeek Web 模式通过会话上下文自动保持一致性，无需此检查
pub async fn find_inconsistent_filenames(
    cfg: &AiRenameConfig,
    source_key: &str,
    file_stems: &[String],
) -> Result<Vec<String>> {
    if file_stems.len() < 3 {
        // 文件太少，无法判断一致性
        return Ok(Vec::new());
    }

    // DeepSeek Web 模式通过会话上下文自动保持一致性，无需额外检查
    if cfg.provider == "deepseek-web" {
        debug!("[{}] DeepSeek Web 模式使用会话上下文保持一致性，跳过一致性检查", source_key);
        return Ok(Vec::new());
    }

    // 如果未配置 api_key，静默跳过一致性检查
    let api_key = match cfg.api_key.clone() {
        Some(key) if !key.is_empty() => key,
        _ => {
            debug!("[{}] 一致性检查需要配置 api_key，跳过", source_key);
            return Ok(Vec::new());
        }
    };

    // 获取数据库连接和对话历史
    let db = crate::database::get_global_db()
        .ok_or_else(|| anyhow!("数据库连接不可用"))?;
    let history = get_conversation_history(db.as_ref(), source_key).await;

    // 构建文件列表字符串
    let file_list = file_stems
        .iter()
        .enumerate()
        .map(|(i, name)| format!("{}. {}", i + 1, name))
        .collect::<Vec<_>>()
        .join("\n");

    let user_content = format!(
        "以下是同一视频源的文件列表，请根据之前的命名风格，找出命名格式与大多数不一致的文件（异类）。\n\
只输出不一致文件的序号，用逗号分隔。如果全部一致则输出\"无\"。\n\
不要解释，不要其他内容。\n\n\
文件列表：\n{}",
        file_list
    );

    // 构建消息列表：system + 最近10条对话历史 + 当前用户消息
    let mut messages = Vec::new();

    // 添加系统消息
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: "你是一个文件命名一致性检测助手。根据之前的命名风格，分析文件名列表，找出命名格式与多数文件不同的异类。只输出序号或\"无\"。".to_string(),
    });

    // 添加最近10条对话历史（让AI参考之前的命名风格）
    let history_to_use: Vec<_> = history.iter().rev().take(10).rev().cloned().collect();
    for msg in &history_to_use {
        messages.push(ChatMessage {
            role: msg.role.clone(),
            content: msg.content.clone(),
        });
    }

    // 添加当前用户消息
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });

    let req_body = ChatRequest {
        model: cfg.model.clone(),
        messages,
        max_tokens: Some(64),
        temperature: Some(0.0),
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(cfg.timeout_seconds.max(3)))
        .build()?;

    let base = cfg.base_url.trim_end_matches('/');
    let res = client
        .post(format!("{}/chat/completions", base))
        .bearer_auth(api_key)
        .json(&req_body)
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("一致性检测请求失败: {} {}", status, body));
    }

    let resp: ChatResponse = res.json().await?;
    let raw = resp
        .choices
        .first()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| anyhow!("No response"))?;

    info!("[{}] 一致性检测结果: {}", source_key, raw);

    // 解析结果
    if raw == "无" || raw.is_empty() || raw.to_lowercase().contains("无") {
        return Ok(Vec::new());
    }

    // 解析序号列表
    let mut inconsistent = Vec::new();
    for part in raw.split([',', '，', ' ', '\n']) {
        let part = part.trim();
        if let Ok(idx) = part.parse::<usize>() {
            if idx >= 1 && idx <= file_stems.len() {
                inconsistent.push(file_stems[idx - 1].clone());
            }
        }
    }

    if !inconsistent.is_empty() {
        info!(
            "[{}] 发现 {} 个命名不一致的文件: {:?}",
            source_key,
            inconsistent.len(),
            inconsistent
        );
    }

    Ok(inconsistent)
}

/// 重命名单个不一致的文件（使用已有的对话历史作为参考）
/// 注意：此功能仅支持 OpenAI 兼容 API，需要配置 api_key
/// DeepSeek Web 模式通过会话上下文自动保持一致性，无需此功能
pub async fn rename_inconsistent_file(
    cfg: &AiRenameConfig,
    source_key: &str,
    file_path: &Path,
    video_prompt_override: &str,
    audio_prompt_override: &str,
) -> Result<std::path::PathBuf> {
    // DeepSeek Web 模式通过会话上下文自动保持一致性，无需修复
    if cfg.provider == "deepseek-web" {
        return Ok(file_path.to_path_buf());
    }

    let current_stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("Invalid file stem"))?;

    let ext = file_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp4");

    let is_audio = ext == "m4a" || ext == "mp3" || ext == "flac";

    // 如果未配置 api_key，静默跳过（返回原路径）
    let api_key = match cfg.api_key.clone() {
        Some(key) if !key.is_empty() => key,
        _ => {
            debug!("[{}] 一致性修复需要配置 api_key，跳过", source_key);
            return Ok(file_path.to_path_buf());
        }
    };

    // 获取数据库连接
    let db = crate::database::get_global_db()
        .ok_or_else(|| anyhow!("数据库连接不可用"))?;

    // 获取对话历史作为参考（从数据库）
    let history = get_conversation_history(db.as_ref(), source_key).await;
    if history.is_empty() {
        return Err(anyhow!("没有对话历史，无法确定一致的命名风格"));
    }

    let prompt = if is_audio {
        if !audio_prompt_override.is_empty() {
            audio_prompt_override
        } else {
            &cfg.audio_prompt_hint
        }
    } else {
        if !video_prompt_override.is_empty() {
            video_prompt_override
        } else {
            &cfg.video_prompt_hint
        }
    };

    let user_content = format!(
        "这个文件的命名格式与同源其他文件不一致，请根据已有的命名风格重新命名。\n\
只输出新文件名（不含扩展名），必须严格遵循之前的命名格式。\n\n\
当前文件名：{}\n\
命名结构提示：{}\n",
        current_stem, prompt
    );

    // 构建消息列表
    let mut messages = Vec::with_capacity(2 + history.len());
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: "你是一个文件命名一致性修复助手。根据之前的命名风格，为这个异类文件生成一致的新名称。只输出文件名本身。".to_string(),
    });

    // 添加历史对话
    for msg in &history {
        messages.push(ChatMessage {
            role: msg.role.clone(),
            content: msg.content.clone(),
        });
    }

    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });

    let req_body = ChatRequest {
        model: cfg.model.clone(),
        messages,
        max_tokens: Some(96),
        temperature: Some(0.0), // 零温度确保一致性
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(cfg.timeout_seconds.max(3)))
        .build()?;

    let base = cfg.base_url.trim_end_matches('/');
    let res = client
        .post(format!("{}/chat/completions", base))
        .bearer_auth(api_key)
        .json(&req_body)
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("重命名请求失败: {} {}", status, body));
    }

    let resp: ChatResponse = res.json().await?;
    let raw = resp
        .choices
        .first()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| anyhow!("No response"))?;

    // 清洗文件名
    let mut new_stem = raw.replace(['"', '\n', '\r'], "");
    new_stem = new_stem.replace(' ', "-");
    new_stem = crate::utils::filenamify::filenamify(&new_stem);

    if new_stem.chars().count() > 180 {
        new_stem = new_stem.chars().take(180).collect();
    }

    if new_stem.is_empty() {
        return Err(anyhow!("生成的文件名为空"));
    }

    // 如果生成的文件名与当前相同，说明AI认为当前命名已经是一致的，跳过
    if new_stem == current_stem {
        info!(
            "[{}] 文件名已一致，无需修改: {}",
            source_key, current_stem
        );
        return Ok(file_path.to_path_buf());
    }

    // 执行重命名
    let new_path = file_path.with_file_name(format!("{}.{}", new_stem, ext));
    fs::rename(file_path, &new_path)?;

    // 重命名侧车文件
    if let Err(e) = rename_sidecars(file_path, &new_stem, ext) {
        warn!("重命名侧车文件失败: {}", e);
    }

    info!(
        "[{}] 一致性修复: {} → {}",
        source_key,
        current_stem,
        new_stem
    );

    Ok(new_path)
}

/// 批量重命名结果
#[derive(Debug, Default)]
pub struct BatchRenameResult {
    pub renamed_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
}

/// 待重命名的文件信息
#[derive(Clone)]
pub struct FileToRename {
    /// 原始文件路径
    pub path: std::path::PathBuf,
    /// 当前文件名（不含扩展名）
    pub current_stem: String,
    /// 扩展名
    pub ext: String,
    /// AI 上下文
    pub ctx: AiRenameContext,
    /// page.id（用于更新数据库）
    pub page_id: i32,
    /// video.id（用于更新数据库）
    pub video_id: i32,
    /// video bvid（用于冲突时追加）
    pub bvid: String,
    /// 是否为单P视频
    pub single_page: bool,
    /// 是否为平铺目录模式（启用时不重命名子文件夹）
    pub flat_folder: bool,
}

/// 批量生成文件名（一次请求处理多个文件）
///
/// # 参数
/// - `cfg`: AI 重命名配置
/// - `source_key`: 视频源标识
/// - `files`: 待重命名的文件列表
/// - `prompt_hint`: 命名提示词
///
/// # 返回
/// - 新文件名列表（与输入顺序对应）
pub async fn ai_generate_filenames_batch(
    cfg: &AiRenameConfig,
    source_key: &str,
    files: &[FileToRename],
    prompt_hint: &str,
) -> Result<Vec<String>> {
    if files.is_empty() {
        return Ok(Vec::new());
    }

    // 构建批量 prompt
    let mut file_list = String::new();
    for (i, file) in files.iter().enumerate() {
        let video_info = file.ctx.to_json_string();
        file_list.push_str(&format!(
            "{}. 当前文件名: {}\n   视频信息: {}\n\n",
            i + 1,
            file.current_stem,
            video_info.replace('\n', " ") // 压缩为单行
        ));
    }

    let full_prompt = format!(
        "请为以下 {} 个视频文件生成新的文件名。\n\
        【重要】严格按照用户指定的命名格式生成，不要添加格式中未要求的任何信息！\n\
        严格按照 JSON 数组格式返回，只输出文件名（不含扩展名），不要解释：\n\
        [\"文件名1\", \"文件名2\", ...]\n\n\
        用户指定的命名格式：{}\n\n\
        文件列表（仅供参考，只提取格式中需要的字段；如信息缺失可访问API参考链接获取）：\n{}",
        files.len(),
        prompt_hint,
        file_list
    );

    // 根据 provider 选择实现
    if cfg.provider == "deepseek-web" {
        ai_generate_filenames_batch_deepseek_web(cfg, source_key, &full_prompt, files.len()).await
    } else {
        ai_generate_filenames_batch_openai(cfg, source_key, &full_prompt, files.len()).await
    }
}

/// DeepSeek Web 批量生成
async fn ai_generate_filenames_batch_deepseek_web(
    cfg: &AiRenameConfig,
    source_key: &str,
    prompt: &str,
    expected_count: usize,
) -> Result<Vec<String>> {
    let _lock = AI_RENAME_LOCK.lock().await;

    let token = cfg.deepseek_web_token.clone()
        .ok_or_else(|| anyhow!("DeepSeek Web Token 未配置"))?;

    let db = crate::database::get_global_db()
        .ok_or_else(|| anyhow!("数据库连接不可用"))?;

    // 从缓存获取会话
    let cached_session = {
        let cache = DEEPSEEK_SESSION_CACHE.lock().await;
        if let Some(session) = cache.get(source_key).cloned() {
            info!("会话缓存命中（内存）: source_key='{}', session_id='{}'", source_key, session.session_id);
            Some(session)
        } else {
            drop(cache);
            if let Some(session) = load_deepseek_session(db.as_ref(), source_key).await {
                info!("会话缓存命中（数据库）: source_key='{}', session_id='{}'", source_key, session.session_id);
                let mut cache = DEEPSEEK_SESSION_CACHE.lock().await;
                cache.insert(source_key.to_string(), session.clone());
                Some(session)
            } else {
                info!("会话缓存未命中: source_key='{}'，将创建新会话", source_key);
                None
            }
        }
    };

    // 调用 DeepSeek Web API（使用原始响应，不清洗）
    let result = super::deepseek_web::deepseek_web_generate_raw(
        &token,
        cached_session,
        prompt,
        cfg.timeout_seconds,
    ).await;

    // 检查是否是会话长度上限错误，需要重建会话
    let (response, new_session) = match result {
        Ok(res) => res,
        Err(e) if e.to_string().contains("SESSION_LIMIT_REACHED") => {
            warn!("[{}] DeepSeek 会话达到长度上限，正在重建会话并带上历史...", source_key);

            // 清除旧会话缓存
            {
                let mut cache = DEEPSEEK_SESSION_CACHE.lock().await;
                cache.remove(source_key);
            }

            // 获取历史记录作为上下文
            let history = get_conversation_history(db.as_ref(), source_key).await;
            let history_context = if !history.is_empty() {
                let mut ctx = String::from("【之前的命名风格参考】\n");
                for msg in &history {
                    if msg.role == "assistant" {
                        // 只保留 assistant 的回复作为命名风格参考
                        ctx.push_str(&format!("{}\n", msg.content));
                    }
                }
                ctx.push_str("\n请严格遵循以上命名风格。\n\n");
                ctx
            } else {
                String::new()
            };

            // 构建带历史上下文的新 prompt
            let new_prompt = format!("{}{}", history_context, prompt);

            // 用新会话重试（session = None 会创建新会话）
            info!("[{}] 使用新会话重试，带上 {} 条历史记录", source_key, history.len());
            super::deepseek_web::deepseek_web_generate_raw(
                &token,
                None, // 创建新会话
                &new_prompt,
                cfg.timeout_seconds,
            ).await?
        }
        Err(e) if e.to_string().contains("读取响应体失败") || e.to_string().contains("error decoding response body") => {
            warn!("[{}] DeepSeek 响应体解码失败，正在重建会话并带上历史...", source_key);

            // 清除旧会话缓存
            {
                let mut cache = DEEPSEEK_SESSION_CACHE.lock().await;
                cache.remove(source_key);
            }

            // 获取历史记录作为上下文
            let history = get_conversation_history(db.as_ref(), source_key).await;
            let history_context = if !history.is_empty() {
                let mut ctx = String::from("【之前的命名风格参考】\n");
                for msg in &history {
                    if msg.role == "assistant" {
                        ctx.push_str(&format!("{}\n", msg.content));
                    }
                }
                ctx.push_str("\n请严格遵循以上命名风格。\n\n");
                ctx
            } else {
                String::new()
            };

            // 构建带历史上下文的新 prompt
            let new_prompt = format!("{}{}", history_context, prompt);

            // 用新会话重试
            info!("[{}] 使用新会话重试（响应体错误），带上 {} 条历史记录", source_key, history.len());
            super::deepseek_web::deepseek_web_generate_raw(
                &token,
                None,
                &new_prompt,
                cfg.timeout_seconds,
            ).await?
        }
        Err(e) => return Err(e),
    };

    // 更新会话缓存
    {
        let mut cache = DEEPSEEK_SESSION_CACHE.lock().await;
        cache.insert(source_key.to_string(), new_session.clone());
    }
    if let Err(e) = save_deepseek_session(db.as_ref(), source_key, &new_session).await {
        warn!("保存 DeepSeek 会话到数据库失败: {}", e);
    }

    // 保存简化的对话历史（供一致性检查参考命名风格）
    let simplified_user_msg = format!("为{}个文件生成命名", expected_count);
    if let Err(e) = add_conversation_message(db.as_ref(), source_key, "user", &simplified_user_msg).await {
        warn!("保存用户消息失败: {}", e);
    }
    // 解析响应并保存文件名列表
    let parsed_names = parse_batch_response(&response, expected_count);
    if let Ok(ref names) = parsed_names {
        let simplified_response = names.join("\n");
        if let Err(e) = add_conversation_message(db.as_ref(), source_key, "assistant", &simplified_response).await {
            warn!("保存助手回复失败: {}", e);
        }
    }

    parsed_names
}

/// OpenAI 兼容 API 批量生成
async fn ai_generate_filenames_batch_openai(
    cfg: &AiRenameConfig,
    source_key: &str,
    prompt: &str,
    expected_count: usize,
) -> Result<Vec<String>> {
    let api_key = cfg.api_key.clone().ok_or_else(|| anyhow!("API key missing"))?;

    let db = crate::database::get_global_db()
        .ok_or_else(|| anyhow!("数据库连接不可用"))?;

    let history = get_conversation_history(db.as_ref(), source_key).await;

    let system_prompt = if history.is_empty() {
        "你是一个文件命名助手。返回 JSON 数组格式的文件名列表，不要解释。".to_string()
    } else {
        "你是一个文件命名助手。严格遵循之前的命名风格，返回 JSON 数组格式。".to_string()
    };

    let mut messages = Vec::with_capacity(2 + history.len());
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt,
    });
    for msg in &history {
        messages.push(ChatMessage {
            role: msg.role.clone(),
            content: msg.content.clone(),
        });
    }
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: prompt.to_string(),
    });

    let req_body = ChatRequest {
        model: cfg.model.clone(),
        messages,
        max_tokens: Some(512), // 批量需要更多 token
        temperature: Some(0.1),
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(cfg.timeout_seconds.max(30)))
        .build()?;

    let base = cfg.base_url.trim_end_matches('/');
    let res = client
        .post(format!("{}/chat/completions", base))
        .bearer_auth(api_key)
        .json(&req_body)
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("批量 AI 请求失败: {} {}", status, body));
    }

    let resp: ChatResponse = res.json().await?;
    let raw = resp
        .choices
        .first()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| anyhow!("No response"))?;

    // 保存简化的对话历史（只保存生成的文件名列表，作为命名风格参考）
    // 不保存完整的 prompt（太长），只保存一个简短的用户消息和生成的文件名
    let simplified_user_msg = format!("为{}个文件生成命名", expected_count);
    if let Err(e) = add_conversation_message(db.as_ref(), source_key, "user", &simplified_user_msg).await {
        warn!("保存用户消息失败: {}", e);
    }
    // 保存生成的文件名列表（JSON格式转为简单列表格式）
    let simplified_response = if let Ok(names) = serde_json::from_str::<Vec<String>>(&raw) {
        names.join("\n")
    } else {
        raw.clone()
    };
    if let Err(e) = add_conversation_message(db.as_ref(), source_key, "assistant", &simplified_response).await {
        warn!("保存助手回复失败: {}", e);
    }

    parse_batch_response(&raw, expected_count)
}

/// 解析批量响应的 JSON 数组
fn parse_batch_response(response: &str, expected_count: usize) -> Result<Vec<String>> {
    // 尝试提取 JSON 数组
    let json_str = if let Some(start) = response.find('[') {
        if let Some(end) = response.rfind(']') {
            &response[start..=end]
        } else {
            response
        }
    } else {
        response
    };

    // 解析 JSON
    let names: Vec<String> = serde_json::from_str(json_str)
        .map_err(|e| anyhow!("解析 JSON 数组失败: {} - 原始响应: {}", e, response))?;

    if names.len() != expected_count {
        warn!(
            "AI 返回数量不匹配: 期望 {}, 实际 {}",
            expected_count, names.len()
        );
    }

    // 清洗文件名
    let cleaned: Vec<String> = names
        .into_iter()
        .map(|name| {
            let mut n = name.replace(['"', '\n', '\r'], "");
            n = n.replace(' ', "-");
            n = crate::utils::filenamify::filenamify(&n);
            if n.chars().count() > 180 {
                n = n.chars().take(180).collect();
            }
            n
        })
        .collect();

    Ok(cleaned)
}

/// 批量重命名视频源下的历史文件
///
/// # 参数
/// - `connection`: 数据库连接
/// - `source_key`: 视频源唯一标识（如 "collection_123"）
/// - `videos`: 视频和其分页列表（已下载的）
/// - `config`: AI 重命名配置
/// - `video_prompt`: 视频自定义提示词
/// - `audio_prompt`: 音频自定义提示词
/// - `flat_folder`: 是否为平铺目录模式
///
/// # 返回
/// - 批量重命名结果（renamed_count, skipped_count, failed_count）
pub async fn batch_rename_history_files(
    connection: &DatabaseConnection,
    source_key: &str,
    videos: Vec<(bili_sync_entity::video::Model, Vec<bili_sync_entity::page::Model>)>,
    config: &AiRenameConfig,
    video_prompt: &str,
    audio_prompt: &str,
    flat_folder: bool,
) -> Result<BatchRenameResult> {
    let mut result = BatchRenameResult::default();

    // 第一步：收集所有需要重命名的文件
    let mut video_files: Vec<FileToRename> = Vec::new();
    let mut audio_files: Vec<FileToRename> = Vec::new();

    info!(
        "[{}] 开始批量重命名，共 {} 个视频",
        source_key,
        videos.len()
    );

    // 跟踪视频和音频的排序位置
    let mut video_sort_index = 1;
    let mut audio_sort_index = 1;

    for (video, pages) in &videos {
        for page_model in pages {
            // 检查 page.path 是否存在
            let page_path = match &page_model.path {
                Some(p) if !p.is_empty() => p.clone(),
                _ => {
                    debug!("[{}] 跳过 page_id={}: path 为空", source_key, page_model.id);
                    result.skipped_count += 1;
                    continue;
                }
            };

            // 检查文件是否存在
            let file_path = Path::new(&page_path);
            if !file_path.exists() {
                debug!("[{}] 跳过 page_id={}: 文件不存在 path={}", source_key, page_model.id, page_path);
                result.skipped_count += 1;
                continue;
            }

            // 获取当前文件名和扩展名
            let current_stem = match file_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => {
                    result.failed_count += 1;
                    continue;
                }
            };

            let ext = file_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("mp4")
                .to_string();

            let is_audio = matches!(ext.as_str(), "m4a" | "mp3" | "flac" | "aac" | "ogg");

            // 根据文件类型获取排序位置
            let current_sort_index = if is_audio {
                let idx = audio_sort_index;
                audio_sort_index += 1;
                idx
            } else {
                let idx = video_sort_index;
                video_sort_index += 1;
                idx
            };

            let ctx = AiRenameContext {
                title: video.name.clone(),
                desc: video.intro.clone(),
                owner: video.upper_name.clone(),
                tname: video.category.to_string(),
                duration: 0,
                pubdate: video.pubtime.format("%Y-%m-%d").to_string(),
                dimension: String::new(),
                part_name: page_model.name.clone(),
                ugc_season: None,
                copyright: String::new(),
                view: 0,
                pid: page_model.pid,
                episode_number: None,
                source_type: source_key.split('_').next().unwrap_or("unknown").to_string(),
                is_audio,
                sort_index: Some(current_sort_index),
                bvid: video.bvid.clone(),
            };

            let file_info = FileToRename {
                path: file_path.to_path_buf(),
                current_stem,
                ext,
                ctx,
                page_id: page_model.id,
                video_id: video.id,
                bvid: video.bvid.clone(),
                single_page: video.single_page.unwrap_or(true),
                flat_folder,
            };

            if is_audio {
                audio_files.push(file_info);
            } else {
                video_files.push(file_info);
            }
        }
    }

    info!(
        "[{}] 收集完成: {} 个视频文件, {} 个音频文件",
        source_key,
        video_files.len(),
        audio_files.len()
    );

    // 第二步：按批次处理视频文件（每批 10 个）
    let batch_size = 10;
    let video_prompt_hint = if !video_prompt.is_empty() {
        video_prompt
    } else {
        &config.video_prompt_hint
    };
    let audio_prompt_hint = if !audio_prompt.is_empty() {
        audio_prompt
    } else {
        &config.audio_prompt_hint
    };

    // 处理视频文件
    for (batch_idx, batch) in video_files.chunks(batch_size).enumerate() {
        info!(
            "[{}] 处理视频批次 {}/{}: {} 个文件",
            source_key,
            batch_idx + 1,
            (video_files.len() + batch_size - 1) / batch_size,
            batch.len()
        );

        match ai_generate_filenames_batch(config, source_key, batch, video_prompt_hint).await {
            Ok(new_names) => {
                for (file, new_stem) in batch.iter().zip(new_names.iter()) {
                    apply_rename(connection, source_key, file, new_stem, &mut result).await;
                }
            }
            Err(e) => {
                warn!("[{}] 视频批次 {} 处理失败: {}", source_key, batch_idx + 1, e);
                result.failed_count += batch.len();
            }
        }
    }

    // 处理音频文件
    for (batch_idx, batch) in audio_files.chunks(batch_size).enumerate() {
        info!(
            "[{}] 处理音频批次 {}/{}: {} 个文件",
            source_key,
            batch_idx + 1,
            (audio_files.len() + batch_size - 1) / batch_size,
            batch.len()
        );

        match ai_generate_filenames_batch(config, source_key, batch, audio_prompt_hint).await {
            Ok(new_names) => {
                for (file, new_stem) in batch.iter().zip(new_names.iter()) {
                    apply_rename(connection, source_key, file, new_stem, &mut result).await;
                }
            }
            Err(e) => {
                warn!("[{}] 音频批次 {} 处理失败: {}", source_key, batch_idx + 1, e);
                result.failed_count += batch.len();
            }
        }
    }

    info!(
        "[{}] 批量重命名完成: 重命名 {} 个, 跳过 {} 个, 失败 {} 个",
        source_key, result.renamed_count, result.skipped_count, result.failed_count
    );

    Ok(result)
}

/// 应用单个文件的重命名
async fn apply_rename(
    connection: &DatabaseConnection,
    source_key: &str,
    file: &FileToRename,
    new_stem: &str,
    result: &mut BatchRenameResult,
) {
    use bili_sync_entity::{page, video};

    // 文件名相同则跳过
    if new_stem == file.current_stem {
        info!("[{}] 跳过(文件名相同): '{}'", source_key, new_stem);
        result.skipped_count += 1;
        return;
    }

    // 新文件名为空则跳过
    if new_stem.is_empty() {
        info!("[{}] 跳过(AI返回空): 原文件名 '{}'", source_key, file.current_stem);
        result.skipped_count += 1;
        return;
    }

    // 构建新路径（处理重复文件名）
    let parent = file.path.parent().unwrap_or(Path::new("."));
    let mut final_stem = new_stem.to_string();
    let mut new_filename = format!("{}.{}", final_stem, file.ext);
    let mut new_path = parent.join(&new_filename);

    // 如果目标文件已存在，添加后缀使其唯一
    let mut suffix = 1;
    while new_path.exists() {
        final_stem = format!("{}-{}", new_stem, suffix);
        new_filename = format!("{}.{}", final_stem, file.ext);
        new_path = parent.join(&new_filename);
        suffix += 1;
        if suffix > 99 {
            // 防止无限循环
            info!(
                "[{}] 跳过(无法生成唯一文件名): {} -> {}",
                source_key, file.current_stem, new_stem
            );
            result.skipped_count += 1;
            return;
        }
    }

    // 如果添加了后缀，记录日志
    if suffix > 1 {
        info!(
            "[{}] 检测到重复文件名，自动添加后缀: {} -> {}",
            source_key, new_stem, final_stem
        );
    }

    // 执行文件重命名
    if let Err(e) = fs::rename(&file.path, &new_path) {
        warn!(
            "[{}] 重命名文件失败: {} -> {} - {}",
            source_key, file.path.display(), new_path.display(), e
        );
        result.failed_count += 1;
        return;
    }

    info!(
        "[{}] 重命名成功: {} -> {}",
        source_key, file.current_stem, final_stem
    );

    // 重命名侧车文件
    if let Err(e) = rename_sidecars(&file.path, &final_stem, &file.ext) {
        warn!("[{}] 重命名侧车文件失败: {}", source_key, e);
    }

    // 更新NFO文件内容（标题标签）
    let nfo_path = parent.join(format!("{}.nfo", final_stem));
    if let Err(e) = update_nfo_content(&nfo_path, &final_stem) {
        warn!("[{}] 更新NFO内容失败: {}", source_key, e);
    }

    // 重命名子文件夹（单P视频 且 非平铺目录模式）
    let should_rename_folder = !file.flat_folder && file.single_page;

    let final_path = if should_rename_folder {
        if let Some(old_dir) = new_path.parent() {
            if let Some(parent_dir) = old_dir.parent() {
                let mut target_dir = parent_dir.join(&final_stem);
                // 如果目标目录已存在且不是当前目录，追加 bvid 避免冲突
                if target_dir.exists() && target_dir != old_dir {
                    target_dir = parent_dir.join(format!("{}-{}", &final_stem, file.bvid));
                }

                if target_dir != old_dir {
                    match std::fs::rename(old_dir, &target_dir) {
                        Ok(_) => {
                            let moved_path = target_dir.join(
                                new_path.file_name().expect("new_path should have file name"),
                            );
                            info!(
                                "[{}] AI 重命名子文件夹成功: {} -> {}",
                                source_key,
                                old_dir.display(),
                                target_dir.display()
                            );

                            // 更新当前 video.path
                            let new_video_path = target_dir.to_string_lossy().to_string();
                            if let Ok(Some(current_video)) = video::Entity::find_by_id(file.video_id).one(connection).await {
                                let mut active_video: video::ActiveModel = current_video.into();
                                active_video.path = Set(new_video_path.clone());
                                if let Err(e) = active_video.update(connection).await {
                                    warn!("[{}] 更新 video.path 失败: {}", source_key, e);
                                }
                            }

                            // 更新同一文件夹中其他视频的路径
                            let old_dir_str = old_dir.to_string_lossy().to_string();
                            let old_dir_str_alt = old_dir_str.replace('/', "\\");

                            if let Ok(other_videos) = video::Entity::find()
                                .filter(video::Column::Id.ne(file.video_id))
                                .filter(
                                    video::Column::Path.eq(&old_dir_str)
                                        .or(video::Column::Path.eq(&old_dir_str_alt))
                                )
                                .all(connection)
                                .await
                            {
                                for other_video in other_videos {
                                    let mut active_other: video::ActiveModel = other_video.clone().into();
                                    active_other.path = Set(new_video_path.clone());
                                    if let Err(e) = active_other.update(connection).await {
                                        warn!("[{}] 更新同文件夹其他视频 video.path 失败: {}", source_key, e);
                                    }

                                    // 更新其他视频的 page 路径
                                    if let Ok(other_pages) = page::Entity::find()
                                        .filter(page::Column::VideoId.eq(other_video.id))
                                        .all(connection)
                                        .await
                                    {
                                        for other_page in other_pages {
                                            if let Some(page_path_str) = other_page.path.clone() {
                                                if page_path_str.starts_with(&old_dir_str) || page_path_str.starts_with(&old_dir_str_alt) {
                                                    let new_page_path = if page_path_str.starts_with(&old_dir_str) {
                                                        page_path_str.replacen(&old_dir_str, &new_video_path, 1)
                                                    } else {
                                                        page_path_str.replacen(&old_dir_str_alt, &new_video_path, 1)
                                                    };
                                                    let mut active_page: page::ActiveModel = other_page.into();
                                                    active_page.path = Set(Some(new_page_path.clone()));
                                                    if let Err(e) = active_page.update(connection).await {
                                                        warn!("[{}] 更新同文件夹其他视频 page.path 失败: {}", source_key, e);
                                                    } else {
                                                        info!("[{}] 同步更新同文件夹页面路径: {} -> {}", source_key, page_path_str, new_page_path);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            moved_path
                        }
                        Err(e) => {
                            warn!("[{}] AI 重命名子文件夹失败: {}", source_key, e);
                            new_path.clone()
                        }
                    }
                } else {
                    new_path.clone()
                }
            } else {
                new_path.clone()
            }
        } else {
            new_path.clone()
        }
    } else {
        new_path.clone()
    };

    // 更新数据库中的 page.path 和 ai_renamed 标记
    let new_path_str = final_path.to_string_lossy().to_string();
    if let Ok(page_model) = page::Entity::find_by_id(file.page_id).one(connection).await {
        if let Some(page_model) = page_model {
            let mut active_page: page::ActiveModel = page_model.into();
            active_page.path = Set(Some(new_path_str));
            active_page.ai_renamed = Set(Some(1)); // 标记为已 AI 重命名，防止重复处理
            if let Err(e) = active_page.update(connection).await {
                warn!("[{}] 更新数据库 page.path 失败: {}", source_key, e);
            }
        }
    }

    result.renamed_count += 1;
}
