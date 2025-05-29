use serde_json::json;

use crate::config::CONFIG;

pub fn video_format_args(video_model: &bili_sync_entity::video::Model) -> serde_json::Value {
    json!({
        "bvid": &video_model.bvid,
        "title": &video_model.name,
        "upper_name": &video_model.upper_name,
        "upper_mid": &video_model.upper_id,
        "pubtime": &video_model.pubtime.and_utc().format(&CONFIG.time_format).to_string(),
        "fav_time": &video_model.favtime.and_utc().format(&CONFIG.time_format).to_string(),
        "show_title": &video_model.name,
    })
}

/// 番剧专用的格式化函数，使用番剧自己的模板配置
pub fn bangumi_page_format_args(
    video_model: &bili_sync_entity::video::Model,
    page_model: &bili_sync_entity::page::Model,
) -> serde_json::Value {
    // 直接使用数据库中存储的集数，如果没有则使用page_model.pid
    let episode_number = video_model.episode_number.unwrap_or(page_model.pid);

    // 使用数据库中存储的季度编号，如果没有则默认为1
    let season_number = video_model.season_number.unwrap_or(1);

    json!({
        "bvid": &video_model.bvid,
        "title": &video_model.name,
        "upper_name": &video_model.upper_name,
        "upper_mid": &video_model.upper_id,
        "ptitle": &page_model.name,
        "pid": episode_number,
        "pid_pad": format!("{:02}", episode_number),
        "season": season_number,
        "season_pad": format!("{:02}", season_number),
    })
}

pub fn page_format_args(
    video_model: &bili_sync_entity::video::Model,
    page_model: &bili_sync_entity::page::Model,
) -> serde_json::Value {
    // 检查是否为番剧类型
    let is_bangumi = match video_model.source_type {
        Some(1) => true, // source_type = 1 表示为番剧
        _ => false,
    };

    // 检查是否为单P视频
    let is_single_page = video_model.single_page.unwrap_or(true);

    // 对于番剧，使用专门的格式化函数
    if is_bangumi {
        bangumi_page_format_args(video_model, page_model)
    } else if !is_single_page {
        // 对于多P视频（非番剧），使用番剧格式的命名，默认季度为1
        let season_number = 1;

        json!({
            "bvid": &video_model.bvid,
            "title": &video_model.name,
            "upper_name": &video_model.upper_name,
            "upper_mid": &video_model.upper_id,
            "ptitle": &page_model.name,
            "pid": page_model.pid,
            "pid_pad": format!("{:02}", page_model.pid),
            "season": season_number,
            "season_pad": format!("{:02}", season_number),
            "pubtime": video_model.pubtime.and_utc().format(&CONFIG.time_format).to_string(),
            "fav_time": video_model.favtime.and_utc().format(&CONFIG.time_format).to_string(),
            "long_title": &page_model.name,
            "show_title": &page_model.name,
        })
    } else {
        // 对于单P视频，使用原有的格式（不包含season_pad）
        json!({
            "bvid": &video_model.bvid,
            "title": &video_model.name,
            "upper_name": &video_model.upper_name,
            "upper_mid": &video_model.upper_id,
            "ptitle": &page_model.name,
            "pid": page_model.pid,
            "pid_pad": format!("{:02}", page_model.pid),
            "pubtime": video_model.pubtime.and_utc().format(&CONFIG.time_format).to_string(),
            "fav_time": video_model.favtime.and_utc().format(&CONFIG.time_format).to_string(),
            "long_title": &page_model.name,
            "show_title": &page_model.name,
        })
    }
}
