//! 关键词过滤工具模块
//!
//! 提供视频标题的正则表达式关键词过滤功能

use regex::Regex;
use tracing::{debug, warn};

/// 检查视频标题是否匹配任一关键词（正则表达式匹配）
///
/// # Arguments
/// * `title` - 视频标题
/// * `keyword_filters` - 关键词配置（JSON数组字符串）
///
/// # Returns
/// * `true` - 匹配到任一关键词，应该被过滤
/// * `false` - 没有匹配任何关键词，不过滤
pub fn should_filter_video(title: &str, keyword_filters: &Option<String>) -> bool {
    let keywords = match keyword_filters {
        Some(filters) => parse_keywords(filters),
        None => return false,
    };

    if keywords.is_empty() {
        return false;
    }

    for regex in keywords {
        if regex.is_match(title) {
            debug!("视频标题 '{}' 匹配关键词过滤器 '{}'", title, regex.as_str());
            return true;
        }
    }

    false
}

/// 解析关键词JSON数组为正则表达式列表
fn parse_keywords(keyword_filters: &str) -> Vec<Regex> {
    let keywords: Vec<String> = match serde_json::from_str(keyword_filters) {
        Ok(k) => k,
        Err(e) => {
            warn!("解析关键词过滤器失败: {}", e);
            return Vec::new();
        }
    };

    keywords
        .into_iter()
        .filter_map(|pattern| {
            match Regex::new(&pattern) {
                Ok(regex) => Some(regex),
                Err(e) => {
                    warn!("编译正则表达式 '{}' 失败: {}", pattern, e);
                    None
                }
            }
        })
        .collect()
}

/// 验证正则表达式是否有效
pub fn validate_regex(pattern: &str) -> Result<(), String> {
    match Regex::new(pattern) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("无效的正则表达式: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_filter_video_no_filters() {
        assert!(!should_filter_video("测试视频", &None));
        assert!(!should_filter_video("测试视频", &Some("[]".to_string())));
    }

    #[test]
    fn test_should_filter_video_simple_match() {
        let filters = Some(r#"["广告", "推广"]"#.to_string());
        assert!(should_filter_video("这是一个广告视频", &filters));
        assert!(should_filter_video("推广内容", &filters));
        assert!(!should_filter_video("正常视频", &filters));
    }

    #[test]
    fn test_should_filter_video_regex_match() {
        let filters = Some(r#"["第\\d+期", "EP\\d+"]"#.to_string());
        assert!(should_filter_video("第123期节目", &filters));
        assert!(should_filter_video("EP45特辑", &filters));
        assert!(!should_filter_video("普通视频", &filters));
    }

    #[test]
    fn test_validate_regex_valid() {
        assert!(validate_regex("测试").is_ok());
        assert!(validate_regex(r"\d+").is_ok());
        assert!(validate_regex(r"^prefix.*suffix$").is_ok());
    }

    #[test]
    fn test_validate_regex_invalid() {
        assert!(validate_regex(r"[").is_err());
        assert!(validate_regex(r"(unclosed").is_err());
    }
}
