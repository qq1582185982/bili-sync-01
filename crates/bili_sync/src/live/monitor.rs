use anyhow::{anyhow, Result};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration, Instant};
use tracing::{debug, error, info, warn};

use bili_sync_entity::{live_monitor, live_record};
use crate::bilibili::BiliClient;
use crate::utils::time_format::now_standard_string;

use super::api::{LiveApiClient, LiveStatus, Quality};
use super::recorder::LiveRecorder;
use super::ws_client::{WebSocketEvent, WebSocketManager};

/// 监控配置
#[derive(Debug, Clone)]
#[allow(dead_code)] // 配置结构体，部分字段暂时未使用但需要保留
pub struct MonitorConfig {
    pub id: i32,
    pub upper_id: i64,
    pub upper_name: String,
    pub room_id: i64,
    pub short_room_id: Option<i64>,
    pub path: PathBuf,
    pub enabled: bool,
    pub check_interval: Duration,
    pub quality: Quality,
    pub format: String,
    pub last_status: LiveStatus,
}

impl From<live_monitor::Model> for MonitorConfig {
    fn from(model: live_monitor::Model) -> Self {
        Self {
            id: model.id,
            upper_id: model.upper_id,
            upper_name: model.upper_name,
            room_id: model.room_id,
            short_room_id: model.short_room_id,
            path: PathBuf::from(model.path),
            enabled: model.enabled,
            check_interval: Duration::from_secs(model.check_interval as u64),
            quality: Quality::from(model.quality.as_str()),
            format: model.format,
            last_status: LiveStatus::from(model.last_status),
        }
    }
}

/// 录制器状态信息
#[derive(Debug)]
#[allow(dead_code)] // 录制器信息结构体，部分字段暂时未使用但需要保留
pub struct RecorderInfo {
    pub recorder: LiveRecorder,
    pub record_id: i32,
    pub start_time: Instant,
    pub retry_count: u32,
    pub last_failure_time: Option<Instant>,
}

/// 直播监控管理器
#[allow(dead_code)] // 监控器结构体，部分字段暂时未使用但需要保留
pub struct LiveMonitor {
    /// 数据库连接
    db: DatabaseConnection,
    /// B站API客户端
    bili_client: Arc<BiliClient>,
    /// 直播API客户端
    live_client: LiveApiClient<'static>,
    /// 监控配置列表
    configs: Arc<RwLock<Vec<MonitorConfig>>>,
    /// 活跃的录制器
    recorders: Arc<Mutex<HashMap<i32, RecorderInfo>>>,
    /// 监控任务句柄
    monitor_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// 是否正在运行
    running: Arc<RwLock<bool>>,
    /// WebSocket 管理器
    ws_manager: Arc<Mutex<WebSocketManager>>,
}

impl LiveMonitor {
    /// 创建新的直播监控器
    pub fn new(db: DatabaseConnection, bili_client: Arc<BiliClient>) -> Self {
        // 这里需要使用unsafe来扩展生命周期，因为LiveApiClient需要静态生命周期
        // 但bili_client是Arc包装的，实际上是安全的
        let live_client = unsafe { 
            std::mem::transmute::<LiveApiClient<'_>, LiveApiClient<'static>>(
                LiveApiClient::new(&*bili_client)
            )
        };

        Self {
            db,
            bili_client,
            live_client,
            configs: Arc::new(RwLock::new(Vec::new())),
            recorders: Arc::new(Mutex::new(HashMap::new())),
            monitor_handle: Arc::new(Mutex::new(None)),
            running: Arc::new(RwLock::new(false)),
            ws_manager: Arc::new(Mutex::new(WebSocketManager::new())),
        }
    }

    /// 启动监控服务
    pub async fn start(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(()); // 已经在运行中
        }

        info!("启动直播监控服务");

        // 加载监控配置
        self.reload_configs().await?;

        *running = true;
        drop(running);

        // 启动监控循环
        let monitor_handle = self.start_monitor_loop().await;
        *self.monitor_handle.lock().await = Some(monitor_handle);

        Ok(())
    }

    /// 停止监控服务  
    pub async fn stop(&self) -> Result<()> {
        info!("停止直播监控服务");

        let mut running = self.running.write().await;
        *running = false;
        drop(running);

        // 停止监控循环
        if let Some(handle) = self.monitor_handle.lock().await.take() {
            handle.abort();
        }

        // 停止所有录制
        self.stop_all_recordings().await?;

        // 停止所有WebSocket连接
        self.ws_manager.lock().await.stop_all().await;

        Ok(())
    }

    /// 重新加载监控配置
    pub async fn reload_configs(&self) -> Result<()> {
        let models = live_monitor::Entity::find()
            .filter(live_monitor::Column::Enabled.eq(true))
            .all(&self.db)
            .await?;

        let configs: Vec<MonitorConfig> = models.into_iter().map(MonitorConfig::from).collect();
        
        info!("加载了 {} 个直播监控配置", configs.len());
        
        // 详细显示每个监控配置的状态
        for config in &configs {
            debug!(
                "监控配置 - ID: {}, UP主: {}, 房间: {}, 当前状态: {:?}", 
                config.id, config.upper_name, config.room_id, config.last_status
            );
        }

        *self.configs.write().await = configs;
        Ok(())
    }

    /// 静态方法：重新加载监控配置（用于spawned task中），返回是否有变化
    async fn reload_configs_static(
        db: &DatabaseConnection, 
        configs: &Arc<RwLock<Vec<MonitorConfig>>>
    ) -> Result<bool> {
        let models = live_monitor::Entity::find()
            .filter(live_monitor::Column::Enabled.eq(true))
            .all(db)
            .await?;

        let new_configs: Vec<MonitorConfig> = models.into_iter().map(MonitorConfig::from).collect();
        
        // 检查配置是否有变化
        let mut configs_guard = configs.write().await;
        let old_configs = &*configs_guard;
        
        // 比较配置是否相同（通过房间ID集合比较）
        let old_rooms: std::collections::HashSet<i64> = old_configs.iter().map(|c| c.room_id).collect();
        let new_rooms: std::collections::HashSet<i64> = new_configs.iter().map(|c| c.room_id).collect();
        
        let has_changes = old_rooms != new_rooms;
        
        if has_changes {
            info!(
                "监控配置发生变化 - 旧房间: {:?}, 新房间: {:?}", 
                old_rooms, new_rooms
            );
            
            // 详细显示每个监控配置的状态
            for config in &new_configs {
                debug!(
                    "更新配置 - ID: {}, UP主: {}, 房间: {}, 当前状态: {:?}", 
                    config.id, config.upper_name, config.room_id, config.last_status
                );
            }
        } else {
            debug!("监控配置无变化，跳过WebSocket连接更新");
        }

        *configs_guard = new_configs;
        drop(configs_guard);
        
        Ok(has_changes)
    }

    /// 启动监控循环 (基于WebSocket事件)
    async fn start_monitor_loop(&self) -> JoinHandle<()> {
        let db = self.db.clone();
        let bili_client = Arc::clone(&self.bili_client);
        let configs = Arc::clone(&self.configs);
        let recorders = Arc::clone(&self.recorders);
        let running = Arc::clone(&self.running);
        let ws_manager = Arc::clone(&self.ws_manager);

        tokio::spawn(async move {
            // 在spawned task中创建LiveApiClient
            let live_client = unsafe { 
                std::mem::transmute::<LiveApiClient<'_>, LiveApiClient<'static>>(
                    LiveApiClient::new(&*bili_client)
                )
            };

            // 首次设置WebSocket连接
            Self::setup_websocket_connections(&db, &configs, &ws_manager).await;

            // 创建定期检查任务（检查录制器状态和配置变化）
            let mut check_interval = interval(Duration::from_secs(60)); // 每分钟检查一次

            loop {
                // 检查是否应该停止运行
                if !*running.read().await {
                    break;
                }

                tokio::select! {
                    // 处理WebSocket事件
                    event = async {
                        let mut manager = ws_manager.lock().await;
                        manager.next_event().await
                    } => {
                        if let Some(event) = event {
                            if let Err(e) = Self::handle_websocket_event(
                                &db,
                                &live_client,
                                &configs,
                                &recorders,
                                event
                            ).await {
                                error!("处理WebSocket事件失败: {}", e);
                            }
                        }
                    }
                    
                    // 定期检查任务
                    _ = check_interval.tick() => {
                        if !*running.read().await {
                            break;
                        }

                        // 检查录制器状态
                        Self::check_recorder_status(&db, &recorders).await;
                        
                        // 重新加载配置并更新WebSocket连接
                        match Self::reload_configs_static(&db, &configs).await {
                            Ok(has_changes) => {
                                if has_changes {
                                    // 配置变化后重新设置WebSocket连接
                                    Self::setup_websocket_connections(&db, &configs, &ws_manager).await;
                                }
                            }
                            Err(e) => {
                                error!("重新加载监控配置失败: {}", e);
                            }
                        }
                    }
                }
            }

            info!("直播监控循环已停止");
        })
    }

    /// 检查单个房间状态
    async fn check_room_status(
        db: &DatabaseConnection,
        live_client: &LiveApiClient<'static>,
        config: &MonitorConfig,
        recorders: &Arc<Mutex<HashMap<i32, RecorderInfo>>>,
    ) -> Result<()> {
        debug!("检查房间 {} ({}) 的状态", config.room_id, config.upper_name);

        // 获取当前直播状态（使用房间ID获取更准确的状态）
        debug!("调用新的API获取房间 {} 状态", config.room_id);
        let (current_status, room_info) = live_client.get_live_status_by_room_id(config.room_id).await?;
        debug!("房间 {} 当前状态: {:?}, room_info存在: {}", config.room_id, current_status, room_info.is_some());

        // 检查是否有活跃录制
        let has_active_recorder = recorders.lock().await.contains_key(&config.id);

        if current_status != config.last_status {
            info!(
                "房间 {} 状态变化: {:?} -> {:?}",
                config.room_id, config.last_status, current_status
            );

            match current_status {
                LiveStatus::Live => {
                    // 开播，启动录制
                    if let Some(room_info) = room_info {
                        Self::start_recording(db, live_client, config, &room_info, recorders).await?;
                    }
                }
                LiveStatus::NotLive => {
                    // 关播，停止录制
                    Self::stop_recording(db, config.id, recorders).await?;
                }
            }

            // 更新数据库中的状态
            Self::update_monitor_status(db, config.id, current_status).await?;
        } else if current_status == LiveStatus::Live && !has_active_recorder {
            // 特殊情况：检测到直播中但没有活跃录制，可能是之前的录制进程意外终止
            warn!("检测到直播中但没有活跃录制，重新启动录制进程");
            if let Some(room_info) = room_info {
                if let Err(e) = Self::start_recording(db, live_client, config, &room_info, recorders).await {
                    error!("重新启动录制失败: {}", e);
                }
            }
        }

        Ok(())
    }

    /// 启动录制
    async fn start_recording(
        db: &DatabaseConnection,
        live_client: &LiveApiClient<'static>,
        config: &MonitorConfig,
        room_info: &super::api::LiveRoomInfo,
        recorders: &Arc<Mutex<HashMap<i32, RecorderInfo>>>,
    ) -> Result<()> {
        info!("开始录制 {} 的直播: {}", config.upper_name, room_info.title);
        debug!("录制配置 - 房间ID: {}, 质量: {:?}, 格式: {}", config.room_id, config.quality, config.format);

        // 获取直播流地址
        debug!("正在获取直播流地址...");
        let play_info = match live_client.get_play_url(config.room_id, config.quality).await {
            Ok(info) => {
                debug!("成功获取直播流信息，流数量: {}", info.durl.len());
                info
            }
            Err(e) => {
                error!("获取直播流地址失败: {}", e);
                return Err(anyhow!("获取直播流地址失败: {}", e));
            }
        };
        
        if play_info.durl.is_empty() {
            error!("直播流地址列表为空");
            return Err(anyhow!("无法获取直播流地址"));
        }

        let stream_url = &play_info.durl[0].url;
        debug!("直播流地址: {}", stream_url);

        // 生成输出文件名
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let safe_title = crate::utils::filenamify::filenamify(&room_info.title);
        let filename = format!("{}_{}_{}_{}.{}", 
            config.upper_name, config.room_id, timestamp, safe_title, config.format);
        let mut output_path = config.path.join(filename);
        
        // 规范化路径分隔符，确保在Windows下使用反斜杠
        if cfg!(windows) {
            let path_str = output_path.to_string_lossy().replace("/", "\\");
            output_path = PathBuf::from(path_str);
        }

        // 确保输出目录存在
        if let Some(parent) = output_path.parent() {
            debug!("创建输出目录: {:?}", parent);
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                error!("创建输出目录失败: {}", e);
                anyhow!("创建输出目录失败: {}", e)
            })?;
        }

        // 创建录制记录
        debug!("创建录制记录到数据库");
        let record = live_record::ActiveModel {
            id: ActiveValue::NotSet,
            monitor_id: ActiveValue::Set(config.id),
            room_id: ActiveValue::Set(config.room_id),
            title: ActiveValue::Set(Some(room_info.title.clone())),
            start_time: ActiveValue::Set(now_standard_string()),
            end_time: ActiveValue::NotSet,
            file_path: ActiveValue::Set(Some(output_path.to_string_lossy().to_string())),
            file_size: ActiveValue::NotSet,
            status: ActiveValue::Set(1), // 1=录制中
        };

        let record_result = match record.insert(db).await {
            Ok(result) => {
                debug!("录制记录已创建，ID: {}", result.id);
                result
            }
            Err(e) => {
                error!("创建录制记录失败: {}", e);
                return Err(anyhow!("创建录制记录失败: {}", e));
            }
        };

        // 启动录制器
        debug!("启动录制器，输出文件: {:?}", output_path);
        let mut recorder = LiveRecorder::new(output_path.clone());
        if let Err(e) = recorder.start(stream_url.clone()).await {
            error!("启动录制器失败: {}", e);
            // 启动失败时，更新录制记录状态为错误
            if let Err(db_err) = Self::update_record_status(db, record_result.id, 3).await {
                error!("更新录制记录状态失败: {}", db_err);
            }
            return Err(anyhow!("启动录制器失败: {}", e));
        }

        // 录制器启动成功，确保状态为录制中
        if let Err(e) = Self::update_record_status(db, record_result.id, 1).await {
            error!("更新录制状态为录制中失败: {}", e);
        }

        // 保存录制器信息
        let recorder_info = RecorderInfo {
            recorder,
            record_id: record_result.id,
            start_time: Instant::now(),
            retry_count: 0,
            last_failure_time: None,
        };

        recorders.lock().await.insert(config.id, recorder_info);

        info!("录制已启动，输出文件: {:?}", output_path);
        Ok(())
    }

    /// 停止录制
    async fn stop_recording(
        db: &DatabaseConnection,
        monitor_id: i32,
        recorders: &Arc<Mutex<HashMap<i32, RecorderInfo>>>,
    ) -> Result<()> {
        let mut recorders_guard = recorders.lock().await;
        
        if let Some(recorder_info) = recorders_guard.remove(&monitor_id) {
            info!("停止录制，监控ID: {}", monitor_id);

            // 停止录制器
            let mut recorder = recorder_info.recorder;
            if let Err(e) = recorder.stop().await {
                error!("停止录制器失败: {}", e);
            }

            // 获取文件大小
            let file_size = if let Some(ref path) = recorder.output_path() {
                match tokio::fs::metadata(path).await {
                    Ok(metadata) => Some(metadata.len() as i64),
                    Err(e) => {
                        warn!("无法获取录制文件大小: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            // 更新录制记录
            let mut record: live_record::ActiveModel = live_record::Entity::find_by_id(recorder_info.record_id)
                .one(db)
                .await?
                .ok_or_else(|| anyhow!("录制记录不存在"))?
                .into();

            record.end_time = ActiveValue::Set(Some(now_standard_string()));
            record.file_size = ActiveValue::Set(file_size);
            record.status = ActiveValue::Set(1); // 完成

            record.update(db).await?;

            info!("录制已停止并保存，记录ID: {}", recorder_info.record_id);
        }

        Ok(())
    }

    /// 停止所有录制
    async fn stop_all_recordings(&self) -> Result<()> {
        let monitor_ids: Vec<i32> = {
            let recorders = self.recorders.lock().await;
            recorders.keys().copied().collect()
        };

        for monitor_id in monitor_ids {
            if let Err(e) = Self::stop_recording(&self.db, monitor_id, &self.recorders).await {
                error!("停止录制失败，监控ID {}: {}", monitor_id, e);
            }
        }

        Ok(())
    }

    /// 更新监控状态
    async fn update_monitor_status(
        db: &DatabaseConnection,
        monitor_id: i32,
        status: LiveStatus,
    ) -> Result<()> {
        let mut model: live_monitor::ActiveModel = live_monitor::Entity::find_by_id(monitor_id)
            .one(db)
            .await?
            .ok_or_else(|| anyhow!("监控配置不存在"))?
            .into();

        model.last_status = ActiveValue::Set(status as i32);
        model.last_check_at = ActiveValue::Set(Some(now_standard_string()));

        model.update(db).await?;
        Ok(())
    }

    /// 更新录制记录状态
    async fn update_record_status(
        db: &DatabaseConnection,
        record_id: i32,
        status: i32,
    ) -> Result<()> {
        let mut model: live_record::ActiveModel = live_record::Entity::find_by_id(record_id)
            .one(db)
            .await?
            .ok_or_else(|| anyhow!("录制记录不存在"))?
            .into();

        model.status = ActiveValue::Set(status);
        if status != 0 { // 如果不是录制中状态，设置结束时间
            model.end_time = ActiveValue::Set(Some(now_standard_string()));
        }

        model.update(db).await?;
        Ok(())
    }

    /// 检查录制器进程状态
    async fn check_recorder_status(
        db: &DatabaseConnection,
        recorders: &Arc<Mutex<HashMap<i32, RecorderInfo>>>,
    ) {
        let mut failed_recorders = Vec::new();

        // 检查所有活跃的录制器
        {
            let mut recorders_guard = recorders.lock().await;
            
            for (monitor_id, recorder_info) in recorders_guard.iter_mut() {
                match recorder_info.recorder.check_process_status() {
                    Ok(is_running) => {
                        if !is_running {
                            warn!("录制器进程已停止，监控ID: {}", monitor_id);
                            failed_recorders.push((*monitor_id, recorder_info.record_id));
                        }
                    }
                    Err(e) => {
                        error!("检查录制器进程状态失败，监控ID: {}, 错误: {}", monitor_id, e);
                        failed_recorders.push((*monitor_id, recorder_info.record_id));
                    }
                }
            }

            // 移除失败的录制器
            for (monitor_id, _) in &failed_recorders {
                recorders_guard.remove(monitor_id);
            }
        }

        // 更新数据库中失败录制的状态
        for (monitor_id, record_id) in failed_recorders {
            debug!("更新失败录制记录状态，监控ID: {}, 录制记录ID: {}", monitor_id, record_id);
            if let Err(e) = Self::update_record_status(db, record_id, 3).await {
                error!("更新录制记录状态失败: {}", e);
            }
        }
    }

    /// 设置WebSocket连接
    async fn setup_websocket_connections(
        _db: &DatabaseConnection,
        configs: &Arc<RwLock<Vec<MonitorConfig>>>,
        ws_manager: &Arc<Mutex<WebSocketManager>>,
    ) {
        let configs_guard = configs.read().await;
        let manager = ws_manager.lock().await;

        // 获取当前需要监控的房间
        let enabled_rooms: std::collections::HashSet<i64> = configs_guard
            .iter()
            .filter(|config| config.enabled)
            .map(|config| config.room_id)
            .collect();

        debug!("需要监控的房间: {:?}", enabled_rooms);

        // 获取当前已连接的房间数量
        let current_connections = manager.connection_count().await;
        debug!("当前WebSocket连接数: {}", current_connections);

        // 如果房间数量相同，可能不需要更新（但这里仍然尝试添加，因为add_room有防重复逻辑）
        let mut added_count = 0;
        let mut failed_count = 0;

        // 添加新的连接
        for &room_id in &enabled_rooms {
            match manager.add_room(room_id).await {
                Ok(_) => {
                    added_count += 1;
                }
                Err(e) => {
                    error!("添加房间 {} 的WebSocket连接失败: {}", room_id, e);
                    failed_count += 1;
                }
            }
        }

        let final_connections = manager.connection_count().await;
        
        if added_count > 0 || failed_count > 0 {
            info!(
                "WebSocket连接更新完成 - 目标房间: {}, 最终连接数: {}, 新增: {}, 失败: {}", 
                enabled_rooms.len(), final_connections, added_count, failed_count
            );
        } else {
            debug!("WebSocket连接无需更新，当前监控 {} 个房间", final_connections);
        }
    }

    /// 处理WebSocket事件
    async fn handle_websocket_event(
        db: &DatabaseConnection,
        live_client: &LiveApiClient<'static>,
        configs: &Arc<RwLock<Vec<MonitorConfig>>>,
        recorders: &Arc<Mutex<HashMap<i32, RecorderInfo>>>,
        event: WebSocketEvent,
    ) -> Result<()> {
        match event {
            WebSocketEvent::LiveStatusChanged { room_id, status, title } => {
                info!(
                    "房间 {} 状态变化: {:?}, 标题: {:?}",
                    room_id, status, title
                );

                // 查找对应的监控配置
                let configs_guard = configs.read().await;
                let config = configs_guard
                    .iter()
                    .find(|c| c.room_id == room_id && c.enabled);

                if let Some(config) = config {
                    match status {
                        LiveStatus::Live => {
                            // 开播，启动录制
                            debug!("房间 {} 开播，获取直播信息并启动录制", room_id);
                            if let Ok((_, room_info)) = live_client.get_live_status_by_room_id(room_id).await {
                                if let Some(room_info) = room_info {
                                    if let Err(e) = Self::start_recording(db, live_client, config, &room_info, recorders).await {
                                        error!("启动录制失败: {}", e);
                                    }
                                } else {
                                    warn!("无法获取房间 {} 的详细信息", room_id);
                                }
                            } else {
                                warn!("获取房间 {} 状态失败", room_id);
                            }
                        }
                        LiveStatus::NotLive => {
                            // 关播，停止录制
                            debug!("房间 {} 关播，停止录制", room_id);
                            if let Err(e) = Self::stop_recording(db, config.id, recorders).await {
                                error!("停止录制失败: {}", e);
                            }
                        }
                    }

                    // 更新数据库中的状态
                    if let Err(e) = Self::update_monitor_status(db, config.id, status).await {
                        error!("更新监控状态失败: {}", e);
                    }
                } else {
                    debug!("房间 {} 不在当前监控列表中或已禁用", room_id);
                }
            }
            WebSocketEvent::ConnectionStatusChanged { room_id, connected, error } => {
                if connected {
                    info!("房间 {} WebSocket 连接已建立", room_id);
                } else {
                    warn!("房间 {} WebSocket 连接断开: {:?}", room_id, error);
                }
            }
            WebSocketEvent::PopularityChanged { room_id, popularity } => {
                debug!("房间 {} 人气值更新: {}", room_id, popularity);
                // 暂时不处理人气值变化
            }
        }

        Ok(())
    }

    /// 获取监控状态统计
    pub async fn get_status(&self) -> Result<MonitorStatus> {
        let configs = self.configs.read().await;
        let recorders = self.recorders.lock().await;
        let running = *self.running.read().await;

        Ok(MonitorStatus {
            running,
            total_monitors: configs.len(),
            enabled_monitors: configs.iter().filter(|c| c.enabled).count(),
            active_recordings: recorders.len(),
        })
    }
}

/// 监控状态信息
#[derive(Debug)]
#[allow(dead_code)] // 状态结构体，暂时未使用但需要保留
pub struct MonitorStatus {
    pub running: bool,
    pub total_monitors: usize,
    pub enabled_monitors: usize,
    pub active_recordings: usize,
}