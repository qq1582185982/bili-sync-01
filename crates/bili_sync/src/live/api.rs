use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::HashMap;
use tokio::time::{Duration, Instant};
use crate::{live_debug, live_warn};

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
#[allow(dead_code)] // API响应结构体，部分字段暂时未使用但需要保留
pub struct LiveRoomInfo {
    /// 直播状态
    pub live_status: i32,
    /// 房间ID
    pub room_id: i64,
    /// 短房间ID  
    pub short_id: i64,
    /// 直播标题
    pub title: String,
    /// 封面图片 (新API使用user_cover字段)
    #[serde(alias = "cover")]
    pub user_cover: String,
    /// 在线人数
    pub online: i32,
    /// UP主ID
    pub uid: i64,
    /// UP主名称 (新API没有uname字段，使用空字符串默认值)
    #[serde(default)]
    pub uname: String,
}

/// 房间初始化信息
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // API响应结构体，部分字段暂时未使用但需要保留
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
#[allow(dead_code)] // API响应结构体，部分字段暂时未使用但需要保留
pub struct PlayUrlInfo {
    /// 流列表
    pub durl: Vec<StreamUrl>,
    /// 质量描述
    pub quality_description: Vec<QualityDesc>,
}

/// 流URL信息
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)] // API响应结构体，部分字段暂时未使用但需要保留
pub struct StreamUrl {
    /// 流地址
    pub url: String,
    /// 长度（秒）
    pub length: i32,
    /// 备用地址列表
    pub backup_url: Option<Vec<String>>,
}

/// 增强型流URL信息（用于URL池管理）
#[derive(Debug, Clone)]
pub struct EnhancedStreamUrl {
    /// 流地址
    pub url: String,
    /// CDN节点标识符
    pub cdn_node: String,
    /// URL过期时间
    pub expires_at: Instant,
    /// 质量等级
    #[allow(dead_code)] // 质量等级字段，用于URL池分级管理
    pub quality: Quality,
    /// 最后使用时间
    pub last_used: Option<Instant>,
    /// 连接成功率（成功次数/总次数）
    pub success_rate: f32,
    /// 是否为主要URL
    pub is_primary: bool,
}

impl EnhancedStreamUrl {
    /// 从基础StreamUrl创建增强版本
    pub fn from_stream_url(stream_url: StreamUrl, quality: Quality) -> Self {
        let cdn_node = Self::extract_cdn_node(&stream_url.url);
        let expires_at = Self::extract_expire_time(&stream_url.url);
        
        Self {
            url: stream_url.url,
            cdn_node,
            expires_at,
            quality,
            last_used: None,
            success_rate: 1.0, // 初始假设100%成功率
            is_primary: false,
        }
    }
    
    /// 检查URL是否即将过期（2分钟内）
    pub fn is_expiring_soon(&self) -> bool {
        self.expires_at.saturating_duration_since(Instant::now()) < Duration::from_secs(120)
    }
    
    /// 检查URL是否已过期
    pub fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
    
    
    /// 从URL中提取CDN节点信息
    fn extract_cdn_node(url: &str) -> String {
        // 从URL中提取CDN节点，例如: cn-gddg-ct-01-08.bilivideo.com
        if let Some(start) = url.find("://") {
            let after_protocol = &url[start + 3..];
            if let Some(end) = after_protocol.find('/') {
                let host = &after_protocol[..end];
                if let Some(dot_pos) = host.find('.') {
                    return host[..dot_pos].to_string();
                }
                return host.to_string();
            }
        }
        "unknown".to_string()
    }
    
    /// 从URL中提取过期时间
    fn extract_expire_time(url: &str) -> Instant {
        // 从URL参数中提取expires时间戳
        if let Some(expires_start) = url.find("expires=") {
            let after_expires = &url[expires_start + 8..];
            if let Some(expires_end) = after_expires.find('&') {
                let expires_str = &after_expires[..expires_end];
                if let Ok(timestamp) = expires_str.parse::<u64>() {
                    // 转换Unix时间戳为Instant
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let duration_until_expire = Duration::from_secs(timestamp.saturating_sub(now));
                    return Instant::now() + duration_until_expire;
                }
            } else {
                // 如果没有&结尾，取到字符串末尾
                if let Ok(timestamp) = after_expires.parse::<u64>() {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let duration_until_expire = Duration::from_secs(timestamp.saturating_sub(now));
                    return Instant::now() + duration_until_expire;
                }
            }
        }
        
        // 如果无法解析过期时间，默认20分钟后过期
        Instant::now() + Duration::from_secs(1200)
    }
}

/// URL池管理器
#[derive(Debug, Clone)]
pub struct StreamUrlPool {
    /// URL池
    urls: Vec<EnhancedStreamUrl>,
    /// 当前使用的URL索引
    current_index: usize,
    /// 最大URL数量
    max_urls: usize,
}

impl StreamUrlPool {
    /// 创建新的URL池
    pub fn new() -> Self {
        Self {
            urls: Vec::new(),
            current_index: 0,
            max_urls: 5, // 最多保持5个备用URL
        }
    }
    
    /// 添加URL到池中
    pub fn add_url(&mut self, url: EnhancedStreamUrl) {
        // 检查是否已存在相同的URL
        if !self.urls.iter().any(|u| u.url == url.url) {
            let cdn_node = url.cdn_node.clone(); // 在移动前克隆需要用的字段
            self.urls.push(url);
            
            // 如果超过最大数量，移除最旧的URL
            if self.urls.len() > self.max_urls {
                // 按最后使用时间排序，移除最久未使用的
                self.urls.sort_by(|a, b| {
                    a.last_used.unwrap_or(Instant::now() - Duration::from_secs(3600))
                        .cmp(&b.last_used.unwrap_or(Instant::now() - Duration::from_secs(3600)))
                });
                self.urls.remove(0);
                
                // 调整当前索引
                if self.current_index > 0 {
                    self.current_index -= 1;
                }
            }
            
            live_debug!("URL池添加新URL，CDN: {}, 总数: {}", cdn_node, self.urls.len());
        }
    }
    
    
    /// 获取最佳URL（综合考虑过期时间和成功率）
    pub fn get_best_url(&mut self) -> Option<&EnhancedStreamUrl> {
        if self.urls.is_empty() {
            return None;
        }
        
        // 过滤掉已过期的URL
        let mut valid_urls: Vec<(usize, &EnhancedStreamUrl)> = self.urls
            .iter()
            .enumerate()
            .filter(|(_, url)| !url.is_expired())
            .collect();
        
        if valid_urls.is_empty() {
            live_warn!("没有有效的URL，使用第一个URL");
            self.current_index = 0;
            return self.urls.get(0);
        }
        
        // 按成功率和剩余时间排序
        valid_urls.sort_by(|(_, a), (_, b)| {
            let score_a = a.success_rate * (a.expires_at.saturating_duration_since(Instant::now()).as_secs() as f32);
            let score_b = b.success_rate * (b.expires_at.saturating_duration_since(Instant::now()).as_secs() as f32);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        
        if let Some((index, _)) = valid_urls.first() {
            self.current_index = *index;
            live_debug!("选择最佳URL: CDN={}, 成功率={:.2}", 
                self.urls[*index].cdn_node, self.urls[*index].success_rate);
        }
        
        self.urls.get(self.current_index)
    }
    
    /// 清理过期的URL
    pub fn cleanup_expired(&mut self) {
        let original_len = self.urls.len();
        self.urls.retain(|url| !url.is_expired());
        
        if self.urls.len() != original_len {
            live_debug!("清理过期URL: {} -> {}", original_len, self.urls.len());
            
            // 调整当前索引
            if self.current_index >= self.urls.len() && !self.urls.is_empty() {
                self.current_index = 0;
            }
        }
    }
    
    /// 获取URL数量
    pub fn len(&self) -> usize {
        self.urls.len()
    }
    
    /// 检查是否为空（保留作为基础辅助方法）
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.urls.is_empty()
    }
    
    /// 获取即将过期的URL数量
    pub fn expiring_soon_count(&self) -> usize {
        self.urls.iter().filter(|url| url.is_expiring_soon()).count()
    }
    
    /// 清空所有URL（复刻bililive-go行为：每次重试都获取全新URL）
    pub fn clear(&mut self) {
        self.urls.clear();
        self.current_index = 0;
        live_debug!("URL池已清空，将强制获取新的流地址");
    }
}

impl Default for StreamUrlPool {
    fn default() -> Self {
        Self::new()
    }
}

/// 质量描述
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // API响应结构体，部分字段暂时未使用但需要保留
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
    #[allow(dead_code)] // API客户端方法，部分暂时未使用但需要保留
    /// 创建新的直播API客户端
    pub fn new(client: &'a BiliClient) -> Self {
        Self { client }
    }


    /// 获取直播间状态和基本信息
    /// 
    /// # Arguments
    /// * `room_id` - 房间ID
    #[allow(dead_code)] // 方法被batch_get_room_status调用，但编译器未检测到
    pub async fn get_room_info(&self, room_id: i64) -> Result<LiveRoomInfo> {
        let url = format!("https://api.live.bilibili.com/room/v1/Room/getRoomInfoOld?mid={}", room_id);
        
        let response: ApiResponse<LiveRoomInfo> = self.client.get_json(&url).await
            .map_err(|e| anyhow!("获取直播间信息失败: {}", e))?;

        if response.code != 0 {
            return Err(anyhow!("API返回错误: {} - {}", response.code, response.message));
        }

        Ok(response.data)
    }


    /// 根据房间ID获取直播间状态（更准确的API）
    /// 
    /// # Arguments  
    /// * `room_id` - 房间ID
    pub async fn get_live_status_by_room_id(&self, room_id: i64) -> Result<(LiveStatus, Option<LiveRoomInfo>)> {
        use crate::live_debug;
        let url = format!("https://api.live.bilibili.com/room/v1/Room/get_info?room_id={}", room_id);
        live_debug!("请求直播API: {}", url);
        
        match self.client.get_json::<ApiResponse<LiveRoomInfo>>(&url).await {
            Ok(response) => {
                live_debug!("API响应: code={}, message={}", response.code, response.message);
                if response.code == 0 {
                    let status = LiveStatus::from(response.data.live_status);
                    live_debug!("解析的直播状态: live_status={} -> {:?}", response.data.live_status, status);
                    Ok((status, Some(response.data)))
                } else {
                    live_debug!("API返回错误: {} - {}", response.code, response.message);
                    Ok((LiveStatus::NotLive, None))
                }
            }
            Err(e) => {
                live_debug!("请求失败: {}", e);
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

    /// 获取多个CDN节点的直播流地址（用于URL池）
    /// 
    /// # Arguments
    /// * `room_id` - 房间ID
    /// * `quality` - 画质质量
    pub async fn get_play_urls_multi(&self, room_id: i64, quality: Quality) -> Result<Vec<EnhancedStreamUrl>> {
        live_debug!("获取房间 {} 的多个CDN节点URL，质量: {:?}", room_id, quality);
        
        let play_info = self.get_play_url(room_id, quality).await?;
        let mut enhanced_urls = Vec::new();
        
        // 处理主要流URL列表
        for stream_url in play_info.durl {
            let enhanced_url = EnhancedStreamUrl::from_stream_url(stream_url.clone(), quality);
            enhanced_urls.push(enhanced_url);
            
            // 处理备用URL
            if let Some(backup_urls) = stream_url.backup_url {
                for backup_url in backup_urls {
                    let backup_stream = StreamUrl {
                        url: backup_url,
                        length: stream_url.length,
                        backup_url: None,
                    };
                    let enhanced_backup = EnhancedStreamUrl::from_stream_url(backup_stream, quality);
                    enhanced_urls.push(enhanced_backup);
                }
            }
        }
        
        // 标记第一个URL为主要URL
        if let Some(first_url) = enhanced_urls.get_mut(0) {
            first_url.is_primary = true;
        }
        
        live_debug!("获取到 {} 个CDN节点URL", enhanced_urls.len());
        for (i, url) in enhanced_urls.iter().enumerate() {
            live_debug!("  {}. CDN: {}, 主要: {}, 过期时间: {:?}", 
                i + 1, url.cdn_node, url.is_primary, 
                url.expires_at.saturating_duration_since(Instant::now()));
        }
        
        Ok(enhanced_urls)
    }
    
    /// 刷新URL池（获取新的URL并更新池）
    /// 
    /// # Arguments
    /// * `room_id` - 房间ID
    /// * `quality` - 画质质量
    /// * `pool` - URL池
    pub async fn refresh_url_pool(&self, room_id: i64, quality: Quality, pool: &mut StreamUrlPool) -> Result<()> {
        live_debug!("刷新房间 {} 的URL池", room_id);
        
        let new_urls = self.get_play_urls_multi(room_id, quality).await?;
        
        let mut added_count = 0;
        for url in new_urls {
            // 只添加还未过期且不存在的URL
            if !url.is_expired() {
                pool.add_url(url);
                added_count += 1;
            }
        }
        
        // 清理过期的URL
        pool.cleanup_expired();
        
        live_debug!("URL池刷新完成，新增: {}, 当前总数: {}, 即将过期: {}", 
            added_count, pool.len(), pool.expiring_soon_count());
        
        Ok(())
    }

    /// 批量获取多个直播间的状态
    /// 
    /// # Arguments
    /// * `room_ids` - 房间ID列表
    #[allow(dead_code)] // 批量状态获取方法，暂未使用但为完整API保留
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

}