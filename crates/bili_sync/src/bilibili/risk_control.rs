use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::bilibili::BiliClient;

/// v_voucher风控验证处理模块
///
/// 当API返回v_voucher时，需要通过验证码验证来获取gaia_vtoken，
/// 然后用gaia_vtoken重新请求原API以绕过风控。
pub struct RiskControl<'a> {
    client: &'a BiliClient,
    v_voucher: String,
}

/// captcha验证码信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptchaInfo {
    /// 验证码类型，通常为"geetest"
    #[serde(rename = "type")]
    pub captcha_type: String,
    /// 验证码token，用于后续验证
    pub token: String,
    /// 极验验证信息
    pub geetest: Option<GeetestInfo>,
}

/// 极验验证信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeetestInfo {
    /// 极验ID
    pub gt: String,
    /// 极验KEY，用于人机验证
    pub challenge: String,
}

/// 验证结果
#[derive(Debug, Clone, Serialize)]
pub struct CaptchaResult {
    /// 验证码challenge
    pub challenge: String,
    /// 验证码token
    pub token: String,
    /// 验证结果validate
    pub validate: String,
    /// 验证结果seccode
    pub seccode: String,
}

/// 验证响应
#[derive(Debug, Deserialize)]
pub struct ValidateResponse {
    /// 验证是否成功，1表示成功
    pub is_valid: i32,
    /// gaia_vtoken，用于恢复正常访问
    pub grisk_id: String,
}

impl<'a> RiskControl<'a> {
    /// 创建新的风控处理实例
    pub fn new(client: &'a BiliClient, v_voucher: String) -> Self {
        Self { client, v_voucher }
    }

    /// 第一步：使用v_voucher申请captcha验证码
    ///
    /// 调用 https://api.bilibili.com/x/gaia-vgate/v1/register
    /// 参数：v_voucher, csrf（可选）
    /// 返回：token, challenge等验证码信息
    pub async fn register(&self) -> Result<CaptchaInfo> {
        tracing::info!("开始申请风控验证码，v_voucher: {}", self.v_voucher);

        let mut form_data = vec![("v_voucher", self.v_voucher.clone())];

        // 如果已登录，添加csrf token
        if let Some(csrf) = self.client.get_csrf_token() {
            form_data.push(("csrf", csrf));
        }

        let response = self
            .client
            .request(
                reqwest::Method::POST,
                "https://api.bilibili.com/x/gaia-vgate/v1/register",
            )
            .await
            .form(&form_data)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        // 检查响应
        if let Some(code) = response["code"].as_i64() {
            if code != 0 {
                let message = response["message"].as_str().unwrap_or("未知错误");
                anyhow::bail!("申请验证码失败: code={}, message={}", code, message);
            }
        }

        // 解析data字段
        let data = response["data"].clone();
        let captcha_info: CaptchaInfo = serde_json::from_value(data)?;

        // 检查是否支持captcha验证
        if captcha_info.geetest.is_none() {
            anyhow::bail!("该风控无法通过captcha解除，geetest字段为null");
        }

        tracing::info!(
            "成功获取验证码信息，类型: {}, token: {}",
            captcha_info.captcha_type,
            captcha_info.token
        );

        Ok(captcha_info)
    }

    /// 第二步：验证captcha并获取gaia_vtoken
    ///
    /// 调用 https://api.bilibili.com/x/gaia-vgate/v1/validate
    /// 参数：challenge, token, validate, seccode, csrf
    /// 返回：grisk_id (即gaia_vtoken)
    pub async fn validate(&self, captcha_result: CaptchaResult) -> Result<String> {
        tracing::info!("开始验证captcha，获取gaia_vtoken");
        tracing::info!(
            "提交验证参数: challenge={}, token={}, validate={}, seccode={}",
            captcha_result.challenge,
            captcha_result.token,
            captcha_result.validate,
            captcha_result.seccode
        );

        let mut form_data = vec![
            ("challenge", captcha_result.challenge),
            ("token", captcha_result.token),
            ("validate", captcha_result.validate),
            ("seccode", captcha_result.seccode),
        ];

        // 如果已登录，添加csrf token
        if let Some(csrf) = self.client.get_csrf_token() {
            form_data.push(("csrf", csrf));
        }

        let response = self
            .client
            .request(
                reqwest::Method::POST,
                "https://api.bilibili.com/x/gaia-vgate/v1/validate",
            )
            .await
            .form(&form_data)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        tracing::info!("B站验证API响应: {}", response);

        // 检查响应
        if let Some(code) = response["code"].as_i64() {
            if code != 0 {
                let message = response["message"].as_str().unwrap_or("未知错误");
                anyhow::bail!("验证失败: code={}, message={}", code, message);
            }
        }

        // 解析data字段
        let data = response["data"].clone();
        let validate_response: ValidateResponse = serde_json::from_value(data)?;

        if validate_response.is_valid != 1 {
            anyhow::bail!("验证未通过，is_valid={}", validate_response.is_valid);
        }

        tracing::info!("验证成功，获取到gaia_vtoken: {}", validate_response.grisk_id);
        Ok(validate_response.grisk_id)
    }

    /// 完整的风控处理流程（需要人工介入验证码）
    ///
    /// 注意：此方法需要实现验证码的人工处理，目前仅作为接口预留
    #[allow(dead_code)]
    pub async fn handle_full_verification(&self) -> Result<String> {
        // 1. 申请验证码
        let captcha_info = self.register().await?;

        tracing::warn!(
            "需要完成验证码验证，请访问极验验证界面：gt={}, challenge={}",
            captcha_info.geetest.as_ref().unwrap().gt,
            captcha_info.geetest.as_ref().unwrap().challenge
        );

        // 2. 这里需要实现验证码的人工处理或自动化验证
        // 由于需要人工介入，暂时不实现
        anyhow::bail!("验证码验证需要人工介入，暂不支持自动化处理");
    }
}
