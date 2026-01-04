use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// 对话消息（用于存储历史）
#[derive(Clone, Debug)]
struct ConversationMessage {
    role: String,
    content: String,
}

// 每个视频源的对话历史缓存
// Key: 视频源唯一标识（如 "collection_123"）
// Value: 对话历史消息列表
lazy_static::lazy_static! {
    static ref CONVERSATION_HISTORY_CACHE: Mutex<HashMap<String, Vec<ConversationMessage>>> = Mutex::new(HashMap::new());
}

/// 清除指定视频源的对话历史缓存
pub fn clear_naming_cache(source_key: &str) {
    if let Ok(mut cache) = CONVERSATION_HISTORY_CACHE.lock() {
        cache.remove(source_key);
        info!("已清除视频源 {} 的AI对话历史", source_key);
    }
}

/// 清除所有对话历史缓存
pub fn clear_all_naming_cache() {
    if let Ok(mut cache) = CONVERSATION_HISTORY_CACHE.lock() {
        cache.clear();
        info!("已清除所有AI对话历史缓存");
    }
}

/// 添加对话消息到历史
fn add_conversation_message(source_key: &str, role: &str, content: &str) {
    if let Ok(mut cache) = CONVERSATION_HISTORY_CACHE.lock() {
        let history = cache.entry(source_key.to_string()).or_insert_with(Vec::new);
        // 最多保留最近 20 条消息（10轮对话）
        if history.len() >= 20 {
            // 移除最早的一对消息（user + assistant）
            history.drain(0..2);
        }
        history.push(ConversationMessage {
            role: role.to_string(),
            content: content.to_string(),
        });
    }
}

/// 获取对话历史
fn get_conversation_history(source_key: &str) -> Vec<ConversationMessage> {
    if let Ok(cache) = CONVERSATION_HISTORY_CACHE.lock() {
        cache.get(source_key).cloned().unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// AI 重命名全局配置（存储在 Config 中）
///
/// 说明：这里走 **OpenAI 兼容** 的 chat/completions 接口（DeepSeek / OpenAI / 其它兼容服务都可）。
/// 如果 api_key 为空，会直接返回错误，由调用方决定是否跳过。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiRenameConfig {
    /// 是否启用 AI 重命名（全局开关）
    pub enabled: bool,
    /// 仅用于前端展示/区分（openai / deepseek / custom）
    pub provider: String,
    /// OpenAI 兼容接口 base url，例如：
    /// - https://api.openai.com/v1
    /// - https://api.deepseek.com/v1
    pub base_url: String,
    /// API Key（用户自备）
    pub api_key: Option<String>,
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
            model: "deepseek-chat".to_string(),
            timeout_seconds: 20,
            // 示例：作者-标题-来源-清晰度-HDR
            video_prompt_hint: "作者-标题-来源-清晰度-HDR(如有)；去掉\"原创/赏析片\"等冗余词；只保留关键信息；用 - 连接".to_string(),
            // 示例：歌手-歌名-版本/栏目
            audio_prompt_hint: "歌手-歌名-版本信息(如\"录音棚\"/\"现场\")；删除表情/情绪文案；只保留关键信息；用 - 连接".to_string(),
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

/// 调用 OpenAI 兼容接口生成新文件名（仅返回 stem，不含扩展名）
///
/// # 参数
/// - `cfg`: 全局AI重命名配置
/// - `source_key`: 视频源唯一标识（如 "collection_123"），用于保持同一源的对话上下文
/// - `title`: 视频标题
/// - `author`: 作者名
/// - `source`: 来源类型（收藏夹/合集/投稿等）
/// - `quality`: 清晰度信息
/// - `is_audio`: 是否为音频文件
/// - `current_filename`: 当前文件名（不含扩展名，可能包含剧集编号等信息）
/// - `video_prompt_override`: 视频源自定义视频提示词（非空时覆盖全局配置）
/// - `audio_prompt_override`: 视频源自定义音频提示词（非空时覆盖全局配置）
pub async fn ai_generate_filename(
    cfg: &AiRenameConfig,
    source_key: &str,
    title: &str,
    author: &str,
    source: &str,
    quality: &str,
    is_audio: bool,
    current_filename: &str,
    video_prompt_override: &str,
    audio_prompt_override: &str,
) -> Result<String> {
    let api_key = cfg.api_key.clone().ok_or_else(|| anyhow!("API key missing"))?;

    // 获取对话历史
    let history = get_conversation_history(source_key);
    let history_len = history.len();

    // 调试日志
    debug!(
        "AI重命名调用 - source_key: '{}', 当前文件名: '{}', 对话历史: {}条消息",
        source_key, current_filename, history_len
    );

    // 优先使用视频源自定义提示词，如果为空则使用全局配置
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

    // 构建用户消息内容
    let user_content = format!(
        "根据以下内容生成新的文件名（只输出文件名，不含扩展名，不要解释，不要引号）：\n\
当前文件名：{}\n\
原视频标题：{}\n\
作者：{}\n\
来源：{}\n\
清晰度：{}\n\
命名结构提示：{}\n",
        current_filename, title, author, source, quality, prompt
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
        content: user_content.clone(),
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

    // 将用户消息和助手回复添加到对话历史
    add_conversation_message(source_key, "user", &user_content);
    add_conversation_message(source_key, "assistant", &name);

    info!(
        "AI重命名成功 [{}]: {} → {}",
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
pub async fn find_inconsistent_filenames(
    cfg: &AiRenameConfig,
    source_key: &str,
    file_stems: &[String],
) -> Result<Vec<String>> {
    if file_stems.len() < 3 {
        // 文件太少，无法判断一致性
        return Ok(Vec::new());
    }

    let api_key = cfg.api_key.clone().ok_or_else(|| anyhow!("API key missing"))?;

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
pub async fn rename_inconsistent_file(
    cfg: &AiRenameConfig,
    source_key: &str,
    file_path: &Path,
    video_prompt_override: &str,
    audio_prompt_override: &str,
) -> Result<std::path::PathBuf> {
    let current_stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("Invalid file stem"))?;

    let ext = file_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp4");

    let is_audio = ext == "m4a" || ext == "mp3" || ext == "flac";

    // 构建重命名请求，强调必须与现有风格一致
    let api_key = cfg.api_key.clone().ok_or_else(|| anyhow!("API key missing"))?;

    // 获取对话历史作为参考
    let history = get_conversation_history(source_key);
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

    if new_stem.is_empty() || new_stem == current_stem {
        return Err(anyhow!("生成的文件名无效或相同"));
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
