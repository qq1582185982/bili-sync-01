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
use super::recorder::{LiveRecorder, RecordStatus};
use super::LiveError;

/// 监控配置
#[derive(Debug, Clone)]
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
pub struct RecorderInfo {
    pub recorder: LiveRecorder,
    pub record_id: i32,
    pub start_time: Instant,
}

/// 直播监控管理器
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

    /// 静态方法：重新加载监控配置（用于spawned task中）
    async fn reload_configs_static(
        db: &DatabaseConnection, 
        configs: &Arc<RwLock<Vec<MonitorConfig>>>
    ) -> Result<()> {
        let models = live_monitor::Entity::find()
            .filter(live_monitor::Column::Enabled.eq(true))
            .all(db)
            .await?;

        let new_configs: Vec<MonitorConfig> = models.into_iter().map(MonitorConfig::from).collect();
        
        // 详细显示每个监控配置的状态
        for config in &new_configs {
            debug!(
                "重新加载配置 - ID: {}, UP主: {}, 房间: {}, 当前状态: {:?}", 
                config.id, config.upper_name, config.room_id, config.last_status
            );
        }

        *configs.write().await = new_configs;
        Ok(())
    }

    /// 启动监控循环
    async fn start_monitor_loop(&self) -> JoinHandle<()> {
        let db = self.db.clone();
        let bili_client = Arc::clone(&self.bili_client);
        let configs = Arc::clone(&self.configs);
        let recorders = Arc::clone(&self.recorders);
        let running = Arc::clone(&self.running);

        tokio::spawn(async move {
            // 在spawned task中创建LiveApiClient
            let live_client = unsafe { 
                std::mem::transmute::<LiveApiClient<'_>, LiveApiClient<'static>>(
                    LiveApiClient::new(&*bili_client)
                )
            };
            
            let mut interval = interval(Duration::from_secs(30)); // 默认30秒检查一次

            loop {
                interval.tick().await;

                if !*running.read().await {
                    break;
                }

                let configs_guard = configs.read().await;
                if configs_guard.is_empty() {
                    continue;
                }

                for config in configs_guard.iter() {
                    if !config.enabled {
                        continue;
                    }

                    if let Err(e) = Self::check_room_status(&db, &live_client, config, &recorders).await {
                        error!("检查房间 {} 状态失败: {}", config.room_id, e);
                    }
                }
                
                // 每次检查完所有房间后，重新加载配置以同步状态变化
                drop(configs_guard);
                if let Err(e) = Self::reload_configs_static(&db, &configs).await {
                    error!("重新加载监控配置失败: {}", e);
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

        // 获取直播流地址
        let play_info = live_client.get_play_url(config.room_id, config.quality).await?;
        
        if play_info.durl.is_empty() {
            return Err(anyhow!("无法获取直播流地址"));
        }

        let stream_url = &play_info.durl[0].url;

        // 生成输出文件名
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let safe_title = crate::utils::filenamify::filenamify(&room_info.title);
        let filename = format!("{}_{}_{}_{}.{}", 
            config.upper_name, config.room_id, timestamp, safe_title, config.format);
        let output_path = config.path.join(filename);

        // 确保输出目录存在
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // 创建录制记录
        let record = live_record::ActiveModel {
            id: ActiveValue::NotSet,
            monitor_id: ActiveValue::Set(config.id),
            room_id: ActiveValue::Set(config.room_id),
            title: ActiveValue::Set(Some(room_info.title.clone())),
            start_time: ActiveValue::Set(now_standard_string()),
            end_time: ActiveValue::NotSet,
            file_path: ActiveValue::Set(Some(output_path.to_string_lossy().to_string())),
            file_size: ActiveValue::NotSet,
            status: ActiveValue::Set(0), // 录制中
        };

        let record_result = record.insert(db).await?;

        // 启动录制器
        let mut recorder = LiveRecorder::new(output_path.clone());
        recorder.start(stream_url.clone()).await?;

        // 保存录制器信息
        let recorder_info = RecorderInfo {
            recorder,
            record_id: record_result.id,
            start_time: Instant::now(),
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
pub struct MonitorStatus {
    pub running: bool,
    pub total_monitors: usize,
    pub enabled_monitors: usize,
    pub active_recordings: usize,
}