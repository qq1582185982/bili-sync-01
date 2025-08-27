use std::collections::VecDeque;
use tracing::debug;

/// 分片信息
#[derive(Debug, Clone)]
pub struct SegmentInfo {
    /// 分片URL
    pub url: String,
    /// 序列号
    pub sequence: u64,
    /// 时长（秒）
    pub duration: f64,
    /// 时间戳（毫秒，从BILI-AUX解析或使用当前时间）
    pub timestamp: i64,
}

/// M3U8播放列表解析器
#[derive(Debug)]
pub struct M3u8Parser {
    /// 上次处理的序列号
    #[allow(dead_code)]
    last_sequence: u64,
    /// 分片缓存（避免重复下载）
    #[allow(dead_code)]
    segments_cache: VecDeque<u64>,
    /// 缓存大小限制
    #[allow(dead_code)]
    cache_size_limit: usize,
}

impl M3u8Parser {
    /// 创建新的解析器
    pub fn new() -> Self {
        Self {
            last_sequence: 0,
            segments_cache: VecDeque::new(),
            cache_size_limit: 50, // 最多缓存50个分片序号
        }
    }

    /// 解析M3U8播放列表，返回新的分片列表
    #[allow(dead_code)]
    pub fn parse_playlist(&mut self, content: &str, base_url: &str) -> Vec<SegmentInfo> {
        let mut segments = Vec::new();
        let mut current_duration = 0.0;
        let mut current_sequence = self.last_sequence + 1;
        let mut _media_sequence = None;
        
        debug!("解析M3U8播放列表，内容长度: {} bytes", content.len());

        // 逐行解析
        for line in content.lines() {
            let line = line.trim();
            
            if line.starts_with("#EXT-X-MEDIA-SEQUENCE:") {
                // 解析媒体序列号
                if let Some(seq_str) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
                    if let Ok(seq) = seq_str.parse::<u64>() {
                        _media_sequence = Some(seq);
                        current_sequence = seq;
                        debug!("播放列表媒体序列号: {}", seq);
                    }
                }
            } else if line.starts_with("#EXTINF:") {
                // 解析分片时长
                if let Some(info_str) = line.strip_prefix("#EXTINF:") {
                    if let Some(duration_str) = info_str.split(',').next() {
                        current_duration = duration_str.parse().unwrap_or(0.0);
                    }
                }
            } else if line.starts_with("#BILI-AUX:") {
                // B站特有标签，包含时间戳信息
                // 格式: #BILI-AUX:timestamp|other_info
                // 暂时忽略，使用系统时间
            } else if line.starts_with("http") || (!line.starts_with('#') && !line.is_empty()) {
                // 这是分片URL
                let segment_url = if line.starts_with("http") {
                    line.to_string()
                } else {
                    // 相对路径，需要拼接基础URL
                    format!("{}{}", base_url, line)
                };

                // 检查是否已处理过此序列号
                if !self.is_sequence_processed(current_sequence) {
                    let segment = SegmentInfo {
                        url: segment_url,
                        sequence: current_sequence,
                        duration: current_duration,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    };

                    segments.push(segment);
                    self.mark_sequence_processed(current_sequence);
                    debug!("新分片: 序列号={}, 时长={:.3}s", current_sequence, current_duration);
                }

                current_sequence += 1;
                current_duration = 0.0; // 重置时长
            }
        }

        // 更新最后处理的序列号
        if let Some(last_segment) = segments.last() {
            self.last_sequence = last_segment.sequence;
        }

        debug!("解析完成，发现 {} 个新分片", segments.len());
        segments
    }

    /// 检查序列号是否已处理过
    #[allow(dead_code)]
    fn is_sequence_processed(&self, sequence: u64) -> bool {
        self.segments_cache.contains(&sequence)
    }

    /// 标记序列号已处理
    #[allow(dead_code)]
    fn mark_sequence_processed(&mut self, sequence: u64) {
        // 添加到缓存
        self.segments_cache.push_back(sequence);
        
        // 限制缓存大小
        while self.segments_cache.len() > self.cache_size_limit {
            self.segments_cache.pop_front();
        }
    }

    /// 解析BILI-AUX标签获取精确时间戳
    #[allow(dead_code)]
    fn parse_bili_aux_timestamp(&self, bili_aux_line: &str) -> Option<i64> {
        // BILI-AUX格式: #BILI-AUX:timestamp|other_info
        if let Some(content) = bili_aux_line.strip_prefix("#BILI-AUX:") {
            if let Some(timestamp_str) = content.split('|').next() {
                // 尝试解析十六进制时间戳
                if let Ok(timestamp) = i64::from_str_radix(timestamp_str, 16) {
                    return Some(timestamp);
                }
                
                // 尝试解析十进制时间戳
                if let Ok(timestamp) = timestamp_str.parse::<i64>() {
                    return Some(timestamp);
                }
            }
        }
        None
    }

}

impl Default for M3u8Parser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_m3u8() {
        let mut parser = M3u8Parser::new();
        let content = r#"
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:6
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:6.0,
segment0.ts
#EXTINF:6.0,
segment1.ts
"#;

        let segments = parser.parse_playlist(content, "https://example.com/");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].sequence, 0);
        assert_eq!(segments[1].sequence, 1);
        assert_eq!(segments[0].duration, 6.0);
    }

    #[test]
    fn test_parse_with_base_url() {
        let mut parser = M3u8Parser::new();
        let content = r#"
#EXTM3U
#EXT-X-MEDIA-SEQUENCE:100
#EXTINF:3.0,
relative_segment.ts
"#;

        let segments = parser.parse_playlist(content, "https://cdn.example.com/live/");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].url, "https://cdn.example.com/live/relative_segment.ts");
        assert_eq!(segments[0].sequence, 100);
    }

    #[test]
    fn test_duplicate_processing() {
        let mut parser = M3u8Parser::new();
        let content = r#"
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:3.0,
segment0.ts
"#;

        // 第一次解析
        let segments1 = parser.parse_playlist(content, "https://example.com/");
        assert_eq!(segments1.len(), 1);

        // 第二次解析相同内容
        let segments2 = parser.parse_playlist(content, "https://example.com/");
        assert_eq!(segments2.len(), 0); // 应该没有新分片
    }
}