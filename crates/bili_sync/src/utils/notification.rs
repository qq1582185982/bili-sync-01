use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::config::NotificationConfig;

// Server酱API请求结构
#[derive(Serialize)]
struct ServerChanRequest {
    title: String,
    desp: String,
}

// Server酱API响应结构
#[derive(Deserialize)]
struct ServerChanResponse {
    #[serde(deserialize_with = "deserialize_code")]
    code: i32,
    message: String,
    #[serde(default)]
    #[allow(dead_code)]
    data: Option<serde_json::Value>,
}

// 自定义反序列化器，支持字符串和整数的code
fn deserialize_code<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = serde_json::Value::deserialize(deserializer)?;

    match value {
        serde_json::Value::Number(n) => n
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| D::Error::custom("code is not a valid i32")),
        serde_json::Value::String(s) => s
            .parse::<i32>()
            .map_err(|_| D::Error::custom(format!("code string '{}' is not a valid i32", s))),
        _ => Err(D::Error::custom("code must be a number or string")),
    }
}

// ========== 企业微信API请求/响应结构 ==========

#[derive(Serialize)]
struct WecomTextRequest {
    msgtype: String,
    text: WecomTextContent,
}

#[derive(Serialize)]
struct WecomTextContent {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mentioned_list: Option<Vec<String>>,
}

#[derive(Serialize)]
struct WecomMarkdownRequest {
    msgtype: String,
    markdown: WecomMarkdownContent,
}

#[derive(Serialize)]
struct WecomMarkdownContent {
    content: String,
}

#[derive(Deserialize, Debug)]
struct WecomResponse {
    errcode: i32,
    errmsg: String,
}

impl WecomResponse {
    fn is_success(&self) -> bool {
        self.errcode == 0
    }
}

// 推送通知客户端
pub struct NotificationClient {
    client: Client,
    config: NotificationConfig,
}

// 扫描结果数据结构
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NewVideoInfo {
    pub title: String,
    pub bvid: String,
    pub upper_name: String,
    pub source_type: String,
    pub source_name: String,
    pub pubtime: Option<String>, // 使用字符串格式的北京时间
    pub episode_number: Option<i32>,
    pub season_number: Option<i32>,
    pub video_id: Option<i32>, // 添加视频ID字段，用于过滤删除队列中的视频
}

#[derive(Debug, Clone)]
pub struct SourceScanResult {
    pub source_type: String,
    pub source_name: String,
    pub new_videos: Vec<NewVideoInfo>,
}

#[derive(Debug, Clone)]
pub struct ScanSummary {
    pub total_sources: usize,
    pub total_new_videos: usize,
    pub scan_duration: Duration,
    pub source_results: Vec<SourceScanResult>,
}

impl NotificationClient {
    pub fn new(config: NotificationConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.notification_timeout))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    // 清理可能导致Server酱数据库问题的特殊字符
    fn sanitize_for_serverchan(text: &str) -> String {
        text
            .replace('「', "[")
            .replace('」', "]")
            .replace('【', "[")
            .replace('】', "]")
            .replace('〖', "[")
            .replace('〗', "]")
            .replace('〔', "[")
            .replace('〕', "]")
            // 移除其他可能有问题的Unicode字符
            .chars()
            .filter(|c| c.is_ascii() || (*c as u32) < 0x10000)
            .collect()
    }

    pub async fn send_scan_completion(&self, summary: &ScanSummary) -> Result<()> {
        if !self.config.enable_scan_notifications {
            debug!("推送通知已禁用，跳过发送");
            return Ok(());
        }

        if summary.total_new_videos < self.config.notification_min_videos {
            debug!(
                "新增视频数量({})未达到推送阈值({})",
                summary.total_new_videos, self.config.notification_min_videos
            );
            return Ok(());
        }

        let active_channel = self.config.active_channel.as_str();
        if active_channel == "none" {
            warn!("推送通知已启用但未选择通知渠道");
            return Ok(());
        }

        let (title, content) = self.format_scan_message(summary);

        // 只向选中的渠道发送
        match active_channel {
            "serverchan" => {
                let Some(ref key) = self.config.serverchan_key else {
                    warn!("Server酱渠道已激活但未配置密钥");
                    return Ok(());
                };

                for attempt in 1..=self.config.notification_retry_count {
                    match self.send_to_serverchan(key, &title, &content).await {
                        Ok(_) => {
                            info!("Server酱推送发送成功");
                            return Ok(());
                        }
                        Err(e) => {
                            warn!(
                                "Server酱推送发送失败 (尝试 {}/{}): {}",
                                attempt, self.config.notification_retry_count, e
                            );
                            if attempt < self.config.notification_retry_count {
                                tokio::time::sleep(Duration::from_secs(2)).await;
                            }
                        }
                    }
                }
                error!("Server酱推送发送失败，已达最大重试次数");
            }
            "wecom" => {
                for attempt in 1..=self.config.notification_retry_count {
                    let wecom_content = self.format_wecom_content(&content);

                    match self.send_to_wecom(&title, &wecom_content).await {
                        Ok(_) => {
                            info!("企业微信推送发送成功");
                            return Ok(());
                        }
                        Err(e) => {
                            warn!(
                                "企业微信推送发送失败 (尝试 {}/{}): {}",
                                attempt, self.config.notification_retry_count, e
                            );
                            if attempt < self.config.notification_retry_count {
                                tokio::time::sleep(Duration::from_secs(2)).await;
                            }
                        }
                    }
                }
                error!("企业微信推送发送失败，已达最大重试次数");
            }
            _ => {
                warn!("未知的通知渠道: {}", active_channel);
            }
        }

        Ok(())
    }

    async fn send_to_serverchan(&self, key: &str, title: &str, content: &str) -> Result<()> {
        let url = format!("https://sctapi.ftqq.com/{}.send", key);
        let request = ServerChanRequest {
            title: title.to_string(),
            desp: content.to_string(),
        };

        let response = self.client.post(&url).json(&request).send().await?;

        let response_text = response.text().await?;
        let server_response: ServerChanResponse = serde_json::from_str(&response_text)
            .map_err(|e| anyhow!("解析响应失败: {}, 响应内容: {}", e, response_text))?;

        if server_response.code == 0 {
            Ok(())
        } else {
            Err(anyhow!("Server酱返回错误: {}", server_response.message))
        }
    }

    /// 发送企业微信通知
    async fn send_to_wecom(&self, title: &str, content: &str) -> Result<()> {
        let Some(ref webhook_url) = self.config.wecom_webhook_url else {
            return Err(anyhow!("未配置企业微信Webhook URL"));
        };

        let response = match self.config.wecom_msgtype.as_str() {
            "text" => {
                let full_content = format!("{}\n\n{}", title, content);

                let mentioned_list = if self.config.wecom_mention_all {
                    Some(vec!["@all".to_string()])
                } else {
                    self.config.wecom_mentioned_list.clone()
                };

                let request = WecomTextRequest {
                    msgtype: "text".to_string(),
                    text: WecomTextContent {
                        content: full_content,
                        mentioned_list,
                    },
                };

                self.client.post(webhook_url).json(&request).send().await?
            }
            "markdown" => {
                // 先拼接完整内容，再进行长度限制
                let full_content = format!("# {}\n\n{}", title, content);
                let markdown_content = self.truncate_wecom_markdown(&full_content);

                let request = WecomMarkdownRequest {
                    msgtype: "markdown".to_string(),
                    markdown: WecomMarkdownContent {
                        content: markdown_content,
                    },
                };

                self.client.post(webhook_url).json(&request).send().await?
            }
            _ => {
                return Err(anyhow!("不支持的企业微信消息类型: {}", self.config.wecom_msgtype));
            }
        };

        let response_text = response.text().await?;
        let wecom_response: WecomResponse = serde_json::from_str(&response_text)
            .map_err(|e| anyhow!("解析企业微信响应失败: {}, 响应内容: {}", e, response_text))?;

        if wecom_response.is_success() {
            Ok(())
        } else {
            Err(anyhow!(
                "企业微信返回错误 (errcode: {}): {}",
                wecom_response.errcode,
                wecom_response.errmsg
            ))
        }
    }

    /// 格式化企业微信消息内容（限制长度）
    /// 企业微信 markdown 消息限制 4096 字符，预留 100 字符给标题和格式
    fn format_wecom_content(&self, content: &str) -> String {
        const MAX_WECOM_LENGTH: usize = 3900;

        if content.len() > MAX_WECOM_LENGTH {
            let mut truncated = content.chars().take(MAX_WECOM_LENGTH - 50).collect::<String>();
            truncated.push_str("\n\n...内容过长，已截断");
            truncated
        } else {
            content.to_string()
        }
    }

    /// 截断企业微信 markdown 消息（严格限制 4096 字符）
    fn truncate_wecom_markdown(&self, content: &str) -> String {
        const MAX_MARKDOWN_LENGTH: usize = 4000;

        if content.len() > MAX_MARKDOWN_LENGTH {
            let mut truncated = content.chars().take(MAX_MARKDOWN_LENGTH - 50).collect::<String>();
            truncated.push_str("\n\n...内容过长，已截断");
            truncated
        } else {
            content.to_string()
        }
    }

    fn format_scan_message(&self, summary: &ScanSummary) -> (String, String) {
        let title = "Bili Sync 扫描完成".to_string();

        // 限制最大内容长度为30KB（留一些余量）
        const MAX_CONTENT_LENGTH: usize = 30000;

        let mut content = format!(
            "📊 **扫描摘要**\n\n- 扫描视频源: {}个\n- 新增视频: {}个\n- 扫描耗时: {:.1}分钟\n\n",
            summary.total_sources,
            summary.total_new_videos,
            summary.scan_duration.as_secs_f64() / 60.0
        );

        if summary.total_new_videos > 0 {
            content.push_str("📹 **新增视频详情**\n\n");

            let mut videos_shown = 0;
            let mut sources_shown = 0;

            for source_result in &summary.source_results {
                if !source_result.new_videos.is_empty() {
                    // 如果内容已经很长，停止添加更多内容
                    if content.len() > MAX_CONTENT_LENGTH - 500 {
                        let remaining_videos = summary.total_new_videos - videos_shown;
                        let remaining_sources = summary
                            .source_results
                            .iter()
                            .filter(|s| !s.new_videos.is_empty())
                            .count()
                            - sources_shown;
                        content.push_str(&format!(
                            "\n...还有 {} 个视频源的 {} 个新视频（内容过长已省略）\n",
                            remaining_sources, remaining_videos
                        ));
                        break;
                    }

                    sources_shown += 1;

                    let icon = match source_result.source_type.as_str() {
                        "收藏夹" => "🎬",
                        "合集" => "📁",
                        "UP主投稿" => "🎯",
                        "稍后再看" => "⏰",
                        "番剧" => "📺",
                        _ => "📄",
                    };

                    // 清理源名称中的特殊字符
                    let clean_source_name = Self::sanitize_for_serverchan(&source_result.source_name);

                    content.push_str(&format!(
                        "{} **{}** - {} ({}个新视频):\n",
                        icon,
                        source_result.source_type,
                        clean_source_name,
                        source_result.new_videos.len()
                    ));

                    // 按照视频类型进行排序
                    let mut sorted_videos = source_result.new_videos.clone();
                    if source_result.source_type == "番剧" {
                        // 番剧按集数降序排列（最新的集数在前）
                        sorted_videos.sort_by(|a, b| b.episode_number.unwrap_or(0).cmp(&a.episode_number.unwrap_or(0)));
                    } else {
                        // 其他视频按发布时间降序排列（最新的在前）
                        sorted_videos.sort_by(|a, b| {
                            b.pubtime
                                .as_ref()
                                .unwrap_or(&String::new())
                                .cmp(a.pubtime.as_ref().unwrap_or(&String::new()))
                        });
                    }

                    // 限制每个源显示的视频数量
                    let max_videos_per_source = 20;
                    let videos_to_show = sorted_videos.len().min(max_videos_per_source);

                    for (idx, video) in sorted_videos.iter().take(videos_to_show).enumerate() {
                        // 如果内容过长，提前结束
                        if content.len() > MAX_CONTENT_LENGTH - 1000 {
                            content.push_str(&format!(
                                "...还有 {} 个视频（内容过长已省略）\n",
                                sorted_videos.len() - idx
                            ));
                            break;
                        }

                        videos_shown += 1;

                        // 清理视频标题中的特殊字符
                        let clean_title = Self::sanitize_for_serverchan(&video.title);
                        let mut video_line =
                            format!("- [{}](https://www.bilibili.com/video/{})", clean_title, video.bvid);

                        // 添加额外信息
                        if source_result.source_type == "番剧" && video.episode_number.is_some() {
                            video_line.push_str(&format!(" (第{}集", video.episode_number.unwrap()));
                            // 番剧也显示时间戳
                            if let Some(pubtime) = &video.pubtime {
                                // 只显示日期部分，不显示时间
                                if let Some(date_part) = pubtime.split(' ').next() {
                                    video_line.push_str(&format!(", {}", date_part));
                                }
                            }
                            video_line.push(')');
                        } else if let Some(pubtime) = &video.pubtime {
                            // 只显示日期部分，不显示时间
                            if let Some(date_part) = pubtime.split(' ').next() {
                                video_line.push_str(&format!(" ({})", date_part));
                            }
                        }

                        content.push_str(&video_line);
                        content.push('\n');
                    }

                    // 如果有未显示的视频，添加提示
                    if sorted_videos.len() > videos_to_show {
                        content.push_str(&format!("...还有 {} 个视频\n", sorted_videos.len() - videos_to_show));
                    }

                    content.push('\n');
                }
            }
        }

        // 最终清理整个内容，确保没有问题字符
        let clean_content = Self::sanitize_for_serverchan(&content);

        // 确保内容不超过限制
        let final_content = if clean_content.len() > MAX_CONTENT_LENGTH {
            let mut truncated = clean_content.chars().take(MAX_CONTENT_LENGTH - 100).collect::<String>();
            truncated.push_str("\n\n...内容过长，已截断");
            truncated
        } else {
            clean_content
        };

        (title, final_content)
    }

    pub async fn test_notification(&self) -> Result<()> {
        let active_channel = self.config.active_channel.as_str();

        if active_channel == "none" {
            return Err(anyhow!("未选择通知渠道"));
        }

        match active_channel {
            "serverchan" => {
                let Some(ref key) = self.config.serverchan_key else {
                    return Err(anyhow!("Server酱渠道已选择但未配置密钥"));
                };

                let title = "Bili Sync 测试推送";
                let content = "这是一条测试推送消息。\n\n如果您收到此消息，说明Server酱推送配置正确。\n\n🎉 推送功能工作正常！";

                self.send_to_serverchan(key, title, content).await?;
                info!("Server酱测试推送发送成功");
                Ok(())
            }
            "wecom" => {
                let title = "Bili Sync 测试推送";
                let content = "这是一条企业微信测试推送消息。\n\n如果您收到此消息，说明企业微信推送配置正确。\n\n🎉 推送功能工作正常！";

                self.send_to_wecom(title, content).await?;
                info!("企业微信测试推送发送成功");
                Ok(())
            }
            _ => Err(anyhow!("未知的通知渠道: {}", active_channel))
        }
    }

    pub async fn send_custom_test(&self, message: &str) -> Result<()> {
        let active_channel = self.config.active_channel.as_str();

        if active_channel == "none" {
            return Err(anyhow!("未选择通知渠道"));
        }

        let title = "Bili Sync 自定义测试推送";
        let content = format!("🧪 **自定义测试消息**\n\n{}", message);

        match active_channel {
            "serverchan" => {
                let Some(ref key) = self.config.serverchan_key else {
                    return Err(anyhow!("Server酱渠道已选择但未配置密钥"));
                };

                self.send_to_serverchan(key, title, &content).await?;
                info!("Server酱自定义测试推送发送成功");
                Ok(())
            }
            "wecom" => {
                let wecom_content = self.format_wecom_content(&content);
                self.send_to_wecom(title, &wecom_content).await?;
                info!("企业微信自定义测试推送发送成功");
                Ok(())
            }
            _ => Err(anyhow!("未知的通知渠道: {}", active_channel))
        }
    }

    /// 发送风控验证通知
    pub async fn send_risk_control(&self, mode: &str) -> Result<()> {
        let active_channel = self.config.active_channel.as_str();

        if active_channel == "none" {
            debug!("未选择通知渠道，跳过风控通知");
            return Ok(());
        }

        let title = "Bili Sync 风控验证提醒";
        let content = match mode {
            "manual" => "检测到B站风控验证，需要手动完成验证码。\n\n请访问管理页面 /captcha 完成验证。".to_string(),
            "auto" => "检测到B站风控验证，正在自动处理验证码...".to_string(),
            _ => format!("检测到B站风控验证（模式: {}）", mode),
        };

        match active_channel {
            "serverchan" => {
                let Some(ref key) = self.config.serverchan_key else {
                    warn!("Server酱渠道已激活但未配置密钥，跳过风控通知");
                    return Ok(());
                };

                match self.send_to_serverchan(key, title, &content).await {
                    Ok(_) => {
                        info!("风控通知推送成功 (Server酱)");
                    }
                    Err(e) => {
                        warn!("风控通知推送失败 (Server酱): {}", e);
                    }
                }
            }
            "wecom" => {
                let wecom_content = self.format_wecom_content(&content);
                match self.send_to_wecom(title, &wecom_content).await {
                    Ok(_) => {
                        info!("风控通知推送成功 (企业微信)");
                    }
                    Err(e) => {
                        warn!("风控通知推送失败 (企业微信): {}", e);
                    }
                }
            }
            _ => {
                warn!("未知的通知渠道: {}", active_channel);
            }
        }

        Ok(())
    }

    /// 发送单P变多P通知
    pub async fn send_single_to_multi_page(
        &self,
        video_name: &str,
        bvid: &str,
        total_pages: usize,
        old_path: Option<&str>,
    ) -> Result<()> {
        let active_channel = self.config.active_channel.as_str();

        if active_channel == "none" {
            debug!("未选择通知渠道，跳过单P变多P通知");
            return Ok(());
        }

        let title = "Bili Sync 视频结构变更提醒";
        let path_info = old_path
            .map(|p| format!("\n\n**原文件路径**: `{}`\n\n请手动清理原单P文件。", p))
            .unwrap_or_default();

        let content = format!(
            "检测到视频从单P变为多P，已自动重置下载状态。\n\n\
            **视频**: {}\n\
            **BVID**: [{}](https://www.bilibili.com/video/{})\n\
            **新分P数**: {}{}",
            Self::sanitize_for_serverchan(video_name),
            bvid,
            bvid,
            total_pages,
            path_info
        );

        match active_channel {
            "serverchan" => {
                let Some(ref key) = self.config.serverchan_key else {
                    warn!("Server酱渠道已激活但未配置密钥，跳过单P变多P通知");
                    return Ok(());
                };

                match self.send_to_serverchan(key, title, &content).await {
                    Ok(_) => {
                        info!("单P变多P通知推送成功 (Server酱)");
                    }
                    Err(e) => {
                        warn!("单P变多P通知推送失败 (Server酱): {}", e);
                    }
                }
            }
            "wecom" => {
                let wecom_content = self.format_wecom_content(&content);
                match self.send_to_wecom(title, &wecom_content).await {
                    Ok(_) => {
                        info!("单P变多P通知推送成功 (企业微信)");
                    }
                    Err(e) => {
                        warn!("单P变多P通知推送失败 (企业微信): {}", e);
                    }
                }
            }
            _ => {
                warn!("未知的通知渠道: {}", active_channel);
            }
        }

        Ok(())
    }
}

// 便捷函数
pub async fn send_scan_notification(summary: ScanSummary) -> Result<()> {
    let config = crate::config::reload_config().notification;
    let client = NotificationClient::new(config);
    client.send_scan_completion(&summary).await
}

#[allow(dead_code)]
pub async fn test_notification() -> Result<()> {
    let config = crate::config::reload_config().notification;
    let client = NotificationClient::new(config);
    client.test_notification().await
}

#[allow(dead_code)]
pub async fn send_custom_test_notification(message: &str) -> Result<()> {
    let config = crate::config::reload_config().notification;
    let client = NotificationClient::new(config);
    client.send_custom_test(message).await
}

/// 发送风控验证通知的便捷函数
pub async fn send_risk_control_notification(mode: &str) -> Result<()> {
    let config = crate::config::reload_config().notification;
    let client = NotificationClient::new(config);
    client.send_risk_control(mode).await
}

/// 发送单P变多P通知的便捷函数
pub async fn send_single_to_multi_page_notification(
    video_name: &str,
    bvid: &str,
    total_pages: usize,
    old_path: Option<&str>,
) -> Result<()> {
    let config = crate::config::reload_config().notification;
    let client = NotificationClient::new(config);
    client.send_single_to_multi_page(video_name, bvid, total_pages, old_path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wecom_response_success() {
        let resp = WecomResponse {
            errcode: 0,
            errmsg: "ok".to_string(),
        };
        assert!(resp.is_success());

        let resp = WecomResponse {
            errcode: 40001,
            errmsg: "invalid webhook url".to_string(),
        };
        assert!(!resp.is_success());
    }

    #[test]
    fn test_notification_config_validation() {
        let mut config = NotificationConfig::default();
        config.enable_scan_notifications = true;

        // 未配置任何渠道应该失败
        assert!(config.validate().is_err());

        // 配置企业微信后应该通过
        config.wecom_webhook_url = Some(
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=test".to_string(),
        );
        assert!(config.validate().is_ok());

        // 错误的URL格式应该失败
        config.wecom_webhook_url = Some("https://example.com/webhook".to_string());
        assert!(config.validate().is_err());

        // 错误的消息类型应该失败
        config.wecom_webhook_url = Some(
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=test".to_string(),
        );
        config.wecom_msgtype = "invalid".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_format_wecom_content() {
        let config = NotificationConfig::default();
        let client = NotificationClient::new(config);

        // 短内容应该保持不变
        let short_content = "测试内容";
        assert_eq!(client.format_wecom_content(short_content), short_content);

        // 长内容应该被截断
        let long_content = "a".repeat(5000);
        let formatted = client.format_wecom_content(&long_content);
        assert!(formatted.len() < 4100);
        assert!(formatted.contains("内容过长，已截断"));
    }
}
