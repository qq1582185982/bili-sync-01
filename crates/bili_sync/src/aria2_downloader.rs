use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::bilibili::Client;
use crate::config::CONFIG_DIR;

/// 嵌入的aria2二进制文件 (编译时自动下载对应平台版本)
#[cfg(target_os = "windows")]
static ARIA2_BINARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/aria2c.exe"));

#[cfg(target_os = "linux")]
static ARIA2_BINARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/aria2c"));

#[cfg(any(target_os = "macos", target_os = "ios"))]
static ARIA2_BINARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/aria2c"));

/// 单个aria2进程实例
#[derive(Debug)]
pub struct Aria2Instance {
    process: tokio::process::Child,
    rpc_port: u16,
    rpc_secret: String,
    active_downloads: std::sync::atomic::AtomicUsize,
    last_used: std::sync::Arc<std::sync::Mutex<std::time::Instant>>,
}

impl Aria2Instance {
    pub fn new(process: tokio::process::Child, rpc_port: u16, rpc_secret: String) -> Self {
        Self {
            process,
            rpc_port,
            rpc_secret,
            active_downloads: std::sync::atomic::AtomicUsize::new(0),
            last_used: std::sync::Arc::new(std::sync::Mutex::new(std::time::Instant::now())),
        }
    }

    pub fn get_load(&self) -> usize {
        self.active_downloads.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn increment_load(&self) {
        self.active_downloads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut last_used) = self.last_used.lock() {
            *last_used = std::time::Instant::now();
        }
    }

    pub fn decrement_load(&self) {
        self.active_downloads.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_healthy(&mut self) -> bool {
        // 检查进程是否还在运行
        match self.process.try_wait() {
            Ok(Some(_)) => false, // 进程已退出
            Ok(None) => true,     // 进程仍在运行
            Err(_) => false,      // 检查失败
        }
    }
}

pub struct Aria2Downloader {
    client: Client,
    aria2_instances: Arc<Mutex<Vec<Aria2Instance>>>,
    aria2_binary_path: PathBuf,
    instance_count: usize,
    #[allow(dead_code)]
    next_instance_index: std::sync::atomic::AtomicUsize,
}

impl Aria2Downloader {
    /// 创建新的aria2下载器实例，支持多进程
    pub async fn new(client: Client) -> Result<Self> {
        // 启动前先清理所有旧的aria2进程
        Self::cleanup_all_aria2_processes().await;

        let aria2_binary_path = Self::extract_aria2_binary().await?;

        // 确定进程数量：根据系统资源动态计算
        let instance_count = Self::calculate_optimal_instance_count();
        info!("创建 {} 个aria2进程实例", instance_count);

        let mut downloader = Self {
            client,
            aria2_instances: Arc::new(Mutex::new(Vec::new())),
            aria2_binary_path,
            instance_count,
            next_instance_index: std::sync::atomic::AtomicUsize::new(0),
        };

        // 启动所有aria2进程实例
        downloader.start_all_instances().await?;
        Ok(downloader)
    }

    /// 计算最优的aria2进程数量
    fn calculate_optimal_instance_count() -> usize {
        let config = crate::config::reload_config();
        let total_threads = config.concurrent_limit.parallel_download.threads;

        // 智能计算：根据总线程数和系统负载动态调整，增加并发进程数
        let optimal_count = match total_threads {
            1..=4 => 1,                                                          // 少量线程用单进程
            5..=8 => 2,                                                          // 中等线程用双进程
            9..=16 => 4,                                                         // 较多线程用四进程 (充分利用16线程)
            17..=32 => 5,                                                        // 大量线程用五进程
            _ => std::cmp::min(8, (total_threads as f64 / 6.0).ceil() as usize), // 超大线程数动态计算，更多进程
        };

        info!(
            "智能分析 - 总线程数: {}, 计算出最优进程数: {}, 决策依据: {}",
            total_threads,
            optimal_count,
            match total_threads {
                1..=4 => "少量线程使用单进程",
                5..=8 => "中等线程使用双进程",
                9..=16 => "充分利用线程数，使用四进程",
                17..=32 => "大量线程使用五进程",
                _ => "超大线程数使用更多进程提升并发",
            }
        );
        optimal_count
    }

    /// 清理所有aria2进程 (Windows兼容)
    async fn cleanup_all_aria2_processes() {
        info!("清理所有旧的aria2进程...");

        #[cfg(target_os = "windows")]
        {
            // Windows: 使用taskkill强制终止所有aria2进程
            let output = tokio::process::Command::new("taskkill")
                .args(["/F", "/IM", "aria2c.exe"])
                .output()
                .await;

            match output {
                Ok(result) => {
                    if result.status.success() {
                        // Windows taskkill 输出使用系统默认编码，不直接解码以避免乱码
                        info!("Windows aria2进程清理完成");
                    } else {
                        debug!("Windows aria2进程清理出现问题，但进程可能已终止");
                    }
                }
                Err(e) => {
                    warn!("Windows aria2进程清理失败: {}", e);
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Linux: 使用pkill强制终止
            let output = tokio::process::Command::new("pkill")
                .args(["-9", "-f", "aria2c"])
                .output()
                .await;

            match output {
                Ok(result) => {
                    if result.status.success() {
                        info!("Linux aria2进程清理完成");
                    } else {
                        debug!("Linux aria2进程清理: 没有找到运行中的aria2进程");
                    }
                }
                Err(e) => {
                    debug!("Linux aria2进程清理失败: {}", e);
                }
            }
        }

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            // macOS: 使用pkill强制终止
            let output = tokio::process::Command::new("pkill")
                .args(["-9", "-f", "aria2c"])
                .output()
                .await;

            match output {
                Ok(result) => {
                    if result.status.success() {
                        info!("macOS aria2进程清理完成");
                    } else {
                        debug!("macOS aria2进程清理: 没有找到运行中的aria2进程");
                    }
                }
                Err(e) => {
                    debug!("macOS aria2进程清理失败: {}", e);
                }
            }
        }

        // 等待进程完全终止
        tokio::time::sleep(Duration::from_millis(2000)).await;
    }

    /// 计算单个实例的最优线程数
    fn calculate_threads_per_instance(total_threads: usize, instance_count: usize) -> usize {
        let base_threads = total_threads / instance_count;
        let remainder = total_threads % instance_count;

        // 基础分配 + 考虑余数
        let threads_per_instance = if remainder > 0 { base_threads + 1 } else { base_threads };

        // 智能限制：根据线程数量动态调整上限
        let max_threads_per_instance = match total_threads {
            1..=16 => total_threads, // 小量线程不限制
            17..=32 => 16,           // 中等线程限制到16
            33..=64 => 20,           // 较多线程限制到20
            65..=128 => 24,          // 大量线程限制到24
            _ => 32,                 // 超大量线程限制到32
        };

        std::cmp::min(threads_per_instance, max_threads_per_instance)
    }

    /// 根据文件大小智能调整线程数
    fn calculate_smart_threads_for_file(file_size_mb: u64, base_threads: usize, total_threads: usize) -> usize {
        let smart_threads = match file_size_mb {
            0..=2 => 1,                                    // 极小文件单线程足够
            3..=10 => std::cmp::min(base_threads, 2),      // 小文件用少量线程
            11..=50 => std::cmp::min(base_threads, 4),     // 中等文件适中线程
            51..=200 => std::cmp::min(base_threads, 8),    // 大文件较多线程
            201..=1000 => std::cmp::min(base_threads, 12), // 很大文件更多线程
            _ => {
                // 超大文件(>1GB): 可以使用更多线程，突破单实例限制
                let max_for_large_file = std::cmp::min(total_threads * 3 / 4, 16);
                std::cmp::max(base_threads, std::cmp::min(max_for_large_file, total_threads))
            }
        };

        std::cmp::max(smart_threads, 1) // 至少1个线程
    }

    /// 尝试获取文件大小（用于智能线程调整）
    async fn try_get_file_size(&self, url: &str) -> Option<u64> {
        match self
            .client
            .head(url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
            )
            .header("Referer", "https://www.bilibili.com")
            .send()
            .await
        {
            Ok(response) => response
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok()),
            Err(_) => None,
        }
    }

    /// 启动所有aria2进程实例
    async fn start_all_instances(&mut self) -> Result<()> {
        let mut instances = Vec::new();

        for i in 0..self.instance_count {
            let rpc_port = Self::find_available_port().await?;
            let rpc_secret = Self::generate_secret();

            info!("启动第 {} 个aria2进程，端口: {}", i + 1, rpc_port);

            let process = self.start_single_instance(rpc_port, &rpc_secret).await?;
            let instance = Aria2Instance::new(process, rpc_port, rpc_secret);

            // 验证连接
            if let Err(e) = self.test_instance_connection(rpc_port, &instance.rpc_secret).await {
                warn!("aria2实例 {} 连接测试失败: {:#}", i + 1, e);
                continue;
            }

            instances.push(instance);
            info!("aria2实例 {} 启动成功", i + 1);
        }

        if instances.is_empty() {
            bail!("没有成功启动任何aria2实例");
        }

        *self.aria2_instances.lock().await = instances;
        info!("成功启动 {} 个aria2实例", self.aria2_instances.lock().await.len());

        Ok(())
    }

    /// 提取嵌入的aria2二进制文件到临时目录，失败时回退到系统aria2
    async fn extract_aria2_binary() -> Result<PathBuf> {
        // 使用配置文件夹存储aria2二进制文件，而不是临时目录
        let binary_name = if cfg!(target_os = "windows") {
            "aria2c.exe"
        } else {
            "aria2c"
        };
        let binary_path = CONFIG_DIR.join(binary_name);

        // 确保配置目录存在
        if let Err(e) = tokio::fs::create_dir_all(&*CONFIG_DIR).await {
            warn!("创建配置目录失败: {}, 将使用临时目录", e);
            let temp_dir = std::env::temp_dir();
            return Self::extract_aria2_binary_to_temp(temp_dir, binary_name).await;
        }

        // 如果文件已存在且可执行，直接返回
        if binary_path.exists() {
            // 验证文件是否为有效的aria2可执行文件
            if Self::is_valid_aria2_binary(&binary_path).await {
                return Ok(binary_path);
            } else {
                // 如果是无效的文件（如占位文件），删除它
                let _ = tokio::fs::remove_file(&binary_path).await;
            }
        }

        // 尝试写入嵌入的二进制文件
        debug!("尝试提取aria2二进制文件到配置目录: {}", binary_path.display());
        match tokio::fs::write(&binary_path, ARIA2_BINARY).await {
            Ok(_) => {
                debug!("aria2二进制文件写入配置目录成功，大小: {} bytes", ARIA2_BINARY.len());

                // 在Unix系统上设置执行权限
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(metadata) = tokio::fs::metadata(&binary_path).await {
                        let mut perms = metadata.permissions();
                        perms.set_mode(0o755);
                        let _ = tokio::fs::set_permissions(&binary_path, perms).await;
                        debug!("已设置aria2二进制文件执行权限");
                    }
                }

                // 验证提取的文件是否有效
                debug!("开始验证提取到配置目录的aria2二进制文件...");
                if Self::is_valid_aria2_binary(&binary_path).await {
                    info!("aria2二进制文件已提取到配置目录: {}", binary_path.display());
                    return Ok(binary_path);
                } else {
                    warn!("配置目录中的aria2二进制文件无效，尝试使用系统aria2");
                    let _ = tokio::fs::remove_file(&binary_path).await;
                }
            }
            Err(e) => {
                warn!("提取aria2二进制文件到配置目录失败: {}, 尝试使用系统aria2", e);
            }
        }

        // 回退到系统安装的aria2
        Self::find_system_aria2().await
    }

    /// 备用方案：提取到临时目录
    async fn extract_aria2_binary_to_temp(temp_dir: PathBuf, binary_name: &str) -> Result<PathBuf> {
        let binary_path = temp_dir.join(format!("bili-sync-{}", binary_name));

        debug!("尝试提取aria2二进制文件到临时目录: {}", binary_path.display());

        // 如果文件已存在且可执行，直接返回
        if binary_path.exists() {
            if Self::is_valid_aria2_binary(&binary_path).await {
                return Ok(binary_path);
            } else {
                let _ = tokio::fs::remove_file(&binary_path).await;
            }
        }

        // 尝试写入嵌入的二进制文件
        match tokio::fs::write(&binary_path, ARIA2_BINARY).await {
            Ok(_) => {
                debug!("aria2二进制文件写入临时目录成功，大小: {} bytes", ARIA2_BINARY.len());

                // 在Unix系统上设置执行权限
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(metadata) = tokio::fs::metadata(&binary_path).await {
                        let mut perms = metadata.permissions();
                        perms.set_mode(0o755);
                        let _ = tokio::fs::set_permissions(&binary_path, perms).await;
                    }
                }

                // 验证提取的文件是否有效
                if Self::is_valid_aria2_binary(&binary_path).await {
                    info!("aria2二进制文件已提取到临时目录: {}", binary_path.display());
                    return Ok(binary_path);
                } else {
                    warn!("临时目录中的aria2二进制文件无效");
                    let _ = tokio::fs::remove_file(&binary_path).await;
                }
            }
            Err(e) => {
                warn!("提取aria2二进制文件到临时目录失败: {}", e);
            }
        }

        // 最终回退到系统安装的aria2
        Self::find_system_aria2().await
    }

    /// 验证aria2二进制文件是否有效
    async fn is_valid_aria2_binary(path: &Path) -> bool {
        if !path.exists() {
            warn!("aria2二进制文件不存在: {}", path.display());
            return false;
        }

        // 尝试执行 aria2c --version 来验证
        match tokio::process::Command::new(path).arg("--version").output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if output.status.success() && stdout.contains("aria2") {
                    debug!("aria2二进制文件验证成功: {}", path.display());
                    true
                } else {
                    warn!(
                        "aria2二进制文件验证失败: {}，退出码: {:?}，stdout: {}，stderr: {}",
                        path.display(),
                        output.status.code(),
                        stdout.trim(),
                        stderr.trim()
                    );
                    false
                }
            }
            Err(e) => {
                warn!("无法执行aria2二进制文件 {}: {}", path.display(), e);
                false
            }
        }
    }

    /// 查找系统安装的aria2
    async fn find_system_aria2() -> Result<PathBuf> {
        let _binary_name = if cfg!(target_os = "windows") {
            "aria2c.exe"
        } else {
            "aria2c"
        };

        // 尝试使用which命令查找
        match tokio::process::Command::new("which").arg("aria2c").output().await {
            Ok(output) if output.status.success() => {
                let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let system_path = PathBuf::from(path_str);

                if Self::is_valid_aria2_binary(&system_path).await {
                    info!("使用系统安装的aria2: {}", system_path.display());
                    return Ok(system_path);
                }
            }
            _ => {}
        }

        // 在Windows上尝试where命令
        #[cfg(target_os = "windows")]
        {
            match tokio::process::Command::new("where").arg("aria2c").output().await {
                Ok(output) if output.status.success() => {
                    let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let system_path = PathBuf::from(path_str);

                    if Self::is_valid_aria2_binary(&system_path).await {
                        info!("使用系统安装的aria2: {}", system_path.display());
                        return Ok(system_path);
                    }
                }
                _ => {}
            }
        }

        // 尝试常见的安装路径
        let common_paths = if cfg!(target_os = "windows") {
            vec![
                PathBuf::from("C:\\Program Files\\aria2\\aria2c.exe"),
                PathBuf::from("C:\\Program Files (x86)\\aria2\\aria2c.exe"),
            ]
        } else {
            vec![
                PathBuf::from("/usr/bin/aria2c"),
                PathBuf::from("/usr/local/bin/aria2c"),
                PathBuf::from("/opt/homebrew/bin/aria2c"),
            ]
        };

        for path in common_paths {
            if Self::is_valid_aria2_binary(&path).await {
                info!("使用系统安装的aria2: {}", path.display());
                return Ok(path);
            }
        }

        bail!("未找到可用的aria2二进制文件，请确保系统已安装aria2")
    }

    /// 查找可用的端口
    async fn find_available_port() -> Result<u16> {
        use std::net::TcpListener;

        // 尝试绑定到随机端口
        let listener = TcpListener::bind("127.0.0.1:0").context("Failed to bind to random port")?;
        let port = listener.local_addr()?.port();
        drop(listener);

        Ok(port)
    }

    /// 生成随机密钥
    fn generate_secret() -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut hasher = DefaultHasher::new();
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .hash(&mut hasher);

        format!("bili-sync-{:x}", hasher.finish())
    }

    /// 启动单个aria2实例
    async fn start_single_instance(&self, rpc_port: u16, rpc_secret: &str) -> Result<tokio::process::Child> {
        let current_config = crate::config::reload_config();
        let total_threads = current_config.concurrent_limit.parallel_download.threads;

        // 智能计算当前实例应该使用的线程数
        let threads = Self::calculate_threads_per_instance(total_threads, self.instance_count);

        info!(
            "启动aria2实例，分配线程数: {} (总线程: {}, 实例数: {})",
            threads, total_threads, self.instance_count
        );

        let mut args = vec![
            "--enable-rpc".to_string(),
            format!("--rpc-listen-port={}", rpc_port),
            "--rpc-allow-origin-all".to_string(),
            format!("--rpc-secret={}", rpc_secret),
            "--continue=true".to_string(),
            format!("--max-connection-per-server={}", threads),
            "--min-split-size=1M".to_string(),
            format!("--split={}", threads),
            "--max-concurrent-downloads=6".to_string(), // 每个实例最多6个文件
            "--disable-ipv6=true".to_string(),
            "--summary-interval=0".to_string(),
            "--quiet=true".to_string(),
        ];

        // 添加SSL/TLS相关配置
        if cfg!(target_os = "linux") {
            let ca_paths = [
                "/etc/ssl/certs/ca-certificates.crt",
                "/etc/pki/tls/certs/ca-bundle.crt",
                "/etc/ssl/ca-bundle.pem",
                "/etc/ssl/cert.pem",
            ];

            let mut ca_found = false;
            for ca_path in &ca_paths {
                if std::path::Path::new(ca_path).exists() {
                    args.push(format!("--ca-certificate={}", ca_path));
                    ca_found = true;
                    break;
                }
            }

            if !ca_found {
                args.push("--check-certificate=false".to_string());
            }
        } else {
            args.push("--check-certificate=false".to_string());
        }

        let child = tokio::process::Command::new(&self.aria2_binary_path)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Failed to start aria2 daemon")?;

        Ok(child)
    }

    /// 测试单个实例的连接
    async fn test_instance_connection(&self, rpc_port: u16, rpc_secret: &str) -> Result<()> {
        let url = format!("http://127.0.0.1:{}/jsonrpc", rpc_port);
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "aria2.getVersion",
            "id": "test",
            "params": [format!("token:{}", rpc_secret)]
        });

        // 等待aria2启动
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(500)).await;

            if let Ok(response) = self.client.post(&url).json(&payload).send().await {
                if response.status().is_success() {
                    return Ok(());
                }
            }
        }

        bail!("aria2 instance connection test failed after retries")
    }

    /// 选择最佳aria2实例（负载均衡）
    async fn select_best_instance(&self) -> Result<(usize, u16, String)> {
        let instances = self.aria2_instances.lock().await;

        if instances.is_empty() {
            bail!("没有可用的aria2实例");
        }

        // 找到负载最低的实例
        let (best_index, _) = instances
            .iter()
            .enumerate()
            .min_by_key(|(_, instance)| instance.get_load())
            .ok_or_else(|| anyhow::anyhow!("无法找到可用实例"))?;

        let instance = &instances[best_index];
        Ok((best_index, instance.rpc_port, instance.rpc_secret.clone()))
    }

    /// 使用aria2下载文件，支持多个URL备选和多进程
    pub async fn fetch_with_aria2_fallback(&self, urls: &[&str], path: &Path) -> Result<()> {
        if urls.is_empty() {
            bail!("No URLs provided");
        }

        // 确保目标目录存在
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file_name = path.file_name().and_then(|n| n.to_str()).context("Invalid file name")?;
        let dir = path
            .parent()
            .and_then(|p| p.to_str())
            .context("Invalid directory path")?;

        // 选择最佳的aria2实例
        let (instance_index, rpc_port, rpc_secret) = self.select_best_instance().await?;

        info!(
            "使用aria2实例 {} (端口: {}) 下载: {}",
            instance_index + 1,
            rpc_port,
            file_name
        );

        // 增加该实例的负载计数
        {
            let instances = self.aria2_instances.lock().await;
            if let Some(instance) = instances.get(instance_index) {
                instance.increment_load();
            }
        }

        // 构建aria2 RPC请求
        let gid = self
            .add_download_task_to_instance(urls, dir, file_name, rpc_port, &rpc_secret)
            .await?;

        // 等待下载完成
        let result = self
            .wait_for_download_on_instance(&gid, rpc_port, &rpc_secret, instance_index)
            .await;

        // 减少该实例的负载计数
        {
            let instances = self.aria2_instances.lock().await;
            if let Some(instance) = instances.get(instance_index) {
                instance.decrement_load();
            }
        }

        // 检查下载结果
        result?;

        // 验证文件是否存在
        if !path.exists() {
            bail!("Download completed but file not found: {}", path.display());
        }

        Ok(())
    }

    /// 添加下载任务到指定实例
    async fn add_download_task_to_instance(
        &self,
        urls: &[&str],
        dir: &str,
        file_name: &str,
        rpc_port: u16,
        rpc_secret: &str,
    ) -> Result<String> {
        let url = format!("http://127.0.0.1:{}/jsonrpc", rpc_port);

        // 智能计算当前实例的线程数
        let current_config = crate::config::reload_config();
        let total_threads = current_config.concurrent_limit.parallel_download.threads;
        let base_threads = Self::calculate_threads_per_instance(total_threads, self.instance_count);

        // 尝试获取文件大小，并根据大小智能调整线程数
        let threads = if let Some(file_size_bytes) = self.try_get_file_size(urls[0]).await {
            let file_size_mb = file_size_bytes / 1_048_576; // 转换为MB
            let smart_threads = Self::calculate_smart_threads_for_file(file_size_mb, base_threads, total_threads);
            info!(
                "文件大小: {} MB，智能调整线程数: {} (基础: {}, 总线程: {})",
                file_size_mb, smart_threads, base_threads, total_threads
            );
            smart_threads
        } else {
            debug!("无法获取文件大小，使用基础线程数: {}", base_threads);
            base_threads
        };

        // 构建基础选项
        let mut options = serde_json::json!({
            "dir": dir,
            "out": file_name,
            "continue": "true",
            "max-connection-per-server": threads.to_string(),
            "split": threads.to_string(),
            "min-split-size": "1M",
            "header": [
                "User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36",
                "Referer: https://www.bilibili.com"
            ]
        });

        // 添加SSL/TLS相关配置
        if cfg!(target_os = "linux") {
            // 对于Linux系统，尝试使用系统CA证书
            let ca_paths = [
                "/etc/ssl/certs/ca-certificates.crt", // Debian/Ubuntu
                "/etc/pki/tls/certs/ca-bundle.crt",   // RHEL/CentOS
                "/etc/ssl/ca-bundle.pem",             // openSUSE
                "/etc/ssl/cert.pem",                  // Alpine
            ];

            let mut ca_found = false;
            for ca_path in &ca_paths {
                if std::path::Path::new(ca_path).exists() {
                    options["ca-certificate"] = serde_json::Value::String(ca_path.to_string());
                    ca_found = true;
                    break;
                }
            }

            if !ca_found {
                options["check-certificate"] = serde_json::Value::String("false".to_string());
            }
        } else {
            options["check-certificate"] = serde_json::Value::String("false".to_string());
        }

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "aria2.addUri",
            "id": "add_download",
            "params": [
                format!("token:{}", rpc_secret),
                urls,
                options
            ]
        });

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .context("Failed to add download task")?;

        let json: serde_json::Value = response.json().await?;

        if let Some(error) = json.get("error") {
            bail!("aria2 error: {}", error);
        }

        let gid = json["result"]
            .as_str()
            .context("Invalid response from aria2")?
            .to_string();

        info!("开始aria2下载: {} (线程数: {})", file_name, threads);
        debug!("添加下载任务成功，GID: {}", gid);
        Ok(gid)
    }

    /// 在指定实例上等待下载完成
    async fn wait_for_download_on_instance(
        &self,
        gid: &str,
        rpc_port: u16,
        rpc_secret: &str,
        _instance_index: usize,
    ) -> Result<()> {
        let url = format!("http://127.0.0.1:{}/jsonrpc", rpc_port);

        loop {
            let payload = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "aria2.tellStatus",
                "id": "check_status",
                "params": [
                    format!("token:{}", rpc_secret),
                    gid
                ]
            });

            let response = self
                .client
                .post(&url)
                .json(&payload)
                .send()
                .await
                .context("Failed to check download status")?;

            let json: serde_json::Value = response.json().await?;

            if let Some(error) = json.get("error") {
                bail!("aria2 status check error: {}", error);
            }

            let result = &json["result"];
            let status = result["status"].as_str().unwrap_or("unknown");

            match status {
                "complete" => {
                    // 获取下载统计信息
                    let total_length = result["totalLength"].as_str().unwrap_or("0");
                    let completed_length = result["completedLength"].as_str().unwrap_or("0");

                    if let (Ok(total), Ok(completed)) = (total_length.parse::<u64>(), completed_length.parse::<u64>()) {
                        let total_mb = total as f64 / 1_048_576.0;
                        let completed_mb = completed as f64 / 1_048_576.0;
                        debug!(
                            "aria2下载完成，GID: {}，总大小: {:.2} MB，已完成: {:.2} MB",
                            gid, total_mb, completed_mb
                        );
                    } else {
                        debug!("aria2下载完成，GID: {}", gid);
                    }
                    return Ok(());
                }
                "error" => {
                    let error_msg = result["errorMessage"].as_str().unwrap_or("Unknown error");
                    bail!("Download failed: {}", error_msg);
                }
                "removed" => {
                    bail!("Download was removed");
                }
                "active" | "waiting" | "paused" => {
                    // 继续等待
                    sleep(Duration::from_millis(1000)).await;
                }
                _ => {
                    warn!("Unknown download status: {}", status);
                    sleep(Duration::from_millis(1000)).await;
                }
            }
        }
    }

    /// 智能下载：对于多进程aria2，直接使用aria2下载
    pub async fn smart_fetch(&self, url: &str, path: &Path) -> Result<()> {
        // 对于多进程aria2，直接使用aria2下载
        self.fetch_with_aria2_fallback(&[url], path).await
    }

    /// 合并视频和音频文件
    pub async fn merge(&self, video_path: &Path, audio_path: &Path, output_path: &Path) -> Result<()> {
        use crate::downloader::Downloader;

        // 使用内置的合并功能
        let temp_downloader = Downloader::new(self.client.clone());
        temp_downloader.merge(video_path, audio_path, output_path).await
    }

    /// 重新启动所有aria2进程
    pub async fn restart(&mut self) -> Result<()> {
        info!("重新启动所有aria2实例...");

        // 关闭现有实例
        self.shutdown().await?;

        // 重新启动实例
        self.start_all_instances().await?;

        info!("所有aria2实例已重新启动");
        Ok(())
    }

    /// 优雅关闭所有aria2进程
    pub async fn shutdown(&self) -> Result<()> {
        info!("正在关闭所有aria2实例...");

        let mut instances = self.aria2_instances.lock().await;
        let mut shutdown_futures = Vec::new();

        for (i, instance) in instances.iter_mut().enumerate() {
            let rpc_port = instance.rpc_port;
            let rpc_secret = instance.rpc_secret.clone();
            let client = self.client.clone();

            // 尝试优雅关闭aria2实例
            let shutdown_future = async move {
                let url = format!("http://127.0.0.1:{}/jsonrpc", rpc_port);
                let payload = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "aria2.shutdown",
                    "id": "shutdown",
                    "params": [format!("token:{}", rpc_secret)]
                });

                let _ = client.post(&url).json(&payload).send().await;
                tokio::time::sleep(Duration::from_millis(1000)).await;
            };

            shutdown_futures.push(shutdown_future);

            // 强制终止进程 - Windows兼容性改进
            if let Err(e) = instance.process.kill().await {
                warn!("终止aria2实例 {} 失败: {}", i + 1, e);

                // 如果普通kill失败，尝试使用系统命令强制终止
                #[cfg(target_os = "windows")]
                {
                    if let Some(pid) = instance.process.id() {
                        let _ = tokio::process::Command::new("taskkill")
                            .args(["/F", "/PID", &pid.to_string()])
                            .output()
                            .await;
                        info!("已强制终止Windows aria2进程 PID: {}", pid);
                    }
                }

                #[cfg(target_os = "linux")]
                {
                    if let Some(pid) = instance.process.id() {
                        let _ = tokio::process::Command::new("kill")
                            .args(["-9", &pid.to_string()])
                            .output()
                            .await;
                        info!("已强制终止Linux aria2进程 PID: {}", pid);
                    }
                }

                #[cfg(any(target_os = "macos", target_os = "ios"))]
                {
                    if let Some(pid) = instance.process.id() {
                        let _ = tokio::process::Command::new("kill")
                            .args(["-9", &pid.to_string()])
                            .output()
                            .await;
                        info!("已强制终止macOS aria2进程 PID: {}", pid);
                    }
                }
            } else {
                debug!("aria2实例 {} 已终止", i + 1);
            }
        }

        // 等待所有优雅关闭完成
        futures::future::join_all(shutdown_futures).await;

        instances.clear();

        // 最后再次确保所有aria2进程都被清理
        tokio::time::sleep(Duration::from_millis(1000)).await;
        Self::cleanup_all_aria2_processes().await;

        info!("所有aria2实例已关闭");
        Ok(())
    }

    /// 健康检查：移除不健康的实例并重新启动
    #[allow(dead_code)]
    pub async fn health_check(&mut self) -> Result<()> {
        let mut instances = self.aria2_instances.lock().await;
        let mut unhealthy_indices = Vec::new();

        // 检查每个实例的健康状态
        for (i, instance) in instances.iter_mut().enumerate() {
            if !instance.is_healthy() {
                warn!("aria2实例 {} 不健康，准备重启", i + 1);
                unhealthy_indices.push(i);
            }
        }

        // 移除不健康的实例
        for &index in unhealthy_indices.iter().rev() {
            instances.remove(index);
        }

        let unhealthy_count = unhealthy_indices.len();
        drop(instances);

        // 重新启动不健康的实例
        if unhealthy_count > 0 {
            info!("重新启动 {} 个不健康的aria2实例", unhealthy_count);

            for _ in 0..unhealthy_count {
                let rpc_port = Self::find_available_port().await?;
                let rpc_secret = Self::generate_secret();

                match self.start_single_instance(rpc_port, &rpc_secret).await {
                    Ok(process) => {
                        let instance = Aria2Instance::new(process, rpc_port, rpc_secret.clone());

                        // 验证连接
                        if self.test_instance_connection(rpc_port, &rpc_secret).await.is_ok() {
                            self.aria2_instances.lock().await.push(instance);
                            info!("成功重启aria2实例，端口: {}", rpc_port);
                        } else {
                            warn!("重启的aria2实例连接测试失败，端口: {}", rpc_port);
                        }
                    }
                    Err(e) => {
                        warn!("重启aria2实例失败: {:#}", e);
                    }
                }
            }
        }

        let current_count = self.aria2_instances.lock().await.len();
        if current_count == 0 {
            bail!("所有aria2实例都不可用");
        }

        info!("健康检查完成，当前可用实例数: {}", current_count);
        Ok(())
    }

    /// 获取所有实例的状态信息
    #[allow(dead_code)]
    pub async fn get_instances_status(&self) -> Vec<(u16, String, usize, bool)> {
        let mut instances = self.aria2_instances.lock().await;
        let mut status_list = Vec::new();

        for instance in instances.iter_mut() {
            let port = instance.rpc_port;
            let secret = instance.rpc_secret.clone();
            let load = instance.get_load();
            let healthy = instance.is_healthy();

            status_list.push((port, secret, load, healthy));
        }

        status_list
    }
}

impl Drop for Aria2Downloader {
    fn drop(&mut self) {
        // 在析构时尝试清理临时文件
        if self.aria2_binary_path.exists() {
            let _ = std::fs::remove_file(&self.aria2_binary_path);
        }
    }
}
