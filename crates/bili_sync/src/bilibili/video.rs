use anyhow::{anyhow, bail, ensure, Result};
use futures::stream::FuturesUnordered;
use futures::TryStreamExt;
use prost::Message;
use reqwest::Method;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::bilibili::analyzer::PageAnalyzer;
use crate::bilibili::client::BiliClient;
use crate::bilibili::credential::encoded_query;
use crate::bilibili::danmaku::{DanmakuElem, DanmakuWriter, DmSegMobileReply};
use crate::bilibili::subtitle::{SubTitle, SubTitleBody, SubTitleInfo, SubTitlesInfo};
use crate::bilibili::{Validate, VideoInfo, MIXIN_KEY};
use crate::hardware::HardwareFingerprint;
use crate::http::headers::create_api_headers;

static MASK_CODE: u64 = 2251799813685247;
static XOR_CODE: u64 = 23442827791579;
static BASE: u64 = 58;
static DATA: &[char] = &[
    'F', 'c', 'w', 'A', 'P', 'N', 'K', 'T', 'M', 'u', 'g', '3', 'G', 'V', '5', 'L', 'j', '7', 'E', 'J', 'n', 'H', 'p',
    'W', 's', 'x', '4', 't', 'b', '8', 'h', 'a', 'Y', 'e', 'v', 'i', 'q', 'B', 'z', '6', 'r', 'k', 'C', 'y', '1', '2',
    'm', 'U', 'S', 'D', 'Q', 'X', '9', 'R', 'd', 'o', 'Z', 'f',
];

pub struct Video<'a> {
    client: &'a BiliClient,
    pub aid: String,
    pub bvid: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct Tag {
    pub tag_name: String,
}

impl serde::Serialize for Tag {
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.tag_name)
    }
}
#[derive(Debug, serde::Deserialize, Default)]
pub struct PageInfo {
    pub cid: i64,
    pub page: i32,
    #[serde(rename = "part")]
    pub name: String,
    pub duration: u32,
    pub first_frame: Option<String>,
    pub dimension: Option<Dimension>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct Dimension {
    pub width: u32,
    pub height: u32,
    pub rotate: u32,
}

impl<'a> Video<'a> {
    pub fn new(client: &'a BiliClient, bvid: String) -> Self {
        let aid = bvid_to_aid(&bvid).to_string();
        Self { client, aid, bvid }
    }

    /// 创建一个使用特定 aid 的 Video 实例，用于番剧等特殊情况
    pub fn new_with_aid(client: &'a BiliClient, bvid: String, aid: String) -> Self {
        Self { client, aid, bvid }
    }

    /// 直接调用视频信息接口获取详细的视频信息，视频信息中包含了视频的分页信息
    pub async fn get_view_info(&self) -> Result<VideoInfo> {
        let mut res = self
            .client
            .request(Method::GET, "https://api.bilibili.com/x/web-interface/view")
            .await
            .query(&[("aid", &self.aid), ("bvid", &self.bvid)])
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?
            .validate()?;
        Ok(serde_json::from_value(res["data"].take())?)
    }

    /// 检查视频是否存在
    /// 调用视频详情API，如果返回-404则表示视频已被删除
    pub async fn check_video_exists(&self) -> Result<bool> {
        let request_url = "https://api.bilibili.com/x/web-interface/view";
        tracing::debug!("检查视频是否存在: {} - BVID: {}", request_url, self.bvid);

        let response = self
            .client
            .request(Method::GET, request_url)
            .await
            .query(&[("bvid", &self.bvid)])
            .send()
            .await;

        let res = match response {
            Ok(resp) => {
                tracing::debug!("视频存在性检查请求成功 - 状态码: {}, BVID: {}", resp.status(), self.bvid);
                resp
            }
            Err(e) => {
                tracing::warn!("视频存在性检查网络错误，假设视频存在 - BVID: {}, 错误: {}", self.bvid, e);
                return Ok(true);
            }
        };

        let json_res = match res.json::<serde_json::Value>().await {
            Ok(json) => {
                tracing::debug!("视频存在性检查响应解析成功 - BVID: {}", self.bvid);
                json
            }
            Err(e) => {
                tracing::warn!("视频存在性检查JSON解析错误，假设视频存在 - BVID: {}, 错误: {}", self.bvid, e);
                return Ok(true);
            }
        };

        // 检查API返回码
        if let Some(code) = json_res["code"].as_i64() {
            tracing::debug!("视频存在性检查返回码: {} - BVID: {}", code, self.bvid);
            if code == -404 {
                tracing::warn!("视频已被删除(API返回-404): BVID={}", self.bvid);
                return Ok(false);
            }
        }

        // 其他情况假设视频存在
        tracing::debug!("视频存在性检查完成，视频存在 - BVID: {}", self.bvid);
        Ok(true)
    }

    /// 调用视频详情API获取epid信息，用于API降级处理
    /// 当普通视频API返回-404错误时，可以通过此方法获取epid，然后尝试番剧API降级
    pub async fn get_video_detail_for_epid(&self) -> Result<Option<String>> {
        tracing::debug!("调用视频详情API获取epid信息: BVID={}", self.bvid);

        let res = match self
            .client
            .request(Method::GET, "https://api.bilibili.com/x/web-interface/view")
            .await
            .query(&[("bvid", &self.bvid)])
            .send()
            .await
        {
            Ok(response) => response,
            Err(e) => {
                tracing::warn!("视频详情API网络请求失败: {}", e);
                return Err(e.into());
            }
        };

        let res = match res.error_for_status() {
            Ok(response) => response,
            Err(e) => {
                tracing::warn!("视频详情API HTTP错误: {}", e);
                return Err(e.into());
            }
        };

        let json_res = match res.json::<serde_json::Value>().await {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!("视频详情API JSON解析失败: {}", e);
                return Err(e.into());
            }
        };

        // 记录API响应（仅在debug级别）
        tracing::debug!(
            "视频详情API响应: {}",
            serde_json::to_string_pretty(&json_res).unwrap_or_else(|_| "无法序列化".to_string())
        );

        // 检查API返回是否成功
        if let Some(code) = json_res["code"].as_i64() {
            if code != 0 {
                let message = json_res["message"].as_str().unwrap_or("未知错误");
                tracing::warn!("视频详情API返回错误: code={}, message={}", code, message);

                // 对于特定的错误码，给出更详细的说明
                match code {
                    -404 => tracing::debug!("视频不存在或已被删除，无法获取epid"),
                    -403 => tracing::debug!("无权限访问该视频，无法获取epid"),
                    62002 => tracing::debug!("稿件不可见，无法获取epid"),
                    _ => tracing::debug!("其他API错误，无法获取epid"),
                }

                return Err(crate::bilibili::BiliError::RequestFailed(code, message.to_string()).into());
            }
        }

        // 检查data字段是否存在
        let data = match json_res.get("data") {
            Some(data) if !data.is_null() => data,
            _ => {
                tracing::debug!("视频详情API返回的data字段为空，无法提取epid");
                return Ok(None);
            }
        };

        // 尝试从返回的JSON中提取epid字段，按优先级尝试不同的位置
        let epid = data["redirect_url"]
            .as_str()
            .and_then(|url| {
                tracing::debug!("检查redirect_url: {}", url);
                // 从redirect_url中提取epid，格式通常为：https://www.bilibili.com/bangumi/play/ep123456
                if url.contains("/bangumi/play/ep") {
                    if let Some(ep_start) = url.find("/ep") {
                        let ep_part = &url[ep_start + 3..]; // 跳过"/ep"
                                                            // 提取数字部分，支持ep123456?参数的格式
                        let epid_str: String = ep_part.chars().take_while(|c| c.is_ascii_digit()).collect();
                        if !epid_str.is_empty() {
                            tracing::debug!("从redirect_url提取到epid: {}", epid_str);
                            return Some(epid_str);
                        }
                    }
                }
                None
            })
            .or_else(|| {
                // 尝试从season.episodes数组中获取epid
                data["season"]["episodes"]
                    .as_array()
                    .and_then(|episodes| {
                        tracing::debug!("检查season.episodes数组，共{}个分集", episodes.len());
                        episodes.first()
                    })
                    .and_then(|ep| ep["id"].as_i64())
                    .map(|id| {
                        let epid_str = id.to_string();
                        tracing::debug!("从season.episodes数组提取到epid: {}", epid_str);
                        epid_str
                    })
            })
            .or_else(|| {
                // 检查是否有直接的epid字段
                data["epid"].as_i64().or_else(|| data["episode_id"].as_i64()).map(|id| {
                    let epid_str = id.to_string();
                    tracing::debug!("从直接字段提取到epid: {}", epid_str);
                    epid_str
                })
            })
            .or_else(|| {
                // 尝试从ugc_season.episodes中获取（用户投稿番剧）
                data["ugc_season"]["episodes"]
                    .as_array()
                    .and_then(|episodes| {
                        tracing::debug!("检查ugc_season.episodes数组，共{}个分集", episodes.len());
                        episodes.first()
                    })
                    .and_then(|ep| ep["id"].as_i64())
                    .map(|id| {
                        let epid_str = id.to_string();
                        tracing::debug!("从ugc_season.episodes数组提取到epid: {}", epid_str);
                        epid_str
                    })
            });

        if let Some(ref epid_value) = epid {
            tracing::debug!("✓ 成功从视频详情API获取到epid: {}", epid_value);
        } else {
            tracing::debug!("视频详情API中未找到epid信息，可能不是番剧视频");
            tracing::debug!("已检查的字段: redirect_url, season.episodes, epid, episode_id, ugc_season.episodes");
        }

        Ok(epid)
    }

    #[allow(unused)]
    pub async fn get_pages(&self) -> Result<Vec<PageInfo>> {
        let mut res = self
            .client
            .request(Method::GET, "https://api.bilibili.com/x/player/pagelist")
            .await
            .query(&[("aid", &self.aid), ("bvid", &self.bvid)])
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?
            .validate()?;
        Ok(serde_json::from_value(res["data"].take())?)
    }

    pub async fn get_tags(&self) -> Result<Vec<Tag>> {
        let mut res = self
            .client
            .request(Method::GET, "https://api.bilibili.com/x/web-interface/view/detail/tag")
            .await
            .query(&[("aid", &self.aid), ("bvid", &self.bvid)])
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?
            .validate()?;
        Ok(serde_json::from_value(res["data"].take())?)
    }

    pub async fn get_danmaku_writer(&self, page: &'a PageInfo, token: CancellationToken) -> Result<DanmakuWriter> {
        let segment_count = page.duration.div_ceil(360);
        debug!("开始获取弹幕，共{}个分段", segment_count);

        // 串行获取弹幕分段，避免并发过多
        let mut all_danmaku: Vec<DanmakuElem> = Vec::new();

        for i in 1..=segment_count {
            if token.is_cancelled() {
                bail!("Danmaku download cancelled");
            }
            match self
                .get_danmaku_segment_with_retry(page, i as i64, 3, token.clone())
                .await
            {
                Ok(mut segment_danmaku) => {
                    debug!("成功获取弹幕分段 {}/{}", i, segment_count);
                    all_danmaku.append(&mut segment_danmaku);
                }
                Err(e) => {
                    warn!("获取弹幕分段 {}/{} 失败: {:#}", i, segment_count, e);
                    // 继续处理其他分段，不因单个分段失败而整体失败
                }
            }
        }

        // 按时间排序
        all_danmaku.sort_by_key(|d| d.progress);
        debug!("弹幕获取完成，共{}条弹幕", all_danmaku.len());

        Ok(DanmakuWriter::new(
            page,
            all_danmaku.into_iter().map(|x| x.into()).collect(),
        ))
    }

    /// 带重试机制的弹幕分段获取
    async fn get_danmaku_segment_with_retry(
        &self,
        page: &PageInfo,
        segment_idx: i64,
        max_retries: usize,
        token: CancellationToken,
    ) -> Result<Vec<DanmakuElem>> {
        let mut last_error = None;

        for attempt in 1..=max_retries {
            if token.is_cancelled() {
                bail!("Danmaku download cancelled");
            }
            match self.get_danmaku_segment(page, segment_idx, token.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < max_retries {
                        let delay = std::time::Duration::from_millis(1000 * attempt as u64);
                        debug!(
                            "弹幕分段{}获取失败，{}ms后重试({}/{}): {:#}",
                            segment_idx,
                            delay.as_millis(),
                            attempt,
                            max_retries,
                            last_error.as_ref().unwrap()
                        );
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap())
    }

    async fn get_danmaku_segment(
        &self,
        page: &PageInfo,
        segment_idx: i64,
        token: CancellationToken,
    ) -> Result<Vec<DanmakuElem>> {
        debug!(
            "请求弹幕片段: type=1, oid={}, pid={}, segment_index={}",
            page.cid, self.aid, segment_idx
        );

        let url = format!(
            "http://api.bilibili.com/x/v2/dm/web/seg.so?type=1&oid={}&pid={}&segment_index={}",
            page.cid, self.aid, segment_idx
        );

        let res = tokio::select! {
            biased;
            _ = token.cancelled() => return Err(anyhow!("Download cancelled")),
            res = self.client.get(&url, token.clone()) => res,
        }?;

        if !res.status().is_success() {
            bail!("弹幕API请求失败，状态码: {}", res.status());
        }

        let headers = res.headers().clone();
        let content_type = headers.get("content-type");
        ensure!(
            content_type.is_some_and(|v| v == "application/octet-stream"),
            "unexpected content type: {:?}, body: {:?}",
            content_type,
            res.text().await
        );
        Ok(DmSegMobileReply::decode(res.bytes().await?)?.elems)
    }

    /// 带质量回退的页面分析器获取
    pub async fn get_page_analyzer_with_fallback(&self, page: &PageInfo) -> Result<PageAnalyzer> {
        // 质量回退列表：从最高到最低，恢复原始顺序
        let quality_levels = ["127", "126", "125", "120", "116", "112", "80", "64", "32", "16"];

        for (attempt, qn) in quality_levels.iter().enumerate() {
            tracing::debug!(
                "尝试获取视频流 (尝试 {}/{}): qn={}",
                attempt + 1,
                quality_levels.len(),
                qn
            );

            match self.get_page_analyzer_with_quality(page, qn).await {
                Ok(analyzer) => {
                    tracing::debug!("✓ 成功获取视频流: qn={}", qn);
                    return Ok(analyzer);
                }
                Err(e) => {
                    // 检查是否为风控验证错误
                    if let Some(crate::bilibili::BiliError::RiskControlVerificationRequired(v_voucher)) =
                        e.downcast_ref::<crate::bilibili::BiliError>()
                    {
                        tracing::warn!("检测到风控，开始验证流程: v_voucher={}", v_voucher);

                        // 尝试进行验证流程
                        match self.handle_risk_control_verification(v_voucher.clone()).await {
                            Ok(gaia_vtoken) => {
                                tracing::info!("风控验证成功，已获取gaia_vtoken，重试获取视频流");
                                self.client.set_gaia_vtoken(gaia_vtoken);

                                // 重试当前质量级别
                                match self.get_page_analyzer_with_quality(page, qn).await {
                                    Ok(analyzer) => {
                                        tracing::info!("✓ 风控验证后成功获取视频流: qn={}", qn);
                                        return Ok(analyzer);
                                    }
                                    Err(retry_err) => {
                                        tracing::warn!("风控验证后重试失败: {}", retry_err);
                                        // 继续尝试下一个质量级别
                                    }
                                }
                            }
                            Err(verify_err) => {
                                tracing::error!("风控验证失败，视频: {}, 错误: {}", self.bvid, verify_err);

                                // 检查是否是端口冲突问题
                                if verify_err.to_string().contains("os error 10048") {
                                    tracing::warn!("检测到端口冲突，建议检查其他验证进程");
                                }

                                return Err(verify_err);
                            }
                        }
                    }

                    // 检查是否为充电专享视频错误（包括试看视频），如果是则不输出详细的质量级别失败日志
                    let (is_charging_video_error, is_trial_video) = {
                        if let Some(bili_err) = e.downcast_ref::<crate::bilibili::BiliError>() {
                            match bili_err {
                                crate::bilibili::BiliError::RequestFailed(87007 | 87008, msg) => {
                                    (true, msg.contains("试看视频"))
                                }
                                crate::bilibili::BiliError::RequestFailed(code, msg) => {
                                    // 检查其他可能的充电专享视频错误码或消息
                                    let is_charging = msg.contains("充电专享")
                                        || msg.contains("需要充电")
                                        || msg.contains("试看视频")
                                        || msg.contains("大会员专享")
                                        || (*code == -403 && msg.contains("access denied"));
                                    (is_charging, msg.contains("试看视频"))
                                }
                                _ => (false, false),
                            }
                        } else {
                            // 检查非BiliError类型的错误是否可能是充电专享视频错误
                            let error_str = e.to_string().to_lowercase();
                            let is_charging = error_str.contains("充电专享")
                                || error_str.contains("需要充电")
                                || error_str.contains("试看视频")
                                || error_str.contains("大会员专享")
                                || error_str.contains("access denied");
                            (is_charging, error_str.contains("试看视频"))
                        }
                    };

                    if !is_charging_video_error {
                        tracing::debug!("× 质量 qn={} 获取失败: {}", qn, e);
                    } else if attempt == 0 && is_trial_video {
                        // 只在第一次尝试时记录试看视频信息
                        tracing::info!("检测到试看视频，需要充电才能观看完整版");
                    }

                    if attempt == quality_levels.len() - 1 {
                        // 最后一次尝试也失败了
                        if is_charging_video_error {
                            if !is_trial_video {
                                tracing::info!("视频需要充电才能观看");
                            }
                            // 对于充电专享视频，统一返回87007错误以便上层正确处理
                            return Err(crate::bilibili::BiliError::RequestFailed(
                                87007,
                                "充电专享视频，需要为UP主充电才能观看".to_string(),
                            )
                            .into());
                        } else {
                            tracing::error!("所有质量级别都获取失败");

                            // 检查是否为HTTP 412风控错误
                            let error_str = e.to_string();
                            if error_str.contains("412 Precondition Failed") {
                                // 先检查视频是否已被删除
                                if let Ok(exists) = self.check_video_exists().await {
                                    if !exists {
                                        tracing::warn!("检测到HTTP 412但视频已被删除，返回404错误而非风控");
                                        return Err(crate::bilibili::BiliError::RequestFailed(
                                            -404,
                                            "视频已被删除".to_string(),
                                        )
                                        .into());
                                    }
                                }
                                tracing::warn!("检测到HTTP 412风控错误，转换为风控异常");
                                return Err(crate::bilibili::BiliError::RiskControlOccurred.into());
                            }

                            // 检查是否可能是隐蔽的充电专享视频（API成功但实际是试看片段）
                            let error_str_lower = error_str.to_lowercase();
                            if error_str_lower.contains("检测到试看")
                                || error_str_lower.contains("试看模式")
                                || error_str_lower.contains("试看片段")
                            {
                                tracing::info!("检测到隐蔽的充电专享视频（试看片段模式）");
                                return Err(crate::bilibili::BiliError::RequestFailed(
                                    87008,
                                    "充电专享视频（试看片段），需要为UP主充电才能观看".to_string(),
                                )
                                .into());
                            }
                        }
                        return Err(e);
                    }
                    // 继续尝试下一个质量级别
                    continue;
                }
            }
        }

        // 理论上不会到达这里
        Err(anyhow!("无法获取任何质量的视频流"))
    }

    /// 带API降级的视频流获取（普通视频->番剧API）
    /// 当普通视频API返回 -404 "啥都木有" 时，自动尝试番剧API
    /// 如果缺少ep_id，会先尝试从视频详情API获取epid信息
    pub async fn get_page_analyzer_with_api_fallback(
        &self,
        page: &PageInfo,
        ep_id: Option<&str>,
    ) -> Result<PageAnalyzer> {
        tracing::debug!("开始API降级获取视频流，BVID: {}, CID: {}", self.bvid, page.cid);

        // 首先尝试普通视频API
        match self.get_page_analyzer_with_fallback(page).await {
            Ok(analyzer) => {
                tracing::debug!("✓ 普通视频API成功获取播放地址");
                Ok(analyzer)
            }
            Err(e) => {
                // 检查错误类型，判断是否需要降级到番剧API
                let should_fallback_to_bangumi = if let Some(crate::bilibili::BiliError::RequestFailed(-404, msg)) =
                    e.downcast_ref::<crate::bilibili::BiliError>()
                {
                    // -404 错误，检查消息是否包含"啥都木有"或其他表示内容不存在的关键词
                    let msg_lower = msg.to_lowercase();
                    msg_lower.contains("啥都木有")
                        || msg_lower.contains("nothing found")
                        || msg_lower.contains("not found")
                        || msg_lower.contains("无内容")
                        || msg_lower.contains("视频不存在")
                } else {
                    false
                };

                if should_fallback_to_bangumi {
                    tracing::debug!("普通视频API返回-404错误，尝试降级到番剧API: {}", e);

                    // 获取epid：优先使用传入的ep_id，如果没有则从视频详情API获取
                    let epid_to_use = if let Some(provided_epid) = ep_id {
                        tracing::debug!("使用提供的ep_id: {}", provided_epid);
                        Some(provided_epid.to_string())
                    } else {
                        tracing::debug!("缺少ep_id，尝试从视频详情API获取epid信息");
                        match self.get_video_detail_for_epid().await {
                            Ok(Some(epid)) => {
                                tracing::debug!("✓ 成功从视频详情API获取到epid: {}", epid);
                                Some(epid)
                            }
                            Ok(None) => {
                                tracing::warn!("视频详情API中未找到epid信息，无法降级到番剧API");
                                None
                            }
                            Err(detail_err) => {
                                tracing::warn!("调用视频详情API失败: {}", detail_err);
                                None
                            }
                        }
                    };

                    // 如果有epid，尝试番剧API降级
                    if let Some(epid) = epid_to_use {
                        tracing::debug!("使用epid {} 尝试番剧API降级", epid);
                        match self.get_bangumi_page_analyzer_with_fallback(page, &epid).await {
                            Ok(analyzer) => {
                                tracing::debug!("✓ 番剧API降级成功，获取到播放地址");
                                Ok(analyzer)
                            }
                            Err(bangumi_err) => {
                                tracing::warn!("× 番剧API降级也失败: {}", bangumi_err);
                                // 返回原始的普通视频API错误，因为这更能反映真实情况
                                Err(e)
                            }
                        }
                    } else {
                        tracing::warn!("无法获取epid，无法降级到番剧API");
                        Err(e)
                    }
                } else {
                    // 不是-404错误或不包含特定消息，直接返回原错误
                    tracing::debug!("普通视频API失败，但不符合降级条件: {}", e);
                    Err(e)
                }
            }
        }
    }

    /// 使用指定质量获取页面分析器
    async fn get_page_analyzer_with_quality(&self, page: &PageInfo, qn: &str) -> Result<PageAnalyzer> {
        // 修复字符串生命周期问题
        let cid_string = page.cid.to_string();

        // 生成硬件指纹
        let fingerprint = HardwareFingerprint::default();
        let hardware = fingerprint.get_hardware();

        // 生成弹幕防挡参数（使用会话固定的硬件指纹）
        let dm_img_str = hardware.generate_dm_img_str();
        let dm_cover_img_str = hardware.generate_dm_cover_img_str();
        let dm_img_list = fingerprint.generate_dm_img_list(page.duration as u32);
        let dm_img_inter = fingerprint.generate_dm_img_inter();

        // 增强的API参数配置，包含硬件指纹和弹幕防挡参数
        let params = vec![
            ("avid", self.aid.as_str()),
            ("cid", cid_string.as_str()),
            ("qn", qn), // 使用指定的质量参数
            ("otype", "json"),
            ("fnval", "4048"), // 恢复原始fnval值
            ("fourk", "1"),    // 启用4K支持
            ("voice_balance", "1"), // 音频平衡
            ("gaia_source", "pre-load"), // Gaia预加载
            ("isGaiaAvoided", "true"), // Gaia避免策略
            ("web_location", "1315873"), // 网页位置标识
            ("dm_img_str", dm_img_str.as_str()), // WebGL信息
            ("dm_cover_img_str", dm_cover_img_str.as_str()), // GPU信息
            ("dm_img_list", dm_img_list.as_str()), // 弹幕交互数据
            ("dm_img_inter", dm_img_inter.as_str()), // 弹幕交互统计
        ];

        let encoded_params = encoded_query(params.clone(), MIXIN_KEY.load().as_deref());
        tracing::debug!("API参数: {:?}", params);
        tracing::debug!("编码后参数: {:?}", encoded_params);

        let request_url = "https://api.bilibili.com/x/player/wbi/playurl";
        tracing::debug!("发起playurl请求: {} - BVID: {}, CID: {}", request_url, self.bvid, page.cid);

        let request = self
            .client
            .request(Method::GET, request_url)
            .await
            .query(&encoded_params)
            .headers(create_api_headers());

        // 请求头日志已在建造器时设置

        let response = request.send().await;
        match &response {
            Ok(resp) => {
                tracing::debug!("playurl请求成功 - 状态码: {}, URL: {}", resp.status(), resp.url());
                tracing::debug!("响应头: {:?}", resp.headers());
            }
            Err(e) => {
                tracing::error!("playurl请求失败 - BVID: {}, 错误: {}", self.bvid, e);
            }
        }

        let res = response?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        tracing::debug!("playurl响应数据大小: {} bytes", serde_json::to_string(&res).unwrap_or_default().len());

        // 添加详细的API响应日志
        tracing::debug!(
            "API完整响应: {}",
            serde_json::to_string_pretty(&res).unwrap_or_else(|_| "无法序列化".to_string())
        );

        // 记录关键字段
        if let Some(code) = res["code"].as_i64() {
            tracing::debug!("API返回code: {}", code);
        }
        if let Some(message) = res["message"].as_str() {
            tracing::debug!("API返回message: {}", message);
        }

        // 检查data字段是否存在
        if res["data"].is_null() {
            tracing::debug!("API返回的data字段为null");
        } else if let Some(dash) = res["data"]["dash"].as_object() {
            tracing::debug!(
                "dash对象存在，视频流数量: {}",
                dash.get("video")
                    .and_then(|v| v.as_array())
                    .map(|v| v.len())
                    .unwrap_or(0)
            );
            tracing::debug!(
                "dash对象存在，音频流数量: {}",
                dash.get("audio")
                    .and_then(|v| v.as_array())
                    .map(|v| v.len())
                    .unwrap_or(0)
            );
        } else {
            tracing::debug!("API返回的data.dash字段不存在或不是对象");
        }

        // 检查API响应中的错误信息
        if let Some(code) = res["code"].as_i64() {
            if code != 0 {
                let message = res["message"].as_str().unwrap_or("未知错误");
                return Err(crate::bilibili::BiliError::RequestFailed(code, message.to_string()).into());
            }
        }

        // 检测v_voucher风控响应
        if let Some(v_voucher) = res["data"]["v_voucher"].as_str() {
            // 检查是否只有v_voucher而没有实际的视频流数据
            let has_dash = res["data"]["dash"]["video"].as_array().is_some_and(|v| !v.is_empty());
            let has_durl = res["data"]["durl"].as_array().is_some_and(|v| !v.is_empty());

            if !has_dash && !has_durl {
                tracing::warn!(
                    "检测到风控v_voucher响应，视频: {} (aid: {}), cid: {}, v_voucher: {}",
                    self.bvid,
                    self.aid,
                    page.cid,
                    v_voucher
                );
                tracing::debug!(
                    "v_voucher响应详情: {}",
                    serde_json::to_string_pretty(&res["data"]).unwrap_or_else(|_| "无法序列化".to_string())
                );
                return Err(crate::bilibili::BiliError::RiskControlVerificationRequired(v_voucher.to_string()).into());
            }
        }

        // 检查是否有可用的视频流 (只接受dash格式，durl是试看片段)
        let has_dash_video = res["data"]["dash"]["video"].as_array().is_some_and(|v| !v.is_empty());
        let has_durl_only = res["data"]["durl"].as_array().is_some_and(|v| !v.is_empty()) && !has_dash_video;

        if has_durl_only {
            // 只在debug级别记录试看视频详情，避免日志过多
            tracing::debug!(
                "试看视频data字段: {}",
                serde_json::to_string_pretty(&res["data"]).unwrap_or_else(|_| "无法序列化".to_string())
            );
            // 返回充电视频错误，触发自动删除
            return Err(crate::bilibili::BiliError::RequestFailed(
                87008,
                "试看视频，需要充电才能观看完整版".to_string(),
            )
            .into());
        }

        // 检查是否为可疑的充电视频：API返回成功但可能是试看片段
        if has_dash_video {
            // 检查视频时长是否异常短（可能是试看片段）
            if let Some(timelength) = res["data"]["timelength"].as_u64() {
                // 如果视频时长小于30秒且同时存在durl字段，可能是试看视频
                if timelength < 30000 && res["data"]["durl"].as_array().is_some_and(|v| !v.is_empty()) {
                    tracing::debug!(
                        "检测到可疑的短视频片段，时长: {}ms，可能为充电专享视频的试看片段",
                        timelength
                    );
                    tracing::debug!(
                        "可疑试看视频data字段: {}",
                        serde_json::to_string_pretty(&res["data"]).unwrap_or_else(|_| "无法序列化".to_string())
                    );
                    return Err(crate::bilibili::BiliError::RequestFailed(
                        87008,
                        "检测到试看片段，可能为充电专享视频".to_string(),
                    )
                    .into());
                }
            }

            // 检查是否存在特定的充电专享视频标识字段
            if let Some(result) = res["data"]["result"].as_str() {
                if result == "suee" {
                    // "suee" 可能是试看片段的标识，结合其他字段进一步判断
                    let has_limited_content = res["data"]["durl"].as_array().is_some_and(|v| !v.is_empty());
                    if has_limited_content {
                        tracing::debug!("检测到result=suee且存在durl，可能为充电专享视频的试看模式");
                        tracing::debug!(
                            "疑似充电专享视频data字段: {}",
                            serde_json::to_string_pretty(&res["data"]).unwrap_or_else(|_| "无法序列化".to_string())
                        );
                        return Err(crate::bilibili::BiliError::RequestFailed(
                            87008,
                            "检测到试看模式，可能为充电专享视频".to_string(),
                        )
                        .into());
                    }
                }
            }
        }

        if !has_dash_video {
            tracing::error!(
                "视频流为空，完整的data字段: {}",
                serde_json::to_string_pretty(&res["data"]).unwrap_or_else(|_| "无法序列化".to_string())
            );
            return Err(crate::bilibili::BiliError::VideoStreamEmpty("API返回的视频流为空".to_string()).into());
        }

        // 记录成功获取的质量信息
        if let Some(quality) = res["data"]["quality"].as_u64() {
            tracing::debug!("API返回的实际质量: {}", quality);
        }
        if let Some(accept_quality) = res["data"]["accept_quality"].as_array() {
            let qualities: Vec<u64> = accept_quality.iter().filter_map(|v| v.as_u64()).collect();
            tracing::debug!("可用质量列表: {:?}", qualities);
        }

        let mut validated_res = res.validate()?;
        Ok(PageAnalyzer::new(validated_res["data"].take()))
    }

    pub async fn get_page_analyzer(&self, page: &PageInfo) -> Result<PageAnalyzer> {
        // 修复字符串生命周期问题
        let cid_string = page.cid.to_string();

        // 生成硬件指纹
        let fingerprint = HardwareFingerprint::default();
        let hardware = fingerprint.get_hardware();

        // 生成弹幕防挡参数（使用会话固定的硬件指纹）
        let dm_img_str = hardware.generate_dm_img_str();
        let dm_cover_img_str = hardware.generate_dm_cover_img_str();
        let dm_img_list = fingerprint.generate_dm_img_list(page.duration as u32);
        let dm_img_inter = fingerprint.generate_dm_img_inter();

        // 增强的API参数配置，包含硬件指纹和弹幕防挡参数
        let params = vec![
            ("avid", self.aid.as_str()),
            ("cid", cid_string.as_str()),
            ("qn", "127"), // 恢复原始qn=127请求8K质量
            ("otype", "json"),
            ("fnval", "4048"), // 恢复原始fnval值
            ("fourk", "1"),    // 启用4K支持
            ("voice_balance", "1"), // 音频平衡
            ("gaia_source", "pre-load"), // Gaia预加载
            ("isGaiaAvoided", "true"), // Gaia避免策略
            ("web_location", "1315873"), // 网页位置标识
            ("dm_img_str", dm_img_str.as_str()), // WebGL信息
            ("dm_cover_img_str", dm_cover_img_str.as_str()), // GPU信息
            ("dm_img_list", dm_img_list.as_str()), // 弹幕交互数据
            ("dm_img_inter", dm_img_inter.as_str()), // 弹幕交互统计
        ];

        tracing::debug!("=== API参数调试 ===");
        tracing::debug!("视频: {} (aid: {})", self.bvid, self.aid);
        let encoded_params = encoded_query(params.clone(), MIXIN_KEY.load().as_deref());
        tracing::debug!("分页: cid: {}", page.cid);
        tracing::debug!("请求参数: {:?}", params);
        tracing::debug!("编码后参数: {:?}", encoded_params);

        let request_url = "https://api.bilibili.com/x/player/wbi/playurl";
        tracing::debug!("发起playurl请求(分页): {} - BVID: {}, CID: {}", request_url, self.bvid, page.cid);

        let request = self
            .client
            .request(Method::GET, request_url)
            .await
            .query(&encoded_params)
            .headers(create_api_headers());

        // 请求头日志已在建造器时设置

        let response = request.send().await;
        match &response {
            Ok(resp) => {
                tracing::debug!("playurl请求成功(分页) - 状态码: {}, URL: {}", resp.status(), resp.url());
                tracing::debug!("响应头: {:?}", resp.headers());
            }
            Err(e) => {
                tracing::error!("playurl请求失败(分页) - BVID: {}, CID: {}, 错误: {}", self.bvid, page.cid, e);
            }
        }

        let res = response?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        tracing::debug!("playurl响应数据大小(分页): {} bytes", serde_json::to_string(&res).unwrap_or_default().len());

        // 增强的API响应调试信息
        tracing::debug!("=== API响应调试 ===");
        if let Some(code) = res["code"].as_i64() {
            tracing::debug!("响应代码: {}", code);
        }
        if let Some(message) = res["message"].as_str() {
            tracing::debug!("响应消息: {}", message);
        }

        // 记录视频质量信息
        if let Some(quality) = res["data"]["quality"].as_u64() {
            tracing::debug!("API返回的当前质量: {}", quality);
        }
        if let Some(accept_quality) = res["data"]["accept_quality"].as_array() {
            let qualities: Vec<u64> = accept_quality.iter().filter_map(|v| v.as_u64()).collect();
            tracing::debug!("API返回的可用质量列表: {:?}", qualities);
        }

        // 记录音频流信息
        if let Some(audio_array) = res["data"]["dash"]["audio"].as_array() {
            tracing::debug!("=== 普通视频音频流信息 ===");
            tracing::debug!("普通视频音频流数量: {}", audio_array.len());
            for (i, audio) in audio_array.iter().enumerate() {
                if let Some(id) = audio["id"].as_u64() {
                    let codec = audio["codecs"].as_str().unwrap_or("unknown");
                    let bandwidth = audio["bandwidth"].as_u64().unwrap_or(0);
                    tracing::debug!(
                        "普通视频音频流{}: ID={}, codec={}, bandwidth={}",
                        i,
                        id,
                        codec,
                        bandwidth
                    );
                }
            }
            tracing::debug!("=== 普通视频音频流信息结束 ===");
        }

        // 记录杜比音频流信息
        if let Some(dolby) = res["data"]["dash"]["dolby"].as_object() {
            tracing::debug!("=== 普通视频Dolby音频信息 ===");
            if let Some(dolby_type) = dolby["type"].as_u64() {
                tracing::debug!("Dolby类型: {}", dolby_type);
            }
            if let Some(dolby_audio) = dolby["audio"].as_array() {
                tracing::debug!("Dolby音频流数量: {}", dolby_audio.len());
                for (i, audio) in dolby_audio.iter().enumerate() {
                    if let Some(id) = audio["id"].as_u64() {
                        tracing::debug!("Dolby音频流{}: ID={}", i, id);
                    }
                }
            }
            tracing::debug!("=== 普通视频Dolby音频信息结束 ===");
        }

        // 记录FLAC音频流信息
        if let Some(flac) = res["data"]["dash"]["flac"].as_object() {
            tracing::debug!("=== 普通视频FLAC音频信息 ===");
            if let Some(flac_display) = flac["display"].as_bool() {
                tracing::debug!("FLAC显示状态: {}", flac_display);
            }
            if let Some(flac_audio) = flac["audio"].as_object() {
                if let Some(id) = flac_audio["id"].as_u64() {
                    tracing::debug!("FLAC音频流ID: {}", id);
                }
            }
            tracing::debug!("=== 普通视频FLAC音频信息结束 ===");
        }

        // 检查是否存在VIP要求
        if let Some(vip_status) = res["data"]["vip_status"].as_i64() {
            tracing::debug!("VIP状态要求: {}", vip_status);
        }
        if let Some(vip_type) = res["data"]["vip_type"].as_i64() {
            tracing::debug!("VIP类型: {}", vip_type);
        }

        tracing::debug!("=== API响应调试结束 ===");

        let mut validated_res = res.validate()?;
        Ok(PageAnalyzer::new(validated_res["data"].take()))
    }

    /// 带质量回退的番剧页面分析器获取
    pub async fn get_bangumi_page_analyzer_with_fallback(&self, page: &PageInfo, ep_id: &str) -> Result<PageAnalyzer> {
        // 质量回退列表：从最高到最低，恢复原始顺序
        let quality_levels = ["127", "126", "125", "120", "116", "112", "80", "64", "32", "16"];

        for (attempt, qn) in quality_levels.iter().enumerate() {
            tracing::debug!(
                "尝试获取番剧视频流 (尝试 {}/{}): qn={}",
                attempt + 1,
                quality_levels.len(),
                qn
            );

            match self.get_bangumi_page_analyzer_with_quality(page, ep_id, qn).await {
                Ok(analyzer) => {
                    tracing::debug!("✓ 成功获取番剧视频流: qn={}", qn);
                    return Ok(analyzer);
                }
                Err(e) => {
                    // 检查是否为充电专享视频错误，如果是则不输出详细的质量级别失败日志
                    let is_charging_video_error = {
                        if let Some(bili_err) = e.downcast_ref::<crate::bilibili::BiliError>() {
                            match bili_err {
                                crate::bilibili::BiliError::RequestFailed(87007 | 87008, _) => true,
                                crate::bilibili::BiliError::RequestFailed(code, msg) => {
                                    // 检查其他可能的充电专享视频错误码或消息
                                    msg.contains("充电专享")
                                        || msg.contains("需要充电")
                                        || msg.contains("试看视频")
                                        || msg.contains("大会员专享")
                                        || (*code == -403 && msg.contains("access denied"))
                                }
                                _ => false,
                            }
                        } else {
                            // 检查非BiliError类型的错误是否可能是充电专享视频错误
                            let error_str = e.to_string().to_lowercase();
                            error_str.contains("充电专享")
                                || error_str.contains("需要充电")
                                || error_str.contains("试看视频")
                                || error_str.contains("大会员专享")
                                || error_str.contains("access denied")
                        }
                    };

                    if !is_charging_video_error {
                        tracing::debug!("× 番剧质量 qn={} 获取失败: {}", qn, e);
                    } else {
                        tracing::debug!("× 番剧质量 qn={} 获取失败: 充电专享视频", qn);
                    }

                    if attempt == quality_levels.len() - 1 {
                        // 最后一次尝试也失败了
                        if is_charging_video_error {
                            tracing::info!("番剧需要充电才能观看");
                            // 对于充电专享番剧，统一返回87007错误以便上层正确处理
                            return Err(crate::bilibili::BiliError::RequestFailed(
                                87007,
                                "充电专享视频，需要为UP主充电才能观看".to_string(),
                            )
                            .into());
                        } else {
                            tracing::error!("所有番剧质量级别都获取失败");

                            // 检查是否为HTTP 412风控错误
                            let error_str = e.to_string();
                            if error_str.contains("412 Precondition Failed") {
                                // 先检查视频是否已被删除
                                if let Ok(exists) = self.check_video_exists().await {
                                    if !exists {
                                        tracing::warn!("检测到番剧HTTP 412但视频已被删除，返回404错误而非风控");
                                        return Err(crate::bilibili::BiliError::RequestFailed(
                                            -404,
                                            "视频已被删除".to_string(),
                                        )
                                        .into());
                                    }
                                }
                                tracing::warn!("检测到番剧HTTP 412风控错误，转换为风控异常");
                                return Err(crate::bilibili::BiliError::RiskControlOccurred.into());
                            }
                        }
                        return Err(e);
                    }
                    // 继续尝试下一个质量级别
                    continue;
                }
            }
        }

        // 理论上不会到达这里
        Err(anyhow!("无法获取任何质量的番剧视频流"))
    }

    /// 使用指定质量获取番剧页面分析器
    async fn get_bangumi_page_analyzer_with_quality(
        &self,
        page: &PageInfo,
        ep_id: &str,
        qn: &str,
    ) -> Result<PageAnalyzer> {
        // 修复字符串生命周期问题
        let cid_string = page.cid.to_string();

        // 生成硬件指纹
        let fingerprint = HardwareFingerprint::default();
        let hardware = fingerprint.get_hardware();

        // 生成弹幕防挡参数（使用会话固定的硬件指纹）
        let dm_img_str = hardware.generate_dm_img_str();
        let dm_cover_img_str = hardware.generate_dm_cover_img_str();
        let dm_img_list = fingerprint.generate_dm_img_list(page.duration as u32);
        let dm_img_inter = fingerprint.generate_dm_img_inter();

        // 增强的番剧API参数配置，包含硬件指纹和弹幕防挡参数
        let params = [
            ("ep_id", ep_id),
            ("cid", cid_string.as_str()),
            ("qn", qn), // 使用指定的质量参数
            ("otype", "json"),
            ("fnval", "4048"), // 恢复原始fnval值
            ("fourk", "1"),    // 启用4K支持
            ("voice_balance", "1"), // 音频平衡
            ("gaia_source", "pre-load"), // Gaia预加载
            ("isGaiaAvoided", "true"), // Gaia避免策略
            ("web_location", "1315873"), // 网页位置标识
            ("dm_img_str", dm_img_str.as_str()), // WebGL信息
            ("dm_cover_img_str", dm_cover_img_str.as_str()), // GPU信息
            ("dm_img_list", dm_img_list.as_str()), // 弹幕交互数据
            ("dm_img_inter", dm_img_inter.as_str()), // 弹幕交互统计
        ];

        tracing::debug!("番剧API参数: {:?}", params);

        let request_url = "https://api.bilibili.com/pgc/player/web/playurl";
        tracing::debug!("发起番剧playurl请求: {} - Episode ID: {}, CID: {}, 质量: {}", request_url, ep_id, page.cid, qn);

        let request = self
            .client
            .request(Method::GET, request_url)
            .await
            .query(&params)
            .headers(create_api_headers());

        // 番剧请求头日志已在建造器时设置

        let response = request.send().await;
        match &response {
            Ok(resp) => {
                tracing::debug!("番剧playurl请求成功 - 状态码: {}, URL: {}", resp.status(), resp.url());
                tracing::debug!("番剧响应头: {:?}", resp.headers());
            }
            Err(e) => {
                tracing::error!("番剧playurl请求失败 - Episode ID: {}, CID: {}, 错误: {}", ep_id, page.cid, e);
            }
        }

        let res = response?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        tracing::debug!("番剧playurl响应数据大小: {} bytes", serde_json::to_string(&res).unwrap_or_default().len());

        // 添加详细的番剧API响应日志
        tracing::debug!(
            "番剧API完整响应: {}",
            serde_json::to_string_pretty(&res).unwrap_or_else(|_| "无法序列化".to_string())
        );

        // 记录关键字段
        if let Some(code) = res["code"].as_i64() {
            tracing::debug!("番剧API返回code: {}", code);
        }
        if let Some(message) = res["message"].as_str() {
            tracing::debug!("番剧API返回message: {}", message);
        }

        // 检查result字段是否存在
        if res["result"].is_null() {
            tracing::debug!("番剧API返回的result字段为null");
        } else if let Some(dash) = res["result"]["dash"].as_object() {
            tracing::debug!(
                "番剧dash对象存在，视频流数量: {}",
                dash.get("video")
                    .and_then(|v| v.as_array())
                    .map(|v| v.len())
                    .unwrap_or(0)
            );
            tracing::debug!(
                "番剧dash对象存在，音频流数量: {}",
                dash.get("audio")
                    .and_then(|v| v.as_array())
                    .map(|v| v.len())
                    .unwrap_or(0)
            );
        } else {
            tracing::debug!("番剧API返回的result.dash字段不存在或不是对象");
        }

        // 检查番剧API响应中的错误信息
        if let Some(code) = res["code"].as_i64() {
            if code != 0 {
                let message = res["message"].as_str().unwrap_or("未知错误");
                return Err(crate::bilibili::BiliError::RequestFailed(code, message.to_string()).into());
            }
        }

        // 检查是否有可用的番剧视频流
        if res["result"]["dash"]["video"].as_array().is_none_or(|v| v.is_empty()) {
            tracing::error!(
                "番剧视频流为空，完整的result字段: {}",
                serde_json::to_string_pretty(&res["result"]).unwrap_or_else(|_| "无法序列化".to_string())
            );
            return Err(crate::bilibili::BiliError::VideoStreamEmpty("番剧API返回的视频流为空".to_string()).into());
        }

        // 记录成功获取的番剧质量信息
        if let Some(quality) = res["result"]["quality"].as_u64() {
            tracing::debug!("番剧API返回的实际质量: {}", quality);
        }
        if let Some(accept_quality) = res["result"]["accept_quality"].as_array() {
            let qualities: Vec<u64> = accept_quality.iter().filter_map(|v| v.as_u64()).collect();
            tracing::debug!("番剧可用质量列表: {:?}", qualities);
        }

        let mut validated_res = res.validate()?;
        Ok(PageAnalyzer::new(validated_res["result"].take()))
    }

    /// 专门为番剧获取播放地址分析器
    pub async fn get_bangumi_page_analyzer(&self, page: &PageInfo, ep_id: &str) -> Result<PageAnalyzer> {
        // 修复字符串生命周期问题
        let cid_string = page.cid.to_string();

        // 生成硬件指纹
        let fingerprint = HardwareFingerprint::default();
        let hardware = fingerprint.get_hardware();

        // 生成弹幕防挡参数（使用会话固定的硬件指纹）
        let dm_img_str = hardware.generate_dm_img_str();
        let dm_cover_img_str = hardware.generate_dm_cover_img_str();
        let dm_img_list = fingerprint.generate_dm_img_list(page.duration as u32);
        let dm_img_inter = fingerprint.generate_dm_img_inter();

        // 增强的番剧API参数配置，包含硬件指纹和弹幕防挡参数
        let params = [
            ("ep_id", ep_id),
            ("cid", cid_string.as_str()),
            ("qn", "127"), // 恢复原始qn=127请求8K质量
            ("otype", "json"),
            ("fnval", "4048"), // 恢复原始fnval值
            ("fourk", "1"),    // 启用4K支持
            ("voice_balance", "1"), // 音频平衡
            ("gaia_source", "pre-load"), // Gaia预加载
            ("isGaiaAvoided", "true"), // Gaia避免策略
            ("web_location", "1315873"), // 网页位置标识
            ("dm_img_str", dm_img_str.as_str()), // WebGL信息
            ("dm_cover_img_str", dm_cover_img_str.as_str()), // GPU信息
            ("dm_img_list", dm_img_list.as_str()), // 弹幕交互数据
            ("dm_img_inter", dm_img_inter.as_str()), // 弹幕交互统计
        ];

        tracing::debug!("=== 番剧API参数调试 ===");
        tracing::debug!("番剧EP: {}", ep_id);
        tracing::debug!("分页: cid: {}", page.cid);
        tracing::debug!("请求参数: {:?}", params);

        let request_url = "https://api.bilibili.com/pgc/player/web/playurl";

        // 构建完整的URL用于日志显示
        let mut full_url = format!("{}?", request_url);
        for (i, (key, value)) in params.iter().enumerate() {
            if i > 0 {
                full_url.push('&');
            }
            full_url.push_str(&format!("{}={}", key, value));
        }

        tracing::debug!("发起番剧playurl请求(分页): {} - EP ID: {}, CID: {}", request_url, ep_id, page.cid);
        tracing::debug!("完整请求URL: {}", full_url);

        let request = self
            .client
            .request(Method::GET, request_url)
            .await
            .query(&params)
            .headers(create_api_headers());

        // 番剧请求头日志已在建造器时设置

        let response = request.send().await;
        match &response {
            Ok(resp) => {
                tracing::debug!("番剧playurl请求成功(分页) - 状态码: {}, URL: {}", resp.status(), resp.url());
                tracing::debug!("番剧响应头(分页): {:?}", resp.headers());
            }
            Err(e) => {
                tracing::error!("番剧playurl请求失败(分页) - EP ID: {}, CID: {}, 错误: {}", ep_id, page.cid, e);
            }
        }

        let res = response?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        tracing::debug!("番剧playurl响应数据大小(分页): {} bytes", serde_json::to_string(&res).unwrap_or_default().len());

        // 增强的番剧API响应调试信息
        tracing::debug!("=== 番剧API响应调试 ===");
        if let Some(code) = res["code"].as_i64() {
            tracing::debug!("响应代码: {}", code);
        }
        if let Some(message) = res["message"].as_str() {
            tracing::debug!("响应消息: {}", message);
        }

        // 记录番剧视频质量信息
        if let Some(quality) = res["result"]["quality"].as_u64() {
            tracing::debug!("番剧API返回的当前质量: {}", quality);
        }
        if let Some(accept_quality) = res["result"]["accept_quality"].as_array() {
            let qualities: Vec<u64> = accept_quality.iter().filter_map(|v| v.as_u64()).collect();
            tracing::debug!("番剧API返回的可用质量列表: {:?}", qualities);
        }

        // 检查番剧会员要求
        if let Some(vip_status) = res["result"]["vip_status"].as_i64() {
            tracing::debug!("番剧VIP状态要求: {}", vip_status);
        }
        if let Some(vip_type) = res["result"]["vip_type"].as_i64() {
            tracing::debug!("番剧VIP类型: {}", vip_type);
        }

        // 记录音频流信息
        if let Some(audio_array) = res["result"]["dash"]["audio"].as_array() {
            tracing::debug!("=== 番剧音频流信息 ===");
            tracing::debug!("番剧音频流数量: {}", audio_array.len());
            for (i, audio) in audio_array.iter().enumerate() {
                if let Some(id) = audio["id"].as_u64() {
                    let codec = audio["codecs"].as_str().unwrap_or("unknown");
                    let bandwidth = audio["bandwidth"].as_u64().unwrap_or(0);
                    tracing::debug!("番剧音频流{}: ID={}, codec={}, bandwidth={}", i, id, codec, bandwidth);
                }
            }
            tracing::debug!("=== 番剧音频流信息结束 ===");
        }

        // 记录杜比音频流信息
        if let Some(dolby) = res["result"]["dash"]["dolby"].as_object() {
            tracing::debug!("=== 番剧Dolby音频信息 ===");
            if let Some(dolby_type) = dolby["type"].as_u64() {
                tracing::debug!("Dolby类型: {}", dolby_type);
            }
            if let Some(dolby_audio) = dolby["audio"].as_array() {
                tracing::debug!("Dolby音频流数量: {}", dolby_audio.len());
                for (i, audio) in dolby_audio.iter().enumerate() {
                    if let Some(id) = audio["id"].as_u64() {
                        tracing::debug!("Dolby音频流{}: ID={}", i, id);
                    }
                }
            }
            tracing::debug!("=== 番剧Dolby音频信息结束 ===");
        }

        // 记录FLAC音频流信息
        if let Some(flac) = res["result"]["dash"]["flac"].as_object() {
            tracing::debug!("=== 番剧FLAC音频信息 ===");
            if let Some(flac_display) = flac["display"].as_bool() {
                tracing::debug!("FLAC显示状态: {}", flac_display);
            }
            if let Some(flac_audio) = flac["audio"].as_object() {
                if let Some(id) = flac_audio["id"].as_u64() {
                    tracing::debug!("FLAC音频流ID: {}", id);
                }
            }
            tracing::debug!("=== 番剧FLAC音频信息结束 ===");
        }

        tracing::debug!("=== 番剧API响应调试结束 ===");

        let mut validated_res = res.validate()?;
        Ok(PageAnalyzer::new(validated_res["result"].take()))
    }

    pub async fn get_subtitles(&self, page: &PageInfo) -> Result<Vec<SubTitle>> {
        let res = self
            .client
            .request(Method::GET, "https://api.bilibili.com/x/player/wbi/v2")
            .await
            .query(&encoded_query(
                vec![("cid", &page.cid.to_string()), ("bvid", &self.bvid), ("aid", &self.aid)],
                MIXIN_KEY.load().as_deref(),
            ))
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?
            .validate()?;

        // 检查字幕数据是否存在
        let subtitle_data = &res["data"]["subtitle"];
        if subtitle_data.is_null() {
            debug!("视频没有字幕数据");
            return Ok(Vec::new());
        }

        // 接口返回的信息，包含了一系列的字幕，每个字幕包含了字幕的语言和 json 下载地址
        let subtitles_info: SubTitlesInfo = serde_json::from_value(subtitle_data.clone())?;
        let tasks = subtitles_info
            .subtitles
            .into_iter()
            .filter(|v| !v.is_ai_sub())
            .map(|v| self.get_subtitle(v))
            .collect::<FuturesUnordered<_>>();
        tasks.try_collect().await
    }

    async fn get_subtitle(&self, info: SubTitleInfo) -> Result<SubTitle> {
        let mut res = self
            .client
            .client // 这里可以直接使用 inner_client，因为该请求不需要鉴权
            .request(Method::GET, format!("https:{}", &info.subtitle_url).as_str(), None)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;
        let body: SubTitleBody = serde_json::from_value(res["body"].take())?;
        Ok(SubTitle { lan: info.lan, body })
    }

    /// 处理风控验证流程
    async fn handle_risk_control_verification(&self, v_voucher: String) -> Result<String> {
        use crate::bilibili::{RiskControl, VerificationRequest, VERIFICATION_COORDINATOR};
        use crate::config::with_config;

        tracing::info!("开始处理风控验证，v_voucher: {}", v_voucher);

        // 获取风控配置
        let risk_config = with_config(|bundle| bundle.config.risk_control.clone());

        if !risk_config.enabled {
            tracing::warn!("风控验证已禁用，跳过验证");
            anyhow::bail!("风控验证已禁用");
        }

        match risk_config.mode.as_str() {
            "skip" => {
                tracing::warn!("风控模式设置为跳过，不进行验证");
                anyhow::bail!("风控模式设置为跳过");
            }
            "manual" => {
                // 创建风控处理器
                let risk_control = RiskControl::new(self.client, v_voucher.clone());

                // 第一步：申请验证码
                let captcha_info = risk_control.register().await?;
                tracing::info!("成功获取验证码信息");

                // 第二步：请求验证协调器处理验证
                let verification_request = VERIFICATION_COORDINATOR
                    .request_verification(v_voucher, captcha_info)
                    .await?;

                match verification_request {
                    VerificationRequest::StartNew(_captcha_info) => {
                        tracing::info!("启动新验证流程，已在管理页 /captcha 提供验证界面");
                        tracing::info!("请在浏览器中访问管理页面完成验证，超时时间: {}秒", risk_config.timeout);

                        // 等待用户完成验证
                        let captcha_result = tokio::time::timeout(
                            std::time::Duration::from_secs(risk_config.timeout),
                            VERIFICATION_COORDINATOR.wait_for_captcha_result(),
                        )
                        .await
                        .map_err(|_| anyhow::anyhow!("验证码验证等待超时"))??;

                        // 使用验证结果获取gaia_vtoken
                        tracing::info!("收到验证结果，正在获取gaia_vtoken");
                        let gaia_vtoken = risk_control.validate(captcha_result).await?;

                        // 保存token到协调器缓存
                        VERIFICATION_COORDINATOR.save_token(gaia_vtoken.clone()).await;
                        tracing::info!("风控验证完成，获取到gaia_vtoken");

                        Ok(gaia_vtoken)
                    }
                    VerificationRequest::WaitForExisting => {
                        tracing::info!("检测到正在进行的验证，等待完成...");
                        let gaia_vtoken = VERIFICATION_COORDINATOR.wait_for_completion().await?;
                        Ok(gaia_vtoken)
                    }
                    VerificationRequest::UseCache(gaia_vtoken) => {
                        tracing::info!("使用缓存的gaia_vtoken");
                        Ok(gaia_vtoken)
                    }
                }
            }
            "auto" => {
                // 创建风控处理器
                let risk_control = RiskControl::new(self.client, v_voucher.clone());

                // 第一步：申请验证码
                let captcha_info = risk_control.register().await?;
                tracing::info!("成功获取验证码信息，准备自动解决");

                // 第二步：请求验证协调器处理
                let verification_request = VERIFICATION_COORDINATOR
                    .request_verification(v_voucher, captcha_info)
                    .await?;

                match verification_request {
                    VerificationRequest::StartNew(_) => {
                        tracing::info!("开始自动解决验证码");

                        // 调用自动解决方法
                        let page_url = "https://www.bilibili.com";
                        let captcha_result = VERIFICATION_COORDINATOR
                            .auto_solve_captcha(&risk_config, page_url)
                            .await?;

                        // 使用验证结果获取gaia_vtoken
                        tracing::info!("自动验证成功，正在获取gaia_vtoken");
                        let gaia_vtoken = risk_control.validate(captcha_result).await?;

                        // 保存token到协调器缓存
                        VERIFICATION_COORDINATOR.save_token(gaia_vtoken.clone()).await;
                        tracing::info!("自动风控验证完成，获取到gaia_vtoken");

                        Ok(gaia_vtoken)
                    }
                    VerificationRequest::WaitForExisting => {
                        tracing::info!("检测到正在进行的验证，等待完成...");
                        let gaia_vtoken = VERIFICATION_COORDINATOR.wait_for_completion().await?;
                        Ok(gaia_vtoken)
                    }
                    VerificationRequest::UseCache(gaia_vtoken) => {
                        tracing::info!("使用缓存的gaia_vtoken");
                        Ok(gaia_vtoken)
                    }
                }
            }
            _ => {
                tracing::error!("未知的风控模式: {}", risk_config.mode);
                anyhow::bail!("未知的风控模式: {}", risk_config.mode);
            }
        }
    }
}

pub fn bvid_to_aid(bvid: &str) -> u64 {
    let mut bvid = bvid.chars().collect::<Vec<_>>();
    (bvid[3], bvid[9]) = (bvid[9], bvid[3]);
    (bvid[4], bvid[7]) = (bvid[7], bvid[4]);
    let mut tmp = 0u64;
    for char in bvid.into_iter().skip(3) {
        let idx = DATA.iter().position(|&x| x == char).expect("invalid bvid");
        tmp = tmp * BASE + idx as u64;
    }
    (tmp & MASK_CODE) ^ XOR_CODE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bvid_to_aid() {
        assert_eq!(bvid_to_aid("BV1Tr421n746"), 1401752220u64);
        assert_eq!(bvid_to_aid("BV1sH4y1s7fe"), 1051892992u64);
    }
}
