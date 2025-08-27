use anyhow::{anyhow, Result};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use crate::{live_debug, live_error, live_info, live_warn};

use super::api::LiveStatus;

/// WebSocket 连接信息
#[derive(Debug, Deserialize)]
struct WebSocketInfo {
    host: String,
    token: String,
}

/// B站WebSocket配置API响应
#[derive(Debug, Deserialize)]
struct DanmuConfigResponse {
    code: i32,
    data: DanmuConfigData,
}

#[derive(Debug, Deserialize)]
struct DanmuConfigData {
    token: String,
    #[serde(rename = "host_server_list")]
    host_list: Vec<HostInfo>,
}

#[derive(Debug, Deserialize)]
struct HostInfo {
    host: String,
    #[allow(dead_code)] // port字段，API响应中包含但目前使用wss_port
    port: u16,
    #[serde(rename = "wss_port")]
    wss_port: u16,
}

/// WebSocket 事件类型
#[derive(Debug, Clone)]
pub enum WebSocketEvent {
    /// 直播状态变化
    LiveStatusChanged {
        room_id: i64,
        status: LiveStatus,
        title: Option<String>,
    },
    /// 连接状态变化
    ConnectionStatusChanged {
        room_id: i64,
        connected: bool,
        error: Option<String>,
    },
    /// 人气值变化
    PopularityChanged {
        room_id: i64,
        popularity: i64,
    },
}

/// Bilibili 直播 WebSocket 客户端
pub struct BilibiliWebSocketClient {
    room_id: i64,
    event_sender: mpsc::UnboundedSender<WebSocketEvent>,
    #[allow(dead_code)]
    connection_handle: Option<tokio::task::JoinHandle<()>>,
}

impl BilibiliWebSocketClient {
    /// 创建新的 WebSocket 客户端
    pub fn new(
        room_id: i64,
        event_sender: mpsc::UnboundedSender<WebSocketEvent>,
    ) -> Self {
        Self {
            room_id,
            event_sender,
            connection_handle: None,
        }
    }

    /// 启动 WebSocket 连接
    pub async fn start(&mut self) -> Result<()> {
        live_info!("启动房间 {} 的 WebSocket 连接", self.room_id);

        let room_id = self.room_id;
        let event_sender = self.event_sender.clone();

        let handle = tokio::spawn(async move {
            Self::connection_loop(room_id, event_sender).await;
        });

        self.connection_handle = Some(handle);
        Ok(())
    }

    /// WebSocket 连接循环
    async fn connection_loop(
        room_id: i64,
        event_sender: mpsc::UnboundedSender<WebSocketEvent>,
    ) {
        let mut retry_count = 0;
        let max_retries = 10;

        loop {
            match Self::handle_connection(room_id, &event_sender).await {
                Ok(_) => {
                    live_info!("房间 {} WebSocket 连接正常结束", room_id);
                    break;
                }
                Err(e) => {
                    retry_count += 1;
                    live_error!(
                        "房间 {} WebSocket 连接失败 (重试 {}/{}): {}",
                        room_id, retry_count, max_retries, e
                    );

                    // 发送连接状态变化事件
                    if let Err(send_err) = event_sender.send(WebSocketEvent::ConnectionStatusChanged {
                        room_id,
                        connected: false,
                        error: Some(e.to_string()),
                    }) {
                        live_error!("发送连接状态事件失败: {}", send_err);
                    }

                    if retry_count >= max_retries {
                        live_error!("房间 {} WebSocket 连接重试次数已达上限，停止重连", room_id);
                        break;
                    }

                    // 指数退避策略
                    let delay = std::time::Duration::from_secs(2_u64.pow(retry_count.min(6)));
                    live_debug!("房间 {} 等待 {:?} 后重连", room_id, delay);
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    /// 处理单个连接
    async fn handle_connection(
        room_id: i64,
        event_sender: &mpsc::UnboundedSender<WebSocketEvent>,
    ) -> Result<()> {
        live_debug!("房间 {} 开始建立 WebSocket 连接", room_id);

        // 获取WebSocket连接信息
        let ws_info = Self::get_websocket_info(room_id).await?;
        live_debug!("房间 {} 获取到WebSocket信息: host={}, token长度={}", room_id, ws_info.host, ws_info.token.len());

        // 连接到B站WebSocket服务器
        let ws_url = format!("wss://{}/sub", ws_info.host);
        let (ws_stream, _) = connect_async(&ws_url).await?;
        
        live_info!("房间 {} WebSocket 连接已建立", room_id);

        // 发送连接成功事件
        event_sender.send(WebSocketEvent::ConnectionStatusChanged {
            room_id,
            connected: true,
            error: None,
        })?;

        // 连接建立后立即查询当前直播状态
        match Self::check_initial_live_status(room_id, event_sender).await {
            Ok(_) => {
                live_debug!("房间 {} 初始状态检查完成", room_id);
            }
            Err(e) => {
                live_warn!("房间 {} 初始状态检查失败: {}", room_id, e);
                // 不中断连接，继续监听WebSocket事件
            }
        }

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // 发送认证包
        let auth_packet = Self::build_auth_packet(room_id, &ws_info.token)?;
        live_debug!("房间 {} 发送认证包，长度: {} bytes", room_id, auth_packet.len());
        ws_sender.send(Message::Binary(auth_packet)).await?;

        let mut last_live_status: Option<LiveStatus> = None;
        let mut heartbeat_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

        // 消息处理和心跳循环
        loop {
            tokio::select! {
                // 处理接收到的消息
                message_result = ws_receiver.next() => {
                    match message_result {
                        Some(Ok(Message::Binary(data))) => {
                            if let Err(e) = Self::handle_binary_message(
                                room_id,
                                event_sender,
                                &data,
                                &mut last_live_status,
                            ).await {
                                live_warn!("处理房间 {} 二进制消息失败: {}", room_id, e);
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            live_info!("房间 {} WebSocket 连接被服务器关闭", room_id);
                            break;
                        }
                        Some(Ok(_)) => {
                            // 忽略其他类型消息
                        }
                        Some(Err(e)) => {
                            live_warn!("房间 {} 接收消息出错: {}", room_id, e);
                            return Err(anyhow!("接收消息失败: {}", e));
                        }
                        None => {
                            live_warn!("房间 {} WebSocket 连接流结束", room_id);
                            break;
                        }
                    }
                }
                
                // 发送心跳包
                _ = heartbeat_interval.tick() => {
                    let heartbeat_packet = Self::build_heartbeat_packet();
                    if let Err(e) = ws_sender.send(Message::Binary(heartbeat_packet)).await {
                        live_error!("发送心跳包失败: {}", e);
                        break;
                    }
                    live_debug!("房间 {} 发送心跳包", room_id);
                }
            }
        }

        live_warn!("房间 {} WebSocket 连接流结束", room_id);
        Ok(())
    }

    /// 检查房间初始直播状态
    async fn check_initial_live_status(
        room_id: i64,
        event_sender: &mpsc::UnboundedSender<WebSocketEvent>,
    ) -> Result<()> {
        live_debug!("检查房间 {} 的初始直播状态", room_id);

        // 使用HTTP API查询当前直播状态
        let url = format!("https://api.live.bilibili.com/room/v1/Room/get_info?room_id={}", room_id);
        
        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .header("Referer", "https://live.bilibili.com/")
            .send()
            .await?;

        let response_text = response.text().await?;
        live_debug!("房间 {} 状态查询响应: {}", room_id, response_text);

        // 解析JSON响应
        let json_value: serde_json::Value = serde_json::from_str(&response_text)?;
        
        if let Some(code) = json_value.get("code").and_then(|c| c.as_i64()) {
            if code == 0 {
                if let Some(data) = json_value.get("data") {
                    if let Some(live_status) = data.get("live_status").and_then(|s| s.as_i64()) {
                        let status = match live_status {
                            1 => LiveStatus::Live,
                            _ => LiveStatus::NotLive,
                        };
                        
                        let title = data.get("title")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string());
                        
                        live_info!("房间 {} 初始状态检查结果: live_status={} -> {:?}, title: {:?}", 
                            room_id, live_status, status, title);

                        // 发送初始状态事件
                        event_sender.send(WebSocketEvent::LiveStatusChanged {
                            room_id,
                            status,
                            title,
                        })?;
                        
                        return Ok(());
                    }
                }
            } else {
                live_warn!("房间 {} 状态查询API返回错误: code={}", room_id, code);
            }
        }

        Err(anyhow!("无法获取房间 {} 的初始状态", room_id))
    }

    /// 获取WebSocket连接信息
    async fn get_websocket_info(room_id: i64) -> Result<WebSocketInfo> {
        let url = format!(
            "https://api.live.bilibili.com/room/v1/Danmu/getConf?room_id={}&platform=pc&player=web",
            room_id
        );
        
        live_debug!("获取房间 {} 的WebSocket配置", room_id);
        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .header("Referer", "https://live.bilibili.com/")
            .send()
            .await?;

        let config: DanmuConfigResponse = response.json().await?;
        live_debug!("房间 {} WebSocket配置响应: code={}", room_id, config.code);
        
        if config.code != 0 {
            return Err(anyhow!("获取WebSocket配置失败，错误码: {}", config.code));
        }

        if config.data.host_list.is_empty() {
            return Err(anyhow!("WebSocket主机列表为空"));
        }

        live_debug!("房间 {} 获取到 {} 个WebSocket主机", room_id, config.data.host_list.len());
        
        // 选择第一个可用的主机
        let host_info = &config.data.host_list[0];
        let host = format!("{}:{}", host_info.host, host_info.wss_port);
        live_debug!("房间 {} 选择WebSocket主机: {}:{}", room_id, host_info.host, host_info.wss_port);

        Ok(WebSocketInfo {
            host,
            token: config.data.token,
        })
    }

    /// 构建认证包
    fn build_auth_packet(room_id: i64, token: &str) -> Result<Vec<u8>> {
        use byteorder::{BigEndian, WriteBytesExt};
        use std::io::Write;

        // 构建认证JSON
        let auth_body = format!(
            r#"{{"uid":0,"roomid":{},"protover":2,"platform":"web","clientver":"2.0.11","type":2,"key":"{}"}}"#,
            room_id, token
        );
        let auth_body_bytes = auth_body.as_bytes();
        
        let mut packet = Vec::new();
        
        // 包长度 (4 bytes)
        packet.write_u32::<BigEndian>(16 + auth_body_bytes.len() as u32)?;
        
        // 包头长度 (2 bytes)
        packet.write_u16::<BigEndian>(16)?;
        
        // 协议版本 (2 bytes) - 使用2支持zlib压缩
        packet.write_u16::<BigEndian>(2)?;
        
        // 操作码 (4 bytes) - 认证包为7
        packet.write_u32::<BigEndian>(7)?;
        
        // 序列ID (4 bytes)
        packet.write_u32::<BigEndian>(1)?;
        
        // 数据
        packet.write_all(auth_body_bytes)?;
        
        Ok(packet)
    }

    /// 构建心跳包
    fn build_heartbeat_packet() -> Vec<u8> {
        use byteorder::{BigEndian, WriteBytesExt};
        
        let mut packet = Vec::new();
        
        // 包长度 (4 bytes) - 心跳包没有数据，只有包头
        let _ = packet.write_u32::<BigEndian>(16);
        
        // 包头长度 (2 bytes)
        let _ = packet.write_u16::<BigEndian>(16);
        
        // 协议版本 (2 bytes) - 使用2支持zlib压缩
        let _ = packet.write_u16::<BigEndian>(2);
        
        // 操作码 (4 bytes) - 心跳包为2
        let _ = packet.write_u32::<BigEndian>(2);
        
        // 序列ID (4 bytes)
        let _ = packet.write_u32::<BigEndian>(1);
        
        packet
    }

    /// 处理二进制消息
    async fn handle_binary_message(
        room_id: i64,
        event_sender: &mpsc::UnboundedSender<WebSocketEvent>,
        data: &[u8],
        last_live_status: &mut Option<LiveStatus>,
    ) -> Result<()> {
        use byteorder::{BigEndian, ReadBytesExt};
        use flate2::read::ZlibDecoder;
        use std::io::{Cursor, Read};

        if data.len() < 16 {
            return Ok(()); // 数据包太短，忽略
        }

        let mut cursor = Cursor::new(data);
        
        // 读取包头
        let packet_len = cursor.read_u32::<BigEndian>()?;
        let header_len = cursor.read_u16::<BigEndian>()?;
        let protocol_ver = cursor.read_u16::<BigEndian>()?;
        let operation = cursor.read_u32::<BigEndian>()?;
        let _seq_id = cursor.read_u32::<BigEndian>()?;
        
        live_debug!("房间 {} 收到消息: len={}, header_len={}, proto_ver={}, op={}", 
            room_id, packet_len, header_len, protocol_ver, operation);
        
        // 获取消息体数据
        let body_data = if protocol_ver == 2 && operation == 5 {
            // 协议版本2，需要zlib解压缩
            let compressed_data = &data[header_len as usize..];
            let mut decoder = ZlibDecoder::new(compressed_data);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed)?;
            live_debug!("房间 {} 解压缩数据: {} bytes -> {} bytes", room_id, compressed_data.len(), decompressed.len());
            
            // 递归处理解压后的数据包（可能包含多个包）
            Box::pin(Self::parse_compressed_packets(room_id, event_sender, &decompressed, last_live_status)).await?;
            return Ok(());
        } else {
            &data[header_len as usize..]
        };
        
        match operation {
            3 => {
                // 人气值消息
                if body_data.len() >= 4 {
                    let mut cursor = Cursor::new(body_data);
                    let popularity = cursor.read_u32::<BigEndian>()? as i64;
                    
                    live_debug!("房间 {} 人气值: {}", room_id, popularity);
                    
                    event_sender.send(WebSocketEvent::PopularityChanged {
                        room_id,
                        popularity,
                    })?;
                }
            }
            5 => {
                // 通知消息 - 解析JSON
                if let Ok(json_str) = std::str::from_utf8(body_data) {
                    live_debug!("房间 {} 收到通知消息: {}", room_id, json_str);
                    Self::handle_notification_message(room_id, event_sender, json_str, last_live_status).await?;
                } else {
                    live_debug!("房间 {} 收到非UTF8通知消息: {:?}", room_id, body_data);
                }
            }
            8 => {
                // 认证回复
                live_debug!("房间 {} 认证回复: {:?}", room_id, std::str::from_utf8(body_data));
                // 尝试解析为JSON
                if let Ok(json_str) = std::str::from_utf8(body_data) {
                    if json_str.contains("\"code\":0") {
                        live_info!("房间 {} 认证成功", room_id);
                    } else {
                        live_warn!("房间 {} 认证失败: {}", room_id, json_str);
                    }
                } else if body_data.len() >= 4 {
                    // 如果不是JSON，尝试解析为二进制
                    let mut cursor = Cursor::new(body_data);
                    let auth_result = cursor.read_u32::<BigEndian>()?;
                    if auth_result == 0 {
                        live_info!("房间 {} 认证成功", room_id);
                    } else {
                        live_warn!("房间 {} 认证失败: {}", room_id, auth_result);
                    }
                }
            }
            _ => {
                live_debug!("房间 {} 收到未知操作码: {}, 数据: {:?}", room_id, operation, 
                    std::str::from_utf8(body_data).unwrap_or("non-utf8"));
            }
        }

        Ok(())
    }

    /// 解析压缩包中的多个数据包
    async fn parse_compressed_packets(
        room_id: i64,
        event_sender: &mpsc::UnboundedSender<WebSocketEvent>,
        data: &[u8],
        last_live_status: &mut Option<LiveStatus>,
    ) -> Result<()> {
        use byteorder::{BigEndian, ReadBytesExt};
        use std::io::Cursor;

        let mut cursor = Cursor::new(data);
        
        while cursor.position() < data.len() as u64 {
            if data.len() - (cursor.position() as usize) < 16 {
                break; // 剩余数据不足一个包头
            }
            
            let packet_start = cursor.position() as usize;
            let packet_len = cursor.read_u32::<BigEndian>()? as usize;
            
            if packet_start + packet_len > data.len() {
                break; // 包长度超出数据范围
            }
            
            // 处理单个包
            let packet_data = &data[packet_start..packet_start + packet_len];
            Box::pin(Self::handle_binary_message(room_id, event_sender, packet_data, last_live_status)).await?;
            
            // 移动到下一个包
            cursor.set_position(packet_start as u64 + packet_len as u64);
        }
        
        Ok(())
    }

    /// 处理通知消息
    async fn handle_notification_message(
        room_id: i64,
        event_sender: &mpsc::UnboundedSender<WebSocketEvent>,
        json_str: &str,
        last_live_status: &mut Option<LiveStatus>,
    ) -> Result<()> {
        use serde_json::Value;
        
        // 尝试解析JSON
        match serde_json::from_str::<Value>(json_str) {
            Ok(json) => {
                if let Some(cmd) = json.get("cmd").and_then(|c| c.as_str()) {
                    live_debug!("房间 {} 收到命令: {}", room_id, cmd);
                    
                    match cmd {
                        "LIVE" => {
                            live_info!("房间 {} 开播通知", room_id);
                            
                            // 检查状态是否变化
                            if *last_live_status != Some(LiveStatus::Live) {
                                *last_live_status = Some(LiveStatus::Live);
                                
                                // 尝试获取标题
                                let title = json.get("data")
                                    .and_then(|data| data.get("live_title"))
                                    .and_then(|title| title.as_str())
                                    .map(|s| s.to_string());
                                
                                event_sender.send(WebSocketEvent::LiveStatusChanged {
                                    room_id,
                                    status: LiveStatus::Live,
                                    title,
                                })?;
                            }
                        }
                        "PREPARING" => {
                            live_info!("房间 {} 准备中通知", room_id);
                            
                            // 检查状态是否变化  
                            if *last_live_status != Some(LiveStatus::NotLive) {
                                *last_live_status = Some(LiveStatus::NotLive);
                                
                                event_sender.send(WebSocketEvent::LiveStatusChanged {
                                    room_id,
                                    status: LiveStatus::NotLive,
                                    title: None,
                                })?;
                            }
                        }
                        "ROOM_CHANGE" => {
                            // 房间信息变化，可能包含直播状态
                            if let Some(data) = json.get("data") {
                                if let Some(live_status) = data.get("live_status").and_then(|s| s.as_u64()) {
                                    let status = match live_status {
                                        1 => LiveStatus::Live,
                                        _ => LiveStatus::NotLive, // 2 (轮播) 也当作未直播处理
                                    };
                                    
                                    if *last_live_status != Some(status) {
                                        live_info!("房间 {} 状态变化: {:?} -> {:?}", room_id, last_live_status, status);
                                        *last_live_status = Some(status);
                                        
                                        let title = data.get("title")
                                            .and_then(|title| title.as_str())
                                            .map(|s| s.to_string());
                                        
                                        event_sender.send(WebSocketEvent::LiveStatusChanged {
                                            room_id,
                                            status,
                                            title,
                                        })?;
                                    }
                                }
                            }
                        }
                        _ => {
                            // 其他命令类型，记录但不处理
                            if cmd != "DANMU_MSG" && cmd != "SEND_GIFT" && cmd != "GUARD_BUY" {
                                live_debug!("房间 {} 收到其他命令: {}", room_id, cmd);
                            }
                        }
                    }
                } else {
                    live_debug!("房间 {} JSON消息缺少cmd字段", room_id);
                }
            }
            Err(e) => {
                // JSON解析失败，可能不是JSON格式或格式不正确
                live_debug!("房间 {} JSON解析失败: {}, 原始数据: {}", room_id, e, json_str);
                
                // 保留原来的字符串匹配方式作为后备
                if json_str.contains("\"cmd\":\"LIVE\"") {
                    live_info!("房间 {} 开播通知 (字符串匹配)", room_id);
                    
                    if *last_live_status != Some(LiveStatus::Live) {
                        *last_live_status = Some(LiveStatus::Live);
                        
                        event_sender.send(WebSocketEvent::LiveStatusChanged {
                            room_id,
                            status: LiveStatus::Live,
                            title: None,
                        })?;
                    }
                } else if json_str.contains("\"cmd\":\"PREPARING\"") {
                    live_info!("房间 {} 准备中通知 (字符串匹配)", room_id);
                    
                    if *last_live_status != Some(LiveStatus::NotLive) {
                        *last_live_status = Some(LiveStatus::NotLive);
                        
                        event_sender.send(WebSocketEvent::LiveStatusChanged {
                            room_id,
                            status: LiveStatus::NotLive,
                            title: None,
                        })?;
                    }
                }
            }
        }
        
        Ok(())
    }

    /// 停止连接
    pub async fn stop(&mut self) {
        live_info!("停止房间 {} 的 WebSocket 连接", self.room_id);
        
        if let Some(handle) = self.connection_handle.take() {
            handle.abort();
            let _ = handle.await;
        }
    }
}

impl Drop for BilibiliWebSocketClient {
    fn drop(&mut self) {
        if let Some(handle) = &self.connection_handle {
            if !handle.is_finished() {
                handle.abort();
            }
        }
    }
}

/// WebSocket 连接管理器  
pub struct WebSocketManager {
    clients: Arc<tokio::sync::RwLock<std::collections::HashMap<i64, BilibiliWebSocketClient>>>,
    event_receiver: mpsc::UnboundedReceiver<WebSocketEvent>,
    event_sender: mpsc::UnboundedSender<WebSocketEvent>,
}

impl WebSocketManager {
    /// 创建新的连接管理器
    pub fn new() -> Self {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        Self {
            clients: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            event_receiver,
            event_sender,
        }
    }

    /// 添加房间监控
    pub async fn add_room(&self, room_id: i64) -> Result<()> {
        let mut clients = self.clients.write().await;
        
        if clients.contains_key(&room_id) {
            live_debug!("房间 {} 的 WebSocket 连接已存在", room_id);
            return Ok(());
        }

        let mut client = BilibiliWebSocketClient::new(room_id, self.event_sender.clone());
        client.start().await?;
        clients.insert(room_id, client);

        live_info!("已添加房间 {} 的 WebSocket 监控", room_id);
        Ok(())
    }


    /// 获取事件接收器
    pub async fn next_event(&mut self) -> Option<WebSocketEvent> {
        self.event_receiver.recv().await
    }

    /// 获取当前连接的房间数量
    pub async fn connection_count(&self) -> usize {
        self.clients.read().await.len()
    }

    /// 停止所有连接
    pub async fn stop_all(&self) {
        let mut clients = self.clients.write().await;
        
        for (room_id, client) in clients.iter_mut() {
            live_info!("停止房间 {} 的 WebSocket 连接", room_id);
            client.stop().await;
        }
        
        clients.clear();
    }
}

impl Default for WebSocketManager {
    fn default() -> Self {
        Self::new()
    }
}