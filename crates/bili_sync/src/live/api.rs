use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::bilibili::BiliClient;

/// 直播间状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveStatus {
    /// 未直播
    NotLive = 0,
    /// 直播中
    Live = 1,
}

impl From<i32> for LiveStatus {
    fn from(value: i32) -> Self {
        match value {
            1 => LiveStatus::Live,
            _ => LiveStatus::NotLive,
        }
    }
}

/// 录制质量等级
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// 流畅 80
    Fluent = 80,
    /// 高清 150  
    High = 150,
    /// 超清 250
    SuperClear = 250,
    /// 蓝光 400
    BluRay = 400,
    /// 原画 10000
    Original = 10000,
}

impl From<&str> for Quality {
    fn from(value: &str) -> Self {
        match value {
            "fluent" => Quality::Fluent,
            "high" => Quality::High,
            "super_clear" => Quality::SuperClear,
            "blue_ray" => Quality::BluRay,
            "original" => Quality::Original,
            _ => Quality::High, // 默认高清
        }
    }
}

/// 直播间基本信息
#[derive(Debug, Deserialize, Clone)]
pub struct LiveRoomInfo {
    /// 直播状态
    pub live_status: i32,
    /// 房间ID
    pub room_id: i64,
    /// 短房间ID  
    pub short_id: i64,
    /// 直播标题
    pub title: String,
    /// 封面图片
    pub cover: String,
    /// 在线人数
    pub online: i32,
    /// UP主ID
    pub uid: i64,
    /// UP主名称
    pub uname: String,
}

/// 房间初始化信息
#[derive(Debug, Deserialize)]
pub struct RoomInitInfo {
    /// 真实房间ID
    pub room_id: i64,
    /// 短房间ID
    pub short_id: i64,
    /// UP主ID
    pub uid: i64,
    /// 是否需要登录
    pub need_p2p: i32,
    /// 是否隐藏
    pub is_hidden: bool,
    /// 是否锁定
    pub is_locked: bool,
    /// 是否为竖屏
    pub is_portrait: bool,
    /// 直播状态
    pub live_status: i32,
    /// 加密状态
    pub encrypted: bool,
}

/// 直播流信息
#[derive(Debug, Deserialize)]
pub struct PlayUrlInfo {
    /// 流列表
    pub durl: Vec<StreamUrl>,
    /// 质量描述
    pub quality_description: Vec<QualityDesc>,
}

/// 流URL信息
#[derive(Debug, Deserialize)]
pub struct StreamUrl {
    /// 流地址
    pub url: String,
    /// 长度（秒）
    pub length: i32,
    /// 备用地址列表
    pub backup_url: Option<Vec<String>>,
}

/// 质量描述
#[derive(Debug, Deserialize)]
pub struct QualityDesc {
    /// 质量代码
    pub qn: i32,
    /// 质量描述
    pub desc: String,
}

/// B站直播API响应结构
#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub message: String,
    pub data: T,
}

/// 直播API客户端
pub struct LiveApiClient<'a> {
    client: &'a BiliClient,
}

impl<'a> LiveApiClient<'a> {
    /// 创建新的直播API客户端
    pub fn new(client: &'a BiliClient) -> Self {
        Self { client }
    }

    /// 获取真实房间ID
    /// 
    /// # Arguments
    /// * `room_id` - 房间ID（可以是短ID或长ID）
    pub async fn get_room_init(&self, room_id: i64) -> Result<RoomInitInfo> {
        let url = format!("https://api.live.bilibili.com/room/v1/Room/room_init?id={}", room_id);
        
        let response: ApiResponse<RoomInitInfo> = self.client.get_json(&url).await
            .map_err(|e| anyhow!("获取房间初始化信息失败: {}", e))?;

        if response.code != 0 {
            return Err(anyhow!("API返回错误: {} - {}", response.code, response.message));
        }

        Ok(response.data)
    }

    /// 获取直播间状态和基本信息
    /// 
    /// # Arguments
    /// * `room_id` - 房间ID
    pub async fn get_room_info(&self, room_id: i64) -> Result<LiveRoomInfo> {
        let url = format!("https://api.live.bilibili.com/room/v1/Room/getRoomInfoOld?mid={}", room_id);
        
        let response: ApiResponse<LiveRoomInfo> = self.client.get_json(&url).await
            .map_err(|e| anyhow!("获取直播间信息失败: {}", e))?;

        if response.code != 0 {
            return Err(anyhow!("API返回错误: {} - {}", response.code, response.message));
        }

        Ok(response.data)
    }

    /// 根据UP主ID获取直播间状态
    /// 
    /// # Arguments  
    /// * `upper_id` - UP主ID
    pub async fn get_live_status_by_uid(&self, upper_id: i64) -> Result<(LiveStatus, Option<LiveRoomInfo>)> {
        // 首先尝试通过UP主ID获取直播间信息
        let url = format!("https://api.live.bilibili.com/room/v1/Room/getRoomInfoOld?mid={}", upper_id);
        
        match self.client.get_json::<ApiResponse<LiveRoomInfo>>(&url).await {
            Ok(response) => {
                if response.code == 0 {
                    let status = LiveStatus::from(response.data.live_status);
                    Ok((status, Some(response.data)))
                } else {
                    // 如果API返回错误，可能是该UP主没有开通直播间
                    Ok((LiveStatus::NotLive, None))
                }
            }
            Err(_) => {
                // 请求失败，可能是网络问题或者UP主没有直播间
                Ok((LiveStatus::NotLive, None))
            }
        }
    }

    /// 获取直播流地址
    /// 
    /// # Arguments
    /// * `room_id` - 房间ID
    /// * `quality` - 画质质量
    pub async fn get_play_url(&self, room_id: i64, quality: Quality) -> Result<PlayUrlInfo> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("cid".to_string(), room_id.to_string());
        params.insert("qn".to_string(), (quality as i32).to_string());
        params.insert("platform".to_string(), "web".to_string());
        params.insert("ptype".to_string(), "8".to_string());
        params.insert("dolby".to_string(), "5".to_string());
        params.insert("panorama".to_string(), "1".to_string());

        let url = "https://api.live.bilibili.com/room/v1/Room/playUrl";
        
        let response: ApiResponse<PlayUrlInfo> = self.client
            .get_json_with_params(url, &params)
            .await
            .map_err(|e| anyhow!("获取直播流地址失败: {}", e))?;

        if response.code != 0 {
            return Err(anyhow!("API返回错误: {} - {}", response.code, response.message));
        }

        Ok(response.data)
    }

    /// 批量获取多个直播间的状态
    /// 
    /// # Arguments
    /// * `room_ids` - 房间ID列表
    pub async fn batch_get_room_status(&self, room_ids: &[i64]) -> Result<Vec<(i64, LiveStatus)>> {
        let mut results = Vec::new();
        
        // 由于B站API限制，这里使用顺序请求而不是并发
        // 可以根据需要调整请求间隔以避免触发频率限制
        for &room_id in room_ids {
            match self.get_room_info(room_id).await {
                Ok(info) => {
                    let status = LiveStatus::from(info.live_status);
                    results.push((room_id, status));
                }
                Err(_) => {
                    // 如果单个房间请求失败，记录为未直播状态
                    results.push((room_id, LiveStatus::NotLive));
                }
            }
            
            // 添加短暂延迟以避免触发API频率限制
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        Ok(results)
    }

    /// 检查直播流是否可用
    /// 
    /// # Arguments
    /// * `stream_url` - 流地址
    pub async fn check_stream_availability(&self, stream_url: &str) -> Result<bool> {
        match self.client.head(stream_url).await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}