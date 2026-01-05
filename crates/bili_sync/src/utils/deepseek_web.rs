//! DeepSeek Web API 客户端
//!
//! 使用 chat.deepseek.com 免费 Web API 进行 AI 聊天

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info};

use super::deepseek_pow::{build_pow_response, encode_pow_header, solve_pow, PowChallenge};

const BASE_URL: &str = "https://chat.deepseek.com";
const APP_VERSION: &str = "20241129.1";

/// DeepSeek Web 会话信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekSession {
    /// 会话 ID
    pub session_id: String,
    /// 最后一条消息 ID（用于连续对话）
    pub parent_message_id: Option<String>,
}

/// API 响应包装
#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    code: i32,
    data: Option<ApiData<T>>,
    msg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiData<T> {
    biz_data: T,
}

/// 创建会话响应
#[derive(Debug, Deserialize)]
struct CreateSessionResponse {
    id: String,
}

/// POW 挑战响应
#[derive(Debug, Deserialize)]
struct PowChallengeResponse {
    challenge: PowChallenge,
}

/// DeepSeek Web API 客户端
pub struct DeepSeekWebClient {
    client: Client,
    token: String,
}

impl DeepSeekWebClient {
    /// 创建新的客户端
    pub fn new(token: &str, timeout_seconds: u64) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds.max(10)))
            .build()?;

        Ok(Self {
            client,
            token: token.to_string(),
        })
    }

    /// 获取默认请求头
    fn get_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            "*/*".parse().unwrap(),
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );
        headers.insert(
            reqwest::header::ORIGIN,
            BASE_URL.parse().unwrap(),
        );
        headers.insert(
            reqwest::header::REFERER,
            format!("{}/", BASE_URL).parse().unwrap(),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".parse().unwrap(),
        );
        headers.insert(
            "x-app-version",
            APP_VERSION.parse().unwrap(),
        );
        headers.insert(
            "x-client-locale",
            "zh_CN".parse().unwrap(),
        );
        headers.insert(
            "x-client-platform",
            "web".parse().unwrap(),
        );
        headers.insert(
            "x-client-version",
            "1.6.0".parse().unwrap(),
        );
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", self.token).parse().unwrap(),
        );
        headers
    }

    /// 创建新会话
    pub async fn create_session(&self) -> Result<String> {
        debug!("创建 DeepSeek 会话...");

        let resp = self
            .client
            .post(format!("{}/api/v0/chat_session/create", BASE_URL))
            .headers(self.get_headers())
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("创建会话失败: HTTP {} - {}", status, body));
        }

        let data: ApiResponse<CreateSessionResponse> = resp.json().await?;

        if data.code != 0 {
            return Err(anyhow!(
                "创建会话失败: code={}, msg={}",
                data.code,
                data.msg.unwrap_or_default()
            ));
        }

        let session_id = data
            .data
            .ok_or_else(|| anyhow!("创建会话响应无数据"))?
            .biz_data
            .id;

        info!("DeepSeek 会话创建成功: {}", session_id);
        Ok(session_id)
    }

    /// 获取 POW 挑战
    async fn get_pow_challenge(&self) -> Result<PowChallenge> {
        debug!("获取 POW 挑战...");

        let resp = self
            .client
            .post(format!("{}/api/v0/chat/create_pow_challenge", BASE_URL))
            .headers(self.get_headers())
            .json(&serde_json::json!({
                "target_path": "/api/v0/chat/completion"
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("获取 POW 挑战失败: HTTP {} - {}", status, body));
        }

        let data: ApiResponse<PowChallengeResponse> = resp.json().await?;

        if data.code != 0 {
            return Err(anyhow!(
                "获取 POW 挑战失败: code={}, msg={}",
                data.code,
                data.msg.unwrap_or_default()
            ));
        }

        let challenge = data
            .data
            .ok_or_else(|| anyhow!("POW 挑战响应无数据"))?
            .biz_data
            .challenge;

        debug!(
            "POW 挑战获取成功: algorithm={}, difficulty={}",
            challenge.algorithm, challenge.difficulty
        );

        Ok(challenge)
    }

    /// 发送聊天消息并获取响应
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `parent_message_id`: 上一条消息 ID（可选，用于连续对话）
    /// - `prompt`: 用户消息
    /// - `thinking_enabled`: 是否启用 R1 深度思考模式
    ///
    /// # 返回
    /// - (响应文本, 新的 message_id)
    pub async fn send_message(
        &self,
        session_id: &str,
        parent_message_id: Option<&str>,
        prompt: &str,
        thinking_enabled: bool,
    ) -> Result<(String, Option<String>)> {
        // 1. 获取并求解 POW 挑战
        let challenge = self.get_pow_challenge().await?;
        let answer = solve_pow(&challenge);
        let pow_response = build_pow_response(&challenge, answer);
        let pow_header = encode_pow_header(&pow_response);

        // 2. 构建请求
        let client_stream_id = format!(
            "{}-{}",
            chrono::Local::now().format("%Y%m%d"),
            uuid::Uuid::new_v4().to_string().replace("-", "")[..16].to_string()
        );

        // parent_message_id 需要转换为数字类型（服务器要求 u32）
        let parent_id_num: Option<u64> = parent_message_id.and_then(|s| s.parse().ok());

        let payload = serde_json::json!({
            "chat_session_id": session_id,
            "parent_message_id": parent_id_num,
            "prompt": prompt,
            "ref_file_ids": [],
            "thinking_enabled": thinking_enabled,
            "search_enabled": false,
            "client_stream_id": client_stream_id
        });

        debug!("发送聊天请求: session={}, thinking={}", session_id, thinking_enabled);

        // 3. 发送请求
        let mut headers = self.get_headers();
        headers.insert("x-ds-pow-response", pow_header.parse().unwrap());

        let resp = self
            .client
            .post(format!("{}/api/v0/chat/completion", BASE_URL))
            .headers(headers)
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("聊天请求失败: HTTP {} - {}", status, body));
        }

        // 4. 解析 SSE 流响应
        let body = resp.text().await?;
        let (response_text, message_id) = self.parse_sse_response(&body)?;

        Ok((response_text, message_id))
    }

    /// 解析 SSE 流响应
    fn parse_sse_response(&self, body: &str) -> Result<(String, Option<String>)> {
        // 首先检查是否是 JSON 错误响应（非 SSE 格式）
        // POW 验证失败等情况下，服务器返回 {"code":40301,"msg":"Invalid PoW response","data":null}
        if body.starts_with('{') && !body.contains("data:") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                if let Some(code) = json.get("code").and_then(|c| c.as_i64()) {
                    if code != 0 {
                        let msg = json.get("msg").and_then(|m| m.as_str()).unwrap_or("未知错误");
                        return Err(anyhow!("API 错误: code={}, msg={}", code, msg));
                    }
                }
            }
        }

        let mut full_response = String::new();
        let mut message_id: Option<String> = None;
        let mut chunk_count = 0;

        for line in body.lines() {
            if !line.starts_with("data:") {
                continue;
            }

            let data_str = line[5..].trim();
            if data_str.is_empty() {
                continue;
            }

            chunk_count += 1;

            if let Ok(data) = serde_json::from_str::<serde_json::Value>(data_str) {
                // 提取字段
                let p_field = data.get("p").and_then(|p| p.as_str());
                let o_field = data.get("o").and_then(|o| o.as_str());
                let v_field = data.get("v");

                // 从 ready 事件提取 response_message_id（数字类型）
                if let Some(id) = data.get("response_message_id") {
                    if let Some(id_num) = id.as_i64() {
                        message_id = Some(id_num.to_string());
                    } else if let Some(id_str) = id.as_str() {
                        message_id = Some(id_str.to_string());
                    }
                }

                // 从 response 对象提取 message_id（数字类型）
                if let Some(id) = data
                    .get("v")
                    .and_then(|v| v.get("response"))
                    .and_then(|r| r.get("message_id"))
                {
                    if let Some(id_num) = id.as_i64() {
                        message_id = Some(id_num.to_string());
                    } else if let Some(id_str) = id.as_str() {
                        message_id = Some(id_str.to_string());
                    }
                }

                // 提取文本内容 - 多种格式处理

                // 格式1: BATCH 操作（包含 fragments 数组）
                // 例: p="response", o="BATCH", v=[{"o":"APPEND","p":"fragments","v":[{"content":"庄",...}]}]
                if p_field == Some("response") && o_field == Some("BATCH") {
                    if let Some(v_array) = v_field.and_then(|v| v.as_array()) {
                        for item in v_array {
                            // 查找 fragments 的 APPEND 操作
                            if item.get("p").and_then(|p| p.as_str()) == Some("fragments") {
                                if let Some(fragments) = item.get("v").and_then(|v| v.as_array()) {
                                    for fragment in fragments {
                                        if let Some(content) = fragment.get("content").and_then(|c| c.as_str()) {
                                            debug!("SSE BATCH/fragments: content='{}', 累计长度={}", content, full_response.len());
                                            full_response.push_str(content);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // 格式2: fragments content 追加
                // 例: p="response/fragments/-1/content", o="APPEND", v="心"
                else if p_field.map(|p| p.contains("fragments") && p.contains("content")).unwrap_or(false)
                    && o_field == Some("APPEND")
                {
                    if let Some(text) = v_field.and_then(|v| v.as_str()) {
                        debug!("SSE fragments/content APPEND: v='{}', 累计长度={}", text, full_response.len());
                        full_response.push_str(text);
                    }
                }
                // 格式3: response/content 格式（R1 模式）
                else if p_field == Some("response/content") {
                    let operation = o_field.unwrap_or("");
                    if let Some(text) = v_field.and_then(|v| v.as_str()) {
                        // 只在 APPEND 或空操作时追加内容（与 chat.js 一致）
                        if operation == "APPEND" || operation.is_empty() {
                            debug!("SSE response/content APPEND: v='{}', 累计长度={}", text, full_response.len());
                            full_response.push_str(text);
                        } else {
                            // SET 或其他操作：记录但忽略（chat.js 也不处理 SET）
                            debug!("SSE 忽略操作 '{}': v='{}'", operation, text);
                        }
                    }
                }
                // 格式4: V3 直接输出（无 p 字段）
                else if p_field.is_none() {
                    if let Some(text) = v_field.and_then(|v| v.as_str()) {
                        debug!("SSE V3直接输出: v='{}', 累计长度={}", text, full_response.len());
                        full_response.push_str(text);
                    }
                }
            }
        }

        if full_response.is_empty() {
            return Err(anyhow!("DeepSeek 响应为空，原始响应: {}...", &body[..body.len().min(200)]));
        }

        debug!("SSE 解析完成: 共{}个数据块, 响应长度={}", chunk_count, full_response.len());

        Ok((full_response, message_id))
    }
}

/// 使用 DeepSeek Web API 生成文件名
///
/// # 参数
/// - `token`: DeepSeek Web Token
/// - `session`: 会话信息（可选，如果为 None 则创建新会话）
/// - `prompt`: 用户消息
/// - `thinking_enabled`: 是否启用 R1 深度思考模式
/// - `timeout_seconds`: 超时时间
///
/// # 返回
/// - (生成的文件名, 更新后的会话信息)
pub async fn deepseek_web_generate(
    token: &str,
    session: Option<DeepSeekSession>,
    prompt: &str,
    thinking_enabled: bool,
    timeout_seconds: u64,
) -> Result<(String, DeepSeekSession)> {
    let client = DeepSeekWebClient::new(token, timeout_seconds)?;

    // 获取或创建会话
    let (session_id, parent_message_id) = match session {
        Some(s) => (s.session_id, s.parent_message_id),
        None => (client.create_session().await?, None),
    };

    // 发送消息
    let (response, new_message_id) = client
        .send_message(&session_id, parent_message_id.as_deref(), prompt, thinking_enabled)
        .await?;

    // 清洗响应
    debug!("DeepSeek 原始响应: '{}', 长度={}", response, response.len());
    let mut name = response.trim().replace(['"', '\n', '\r'], "");
    name = name.replace(' ', "-");
    debug!("清洗后（filenamify前）: '{}'", name);
    name = crate::utils::filenamify::filenamify(&name);
    debug!("filenamify后: '{}'", name);

    if name.chars().count() > 180 {
        name = name.chars().take(180).collect();
    }

    if name.is_empty() {
        return Err(anyhow!("DeepSeek 生成的文件名为空"));
    }

    let updated_session = DeepSeekSession {
        session_id,
        parent_message_id: new_message_id,
    };

    Ok((name, updated_session))
}
