use anyhow::{anyhow, Result};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

use super::api::LiveStatus;

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
        info!("启动房间 {} 的 WebSocket 连接", self.room_id);

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
                    info!("房间 {} WebSocket 连接正常结束", room_id);
                    break;
                }
                Err(e) => {
                    retry_count += 1;
                    error!(
                        "房间 {} WebSocket 连接失败 (重试 {}/{}): {}",
                        room_id, retry_count, max_retries, e
                    );

                    // 发送连接状态变化事件
                    if let Err(send_err) = event_sender.send(WebSocketEvent::ConnectionStatusChanged {
                        room_id,
                        connected: false,
                        error: Some(e.to_string()),
                    }) {
                        error!("发送连接状态事件失败: {}", send_err);
                    }

                    if retry_count >= max_retries {
                        error!("房间 {} WebSocket 连接重试次数已达上限，停止重连", room_id);
                        break;
                    }

                    // 指数退避策略
                    let delay = std::time::Duration::from_secs(2_u64.pow(retry_count.min(6)));
                    debug!("房间 {} 等待 {:?} 后重连", room_id, delay);
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
        debug!("房间 {} 开始建立 WebSocket 连接", room_id);

        // 连接到B站WebSocket服务器
        let ws_url = "ws://broadcastlv.chat.bilibili.com:2244/sub";
        let (ws_stream, _) = connect_async(ws_url).await?;
        
        info!("房间 {} WebSocket 连接已建立", room_id);

        // 发送连接成功事件
        event_sender.send(WebSocketEvent::ConnectionStatusChanged {
            room_id,
            connected: true,
            error: None,
        })?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // 发送认证包
        let auth_packet = Self::build_auth_packet(room_id)?;
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
                                warn!("处理房间 {} 二进制消息失败: {}", room_id, e);
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("房间 {} WebSocket 连接被服务器关闭", room_id);
                            break;
                        }
                        Some(Ok(_)) => {
                            // 忽略其他类型消息
                        }
                        Some(Err(e)) => {
                            warn!("房间 {} 接收消息出错: {}", room_id, e);
                            return Err(anyhow!("接收消息失败: {}", e));
                        }
                        None => {
                            warn!("房间 {} WebSocket 连接流结束", room_id);
                            break;
                        }
                    }
                }
                
                // 发送心跳包
                _ = heartbeat_interval.tick() => {
                    let heartbeat_packet = Self::build_heartbeat_packet();
                    if let Err(e) = ws_sender.send(Message::Binary(heartbeat_packet)).await {
                        error!("发送心跳包失败: {}", e);
                        break;
                    }
                    debug!("房间 {} 发送心跳包", room_id);
                }
            }
        }

        warn!("房间 {} WebSocket 连接流结束", room_id);
        Ok(())
    }

    /// 构建认证包
    fn build_auth_packet(room_id: i64) -> Result<Vec<u8>> {
        use byteorder::{BigEndian, WriteBytesExt};
        use std::io::Write;

        let auth_body = format!(r#"{{"roomid": {}}}"#, room_id);
        let auth_body_bytes = auth_body.as_bytes();
        
        let mut packet = Vec::new();
        
        // 包长度 (4 bytes)
        packet.write_u32::<BigEndian>(16 + auth_body_bytes.len() as u32)?;
        
        // 包头长度 (2 bytes)
        packet.write_u16::<BigEndian>(16)?;
        
        // 协议版本 (2 bytes) 
        packet.write_u16::<BigEndian>(1)?;
        
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
        
        // 协议版本 (2 bytes)
        let _ = packet.write_u16::<BigEndian>(1);
        
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
        use std::io::Cursor;

        if data.len() < 16 {
            return Ok(()); // 数据包太短，忽略
        }

        let mut cursor = Cursor::new(data);
        
        // 读取包头
        let _packet_len = cursor.read_u32::<BigEndian>()?;
        let _header_len = cursor.read_u16::<BigEndian>()?;
        let _protocol_ver = cursor.read_u16::<BigEndian>()?;
        let operation = cursor.read_u32::<BigEndian>()?;
        let _seq_id = cursor.read_u32::<BigEndian>()?;
        
        match operation {
            3 => {
                // 人气值消息
                if data.len() >= 20 {
                    let mut cursor = Cursor::new(&data[16..]);
                    let popularity = cursor.read_u32::<BigEndian>()? as i64;
                    
                    debug!("房间 {} 人气值: {}", room_id, popularity);
                    
                    event_sender.send(WebSocketEvent::PopularityChanged {
                        room_id,
                        popularity,
                    })?;
                }
            }
            5 => {
                // 通知消息 - 解析JSON
                let json_data = &data[16..];
                if let Ok(json_str) = std::str::from_utf8(json_data) {
                    Self::handle_notification_message(room_id, event_sender, json_str, last_live_status).await?;
                }
            }
            8 => {
                // 认证回复，暂时忽略
                debug!("房间 {} 认证回复", room_id);
            }
            _ => {
                debug!("房间 {} 收到未知操作码: {}", room_id, operation);
            }
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
        // 这里可以解析JSON消息来获取直播状态变化
        // 由于消息格式复杂，暂时简化处理
        
        if json_str.contains("\"cmd\":\"LIVE\"") {
            debug!("房间 {} 开播通知", room_id);
            
            // 检查状态是否变化
            if *last_live_status != Some(LiveStatus::Live) {
                *last_live_status = Some(LiveStatus::Live);
                
                event_sender.send(WebSocketEvent::LiveStatusChanged {
                    room_id,
                    status: LiveStatus::Live,
                    title: None,
                })?;
            }
        } else if json_str.contains("\"cmd\":\"PREPARING\"") {
            debug!("房间 {} 准备中通知", room_id);
            
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
        // 可以添加更多消息类型的处理
        
        Ok(())
    }

    /// 停止连接
    pub async fn stop(&mut self) {
        info!("停止房间 {} 的 WebSocket 连接", self.room_id);
        
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
            debug!("房间 {} 的 WebSocket 连接已存在", room_id);
            return Ok(());
        }

        let mut client = BilibiliWebSocketClient::new(room_id, self.event_sender.clone());
        client.start().await?;
        clients.insert(room_id, client);

        info!("已添加房间 {} 的 WebSocket 监控", room_id);
        Ok(())
    }

    /// 移除房间监控
    pub async fn remove_room(&self, room_id: i64) -> Result<()> {
        let mut clients = self.clients.write().await;
        
        if let Some(mut client) = clients.remove(&room_id) {
            client.stop().await;
            info!("已移除房间 {} 的 WebSocket 监控", room_id);
        }

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
            info!("停止房间 {} 的 WebSocket 连接", room_id);
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