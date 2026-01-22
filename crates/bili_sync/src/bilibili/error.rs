use thiserror::Error;

#[derive(Error, Debug)]
pub enum BiliError {
    #[error("risk control occurred")]
    RiskControlOccurred,
    #[error("risk control verification required, v_voucher: {0}")]
    RiskControlVerificationRequired(String),
    #[error("request failed, status code: {0}, message: {1}")]
    RequestFailed(i64, String),
    #[error("video stream empty: {0}")]
    VideoStreamEmpty(String),
}
