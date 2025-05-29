use std::pin::Pin;

use anyhow::{Result, bail};
use async_stream::try_stream;
use chrono::{DateTime, Utc};
use futures::Stream;
use serde::Deserialize;
use tracing;

use super::{BiliClient, Validate, VideoInfo};

pub struct Bangumi {
    client: BiliClient,
    media_id: Option<String>,
    season_id: Option<String>,
    ep_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BangumiEpisode {
    pub id: i64,       // ep_id
    pub aid: i64,      // 视频 aid
    pub bvid: String,  // 视频 bvid
    pub cid: i64,      // 视频 cid
    pub title: String, // 集标题
    #[allow(dead_code)]
    pub long_title: String, // 集副标题
    pub pub_time: i64, // 发布时间戳
    #[allow(dead_code)]
    pub duration: i64, // 视频时长（毫秒）
    pub show_title: String, // 显示标题
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
            let resp = self.client.get(&url).await?;
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
            let resp = self.client.get(&url).await?;
            let json: serde_json::Value = resp.json().await?;
            json.validate()?["result"]["season_id"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        } else {
            bail!("season_id or ep_id is required");
        };

        let url = format!("https://api.bilibili.com/pgc/view/web/season?season_id={}", season_id);
        let resp = self.client.get(&url).await?;
        let json: serde_json::Value = resp.json().await?;
        json.validate().map(|v| v["result"].clone())
    }

    /// 获取番剧分集信息
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
            };
            tracing::debug!("解析剧集：{} (EP{}) BV号: {}", ep.title, ep.id, ep.bvid);
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
    pub fn to_video_stream(&self) -> Pin<Box<dyn Stream<Item = Result<VideoInfo>> + Send>> {
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
            let current_season_id = season_info["season_id"].as_str().unwrap_or_default().to_string();

            // 计算当前季度在seasons数组中的位置，作为季度编号
            let season_number = if let Some(seasons) = season_info["seasons"].as_array() {
                seasons.iter().position(|season| {
                    season["season_id"].as_str().unwrap_or_default() == current_season_id ||
                    season["season_id"].as_i64().map(|id| id.to_string()).as_deref() == Some(&current_season_id)
                }).map(|pos| (pos + 1) as i32)
            } else {
                Some(1) // 如果没有seasons数组，默认为第1季
            };

            debug!("番剧标题: {}, 季度编号: {:?}", title, season_number);

            let episodes = bangumi.get_episodes().await?;
            debug!("获取到 {} 集番剧内容", episodes.len());

            for episode in episodes {
                // 将发布时间戳转换为 DateTime<Utc>
                let pub_time = DateTime::<Utc>::from_timestamp(episode.pub_time, 0)
                    .unwrap_or_else(Utc::now);

                // 使用show_title字段作为标题
                let episode_title = if !episode.show_title.is_empty() {
                    episode.show_title.clone()
                } else {
                    format!("{} - {}", title, episode.title)
                };

                // 直接从API的title字段获取集数
                let episode_number = episode.title.parse::<i32>().ok();

                tracing::debug!("生成番剧视频信息: {}, BV: {}, 集数: {:?}", episode_title, episode.bvid, episode_number);

                yield VideoInfo::Bangumi {
                    title: episode_title,
                    season_id: current_season_id.clone(),
                    ep_id: episode.id.to_string(),
                    bvid: episode.bvid.clone(),
                    cid: episode.cid.to_string(),
                    aid: episode.aid.to_string(),
                    cover: cover.clone(),
                    intro: intro.clone(),
                    pubtime: pub_time,
                    show_title: Some(episode.show_title.clone()),
                    season_number,
                    episode_number,
                }
            }
        })
    }

    /// 将所有季度的番剧转换为视频流
    pub fn to_all_seasons_video_stream(&self) -> Pin<Box<dyn Stream<Item = Result<VideoInfo>> + Send>> {
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

            // 对每个季度进行处理
            for (season_index, season) in seasons.iter().enumerate() {
                debug!("处理季度: {} (season_id: {})", season.season_title, season.season_id);
                let season_id_clone = season.season_id.clone(); // 先克隆一份
                let season_bangumi = Bangumi::new(&client, season.media_id.clone(), Some(season.season_id.clone()), None);
                let season_info = season_bangumi.get_season_info().await?;

                let cover = season_info["cover"].as_str().unwrap_or_default().to_string();
                let title = season_info["title"].as_str().unwrap_or_default().to_string();
                let intro = season_info["evaluate"].as_str().unwrap_or_default().to_string();

                // 季度编号就是在seasons数组中的位置+1
                let season_number = Some((season_index + 1) as i32);

                let episodes = season_bangumi.get_episodes().await?;
                debug!("季度 {} (第{}季) 获取到 {} 集番剧内容", season.season_title, season_index + 1, episodes.len());

                for episode in episodes {
                    // 将发布时间戳转换为 DateTime<Utc>
                    let pub_time = DateTime::<Utc>::from_timestamp(episode.pub_time, 0)
                        .unwrap_or_else(Utc::now);

                    // 使用show_title字段作为标题
                    let episode_title = if !episode.show_title.is_empty() {
                        episode.show_title.clone()
                    } else {
                        format!("{} - {}", title, episode.title)
                    };

                    // 直接从API的title字段获取集数
                    let episode_number = episode.title.parse::<i32>().ok();

                    tracing::debug!("生成番剧视频信息: {}, BV: {}, 集数: {:?}", episode_title, episode.bvid, episode_number);

                    yield VideoInfo::Bangumi {
                        title: episode_title,
                        season_id: season_id_clone.clone(),
                        ep_id: episode.id.to_string(),
                        bvid: episode.bvid.clone(),
                        cid: episode.cid.to_string(),
                        aid: episode.aid.to_string(),
                        cover: cover.clone(),
                        intro: intro.clone(),
                        pubtime: pub_time,
                        show_title: Some(episode.show_title.clone()),
                        season_number,
                        episode_number,
                    }
                }
            }
        })
    }
}
