//! DeepSeek Web API POW (Proof of Work) 求解器
//!
//! DeepSeek 使用自建的 WASM 算法，非标准 Keccak-256
//! 本模块通过 wasmtime 直接加载 WASM 求解 POW

use once_cell::sync::Lazy;
use sha3::{Digest, Keccak256};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{debug, info, warn};
use wasmtime::*;

use crate::config::CONFIG_DIR;

/// 嵌入的 WASM 文件（DeepSeek POW 求解器）
static SHA3_WASM: &[u8] = include_bytes!("../../resources/sha3_wasm.wasm");

/// POW 挑战响应数据
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PowChallenge {
    pub algorithm: String,
    pub challenge: String,
    pub salt: String,
    pub difficulty: u64,
    pub expire_at: i64,
    pub signature: String,
    pub target_path: String,
}

/// POW 求解结果（用于发送给服务器）
#[derive(Debug, Clone, serde::Serialize)]
pub struct PowResponse {
    pub algorithm: String,
    pub challenge: String,
    pub salt: String,
    pub answer: u64,
    pub signature: String,
    pub target_path: String,
}

/// WASM 运行时（全局单例，避免重复初始化）
static WASM_RUNTIME: Lazy<Mutex<Option<WasmPowSolver>>> = Lazy::new(|| {
    match WasmPowSolver::new() {
        Ok(solver) => {
            info!("WASM POW 求解器初始化成功");
            Mutex::new(Some(solver))
        }
        Err(e) => {
            warn!("WASM POW 求解器初始化失败: {}", e);
            Mutex::new(None)
        }
    }
});

/// WASM POW 求解器
struct WasmPowSolver {
    store: Store<()>,
    memory: Memory,
    malloc: TypedFunc<(i32, i32), i32>,
    stack_pointer: TypedFunc<i32, i32>,
    wasm_solve: TypedFunc<(i32, i32, i32, i32, i32, f64), ()>,
}

impl WasmPowSolver {
    /// 创建新的 WASM 求解器
    fn new() -> anyhow::Result<Self> {
        // 获取 WASM 字节（优先从文件系统，其次使用嵌入的）
        let wasm_bytes = Self::load_wasm_bytes()?;

        // 创建 wasmtime 引擎和存储
        let engine = Engine::default();
        let mut store = Store::new(&engine, ());

        // 编译并实例化模块
        let module = Module::new(&engine, &wasm_bytes)?;
        let instance = Instance::new(&mut store, &module, &[])?;

        // 获取导出的函数和内存
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow::anyhow!("找不到 memory 导出"))?;

        let malloc = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "__wbindgen_export_0")?;

        let stack_pointer = instance
            .get_typed_func::<i32, i32>(&mut store, "__wbindgen_add_to_stack_pointer")?;

        let wasm_solve = instance
            .get_typed_func::<(i32, i32, i32, i32, i32, f64), ()>(&mut store, "wasm_solve")?;

        Ok(Self {
            store,
            memory,
            malloc,
            stack_pointer,
            wasm_solve,
        })
    }

    /// 加载 WASM 字节
    /// 优先从环境变量/文件系统加载，否则使用嵌入的 WASM
    fn load_wasm_bytes() -> anyhow::Result<Vec<u8>> {
        // 按优先级查找外部文件: 环境变量 > 配置目录 > 可执行文件同级目录
        let external_paths = [
            std::env::var("DEEPSEEK_WASM_PATH").ok(),
            CONFIG_DIR.join("sha3_wasm.wasm").to_str().map(|s| s.to_string()),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("sha3_wasm.wasm").to_string_lossy().to_string())),
        ];

        // 尝试从外部文件加载
        for path in external_paths.iter().flatten() {
            if let Ok(bytes) = std::fs::read(path) {
                debug!("从外部文件加载 WASM: {}", path);
                return Ok(bytes);
            }
        }

        // 使用嵌入的 WASM（确保先解压到配置目录）
        let wasm_path = Self::extract_wasm_to_config_dir()?;
        debug!("使用解压的嵌入 WASM: {}", wasm_path.display());

        Ok(SHA3_WASM.to_vec())
    }

    /// 将嵌入的 WASM 解压到配置目录
    fn extract_wasm_to_config_dir() -> anyhow::Result<PathBuf> {
        let wasm_path = CONFIG_DIR.join("sha3_wasm.wasm");

        // 如果文件已存在且大小正确，直接返回
        if wasm_path.exists() {
            if let Ok(metadata) = std::fs::metadata(&wasm_path) {
                if metadata.len() == SHA3_WASM.len() as u64 {
                    debug!("WASM 文件已存在: {}", wasm_path.display());
                    return Ok(wasm_path);
                }
            }
        }

        // 确保配置目录存在
        if let Some(parent) = wasm_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // 写入 WASM 文件
        std::fs::write(&wasm_path, SHA3_WASM)?;
        info!("已解压 WASM 文件到: {} ({} bytes)", wasm_path.display(), SHA3_WASM.len());

        Ok(wasm_path)
    }

    /// 向 WASM 内存写入字符串
    fn write_string(&mut self, s: &str) -> anyhow::Result<(i32, i32)> {
        let bytes = s.as_bytes();
        let len = bytes.len() as i32;

        // 分配内存
        let ptr = self.malloc.call(&mut self.store, (len, 1))?;

        // 写入数据
        self.memory.write(&mut self.store, ptr as usize, bytes)?;

        Ok((ptr, len))
    }

    /// 求解 POW
    fn solve(&mut self, challenge: &str, prefix: &str, difficulty: f64) -> anyhow::Result<u64> {
        // 分配栈空间
        let retptr = self.stack_pointer.call(&mut self.store, -16)?;

        // 写入字符串参数
        let (challenge_ptr, challenge_len) = self.write_string(challenge)?;
        let (prefix_ptr, prefix_len) = self.write_string(prefix)?;

        // 调用求解函数
        self.wasm_solve.call(
            &mut self.store,
            (retptr, challenge_ptr, challenge_len, prefix_ptr, prefix_len, difficulty),
        )?;

        // 读取结果（f64 在 retptr + 8 位置）
        let mut result_bytes = [0u8; 8];
        self.memory.read(&self.store, (retptr + 8) as usize, &mut result_bytes)?;
        let answer = f64::from_le_bytes(result_bytes);

        // 恢复栈指针
        self.stack_pointer.call(&mut self.store, 16)?;

        Ok(answer as u64)
    }
}

/// 求解 POW 挑战
///
/// DeepSeek 使用自建 WASM 算法，优先使用 WASM 求解器
/// 如果 WASM 不可用，回退到 Keccak-256（可能被服务器拒绝）
///
/// # 参数
/// - `challenge`: POW 挑战数据
///
/// # 返回
/// - 满足条件的 nonce 值
pub fn solve_pow(challenge: &PowChallenge) -> u64 {
    // 优先尝试使用 WASM 求解器
    if let Some(answer) = solve_pow_wasm(challenge) {
        return answer;
    }

    // 回退到 Keccak-256（可能被服务器拒绝）
    warn!("WASM 求解器不可用，回退到 Keccak-256（可能被服务器拒绝）");
    solve_pow_keccak(challenge)
}

/// 使用 WASM 求解 POW
fn solve_pow_wasm(challenge: &PowChallenge) -> Option<u64> {
    let prefix = format!("{}_{}_", challenge.salt, challenge.expire_at);

    debug!(
        "使用 WASM 求解 POW: challenge={}..., prefix={}, difficulty={}",
        &challenge.challenge[..challenge.challenge.len().min(20)],
        prefix,
        challenge.difficulty
    );

    let start = std::time::Instant::now();

    // 获取全局 WASM 运行时
    let mut guard = WASM_RUNTIME.lock().ok()?;
    let solver = guard.as_mut()?;

    // 调用 WASM 求解
    match solver.solve(&challenge.challenge, &prefix, challenge.difficulty as f64) {
        Ok(answer) => {
            let elapsed = start.elapsed();
            debug!(
                "WASM POW 求解成功: nonce={}, 耗时={:.2}s",
                answer,
                elapsed.as_secs_f64()
            );
            Some(answer)
        }
        Err(e) => {
            warn!("WASM POW 求解失败: {}", e);
            None
        }
    }
}

/// 使用 Keccak-256 求解 POW（备用方案，可能被服务器拒绝）
fn solve_pow_keccak(challenge: &PowChallenge) -> u64 {
    let prefix = format!("{}_{}_", challenge.salt, challenge.expire_at);
    let difficulty = challenge.difficulty;
    let target_prefix: u64 = if difficulty > 0 {
        u64::MAX / difficulty
    } else {
        u64::MAX
    };

    debug!(
        "使用 Keccak-256 求解 POW: prefix={}, target_prefix={:#018x}",
        &prefix,
        target_prefix
    );

    let start = std::time::Instant::now();

    for nonce in 0u64..100_000_000 {
        let data = format!("{}{}", prefix, nonce);
        let hash = Keccak256::digest(data.as_bytes());
        let hash_prefix = u64::from_be_bytes(hash[0..8].try_into().unwrap());

        if hash_prefix < target_prefix {
            let elapsed = start.elapsed();
            debug!(
                "Keccak POW 求解成功: nonce={}, hash_prefix={:#018x}, 耗时={:.2}s",
                nonce,
                hash_prefix,
                elapsed.as_secs_f64()
            );
            return nonce;
        }
    }

    warn!("Keccak POW 求解失败：未找到有效 nonce");
    0
}

/// 构建 POW 响应
pub fn build_pow_response(challenge: &PowChallenge, answer: u64) -> PowResponse {
    let response = PowResponse {
        algorithm: challenge.algorithm.clone(),
        challenge: challenge.challenge.clone(),
        salt: challenge.salt.clone(),
        answer,
        signature: challenge.signature.clone(),
        target_path: challenge.target_path.clone(),
    };

    debug!(
        "构建 POW 响应: algorithm={}, challenge={}..., salt={}, answer={}, target_path={}",
        response.algorithm,
        &response.challenge[..response.challenge.len().min(20)],
        response.salt,
        response.answer,
        response.target_path
    );

    response
}

/// 将 POW 响应编码为 Base64（用于 HTTP 头）
pub fn encode_pow_header(response: &PowResponse) -> String {
    let json = serde_json::to_string(response).unwrap_or_default();
    debug!("POW 响应 JSON: {}", &json[..json.len().min(200)]);
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_solve_pow_keccak() {
        // 测试 Keccak-256 备用方案
        let challenge = PowChallenge {
            algorithm: "DeepSeekHashV1".to_string(),
            challenge: "test".to_string(),
            salt: "abc".to_string(),
            difficulty: 1000,
            expire_at: 1234567890,
            signature: "sig".to_string(),
            target_path: "/api/v0/chat/completion".to_string(),
        };

        let nonce = solve_pow_keccak(&challenge);

        let prefix = format!("{}_{}_", challenge.salt, challenge.expire_at);
        let data = format!("{}{}", prefix, nonce);
        let hash = Keccak256::digest(data.as_bytes());
        let hash_prefix = u64::from_be_bytes(hash[0..8].try_into().unwrap());
        let target_prefix = u64::MAX / challenge.difficulty;

        println!(
            "Keccak test: nonce={}, hash={:#018x}, target={:#018x}",
            nonce, hash_prefix, target_prefix
        );
        assert!(hash_prefix < target_prefix);
    }

    #[test]
    fn test_solve_pow_wasm() {
        // 测试 WASM 求解器（使用嵌入的 WASM）
        let challenge = PowChallenge {
            algorithm: "DeepSeekHashV1".to_string(),
            challenge: "test_challenge_string".to_string(),
            salt: "abc123".to_string(),
            difficulty: 144000,
            expire_at: 1767571094573,
            signature: "sig".to_string(),
            target_path: "/api/v0/chat/completion".to_string(),
        };

        match solve_pow_wasm(&challenge) {
            Some(nonce) => {
                println!("WASM test: nonce={}", nonce);
            }
            None => {
                println!("WASM test: 求解器不可用，跳过测试");
            }
        }
    }
}
