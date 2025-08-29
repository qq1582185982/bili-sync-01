use std::pin::Pin;

use anyhow::{bail, Result};
use async_stream::try_stream;
use chrono::{DateTime, Utc};
use futures::Stream;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use tracing;

use super::{BiliClient, Validate, VideoInfo};

/// 检测是否为预告片
/// 根据用户确认，section_type: 1 是最可靠的预告片标识
fn is_preview_episode(episode: &serde_json::Value) -> bool {
    episode["section_type"].as_i64().unwrap_or(0) == 1
}

/// 智能集数分配算法，解决特殊剧集集数冲突问题
///
/// 该函数处理B站API返回的剧集数据，为每个剧集分配唯一的集数：
/// 1. 首先处理可以解析为数字的正常集数（如"001", "002"等）
/// 2. 然后按发布时间顺序为特殊剧集（如"中章", "终章 上"等）分配后续集数
/// 3. 确保同一season内所有剧集都有唯一的集数
fn assign_episode_numbers(episodes: &[serde_json::Value]) -> std::collections::HashMap<String, i32> {
    use std::collections::HashMap;

    let mut episode_assignments = HashMap::new();
    let mut parsed_episodes = Vec::new();
    let mut special_episodes = Vec::new();

    // 第一步：分离可解析的数字集数和特殊剧集
    for episode in episodes {
        let ep_id = episode["id"].as_i64().unwrap_or(0).to_string();
        let episode_title_raw = episode["title"].as_str().unwrap_or_default();
        let pub_time = episode["pub_time"].as_i64().unwrap_or(0);

        if let Ok(episode_num) = episode_title_raw.parse::<i32>() {
            // 可解析的数字集数
            parsed_episodes.push((ep_id, episode_num, pub_time));
        } else {
            // 特殊剧集，按发布时间排序
            special_episodes.push((ep_id, episode_title_raw.to_string(), pub_time));
        }
    }

    // 第二步：为可解析的数字集数分配集数
    for (ep_id, episode_num, _) in parsed_episodes {
        episode_assignments.insert(ep_id, episode_num);
    }

    // 第三步：为特殊剧集分配集数
    // 首先按发布时间排序
    special_episodes.sort_by_key(|(_, _, pub_time)| *pub_time);

    // 找出已分配的最大集数
    let max_assigned = episode_assignments.values().max().copied().unwrap_or(0);

    // 为特殊剧集分配从 max_assigned + 1 开始的集数
    for (index, (ep_id, title, _)) in special_episodes.iter().enumerate() {
        let assigned_number = max_assigned + 1 + index as i32;
        episode_assignments.insert(ep_id.clone(), assigned_number);

        tracing::debug!("为特殊剧集 '{}' (EP{}) 分配集数: {}", title, ep_id, assigned_number);
    }

    episode_assignments
}

pub struct Bangumi {
    client: BiliClient,
    media_id: Option<String>,
    season_id: Option<String>,
    ep_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // 整个结构体已弃用，直接从JSON解析更高效
pub struct BangumiEpisode {
    pub id: i64,                    // ep_id
    pub aid: i64,                   // 视频 aid
    pub bvid: String,               // 视频 bvid
    pub cid: i64,                   // 视频 cid
    pub title: String,              // 集标题
    pub long_title: String,         // 集副标题
    pub pub_time: i64,              // 发布时间戳
    pub duration: i64,              // 视频时长（毫秒）
    pub show_title: String,         // 显示标题
    pub cover: String,              // 单集封面
    pub share_copy: Option<String>, // 详细的分享标题
}

#[derive(Debug, Deserialize, Clone)]
pub struct BangumiSeason {
    pub season_id: String,        // 季度ID
    pub media_id: Option<String>, // 媒体ID
    pub season_title: String,     // 季度标题
    #[allow(dead_code)]
    pub cover: String, // 封面图
}

impl Bangumi {
    pub fn new(
        client: &BiliClient,
        media_id: Option<String>,
        season_id: Option<String>,
        ep_id: Option<String>,
    ) -> Self {
        Self {
            client: client.clone(),
            media_id,
            season_id,
            ep_id,
        }
    }

    /// 从 media_id 获取番剧信息
    #[allow(dead_code)]
    pub async fn get_media_info(&self) -> Result<serde_json::Value> {
        if let Some(media_id) = &self.media_id {
            let url = format!("https://api.bilibili.com/pgc/review/user?media_id={}", media_id);
            let resp = self.client.get(&url, CancellationToken::new()).await?;
            let json: serde_json::Value = resp.json().await?;
            json.validate().map(|v| v["result"]["media"].clone())
        } else {
            bail!("media_id is required");
        }
    }

    /// 通过 season_id 获取番剧详情
    pub async fn get_season_info(&self) -> Result<serde_json::Value> {
        let season_id = if let Some(season_id) = &self.season_id {
            season_id.clone()
        } else if let Some(ep_id) = &self.ep_id {
            // 通过 ep_id 获取 season_id
            let url = format!("https://api.bilibili.com/pgc/view/web/season?ep_id={}", ep_id);
            let resp = self.client.get(&url, CancellationToken::new()).await?;
            let json: serde_json::Value = resp.json().await?;
            json.validate()?["result"]["season_id"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        } else {
            bail!("season_id or ep_id is required");
        };

        let url = format!("https://api.bilibili.com/pgc/view/web/season?season_id={}", season_id);
        let resp = self.client.get(&url, CancellationToken::new()).await?;
        let json: serde_json::Value = resp.json().await?;
        json.validate().map(|v| v["result"].clone())
    }

    /// 轻量级检查番剧是否有更新
    /// 返回 (是否有更新, 最新剧集时间)
    pub async fn check_update(&self, last_check_time: Option<DateTime<Utc>>) -> Result<(bool, Option<DateTime<Utc>>)> {
        // 获取最小信息来判断是否有更新
        let season_info = self.get_season_info().await?;

        // 获取最新更新时间
        let latest_episode_time = season_info["new_ep"]["pub_time"]
            .as_i64()
            .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));

        // 如果没有上次检查时间，视为有更新
        let has_update = match (last_check_time, latest_episode_time) {
            (Some(last), Some(latest)) => latest > last,
            (None, Some(_)) => true,
            _ => false,
        };

        Ok((has_update, latest_episode_time))
    }

    /// 获取番剧分集信息（已弃用，直接从season_info解析episodes更高效）
    #[allow(dead_code)]
    pub async fn get_episodes(&self) -> Result<Vec<BangumiEpisode>> {
        let season_info = self.get_season_info().await?;
        let episodes = season_info["episodes"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Failed to get episodes from season info"))?;

        debug!("获取到番剧分集信息，共 {} 集", episodes.len());

        let mut result = Vec::new();

        for episode in episodes {
            let ep = BangumiEpisode {
                id: episode["id"].as_i64().unwrap_or_default(),
                aid: episode["aid"].as_i64().unwrap_or_default(),
                bvid: episode["bvid"].as_str().unwrap_or_default().to_string(),
                cid: episode["cid"].as_i64().unwrap_or_default(),
                title: episode["title"].as_str().unwrap_or_default().to_string(),
                long_title: episode["long_title"].as_str().unwrap_or_default().to_string(),
                pub_time: episode["pub_time"].as_i64().unwrap_or_default(),
                duration: episode["duration"].as_i64().unwrap_or_default(),
                show_title: episode["show_title"].as_str().unwrap_or_default().to_string(),
                cover: episode["cover"].as_str().unwrap_or_default().to_string(),
                share_copy: episode["share_copy"].as_str().map(|s| s.to_string()),
            };
            tracing::debug!(
                "解析剧集：{} (EP{}) BV号: {} 封面: {} share_copy: {:?}",
                ep.title,
                ep.id,
                ep.bvid,
                ep.cover,
                ep.share_copy
            );
            result.push(ep);
        }

        Ok(result)
    }

    /// 获取番剧所有相关季度信息
    pub async fn get_all_seasons(&self) -> Result<Vec<BangumiSeason>> {
        let season_info = self.get_season_info().await?;
        let seasons = season_info["seasons"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Failed to get seasons from season info"))?;

        debug!("获取到番剧相关季度信息，共 {} 季", seasons.len());

        let mut result = Vec::new();

        for season in seasons {
            let season_id = if let Some(id) = season["season_id"].as_str() {
                id.to_string()
            } else if let Some(id) = season["season_id"].as_i64() {
                id.to_string()
            } else {
                tracing::warn!("无法获取season_id，跳过该季度");
                continue;
            };

            let season_data = BangumiSeason {
                season_id,
                media_id: season["media_id"].as_i64().map(|id| id.to_string()),
                season_title: season["season_title"].as_str().unwrap_or_default().to_string(),
                cover: season["cover"].as_str().unwrap_or_default().to_string(),
            };
            debug!(
                "解析季度：{} (season_id: {})",
                season_data.season_title, season_data.season_id
            );
            result.push(season_data);
        }

        Ok(result)
    }

    /// 将单季番剧转换为视频流
    #[allow(dead_code)]
    pub fn to_video_stream(&self) -> Pin<Box<dyn Stream<Item = Result<VideoInfo>> + Send>> {
        self.to_video_stream_incremental(None)
    }

    /// 将单季番剧转换为视频流（支持增量获取）
    pub fn to_video_stream_incremental(
        &self,
        latest_row_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Pin<Box<dyn Stream<Item = Result<VideoInfo>> + Send>> {
        let client = self.client.clone();
        let season_id = self.season_id.clone();
        let media_id = self.media_id.clone();
        let ep_id = self.ep_id.clone();

        Box::pin(try_stream! {
            debug!("开始生成番剧视频流");
            let bangumi = Bangumi::new(&client, media_id, season_id, ep_id);
            let season_info = bangumi.get_season_info().await?;

            let cover = season_info["cover"].as_str().unwrap_or_default().to_string();
            let title = season_info["title"].as_str().unwrap_or_default().to_string();
            let intro = season_info["evaluate"].as_str().unwrap_or_default().to_string();
            let current_season_id = season_info["season_id"]
                .as_i64()
                .map(|id| id.to_string())
                .or_else(|| season_info["season_id"].as_str().map(|s| s.to_string()))
                .unwrap_or_default();
            let show_season_type = season_info["show_season_type"].as_i64().map(|v| v as i32);
            let actors = season_info["actors"].as_str().map(|s| s.to_string());

            // 计算当前季度在seasons数组中的位置，作为季度编号
            let season_number = if let Some(seasons) = season_info["seasons"].as_array() {
                if seasons.is_empty() {
                    // 单季度番剧：seasons数组为空，默认为第1季
                    Some(1)
                } else {
                    // 多季度番剧：从seasons数组中查找当前季度的位置
                    Some(seasons.iter().position(|season| {
                        // 支持字符串和数字两种类型的season_id比较
                        let season_id_str = season["season_id"]
                            .as_i64()
                            .map(|id| id.to_string())
                            .or_else(|| season["season_id"].as_str().map(|s| s.to_string()))
                            .unwrap_or_default();
                        season_id_str == current_season_id
                    }).map(|pos| (pos + 1) as i32).unwrap_or(1)) // 如果找不到位置，默认为第1季
                }
            } else {
                Some(1) // 如果没有seasons数组，默认为第1季
            };

            debug!("番剧标题: {}, 季度编号: {:?}", title, season_number);

            // 统计变量
            let mut total_episodes = 0;
            let mut new_episodes = 0;
            let mut skipped_episodes = 0;
            let mut preview_episodes = 0;

            // 直接从 season_info 中解析分集信息，避免重复API调用
            let episodes = season_info["episodes"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("Failed to get episodes from season info"))?;
            debug!("获取到 {} 集番剧内容", episodes.len());

            // 使用智能集数分配算法为所有剧集分配唯一集数
            let episode_number_map = assign_episode_numbers(episodes);
            debug!("智能集数分配完成，共分配 {} 个集数", episode_number_map.len());

            // 获取当前配置
            let config = crate::config::reload_config();

            for episode in episodes {
                total_episodes += 1;

                // 检查是否为预告片并跳过
                if config.skip_bangumi_preview && is_preview_episode(episode) {
                    let episode_title_raw = episode["title"].as_str().unwrap_or_default().to_string();
                    let show_title = episode["show_title"].as_str().unwrap_or_default().to_string();
                    preview_episodes += 1;
                    debug!("跳过预告片：{} ({})", show_title, episode_title_raw);
                    continue;
                }

                // 解析分集信息
                let ep_id = episode["id"].as_i64().unwrap_or_default();
                let aid = episode["aid"].as_i64().unwrap_or_default();
                let bvid = episode["bvid"].as_str().unwrap_or_default().to_string();
                let cid = episode["cid"].as_i64().unwrap_or_default();
                let episode_title_raw = episode["title"].as_str().unwrap_or_default().to_string();
                let _long_title = episode["long_title"].as_str().unwrap_or_default().to_string();
                let pub_time_timestamp = episode["pub_time"].as_i64().unwrap_or_default();
                let _duration = episode["duration"].as_i64().unwrap_or_default();
                let show_title = episode["show_title"].as_str().unwrap_or_default().to_string();
                let episode_cover_url = episode["cover"].as_str().unwrap_or_default().to_string();
                let share_copy = episode["share_copy"].as_str().map(|s| s.to_string());

                tracing::debug!(
                    "解析剧集：{} (EP{}) BV号: {} 封面: {} share_copy: {:?}",
                    episode_title_raw,
                    ep_id,
                    bvid,
                    episode_cover_url,
                    share_copy
                );

                // 将发布时间戳转换为 DateTime<Utc>
                let pub_time = DateTime::<Utc>::from_timestamp(pub_time_timestamp, 0)
                    .unwrap_or_else(Utc::now);

                // 增量获取：检查旧集数是否需要字段更新
                if let Some(latest_time) = latest_row_at {
                    if pub_time <= latest_time {
                        tracing::trace!("检查旧集数字段更新需求：{} (发布时间: {}, 最新时间: {})", episode_title_raw, pub_time, latest_time);

                        // 为旧集数构建 VideoInfo，用于后续的字段更新检查
                        let episode_title = if !show_title.is_empty() {
                            show_title.clone()
                        } else {
                            format!("{} - {}", title, episode_title_raw)
                        };

                        let episode_number = episode_number_map.get(&ep_id.to_string()).copied();
                        let episode_cover = if !episode_cover_url.is_empty() {
                            episode_cover_url.clone()
                        } else {
                            cover.clone()
                        };

                        // 生成 VideoInfo 用于字段更新检查
                        // 注意：这里生成的 VideoInfo 会在上层的 create_videos 函数中进行字段更新检查
                        yield VideoInfo::Bangumi {
                            title: episode_title,
                            season_id: current_season_id.clone(),
                            ep_id: ep_id.to_string(),
                            bvid,
                            cid: cid.to_string(),
                            aid: aid.to_string(),
                            cover: episode_cover,
                            intro: intro.clone(),
                            pubtime: pub_time,
                            show_title: Some(show_title),
                            season_number,
                            episode_number,
                            share_copy,
                            show_season_type,
                            actors: actors.clone(),
                        };

                        skipped_episodes += 1;
                        continue;
                    }
                }

                new_episodes += 1;

                // 使用show_title字段作为标题
                let episode_title = if !show_title.is_empty() {
                    show_title.clone()
                } else {
                    format!("{} - {}", title, episode_title_raw)
                };

                // 使用智能分配的集数
                let episode_number = episode_number_map.get(&ep_id.to_string()).copied();

                // 使用单集封面，如果没有则回退到季度封面
                let episode_cover = if !episode_cover_url.is_empty() {
                    episode_cover_url.clone()
                } else {
                    cover.clone()
                };

                tracing::debug!("生成番剧视频信息: {}, BV: {}, 集数: {:?}, 封面: {}", episode_title, bvid, episode_number, episode_cover);

                yield VideoInfo::Bangumi {
                    title: episode_title,
                    season_id: current_season_id.clone(),
                    ep_id: ep_id.to_string(),
                    bvid,
                    cid: cid.to_string(),
                    aid: aid.to_string(),
                    cover: episode_cover,
                    intro: intro.clone(),
                    pubtime: pub_time,
                    show_title: Some(show_title),
                    season_number,
                    episode_number,
                    share_copy,
                    show_season_type,
                    actors: actors.clone(),
                }
            }

            // 输出统计信息
            if latest_row_at.is_some() {
                if preview_episodes > 0 {
                    tracing::info!(
                        "单季度番剧「{}」增量获取完成：跳过 {} 集旧内容，跳过 {} 个预告片，处理 {} 集新内容，总计 {} 集",
                        title, skipped_episodes, preview_episodes, new_episodes, total_episodes
                    );
                } else {
                    tracing::info!(
                        "单季度番剧「{}」增量获取完成：跳过 {} 集旧内容，处理 {} 集新内容，总计 {} 集",
                        title, skipped_episodes, new_episodes, total_episodes
                    );
                }
            } else if preview_episodes > 0 {
                tracing::info!(
                    "单季度番剧「{}」全量获取完成：跳过 {} 个预告片，处理 {} 集内容，总计 {} 集",
                    title, preview_episodes, new_episodes, total_episodes
                );
            } else {
                tracing::info!("单季度番剧「{}」全量获取完成：处理 {} 集内容", title, total_episodes);
            }
        })
    }

    /// 将所有季度的番剧转换为视频流
    #[allow(dead_code)]
    pub fn to_all_seasons_video_stream(&self) -> Pin<Box<dyn Stream<Item = Result<VideoInfo>> + Send>> {
        self.to_all_seasons_video_stream_incremental(None)
    }

    /// 将所有季度的番剧转换为视频流（支持增量获取）
    pub fn to_all_seasons_video_stream_incremental(
        &self,
        latest_row_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Pin<Box<dyn Stream<Item = Result<VideoInfo>> + Send>> {
        let client = self.client.clone();
        let season_id = self.season_id.clone();
        let media_id = self.media_id.clone();
        let ep_id = self.ep_id.clone();

        Box::pin(try_stream! {
            debug!("开始生成所有季度的番剧视频流");
            let bangumi = Bangumi::new(&client, media_id, season_id, ep_id);

            // 获取所有季度信息
            let seasons = bangumi.get_all_seasons().await?;
            debug!("获取到 {} 个相关季度", seasons.len());

            let mut processed_seasons = 0;
            let mut total_episodes = 0;
            let mut new_episodes = 0;
            let mut skipped_episodes = 0;
            let mut preview_episodes = 0;

            // 对每个季度进行处理
            for (season_index, season) in seasons.iter().enumerate() {
                debug!("处理季度: {} (season_id: {})", season.season_title, season.season_id);
                let season_id_clone = season.season_id.clone(); // 先克隆一份
                let season_bangumi = Bangumi::new(&client, season.media_id.clone(), Some(season.season_id.clone()), None);
                let season_info = season_bangumi.get_season_info().await?;

                let cover = season_info["cover"].as_str().unwrap_or_default().to_string();
                let title = season_info["title"].as_str().unwrap_or_default().to_string();
                let intro = season_info["evaluate"].as_str().unwrap_or_default().to_string();
                let show_season_type = season_info["show_season_type"].as_i64().map(|v| v as i32);
                let actors = season_info["actors"].as_str().map(|s| s.to_string());

                // 季度编号就是在seasons数组中的位置+1
                let season_number = Some((season_index + 1) as i32);

                // 直接从 season_info 中解析分集信息，避免重复API调用
                let episodes = season_info["episodes"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Failed to get episodes from season info"))?;
                debug!("季度 {} (第{}季) 获取到 {} 集番剧内容", season.season_title, season_index + 1, episodes.len());

                // 使用智能集数分配算法为该季度的所有剧集分配唯一集数
                let episode_number_map = assign_episode_numbers(episodes);
                debug!("季度 {} 智能集数分配完成，共分配 {} 个集数", season.season_title, episode_number_map.len());

                let mut season_has_new_episodes = false;

                // 获取当前配置
                let config = crate::config::reload_config();

                for episode in episodes {
                    total_episodes += 1;

                    // 检查是否为预告片并跳过
                    if config.skip_bangumi_preview && is_preview_episode(episode) {
                        let episode_title_raw = episode["title"].as_str().unwrap_or_default().to_string();
                        let show_title = episode["show_title"].as_str().unwrap_or_default().to_string();
                        preview_episodes += 1;
                        debug!("跳过预告片：{} ({}) - 季度: {}", show_title, episode_title_raw, season.season_title);
                        continue;
                    }

                    // 解析分集信息
                    let ep_id = episode["id"].as_i64().unwrap_or_default();
                    let aid = episode["aid"].as_i64().unwrap_or_default();
                    let bvid = episode["bvid"].as_str().unwrap_or_default().to_string();
                    let cid = episode["cid"].as_i64().unwrap_or_default();
                    let episode_title_raw = episode["title"].as_str().unwrap_or_default().to_string();
                    let _long_title = episode["long_title"].as_str().unwrap_or_default().to_string();
                    let pub_time_timestamp = episode["pub_time"].as_i64().unwrap_or_default();
                    let _duration = episode["duration"].as_i64().unwrap_or_default();
                    let show_title = episode["show_title"].as_str().unwrap_or_default().to_string();
                    let episode_cover_url = episode["cover"].as_str().unwrap_or_default().to_string();
                    let share_copy = episode["share_copy"].as_str().map(|s| s.to_string());

                    tracing::debug!(
                        "解析剧集：{} (EP{}) BV号: {} 封面: {} share_copy: {:?}",
                        episode_title_raw,
                        ep_id,
                        bvid,
                        episode_cover_url,
                        share_copy
                    );

                    // 将发布时间戳转换为 DateTime<Utc>
                    let pub_time = DateTime::<Utc>::from_timestamp(pub_time_timestamp, 0)
                        .unwrap_or_else(Utc::now);

                    // 增量获取：跳过早于latest_row_at的集数
                    if let Some(latest_time) = latest_row_at {
                        if pub_time <= latest_time {
                            tracing::trace!("检查旧集数字段更新需求：{} (发布时间: {}, 最新时间: {})", episode_title_raw, pub_time, latest_time);

                            // 为旧集数构建 VideoInfo，用于后续的字段更新检查
                            let episode_title = if !show_title.is_empty() {
                                show_title.clone()
                            } else {
                                format!("{} - {}", title, episode_title_raw)
                            };

                            let episode_number = episode_number_map.get(&ep_id.to_string()).copied();
                            let episode_cover = if !episode_cover_url.is_empty() {
                                episode_cover_url.clone()
                            } else {
                                cover.clone()
                            };

                            // 生成 VideoInfo 用于字段更新检查
                            yield VideoInfo::Bangumi {
                                title: episode_title,
                                season_id: season_id_clone.clone(),
                                ep_id: ep_id.to_string(),
                                bvid,
                                cid: cid.to_string(),
                                aid: aid.to_string(),
                                cover: episode_cover,
                                intro: intro.clone(),
                                pubtime: pub_time,
                                show_title: Some(show_title),
                                season_number,
                                episode_number,
                                share_copy,
                                show_season_type,
                                actors: actors.clone(),
                            };

                            skipped_episodes += 1;
                            continue;
                        }
                    }

                    season_has_new_episodes = true;
                    new_episodes += 1;

                    // 使用show_title字段作为标题
                    let episode_title = if !show_title.is_empty() {
                        show_title.clone()
                    } else {
                        format!("{} - {}", title, episode_title_raw)
                    };

                    // 直接从API的title字段获取集数
                    let episode_number = episode_number_map.get(&ep_id.to_string()).copied();

                    // 使用单集封面，如果没有则回退到季度封面
                    let episode_cover = if !episode_cover_url.is_empty() {
                        episode_cover_url.clone()
                    } else {
                        cover.clone()
                    };

                    tracing::debug!("生成番剧视频信息: {}, BV: {}, 集数: {:?}, 封面: {}", episode_title, bvid, episode_number, episode_cover);

                    yield VideoInfo::Bangumi {
                        title: episode_title,
                        season_id: season_id_clone.clone(),
                        ep_id: ep_id.to_string(),
                        bvid,
                        cid: cid.to_string(),
                        aid: aid.to_string(),
                        cover: episode_cover,
                        intro: intro.clone(),
                        pubtime: pub_time,
                        show_title: Some(show_title),
                        season_number,
                        episode_number,
                        share_copy,
                        show_season_type,
                        actors: actors.clone(),
                    }
                }

                if season_has_new_episodes {
                    processed_seasons += 1;
                }
            }

            if latest_row_at.is_some() {
                if preview_episodes > 0 {
                    tracing::info!(
                        "所有季度番剧增量获取完成：跳过 {} 集旧内容，跳过 {} 个预告片，处理 {} 集新内容，涉及 {}/{} 个季度，总计 {} 集",
                        skipped_episodes, preview_episodes, new_episodes, processed_seasons, seasons.len(), total_episodes
                    );
                } else {
                    tracing::info!(
                        "所有季度番剧增量获取完成：跳过 {} 集旧内容，处理 {} 集新内容，涉及 {}/{} 个季度，总计 {} 集",
                        skipped_episodes, new_episodes, processed_seasons, seasons.len(), total_episodes
                    );
                }
            } else if preview_episodes > 0 {
                tracing::info!(
                    "所有季度番剧全量获取完成：跳过 {} 个预告片，处理了 {} 个季度，共 {} 集内容，总计 {} 集",
                    preview_episodes, seasons.len(), new_episodes, total_episodes
                );
            } else {
                tracing::info!("所有季度番剧全量获取完成：处理了 {} 个季度，共 {} 集内容", seasons.len(), total_episodes);
            }
        })
    }

    /// 将选中的季度的番剧转换为视频流
    #[allow(dead_code)]
    pub fn to_selected_seasons_video_stream(
        &self,
        selected_seasons: Vec<String>,
    ) -> Pin<Box<dyn Stream<Item = Result<VideoInfo>> + Send>> {
        self.to_selected_seasons_video_stream_incremental(selected_seasons, None)
    }

    /// 将选中的季度的番剧转换为视频流（支持增量获取）
    pub fn to_selected_seasons_video_stream_incremental(
        &self,
        selected_seasons: Vec<String>,
        latest_row_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Pin<Box<dyn Stream<Item = Result<VideoInfo>> + Send>> {
        let client = self.client.clone();
        let season_id = self.season_id.clone();
        let media_id = self.media_id.clone();
        let ep_id = self.ep_id.clone();

        Box::pin(try_stream! {
            debug!("开始生成选中季度的番剧视频流");
            let bangumi = Bangumi::new(&client, media_id, season_id, ep_id);

            // 获取所有季度信息
            let all_seasons = bangumi.get_all_seasons().await?;

            // 过滤出选中的季度
            let seasons: Vec<BangumiSeason> = all_seasons.into_iter()
                .filter(|s| selected_seasons.contains(&s.season_id))
                .collect();

            debug!("筛选出 {} 个选中的季度", seasons.len());

            let mut processed_seasons = 0;
            let mut total_episodes = 0;
            let mut new_episodes = 0;
            let mut skipped_episodes = 0;
            let mut preview_episodes = 0;

            // 对每个选中的季度进行处理
            for (season_index, season) in seasons.iter().enumerate() {
                debug!("处理选中的季度: {} (season_id: {})", season.season_title, season.season_id);
                let season_id_clone = season.season_id.clone();
                let season_bangumi = Bangumi::new(&client, season.media_id.clone(), Some(season.season_id.clone()), None);
                let season_info = season_bangumi.get_season_info().await?;

                let cover = season_info["cover"].as_str().unwrap_or_default().to_string();
                let title = season_info["title"].as_str().unwrap_or_default().to_string();
                let intro = season_info["evaluate"].as_str().unwrap_or_default().to_string();
                let show_season_type = season_info["show_season_type"].as_i64().map(|v| v as i32);
                let actors = season_info["actors"].as_str().map(|s| s.to_string());

                // 获取当前季度在所有季度中的真实位置
                let season_number = if let Some(all_seasons_array) = season_info["seasons"].as_array() {
                    all_seasons_array.iter().position(|s| {
                        // 支持字符串和数字两种类型的season_id比较
                        let s_season_id = s["season_id"]
                            .as_i64()
                            .map(|id| id.to_string())
                            .or_else(|| s["season_id"].as_str().map(|s| s.to_string()))
                            .unwrap_or_default();
                        s_season_id == season.season_id
                    }).map(|pos| (pos + 1) as i32)
                } else {
                    Some((season_index + 1) as i32)
                };

                // 直接从 season_info 中解析分集信息，避免重复API调用
                let episodes = season_info["episodes"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Failed to get episodes from season info"))?;
                debug!("季度 {} (第{}季) 获取到 {} 集番剧内容", season.season_title, season_number.unwrap_or(0), episodes.len());

                // 使用智能集数分配算法为该选中季度的所有剧集分配唯一集数
                let episode_number_map = assign_episode_numbers(episodes);
                debug!("选中季度 {} 智能集数分配完成，共分配 {} 个集数", season.season_title, episode_number_map.len());

                let mut season_has_new_episodes = false;

                // 获取当前配置
                let config = crate::config::reload_config();

                for episode in episodes {
                    total_episodes += 1;

                    // 检查是否为预告片并跳过
                    if config.skip_bangumi_preview && is_preview_episode(episode) {
                        let episode_title_raw = episode["title"].as_str().unwrap_or_default().to_string();
                        let show_title = episode["show_title"].as_str().unwrap_or_default().to_string();
                        preview_episodes += 1;
                        debug!("跳过预告片：{} ({}) - 选中季度: {}", show_title, episode_title_raw, season.season_title);
                        continue;
                    }

                    // 解析分集信息
                    let ep_id = episode["id"].as_i64().unwrap_or_default();
                    let aid = episode["aid"].as_i64().unwrap_or_default();
                    let bvid = episode["bvid"].as_str().unwrap_or_default().to_string();
                    let cid = episode["cid"].as_i64().unwrap_or_default();
                    let episode_title_raw = episode["title"].as_str().unwrap_or_default().to_string();
                    let _long_title = episode["long_title"].as_str().unwrap_or_default().to_string();
                    let pub_time_timestamp = episode["pub_time"].as_i64().unwrap_or_default();
                    let _duration = episode["duration"].as_i64().unwrap_or_default();
                    let show_title = episode["show_title"].as_str().unwrap_or_default().to_string();
                    let episode_cover_url = episode["cover"].as_str().unwrap_or_default().to_string();
                    let share_copy = episode["share_copy"].as_str().map(|s| s.to_string());

                    tracing::debug!(
                        "解析剧集：{} (EP{}) BV号: {} 封面: {} share_copy: {:?}",
                        episode_title_raw,
                        ep_id,
                        bvid,
                        episode_cover_url,
                        share_copy
                    );

                    // 将发布时间戳转换为 DateTime<Utc>
                    let pub_time = DateTime::<Utc>::from_timestamp(pub_time_timestamp, 0)
                        .unwrap_or_else(Utc::now);

                    // 增量获取：跳过早于latest_row_at的集数
                    if let Some(latest_time) = latest_row_at {
                        if pub_time <= latest_time {
                            tracing::trace!("检查旧集数字段更新需求：{} (发布时间: {}, 最新时间: {})", episode_title_raw, pub_time, latest_time);

                            // 为旧集数构建 VideoInfo，用于后续的字段更新检查
                            let episode_title = if !show_title.is_empty() {
                                show_title.clone()
                            } else {
                                format!("{} - {}", title, episode_title_raw)
                            };

                            let episode_number = episode_number_map.get(&ep_id.to_string()).copied();
                            let episode_cover = if !episode_cover_url.is_empty() {
                                episode_cover_url.clone()
                            } else {
                                cover.clone()
                            };

                            // 生成 VideoInfo 用于字段更新检查
                            yield VideoInfo::Bangumi {
                                title: episode_title,
                                season_id: season_id_clone.clone(),
                                ep_id: ep_id.to_string(),
                                bvid,
                                cid: cid.to_string(),
                                aid: aid.to_string(),
                                cover: episode_cover,
                                intro: intro.clone(),
                                pubtime: pub_time,
                                show_title: Some(show_title),
                                season_number,
                                episode_number,
                                share_copy,
                                show_season_type,
                                actors: actors.clone(),
                            };

                            skipped_episodes += 1;
                            continue;
                        }
                    }

                    season_has_new_episodes = true;
                    new_episodes += 1;

                    // 使用show_title字段作为标题
                    let episode_title = if !show_title.is_empty() {
                        show_title.clone()
                    } else {
                        format!("{} - {}", title, episode_title_raw)
                    };

                    // 直接从API的title字段获取集数
                    let episode_number = episode_number_map.get(&ep_id.to_string()).copied();

                    // 使用单集封面，如果没有则回退到季度封面
                    let episode_cover = if !episode_cover_url.is_empty() {
                        episode_cover_url.clone()
                    } else {
                        cover.clone()
                    };

                    tracing::debug!("生成番剧视频信息: {}, BV: {}, 集数: {:?}, 封面: {}", episode_title, bvid, episode_number, episode_cover);

                    yield VideoInfo::Bangumi {
                        title: episode_title,
                        season_id: season_id_clone.clone(),
                        ep_id: ep_id.to_string(),
                        bvid,
                        cid: cid.to_string(),
                        aid: aid.to_string(),
                        cover: episode_cover,
                        intro: intro.clone(),
                        pubtime: pub_time,
                        show_title: Some(show_title),
                        season_number,
                        episode_number,
                        share_copy,
                        show_season_type,
                        actors: actors.clone(),
                    }
                }

                if season_has_new_episodes {
                    processed_seasons += 1;
                }
            }

            if latest_row_at.is_some() {
                if preview_episodes > 0 {
                    tracing::info!(
                        "选中季度番剧增量获取完成：跳过 {} 集旧内容，跳过 {} 个预告片，处理 {} 集新内容，涉及 {}/{} 个季度，总计 {} 集",
                        skipped_episodes, preview_episodes, new_episodes, processed_seasons, seasons.len(), total_episodes
                    );
                } else {
                    tracing::info!(
                        "选中季度番剧增量获取完成：跳过 {} 集旧内容，处理 {} 集新内容，涉及 {}/{} 个季度，总计 {} 集",
                        skipped_episodes, new_episodes, processed_seasons, seasons.len(), total_episodes
                    );
                }
            } else if preview_episodes > 0 {
                tracing::info!(
                    "选中季度番剧全量获取完成：跳过 {} 个预告片，处理了 {} 个季度，共 {} 集内容，总计 {} 集",
                    preview_episodes, seasons.len(), new_episodes, total_episodes
                );
            } else {
                tracing::info!("选中季度番剧全量获取完成：处理了 {} 个季度，共 {} 集内容", seasons.len(), total_episodes);
            }
        })
    }

    #[allow(dead_code)]
    pub async fn get_video_info(&self, ep_id: &str) -> Result<VideoInfo> {
        let url = format!("https://api.bilibili.com/pgc/view/web/season?ep_id={}", ep_id);
        let resp = self.client.get(&url, CancellationToken::new()).await?;
        let json: serde_json::Value = resp.json().await?;
        let validated = json.validate()?;

        let result = &validated["result"];
        let title = result["title"].as_str().unwrap_or_default().to_string();
        let season_id = result["season_id"]
            .as_i64()
            .map(|id| id.to_string())
            .or_else(|| result["season_id"].as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        let ep_id = result["ep_id"].as_str().unwrap_or_default().to_string();
        let bvid = result["bvid"].as_str().unwrap_or_default().to_string();
        let cid = result["cid"].as_i64().unwrap_or_default().to_string();
        let aid = result["aid"].as_i64().unwrap_or_default().to_string();
        let cover = result["cover"].as_str().unwrap_or_default().to_string();
        let intro = result["evaluate"].as_str().unwrap_or_default().to_string();
        let pub_time = result["pub_time"].as_i64().unwrap_or_default();
        let show_title = result["show_title"].as_str().map(|s| s.to_string());
        let _duration = result["duration"].as_i64().unwrap_or_default();

        Ok(VideoInfo::Bangumi {
            title,
            season_id,
            ep_id,
            bvid,
            cid,
            aid,
            cover,
            intro,
            pubtime: DateTime::<Utc>::from_timestamp(pub_time, 0).unwrap_or_else(Utc::now),
            show_title,
            season_number: None,
            episode_number: None,
            share_copy: result["share_copy"].as_str().map(|s| s.to_string()),
            show_season_type: result["show_season_type"].as_i64().map(|v| v as i32),
            actors: result["actors"].as_str().map(|s| s.to_string()),
        })
    }
}
