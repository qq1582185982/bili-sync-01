use std::sync::Arc;
use anyhow::Result;
use sea_orm::DatabaseConnection;
use tracing::{info, error};
use std::future::pending;

use crate::bilibili::BiliClient;
use crate::live::LiveMonitor;

/// 直播监控任务
/// 
/// 这个任务会：
/// 1. 创建直播监控服务实例
/// 2. 启动监控循环
/// 3. 持续监控配置的直播间
/// 4. 在直播状态变化时自动开始/停止录制
pub async fn live_monitor_service(database_connection: Arc<DatabaseConnection>) -> Result<()> {
    info!("启动直播监控服务");
    
    // 创建BiliClient实例 (使用空cookie，直播监控不需要登录)
    let bili_client = Arc::new(BiliClient::new(String::new()));
    
    // 创建LiveMonitor实例
    let monitor = LiveMonitor::new((*database_connection).clone(), bili_client);
    
    // 启动监控服务
    if let Err(e) = monitor.start().await {
        error!("直播监控服务启动失败: {}", e);
        return Err(e);
    }
    
    info!("直播监控服务已启动");
    
    // 使用pending()创建一个永不完成的future
    // 当main.rs中的tokio::select!收到取消信号时，这个future会被取消
    pending::<()>().await;
    
    // 这里只有在任务被取消时才会到达
    info!("收到停止信号，正在停止直播监控服务");
    
    // 停止监控服务
    if let Err(e) = monitor.stop().await {
        error!("直播监控服务停止失败: {}", e);
    } else {
        info!("直播监控服务已停止");
    }
    
    Ok(())
}