use std::sync::Arc;
use anyhow::Result;
use sea_orm::DatabaseConnection;
use tracing::{info, error};
use tokio_util::sync::CancellationToken;

use crate::bilibili::BiliClient;
use crate::live::LiveMonitor;

/// 直播监控任务
/// 
/// 这个任务会：
/// 1. 创建直播监控服务实例
/// 2. 启动监控循环
/// 3. 持续监控配置的直播间
/// 4. 在直播状态变化时自动开始/停止录制
/// 5. 在收到取消信号时正确停止所有录制并等待合并完成
pub async fn live_monitor_service(
    database_connection: Arc<DatabaseConnection>,
    cancellation_token: CancellationToken,
) -> Result<()> {
    info!("启动直播监控服务");
    
    // 创建BiliClient实例 (使用空cookie，直播监控不需要登录)
    let bili_client = Arc::new(BiliClient::new(String::new()));
    
    // 创建LiveMonitor实例并包装在Arc中，以便在取消时能访问同一实例
    let monitor = Arc::new(LiveMonitor::new((*database_connection).clone(), bili_client));
    
    // 克隆Arc用于后续停止操作
    let monitor_for_stop = monitor.clone();
    
    // 启动监控服务
    if let Err(e) = monitor.start().await {
        error!("直播监控服务启动失败: {}", e);
        return Err(e);
    }
    
    info!("直播监控服务已启动");
    
    // 等待取消信号
    cancellation_token.cancelled().await;
    
    info!("收到停止信号，正在停止直播监控服务");
    
    // 使用克隆的Arc停止监控服务，确保调用的是同一个实例
    // 这会等待所有录制停止和FFmpeg合并完成
    if let Err(e) = monitor_for_stop.stop().await {
        error!("直播监控服务停止失败: {}", e);
    } else {
        info!("直播监控服务已停止");
    }
    
    Ok(())
}