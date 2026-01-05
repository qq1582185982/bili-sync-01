use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
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
}

impl AiRenameContext {
    /// 构建发送给 AI 的 JSON 信息
    pub fn to_json_string(&self) -> String {
        let mut info = serde_json::json!({
            "标题": self.title,
            "UP主": self.owner,
            "来源": self.source_type,
        });

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

        serde_json::to_string_pretty(&info).unwrap_or_default()
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
    /// 是否启用 R1 深度思考模式 - 仅 deepseek-web 有效
    #[serde(default)]
    pub thinking_enabled: bool,
    /// 模型名，例如 gpt-4o-mini / deepseek-chat
    pub model: String,
    /// 请求超时（秒）
    pub timeout_seconds: u64,
    /// 视频提示词（不含扩展名）
    pub video_prompt_hint: String,
    /// 音频提示词（不含扩展名）
    pub audio_prompt_hint: String,
}

impl Default for AiRenameConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "custom".to_string(),
            base_url: "https://api.deepseek.com/v1".to_string(),
            api_key: None,
            deepseek_web_token: None,
            thinking_enabled: false,
            model: "deepseek-chat".to_string(),
            timeout_seconds: 20,
            // 视频命名规则
            video_prompt_hint: "【命名结构】精简标题-作者-时间(YYYYMMDD)；\
【标题规则】仅保留核心主题，去除修饰性/情绪性/营销性词语，不使用表情；\
【符号规则】仅用英文连字符-，禁止其他特殊符号".to_string(),
            // 音频命名规则
            audio_prompt_hint: "【命名结构】歌手-歌名-版本(如录音棚/现场)-时间(YYYYMMDD)；\
【规则】去除表情/情绪文案，仅用英文连字符-连接".to_string(),
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
        "根据以下视频信息生成新的文件名（只输出文件名，不含扩展名，不要解释，不要引号）：\n\n\
当前文件名：{}\n\n\
视频信息：\n{}\n\n\
命名结构提示：{}\n",
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
    let (name, new_session) = deepseek_web_generate(
        &token,
        cached_session,
        &full_prompt,
        cfg.thinking_enabled,
        cfg.timeout_seconds,
    ).await?;

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
pub fn rename_sidecars(old: &Path, new_stem: &str) -> Result<()> {
    let parent = old.parent().ok_or_else(|| anyhow!("Invalid path"))?;
    let stem = old
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("Invalid stem"))?;

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

            // 检查文件名是否以旧stem开头
            if !filename.starts_with(stem) {
                continue;
            }

            // 获取stem之后的后缀部分（如 "-fanart.jpg", ".nfo", ".zh-CN.default.ass"）
            let suffix = &filename[stem.len()..];

            // 跳过主视频/音频文件本身（已经被重命名了）
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

    // 构建文件列表字符串
    let file_list = file_stems
        .iter()
        .enumerate()
        .map(|(i, name)| format!("{}. {}", i + 1, name))
        .collect::<Vec<_>>()
        .join("\n");

    let user_content = format!(
        "以下是同一视频源的文件列表，请找出命名格式与大多数不一致的文件（异类）。\n\
只输出不一致文件的序号，用逗号分隔。如果全部一致则输出\"无\"。\n\
不要解释，不要其他内容。\n\n\
文件列表：\n{}",
        file_list
    );

    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "你是一个文件命名一致性检测助手。分析文件名列表，找出命名格式与多数文件不同的异类。只输出序号或\"无\"。".to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_content,
        },
    ];

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
    if let Err(e) = rename_sidecars(file_path, &new_stem) {
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
