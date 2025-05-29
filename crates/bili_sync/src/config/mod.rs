use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use arc_swap::ArcSwapOption;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

mod clap;
mod global;
mod item;

use crate::adapter::Args;
use crate::bilibili::{CollectionItem, Credential, DanmakuOption, FilterOption};
pub use crate::config::clap::version;
pub use crate::config::global::{ARGS, CONFIG, CONFIG_DIR, TEMPLATE, reload_config};
use crate::config::item::{ConcurrentLimit, deserialize_collection_list, serialize_collection_list};
pub use crate::config::item::{NFOTimeType, PathSafeTemplate, RateLimit};

// 定义番剧配置结构体
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct BangumiConfig {
    pub season_id: Option<String>,
    pub media_id: Option<String>,
    pub ep_id: Option<String>,
    pub path: PathBuf,
    #[serde(default = "default_download_all_seasons")]
    pub download_all_seasons: bool,
    /// 番剧专用的 video_name 模板，如果未设置则使用全局配置
    #[serde(default)]
    pub video_name: Option<String>,
    /// 番剧专用的 page_name 模板，如果未设置则使用全局 bangumi_name 配置
    #[serde(default)]
    pub page_name: Option<String>,
}

// 定义收藏夹配置结构体
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct FavoriteConfig {
    pub fid: String,
    pub path: PathBuf,
    #[serde(default = "default_download_all_seasons")]
    pub download_all_seasons: bool,
    #[serde(default = "default_page_name")]
    pub page_name: Option<String>,
}

// 定义合集配置结构体
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct CollectionConfig {
    pub collection_type: String, // "season" 或 "series"
    pub upper_id: String,
    pub collection_id: String,
    pub path: PathBuf,
    #[serde(default = "default_download_all_seasons")]
    pub download_all_seasons: bool,
    #[serde(default = "default_page_name")]
    pub page_name: Option<String>,
}

// 定义UP主投稿配置结构体
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct SubmissionConfig {
    pub upper_id: String,
    pub path: PathBuf,
    #[serde(default = "default_download_all_seasons")]
    pub download_all_seasons: bool,
    #[serde(default = "default_page_name")]
    pub page_name: Option<String>,
}

// 定义稍后再看配置结构体
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct WatchLaterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub path: PathBuf,
    #[serde(default = "default_download_all_seasons")]
    pub download_all_seasons: bool,
    #[serde(default = "default_page_name")]
    pub page_name: Option<String>,
}

fn default_time_format() -> String {
    "%Y-%m-%d".to_string()
}

/// 默认的 auth_token 实现，生成随机 16 位字符串
fn default_auth_token() -> Option<String> {
    let byte_choices = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()_+-=";
    let mut rng = rand::thread_rng();
    Some(
        (0..16)
            .map(|_| *(byte_choices.choose(&mut rng).expect("choose byte failed")) as char)
            .collect(),
    )
}

fn default_bind_address() -> String {
    "0.0.0.0:12345".to_string()
}

fn default_download_all_seasons() -> bool {
    false
}

fn default_page_name() -> Option<String> {
    Some("{{title}}".to_string())
}

fn default_multi_page_name() -> Cow<'static, str> {
    Cow::Borrowed("{{title}}-P{{pid_pad}}")
}

fn default_bangumi_name() -> Cow<'static, str> {
    Cow::Borrowed("S{{season_pad}}E{{pid_pad}}-{{pid_pad}}")
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_auth_token")]
    pub auth_token: Option<String>,
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    pub credential: ArcSwapOption<Credential>,
    pub filter_option: FilterOption,
    #[serde(default)]
    pub danmaku_option: DanmakuOption,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub favorite_list_v2: Vec<FavoriteConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub collection_list_v2: Vec<CollectionConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub submission_list_v2: Vec<SubmissionConfig>,
    // 保留旧的配置格式以兼容性
    #[serde(default)]
    pub favorite_list: HashMap<String, PathBuf>,
    #[serde(
        default,
        serialize_with = "serialize_collection_list",
        deserialize_with = "deserialize_collection_list"
    )]
    pub collection_list: HashMap<CollectionItem, PathBuf>,
    #[serde(default)]
    pub submission_list: HashMap<String, PathBuf>,
    #[serde(default)]
    pub watch_later: WatchLaterConfig,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub bangumi: Vec<BangumiConfig>,
    pub video_name: Cow<'static, str>,
    pub page_name: Cow<'static, str>,
    #[serde(default = "default_multi_page_name")]
    pub multi_page_name: Cow<'static, str>,
    #[serde(default = "default_bangumi_name")]
    pub bangumi_name: Cow<'static, str>,
    pub folder_structure: Cow<'static, str>,
    pub interval: u64,
    pub upper_path: PathBuf,
    #[serde(default)]
    pub nfo_time_type: NFOTimeType,
    #[serde(default)]
    pub concurrent_limit: ConcurrentLimit,
    #[serde(default = "default_time_format")]
    pub time_format: String,
    #[serde(default)]
    pub cdn_sorting: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auth_token: default_auth_token(),
            bind_address: default_bind_address(),
            credential: ArcSwapOption::from(Some(Arc::new(Credential::default()))),
            filter_option: FilterOption::default(),
            danmaku_option: DanmakuOption::default(),
            favorite_list_v2: Vec::new(),
            collection_list_v2: Vec::new(),
            submission_list_v2: Vec::new(),
            favorite_list: HashMap::new(),
            collection_list: HashMap::new(),
            submission_list: HashMap::new(),
            watch_later: Default::default(),
            bangumi: Vec::new(),
            video_name: Cow::Borrowed("{{title}}"),
            page_name: Cow::Borrowed("{{title}}"),
            multi_page_name: Cow::Borrowed("{{title}}-P{{pid_pad}}"),
            bangumi_name: Cow::Borrowed("S{{season_pad}}E{{pid_pad}}-{{pid_pad}}"),
            folder_structure: Cow::Borrowed("Season 1"),
            interval: 1200,
            upper_path: CONFIG_DIR.join("upper_face"),
            nfo_time_type: NFOTimeType::FavTime,
            concurrent_limit: ConcurrentLimit::default(),
            time_format: default_time_format(),
            cdn_sorting: true,
        }
    }
}

impl Config {
    pub fn save(&self) -> Result<()> {
        let config_path = CONFIG_DIR.join("config.toml");
        std::fs::create_dir_all(&*CONFIG_DIR)?;

        // 使用 toml_edit 库来原生支持注释，而不是手动字符串操作
        let config_content = self.save_with_structured_comments()?;

        std::fs::write(config_path, config_content)?;
        Ok(())
    }

    /// 使用结构化方式生成带注释的配置文件内容
    fn save_with_structured_comments(&self) -> Result<String> {
        // 先序列化为基本的 TOML 字符串
        let toml_str = toml::to_string_pretty(self)?;

        // 使用 toml_edit 解析并添加注释
        let mut doc = toml_str.parse::<toml_edit::DocumentMut>()?;

        // 为各个部分添加注释
        self.add_structured_comments(&mut doc);

        Ok(doc.to_string())
    }

    /// 使用 toml_edit 的原生 API 添加注释
    fn add_structured_comments(&self, doc: &mut toml_edit::DocumentMut) {
        // 为收藏夹部分添加注释
        if let Some(favorite_item) = doc.get_mut("favorite_list") {
            if let Some(table) = favorite_item.as_table_mut() {
                table
                    .decor_mut()
                    .set_prefix("\n# 收藏夹配置\n# 格式: 收藏夹ID = \"保存路径\"\n# 收藏夹ID可以从收藏夹URL中获取\n");
            }
        }

        // 为合集部分添加注释
        if let Some(collection_item) = doc.get_mut("collection_list") {
            if let Some(table) = collection_item.as_table_mut() {
                table.decor_mut().set_prefix("\n# 合集配置\n# 格式: 合集类型:UP主ID:合集ID = \"保存路径\"\n# 合集类型: season(视频合集) 或 series(视频列表)\n");
            }
        }

        // 为UP主投稿部分添加注释
        if let Some(submission_item) = doc.get_mut("submission_list") {
            if let Some(table) = submission_item.as_table_mut() {
                table
                    .decor_mut()
                    .set_prefix("\n# UP主投稿配置\n# 格式: UP主ID = \"保存路径\"\n# UP主ID可以从UP主空间URL中获取\n");
            }
        }

        // 为番剧部分添加注释
        if let Some(bangumi_item) = doc.get_mut("bangumi") {
            if let Some(array) = bangumi_item.as_array_mut() {
                if !array.is_empty() {
                    array.decor_mut().set_prefix("\n# 番剧配置，可以添加多个[[bangumi]]块\n# season_id: 番剧的season_id，可以从B站番剧页面URL中获取\n# path: 保存番剧的本地路径，必须是绝对路径\n# 注意: season_id和path不能为空，否则程序会报错\n");
                }
            }
        }

        // 为并发限制部分添加注释
        if let Some(concurrent_item) = doc.get_mut("concurrent_limit") {
            if let Some(table) = concurrent_item.as_table_mut() {
                table
                    .decor_mut()
                    .set_prefix("\n# 并发下载配置\n# video: 同时下载的视频数量\n# page: 每个视频同时下载的分页数量\n");

                // 为并行下载子部分添加注释
                if let Some(parallel_item) = table.get_mut("parallel_download") {
                    if let Some(sub_table) = parallel_item.as_table_mut() {
                        sub_table.decor_mut().set_prefix("\n# 多线程下载配置\n# enabled: 是否启用多线程下载\n# threads: 每个文件的下载线程数\n# min_size: 最小文件大小(字节)，小于此大小的文件不使用多线程下载\n");
                    }
                }
            }
        }

        // 为凭据部分添加注释
        if let Some(credential_item) = doc.get_mut("credential") {
            if let Some(table) = credential_item.as_table_mut() {
                table
                    .decor_mut()
                    .set_prefix("\n# B站登录凭据信息\n# 请从浏览器开发者工具中获取这些值\n");
            }
        }

        // 为过滤选项添加注释
        if let Some(filter_item) = doc.get_mut("filter_option") {
            if let Some(table) = filter_item.as_table_mut() {
                table
                    .decor_mut()
                    .set_prefix("\n# 视频质量过滤配置\n# 可以设置视频和音频的质量范围\n");
            }
        }

        // 为弹幕选项添加注释
        if let Some(danmaku_item) = doc.get_mut("danmaku_option") {
            if let Some(table) = danmaku_item.as_table_mut() {
                table
                    .decor_mut()
                    .set_prefix("\n# 弹幕样式配置\n# 用于设置下载弹幕的显示样式\n");
            }
        }
    }

    #[cfg(not(test))]
    fn load() -> Result<Self> {
        let config_path = CONFIG_DIR.join("config.toml");
        let config_content = std::fs::read_to_string(config_path)?;
        Ok(toml::from_str(&config_content)?)
    }

    #[cfg(test)]
    fn load() -> Result<Self> {
        // 在测试环境下，返回默认配置
        Ok(Self::default())
    }

    pub fn as_video_sources(&self) -> Vec<(Args<'_>, &PathBuf)> {
        let mut params = Vec::new();

        // 优先使用新的v2配置
        if !self.favorite_list_v2.is_empty() {
            self.favorite_list_v2
                .iter()
                .for_each(|config| params.push((Args::Favorite { fid: &config.fid }, &config.path)));
        } else {
            // 回退到旧配置
            self.favorite_list
                .iter()
                .for_each(|(fid, path)| params.push((Args::Favorite { fid }, path)));
        }

        if !self.collection_list_v2.is_empty() {
            // 对于新的v2配置，我们需要临时创建CollectionItem
            // 但由于生命周期问题，我们暂时回退到旧配置的处理方式
            // TODO: 需要重新设计Args结构来支持新的配置格式
        } else {
            // 回退到旧配置
            self.collection_list
                .iter()
                .for_each(|(collection_item, path)| params.push((Args::Collection { collection_item }, path)));
        }

        if !self.submission_list_v2.is_empty() {
            self.submission_list_v2.iter().for_each(|config| {
                params.push((
                    Args::Submission {
                        upper_id: &config.upper_id,
                    },
                    &config.path,
                ))
            });
        } else {
            // 回退到旧配置
            self.submission_list
                .iter()
                .for_each(|(upper_id, path)| params.push((Args::Submission { upper_id }, path)));
        }

        if self.watch_later.enabled {
            params.push((Args::WatchLater, &self.watch_later.path));
        }

        // 处理番剧配置
        self.bangumi.iter().for_each(|bangumi| {
            params.push((
                Args::Bangumi {
                    season_id: &bangumi.season_id,
                    media_id: &bangumi.media_id,
                    ep_id: &bangumi.ep_id,
                },
                &bangumi.path,
            ))
        });
        params
    }

    #[cfg(not(test))]
    pub fn check(&self) -> bool {
        let mut ok = true;
        let mut critical_error = false;

        let video_sources = self.as_video_sources();
        if video_sources.is_empty() && self.bangumi.is_empty() {
            ok = false;
            // 移除错误日志
            // error!("没有配置任何需要扫描的内容，程序空转没有意义");
        }
        for (args, path) in video_sources {
            if !path.is_absolute() {
                ok = false;
                error!("{:?} 保存的路径应为绝对路径，检测到: {}", args, path.display());
            }
        }
        // 检查番剧配置的路径
        for bangumi in &self.bangumi {
            if !bangumi.path.is_absolute() {
                ok = false;
                let season_id_display = match &bangumi.season_id {
                    Some(id) => id.clone(),
                    None => "未知".to_string(),
                };
                error!(
                    "番剧 {} 保存的路径应为绝对路径，检测到: {}",
                    season_id_display,
                    bangumi.path.display()
                );
            }
        }
        if !self.upper_path.is_absolute() {
            ok = false;
            error!("up 主头像保存的路径应为绝对路径");
        }
        if self.video_name.is_empty() {
            ok = false;
            error!("未设置 video_name 模板");
        }
        if self.page_name.is_empty() {
            ok = false;
            error!("未设置 page_name 模板");
        }
        if self.multi_page_name.is_empty() {
            ok = false;
            error!("未设置 multi_page_name 模板");
        }
        if self.bangumi_name.is_empty() {
            ok = false;
            error!("未设置 bangumi_name 模板");
        }
        if self.folder_structure.is_empty() {
            ok = false;
            error!("未设置 folder_structure 模板");
        }
        let credential = self.credential.load();
        match credential.as_deref() {
            Some(credential) => {
                if credential.sessdata.is_empty()
                    || credential.bili_jct.is_empty()
                    || credential.buvid3.is_empty()
                    || credential.dedeuserid.is_empty()
                    || credential.ac_time_value.is_empty()
                {
                    ok = false;
                    critical_error = true;
                    error!("请到配置文件添加哔哩哔哩账号的身份凭据 稍后重新运行");
                }
            }
            None => {
                ok = false;
                critical_error = true;
                error!("请到配置文件添加哔哩哔哩账号的身份凭据 稍后重新运行");
            }
        }
        if !(self.concurrent_limit.video > 0 && self.concurrent_limit.page > 0) {
            ok = false;
            error!("video 和 page 允许的并发数必须大于 0");
        }

        if critical_error {
            panic!(
                "位于 {} 的配置文件存在严重错误，请参考提示信息修复后继续运行",
                CONFIG_DIR.join("config.toml").display()
            );
        }

        ok
    }
}
