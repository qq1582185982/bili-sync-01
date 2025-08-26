//! 直播录制配置
//! 
//! 包含直播录制相关的所有配置项和默认值

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use std::time::Duration;

/// 直播录制配置
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LiveRecordingConfig {
    /// 自动合并配置
    pub auto_merge: AutoMergeConfig,
    /// 录制质量配置
    pub quality: RecordingQualityConfig,
    /// 文件管理配置
    pub file_management: FileManagementConfig,
}

/// 自动合并配置
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AutoMergeConfig {
    /// 是否启用自动合并
    pub enabled: bool,
    /// 自动合并时长阈值（秒）
    /// 当录制时长达到此阈值时自动触发合并
    pub duration_threshold: u64,
    /// 自动合并后是否保留分片文件
    pub keep_segments_after_merge: bool,
    /// 合并输出格式
    pub output_format: String,
    /// 合并输出质量
    pub output_quality: MergeQuality,
}

/// 录制质量配置
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RecordingQualityConfig {
    /// 首选录制格式
    pub preferred_format: String,
    /// 录制分辨率
    pub resolution: String,
    /// 录制帧率
    pub frame_rate: u32,
}

/// 文件管理配置
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FileManagementConfig {
    /// 分片文件保留数量
    pub max_segments_to_keep: usize,
    /// 录制文件命名模板
    pub filename_template: String,
    /// 自动清理旧文件的天数
    pub auto_cleanup_days: u32,
}

/// 合并质量配置
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub enum MergeQuality {
    /// 流复制（无损，速度快）
    StreamCopy,
    /// 重编码（有损，文件小）
    Reencode { 
        video_codec: String, 
        audio_codec: String, 
        bitrate: String 
    },
    /// 自动选择（先尝试流复制，失败则重编码）
    Auto,
}

impl Default for LiveRecordingConfig {
    fn default() -> Self {
        Self {
            auto_merge: AutoMergeConfig::default(),
            quality: RecordingQualityConfig::default(),
            file_management: FileManagementConfig::default(),
        }
    }
}

impl Default for AutoMergeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            duration_threshold: 600, // 10分钟
            keep_segments_after_merge: false,
            output_format: "mp4".to_string(),
            output_quality: MergeQuality::Auto,
        }
    }
}

impl Default for RecordingQualityConfig {
    fn default() -> Self {
        Self {
            preferred_format: "flv".to_string(),
            resolution: "1080p".to_string(),
            frame_rate: 30,
        }
    }
}

impl Default for FileManagementConfig {
    fn default() -> Self {
        Self {
            max_segments_to_keep: 50,
            filename_template: "{upper_name}_{room_id}_{date}_{time}_{title}.{ext}".to_string(),
            auto_cleanup_days: 7,
        }
    }
}

impl AutoMergeConfig {
    /// 获取时长阈值的Duration
    pub fn duration_threshold_as_duration(&self) -> Duration {
        Duration::from_secs(self.duration_threshold)
    }
    
    /// 检查是否应该触发自动合并
    pub fn should_auto_merge(&self, current_duration_secs: f64) -> bool {
        self.enabled && current_duration_secs >= self.duration_threshold as f64
    }
}

impl MergeQuality {
    /// 获取FFmpeg参数
    pub fn get_ffmpeg_args(&self) -> Vec<String> {
        match self {
            MergeQuality::StreamCopy => {
                vec![
                    "-c".to_string(), 
                    "copy".to_string(),
                    "-avoid_negative_ts".to_string(), 
                    "make_zero".to_string(),
                    "-fflags".to_string(), 
                    "+genpts".to_string(),
                ]
            }
            MergeQuality::Reencode { video_codec, audio_codec, bitrate } => {
                vec![
                    "-c:v".to_string(), video_codec.clone(),
                    "-c:a".to_string(), audio_codec.clone(),
                    "-b:v".to_string(), bitrate.clone(),
                    "-avoid_negative_ts".to_string(), "make_zero".to_string(),
                    "-fflags".to_string(), "+genpts".to_string(),
                ]
            }
            MergeQuality::Auto => {
                // 默认先尝试流复制
                Self::StreamCopy.get_ffmpeg_args()
            }
        }
    }
    
    /// 获取重编码的FFmpeg参数（当流复制失败时使用）
    pub fn get_fallback_ffmpeg_args(&self) -> Vec<String> {
        vec![
            "-c:v".to_string(), "libx264".to_string(),
            "-c:a".to_string(), "aac".to_string(),
            "-avoid_negative_ts".to_string(), "make_zero".to_string(),
            "-fflags".to_string(), "+genpts".to_string(),
        ]
    }
}

/// 配置键名常量
pub mod config_keys {
    pub const LIVE_RECORDING_CONFIG: &str = "live_recording_config";
    pub const AUTO_MERGE_ENABLED: &str = "auto_merge_enabled";
    pub const AUTO_MERGE_DURATION: &str = "auto_merge_duration_seconds";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LiveRecordingConfig::default();
        assert!(!config.auto_merge.enabled);
        assert_eq!(config.auto_merge.duration_threshold, 600);
        assert_eq!(config.auto_merge.output_format, "mp4");
    }

    #[test]
    fn test_should_auto_merge() {
        let mut config = AutoMergeConfig::default();
        config.enabled = true;
        config.duration_threshold = 300; // 5分钟

        assert!(!config.should_auto_merge(299.0));
        assert!(config.should_auto_merge(300.0));
        assert!(config.should_auto_merge(600.0));
        
        config.enabled = false;
        assert!(!config.should_auto_merge(600.0));
    }

    #[test]
    fn test_ffmpeg_args() {
        let stream_copy = MergeQuality::StreamCopy;
        let args = stream_copy.get_ffmpeg_args();
        assert!(args.contains(&"-c".to_string()));
        assert!(args.contains(&"copy".to_string()));

        let reencode = MergeQuality::Reencode {
            video_codec: "libx264".to_string(),
            audio_codec: "aac".to_string(),
            bitrate: "2M".to_string(),
        };
        let args = reencode.get_ffmpeg_args();
        assert!(args.contains(&"-c:v".to_string()));
        assert!(args.contains(&"libx264".to_string()));
    }
}