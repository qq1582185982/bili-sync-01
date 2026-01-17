use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use once_cell::sync::Lazy;
use tokio::sync::{oneshot, Mutex, Notify};

use super::{CaptchaInfo, CaptchaResult, CaptchaSolver};
use crate::config::RiskControlConfig;

/// 验证请求类型
#[derive(Debug)]
pub enum VerificationRequest {
    /// 开始新的验证流程
    StartNew(CaptchaInfo),
    /// 等待现有验证完成
    WaitForExisting,
    /// 使用缓存的token
    UseCache(String),
}

/// 验证状态
#[derive(Debug)]
enum VerificationState {
    /// 空闲状态
    Idle,
    /// 等待用户验证
    WaitingForUser {
        captcha_info: CaptchaInfo,
        result_sender: Option<oneshot::Sender<CaptchaResult>>,
    },
    /// 验证完成，token可用
    Completed { gaia_vtoken: String, expires_at: Instant },
}

/// 全局验证协调器
pub struct VerificationCoordinator {
    state: Arc<Mutex<VerificationState>>,
    notify: Arc<Notify>,
}

impl VerificationCoordinator {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(VerificationState::Idle)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// 请求验证，返回应该执行的操作
    pub async fn request_verification(
        &self,
        v_voucher: String,
        captcha_info: CaptchaInfo,
    ) -> Result<VerificationRequest> {
        let mut state = self.state.lock().await;

        match &*state {
            VerificationState::Idle => {
                tracing::info!("启动新的验证流程，v_voucher: {}", v_voucher);
                // 设置为等待状态
                *state = VerificationState::WaitingForUser {
                    captcha_info: captcha_info.clone(),
                    result_sender: None,
                };
                Ok(VerificationRequest::StartNew(captcha_info))
            }
            VerificationState::WaitingForUser { .. } => {
                tracing::info!("检测到正在进行的验证，等待完成...");
                Ok(VerificationRequest::WaitForExisting)
            }
            VerificationState::Completed {
                gaia_vtoken,
                expires_at,
            } => {
                if expires_at > &Instant::now() {
                    tracing::info!("使用缓存的gaia_vtoken");
                    Ok(VerificationRequest::UseCache(gaia_vtoken.clone()))
                } else {
                    tracing::info!("缓存的token已过期，启动新的验证流程");
                    *state = VerificationState::WaitingForUser {
                        captcha_info: captcha_info.clone(),
                        result_sender: None,
                    };
                    Ok(VerificationRequest::StartNew(captcha_info))
                }
            }
        }
    }

    /// 获取当前验证码信息（用于API）
    pub async fn get_captcha_info(&self) -> Option<CaptchaInfo> {
        let state = self.state.lock().await;
        match &*state {
            VerificationState::WaitingForUser { captcha_info, .. } => Some(captcha_info.clone()),
            _ => None,
        }
    }

    /// 提交验证结果（用于API）
    pub async fn submit_captcha_result(&self, result: CaptchaResult) -> Result<()> {
        let mut state = self.state.lock().await;

        match std::mem::replace(&mut *state, VerificationState::Idle) {
            VerificationState::WaitingForUser { result_sender, .. } => {
                if let Some(sender) = result_sender {
                    let _ = sender.send(result);
                }
                tracing::info!("验证结果已提交，等待处理...");
                Ok(())
            }
            old_state => {
                *state = old_state;
                anyhow::bail!("当前没有等待验证的状态");
            }
        }
    }

    /// 等待验证完成，返回验证结果
    pub async fn wait_for_captcha_result(&self) -> Result<CaptchaResult> {
        // 首先设置result_sender
        let receiver = {
            let mut state = self.state.lock().await;
            match &mut *state {
                VerificationState::WaitingForUser { result_sender, .. } => {
                    let (tx, rx) = oneshot::channel();
                    *result_sender = Some(tx);
                    rx
                }
                _ => {
                    anyhow::bail!("验证状态异常");
                }
            }
        };

        // 等待验证完成（带超时）
        let result = tokio::time::timeout(Duration::from_secs(300), receiver).await;

        match result {
            Ok(Ok(captcha_result)) => {
                tracing::info!("收到验证结果");
                Ok(captcha_result)
            }
            Ok(Err(_)) => {
                // 重置状态
                let mut state = self.state.lock().await;
                *state = VerificationState::Idle;
                anyhow::bail!("验证通道已关闭");
            }
            Err(_) => {
                // 超时，重置状态
                let mut state = self.state.lock().await;
                *state = VerificationState::Idle;
                anyhow::bail!("验证超时");
            }
        }
    }

    /// 保存验证成功的token
    pub async fn save_token(&self, gaia_vtoken: String) {
        let mut state = self.state.lock().await;
        *state = VerificationState::Completed {
            gaia_vtoken,
            expires_at: Instant::now() + Duration::from_secs(3600), // 1小时过期
        };

        // 通知所有等待的进程
        self.notify.notify_waiters();
        tracing::info!("gaia_vtoken已保存并通知等待进程");
    }

    /// 获取缓存的token
    pub async fn get_cached_token(&self) -> Option<String> {
        let state = self.state.lock().await;
        match &*state {
            VerificationState::Completed {
                gaia_vtoken,
                expires_at,
            } => {
                if expires_at > &Instant::now() {
                    Some(gaia_vtoken.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 等待验证完成的通知
    pub async fn wait_for_completion(&self) -> Result<String> {
        loop {
            // 检查是否已完成
            if let Some(token) = self.get_cached_token().await {
                return Ok(token);
            }

            // 等待通知
            self.notify.notified().await;
        }
    }

    /// 自动解决验证码
    pub async fn auto_solve_captcha(&self, config: &RiskControlConfig, page_url: &str) -> Result<CaptchaResult> {
        let captcha_info = self
            .get_captcha_info()
            .await
            .ok_or_else(|| anyhow::anyhow!("当前没有待验证的验证码"))?;

        // 检查是否有GeeTest信息
        let geetest_info = captcha_info
            .geetest
            .ok_or_else(|| anyhow::anyhow!("验证码信息中缺少GeeTest数据"))?;

        // 获取自动解决配置
        let auto_solve_config = config
            .auto_solve
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("自动解决功能未配置"))?;

        tracing::info!("开始自动解决验证码，服务: {}", auto_solve_config.service);

        // 创建验证码解决器
        let solver = CaptchaSolver::new(auto_solve_config.clone());

        // 解决验证码
        let result = solver
            .solve_geetest(&geetest_info, &captcha_info.token, page_url)
            .await?;

        tracing::info!("验证码自动解决成功");
        Ok(result)
    }
}

/// 全局验证协调器实例
pub static VERIFICATION_COORDINATOR: Lazy<VerificationCoordinator> = Lazy::new(VerificationCoordinator::new);
