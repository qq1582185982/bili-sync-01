//! 关键词过滤工具模块
//!
//! 提供视频标题的正则表达式关键词过滤功能
//! 支持两种模式：
//! - blacklist（黑名单）：匹配关键词的视频将被排除
//! - whitelist（白名单）：只下载匹配关键词的视频

use regex::Regex;
use tracing::{debug, warn};

/// 关键词过滤模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeywordFilterMode {
    /// 黑名单模式：匹配关键词的视频将被排除（不下载）
    #[default]
    Blacklist,
    /// 白名单模式：只下载匹配关键词的视频
    Whitelist,
}

impl KeywordFilterMode {
    /// 从字符串解析过滤模式
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "whitelist" => KeywordFilterMode::Whitelist,
            _ => KeywordFilterMode::Blacklist, // 默认为黑名单模式
        }
    }

    /// 转换为字符串
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            KeywordFilterMode::Blacklist => "blacklist",
            KeywordFilterMode::Whitelist => "whitelist",
        }
    }
}

/// 检查视频标题是否应该被过滤（支持黑名单/白名单模式）
///
/// # Arguments
/// * `title` - 视频标题
/// * `keyword_filters` - 关键词配置（JSON数组字符串）
/// * `filter_mode` - 过滤模式（黑名单/白名单）
///
/// # Returns
/// * `true` - 视频应该被过滤（不下载）
/// * `false` - 视频不应该被过滤（可以下载）
pub fn should_filter_video_with_mode(
    title: &str,
    keyword_filters: &Option<String>,
    filter_mode: &Option<String>,
) -> bool {
    let keywords = match keyword_filters {
        Some(filters) => parse_keywords(filters),
        None => return false, // 没有关键词配置，不过滤任何视频
    };

    if keywords.is_empty() {
        return false; // 关键词列表为空，不过滤任何视频
    }

    let mode = filter_mode
        .as_ref()
        .map(|m| KeywordFilterMode::from_str(m))
        .unwrap_or_default();

    let matches_any = keywords.iter().any(|regex| regex.is_match(title));

    match mode {
        KeywordFilterMode::Blacklist => {
            // 黑名单模式：匹配到关键词则过滤
            if matches_any {
                debug!(
                    "视频标题 '{}' 匹配黑名单关键词，将被过滤",
                    title
                );
                true
            } else {
                false
            }
        }
        KeywordFilterMode::Whitelist => {
            // 白名单模式：没有匹配到关键词则过滤
            if matches_any {
                debug!(
                    "视频标题 '{}' 匹配白名单关键词，将被下载",
                    title
                );
                false
            } else {
                debug!(
                    "视频标题 '{}' 未匹配白名单关键词，将被过滤",
                    title
                );
                true
            }
        }
    }
}

/// 检查视频标题是否匹配任一关键词（正则表达式匹配）
///
/// 注意：此函数仅支持黑名单模式，保留用于向后兼容
/// 推荐使用 `should_filter_video_with_mode` 函数
///
/// # Arguments
/// * `title` - 视频标题
/// * `keyword_filters` - 关键词配置（JSON数组字符串）
///
/// # Returns
/// * `true` - 匹配到任一关键词，应该被过滤
/// * `false` - 没有匹配任何关键词，不过滤
#[allow(dead_code)]
pub fn should_filter_video(title: &str, keyword_filters: &Option<String>) -> bool {
    should_filter_video_with_mode(title, keyword_filters, &None)
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

    #[test]
    fn test_keyword_filter_mode_blacklist() {
        let filters = Some(r#"["广告", "推广"]"#.to_string());
        let mode = Some("blacklist".to_string());
        // 黑名单模式：匹配到关键词的视频应该被过滤
        assert!(should_filter_video_with_mode("这是一个广告视频", &filters, &mode));
        assert!(should_filter_video_with_mode("推广内容", &filters, &mode));
        // 不匹配的视频不应该被过滤
        assert!(!should_filter_video_with_mode("正常视频", &filters, &mode));
    }

    #[test]
    fn test_keyword_filter_mode_whitelist() {
        let filters = Some(r#"["教程", "学习"]"#.to_string());
        let mode = Some("whitelist".to_string());
        // 白名单模式：匹配到关键词的视频不应该被过滤（可以下载）
        assert!(!should_filter_video_with_mode("Python教程第一集", &filters, &mode));
        assert!(!should_filter_video_with_mode("学习笔记", &filters, &mode));
        // 不匹配关键词的视频应该被过滤（不下载）
        assert!(should_filter_video_with_mode("娱乐视频", &filters, &mode));
        assert!(should_filter_video_with_mode("游戏实况", &filters, &mode));
    }

    #[test]
    fn test_keyword_filter_mode_default() {
        let filters = Some(r#"["广告"]"#.to_string());
        // 不指定模式时默认为黑名单模式
        assert!(should_filter_video_with_mode("广告视频", &filters, &None));
        assert!(!should_filter_video_with_mode("正常视频", &filters, &None));
    }

    #[test]
    fn test_keyword_filter_mode_from_str() {
        assert_eq!(KeywordFilterMode::from_str("blacklist"), KeywordFilterMode::Blacklist);
        assert_eq!(KeywordFilterMode::from_str("whitelist"), KeywordFilterMode::Whitelist);
        assert_eq!(KeywordFilterMode::from_str("WHITELIST"), KeywordFilterMode::Whitelist);
        assert_eq!(KeywordFilterMode::from_str("Blacklist"), KeywordFilterMode::Blacklist);
        // 无效值默认为黑名单
        assert_eq!(KeywordFilterMode::from_str("invalid"), KeywordFilterMode::Blacklist);
        assert_eq!(KeywordFilterMode::from_str(""), KeywordFilterMode::Blacklist);
    }
}
