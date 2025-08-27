//! B站直播API和质量获取模块
//! 
//! 主要功能：
//! 1. 获取直播间信息
//! 2. 解析master playlist获取可用质量等级
//! 3. 选择合适的流URL

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::{live_debug, live_warn};
use utoipa::ToSchema;

/// B站直播API客户端
pub struct BilibiliLiveApi {
    client: Client,
    cookies: Option<String>,
}

/// 直播间基本信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub room_id: i64,
    pub short_id: Option<i64>,
    pub uid: i64,
    pub title: String,
    pub live_status: i32, // 0=未直播, 1=直播中, 2=轮播
    pub cover: Option<String>,
    pub user_name: String,
}

/// 播放质量信息
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamQuality {
    /// 质量等级 (qn)
    pub qn: u32,
    /// 质量名称
    pub description: String,
    /// 分辨率
    pub resolution: Option<String>,
    /// 帧率
    pub frame_rate: Option<u32>,
    /// 码率
    pub bitrate: Option<u32>,
}

/// 流媒体信息
#[derive(Debug, Clone)]
pub struct StreamInfo {
    /// 播放URL
    pub url: String,
    /// CDN节点信息
    #[allow(dead_code)]
    pub host: String,
    /// 质量信息
    pub quality: StreamQuality,
}

/// 播放URL响应
#[derive(Debug, Deserialize)]
struct PlayUrlResponse {
    code: i32,
    #[allow(dead_code)]
    message: String,
    data: Option<PlayUrlData>,
}

#[derive(Debug, Deserialize)]
struct PlayUrlData {
    current_qn: u32,
    #[allow(dead_code)]
    accept_quality: Vec<u32>,
    durl: Option<Vec<DurlInfo>>,
    quality_description: Vec<QualityDescription>,
}

#[derive(Debug, Deserialize)]
struct DurlInfo {
    url: String,
    #[allow(dead_code)]
    length: u32,
    #[allow(dead_code)]
    order: u32,
    #[allow(dead_code)]
    stream_type: u32,
    #[allow(dead_code)]
    p2p_type: u32,
}

#[derive(Debug, Deserialize)]
struct QualityDescription {
    qn: u32,
    desc: String,
}

/// 直播间信息响应
#[derive(Debug, Deserialize)]
struct RoomInfoResponse {
    code: i32,
    message: String,
    data: Option<RoomInfoData>,
}

#[derive(Debug, Deserialize)]
struct RoomInfoData {
    room_id: i64,
    short_id: i64,
    uid: i64,
    title: String,
    live_status: i32,
    cover: String,
    uname: String,
}

impl BilibiliLiveApi {
    /// 创建新的API客户端
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
            
        Self {
            client,
            cookies: None,
        }
    }

    /// 设置cookies（用于提升API访问权限）
    #[cfg(test)]
    pub fn with_cookies(mut self, cookies: String) -> Self {
        self.cookies = Some(cookies);
        self
    }

    /// 获取直播间基本信息
    pub async fn get_room_info(&self, room_id: i64) -> Result<RoomInfo> {
        let url = format!(
            "https://api.live.bilibili.com/room/v1/Room/get_info?room_id={}",
            room_id
        );

        live_debug!("获取直播间信息: {}", url);
        
        let mut request = self.client.get(&url);
        if let Some(cookies) = &self.cookies {
            request = request.header("Cookie", cookies);
        }

        let response = request.send().await
            .context("请求直播间信息失败")?;

        if !response.status().is_success() {
            return Err(anyhow!("API请求失败: {}", response.status()));
        }

        let response_data: RoomInfoResponse = response.json().await
            .context("解析直播间信息响应失败")?;

        if response_data.code != 0 {
            return Err(anyhow!("API返回错误: {} - {}", response_data.code, response_data.message));
        }

        let data = response_data.data
            .ok_or_else(|| anyhow!("直播间信息数据为空"))?;

        Ok(RoomInfo {
            room_id: data.room_id,
            short_id: if data.short_id > 0 { Some(data.short_id) } else { None },
            uid: data.uid,
            title: data.title,
            live_status: data.live_status,
            cover: if data.cover.is_empty() { None } else { Some(data.cover) },
            user_name: data.uname,
        })
    }

    /// 获取直播流信息
    /// 返回可用的质量等级和对应的播放URL
    pub async fn get_play_url(&self, room_id: i64, quality: Option<u32>) -> Result<Vec<StreamInfo>> {
        let qn = quality.unwrap_or(10000); // 默认原画
        
        let url = format!(
            "https://api.live.bilibili.com/xlive/web-room/v2/index/getRoomPlayInfo?room_id={}&protocol=0,1&format=0,1,2&codec=0,1&qn={}&platform=web&ptype=8",
            room_id, qn
        );

        live_debug!("获取播放URL: {}", url);
        
        let mut request = self.client.get(&url);
        if let Some(cookies) = &self.cookies {
            request = request.header("Cookie", cookies);
        }

        let response = request.send().await
            .context("请求播放URL失败")?;

        if !response.status().is_success() {
            return Err(anyhow!("API请求失败: {}", response.status()));
        }

        let response_text = response.text().await
            .context("获取响应文本失败")?;

        live_debug!("播放URL响应: {}", response_text);

        // 尝试解析为JSON（新版API）
        if let Ok(response_data) = serde_json::from_str::<PlayUrlResponse>(&response_text) {
            if response_data.code == 0 && response_data.data.is_some() {
                return self.parse_play_url_response(response_data.data.unwrap()).await;
            }
        }

        // 如果JSON解析失败，可能需要处理其他格式或错误
        live_warn!("无法解析播放URL响应，可能格式不匹配");
        Err(anyhow!("无法获取有效的播放URL"))
    }

    /// 解析播放URL响应
    async fn parse_play_url_response(&self, data: PlayUrlData) -> Result<Vec<StreamInfo>> {
        let mut streams = Vec::new();

        // 创建质量描述映射
        let quality_map: HashMap<u32, String> = data.quality_description
            .into_iter()
            .map(|q| (q.qn, q.desc))
            .collect();

        if let Some(durl_list) = data.durl {
            for durl in durl_list {
                // 解析URL获取host信息
                let url = &durl.url;
                let host = self.extract_host_from_url(url)
                    .unwrap_or_else(|| "unknown".to_string());

                let quality = StreamQuality {
                    qn: data.current_qn,
                    description: quality_map.get(&data.current_qn)
                        .cloned()
                        .unwrap_or_else(|| format!("质量{}", data.current_qn)),
                    resolution: None, // 需要从其他API获取
                    frame_rate: None,
                    bitrate: None,
                };

                streams.push(StreamInfo {
                    url: url.clone(),
                    host,
                    quality,
                });
            }
        }

        if streams.is_empty() {
            return Err(anyhow!("未找到可用的播放流"));
        }

        Ok(streams)
    }

    /// 从URL中提取主机名
    fn extract_host_from_url(&self, url: &str) -> Option<String> {
        if let Ok(parsed_url) = url::Url::parse(url) {
            parsed_url.host_str().map(|h| h.to_string())
        } else {
            None
        }
    }

    /// 获取所有可用的质量等级
    /// 这个方法会尝试获取直播间支持的所有质量等级
    pub async fn get_available_qualities(&self, room_id: i64) -> Result<Vec<StreamQuality>> {
        // 先用最高质量尝试获取完整信息
        let streams = self.get_play_url(room_id, Some(10000)).await?;
        
        if streams.is_empty() {
            return Err(anyhow!("未获取到任何流信息"));
        }

        // 从第一个流中提取质量信息
        // 注意：这里可能需要根据实际API响应结构调整
        let qualities = vec![streams[0].quality.clone()];

        // TODO: 如果API支持，可以在这里添加获取其他质量等级的逻辑
        // 比如遍历常用的质量等级进行测试

        Ok(qualities)
    }

    /// 根据质量等级获取最佳播放URL
    #[allow(dead_code)]
    pub async fn get_best_stream_url(&self, room_id: i64, preferred_qn: u32) -> Result<String> {
        let streams = self.get_play_url(room_id, Some(preferred_qn)).await?;
        
        if streams.is_empty() {
            return Err(anyhow!("未找到可用的播放流"));
        }

        // 选择第一个可用的流
        // 可以根据需要添加更复杂的选择逻辑，比如优选某些CDN
        Ok(streams[0].url.clone())
    }
}

impl Default for BilibiliLiveApi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_extract_host() {
        let api = BilibiliLiveApi::new();
        
        let url = "https://d1--cn-gotcha103.bilivideo.com/live-bvc/545729/live_22344608_1234567.flv";
        let host = api.extract_host_from_url(url);
        assert_eq!(host, Some("d1--cn-gotcha103.bilivideo.com".to_string()));
    }

    #[test]
    fn test_api_creation() {
        let api = BilibiliLiveApi::new();
        assert!(api.cookies.is_none());
        
        let api_with_cookies = BilibiliLiveApi::new()
            .with_cookies("test=value".to_string());
        assert!(api_with_cookies.cookies.is_some());
    }
}