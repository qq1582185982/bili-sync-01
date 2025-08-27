//! 直播录制配置
//! 
//! 包含直播录制相关的所有配置项和默认值

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// 录制模式
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub enum RecordingMode {
    /// FFmpeg模式 - 直接录制到文件
    #[serde(rename = "ffmpeg")]
    FFmpeg,
    /// 分片模式 - HLS分片下载并合并
    #[serde(rename = "segment")]
    Segment,
}

impl Default for RecordingMode {
    fn default() -> Self {
        Self::FFmpeg
    }
}

impl std::fmt::Display for RecordingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordingMode::FFmpeg => write!(f, "ffmpeg"),
            RecordingMode::Segment => write!(f, "segment"),
        }
    }
}

impl std::str::FromStr for RecordingMode {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ffmpeg" => Ok(Self::FFmpeg),
            "segment" => Ok(Self::Segment),
            _ => Err(format!("Invalid recording mode: {}", s))
        }
    }
}

/// 直播录制配置
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LiveRecordingConfig {
    /// 录制模式
    pub recording_mode: RecordingMode,
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
    /// B站质量等级 (qn)
    /// 10000=原画, 800=4K, 401=蓝光杜比, 400=蓝光, 250=超清, 150=高清, 80=流畅
    pub quality_level: u32,
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

/// B站直播质量等级常量
pub mod bilibili_quality {
    /// 原画
    pub const ORIGINAL: u32 = 10000;
    /// 4K
    pub const UHD_4K: u32 = 800;
    /// 蓝光杜比
    pub const BLURAY_DOLBY: u32 = 401;
    /// 蓝光
    pub const BLURAY: u32 = 400;
    /// 超清
    pub const SUPER_HIGH: u32 = 250;
    /// 高清
    pub const HIGH: u32 = 150;
    /// 流畅
    pub const SMOOTH: u32 = 80;
}

/// B站直播质量等级信息
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QualityInfo {
    /// 质量等级 (qn)
    pub qn: u32,
    /// 质量名称
    pub name: String,
    /// 描述
    pub description: String,
}

impl Default for LiveRecordingConfig {
    fn default() -> Self {
        Self {
            recording_mode: RecordingMode::default(),
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
            quality_level: 10000, // 原画
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

impl RecordingQualityConfig {
    /// 获取质量等级的名称
    pub fn get_quality_name(&self) -> String {
        match self.quality_level {
            10000 => "原画".to_string(),
            800 => "4K".to_string(),
            401 => "蓝光杜比".to_string(),
            400 => "蓝光".to_string(),
            250 => "超清".to_string(),
            150 => "高清".to_string(),
            80 => "流畅".to_string(),
            _ => format!("自定义({})", self.quality_level),
        }
    }

    /// 获取所有可用的质量等级
    pub fn get_available_qualities() -> Vec<QualityInfo> {
        vec![
            QualityInfo {
                qn: bilibili_quality::ORIGINAL,
                name: "原画".to_string(),
                description: "最高画质，原始分辨率".to_string(),
            },
            QualityInfo {
                qn: bilibili_quality::UHD_4K,
                name: "4K".to_string(),
                description: "4K超高清画质".to_string(),
            },
            QualityInfo {
                qn: bilibili_quality::BLURAY_DOLBY,
                name: "蓝光杜比".to_string(),
                description: "蓝光画质，支持杜比音效".to_string(),
            },
            QualityInfo {
                qn: bilibili_quality::BLURAY,
                name: "蓝光".to_string(),
                description: "蓝光画质".to_string(),
            },
            QualityInfo {
                qn: bilibili_quality::SUPER_HIGH,
                name: "超清".to_string(),
                description: "超清画质，通常为720p或1080p".to_string(),
            },
            QualityInfo {
                qn: bilibili_quality::HIGH,
                name: "高清".to_string(),
                description: "高清画质，通常为720p".to_string(),
            },
            QualityInfo {
                qn: bilibili_quality::SMOOTH,
                name: "流畅".to_string(),
                description: "流畅画质，通常为480p".to_string(),
            },
        ]
    }
}

impl AutoMergeConfig {
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
        assert_eq!(config.quality.quality_level, bilibili_quality::ORIGINAL);
        assert_eq!(config.quality.preferred_format, "flv");
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
    fn test_quality_levels() {
        let config = RecordingQualityConfig::default();
        assert_eq!(config.get_quality_name(), "原画");
        
        let qualities = RecordingQualityConfig::get_available_qualities();
        assert_eq!(qualities.len(), 7);
        assert_eq!(qualities[0].qn, bilibili_quality::ORIGINAL);
        assert_eq!(qualities[0].name, "原画");
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