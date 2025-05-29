mod http_server;
pub mod video_downloader;

pub use http_server::http_server;
pub use video_downloader::video_downloader;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 全局任务控制器，用于控制定时扫描任务的暂停和恢复
pub struct TaskController {
    /// 是否暂停定时扫描任务
    pub is_paused: AtomicBool,
}

impl TaskController {
    pub fn new() -> Self {
        Self {
            is_paused: AtomicBool::new(false),
        }
    }

    /// 暂停定时扫描任务
    pub fn pause(&self) {
        self.is_paused.store(true, Ordering::SeqCst);
        info!("定时扫描任务已暂停");
    }

    /// 恢复定时扫描任务
    pub fn resume(&self) {
        self.is_paused.store(false, Ordering::SeqCst);
        info!("定时扫描任务已恢复");
    }

    /// 检查是否暂停
    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    /// 等待直到任务恢复（非阻塞检查）
    pub async fn wait_if_paused(&self) {
        while self.is_paused() {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }
}

impl Default for TaskController {
    fn default() -> Self {
        Self::new()
    }
}

/// 全局任务控制器实例
pub static TASK_CONTROLLER: once_cell::sync::Lazy<Arc<TaskController>> = 
    once_cell::sync::Lazy::new(|| Arc::new(TaskController::new()));

/// 暂停定时扫描任务的便捷函数
pub fn pause_scanning() {
    TASK_CONTROLLER.pause();
}

/// 恢复定时扫描任务的便捷函数
pub fn resume_scanning() {
    TASK_CONTROLLER.resume();
}
