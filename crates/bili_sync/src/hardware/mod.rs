pub mod fingerprint;
pub mod gpu;
pub mod webgl;

pub use fingerprint::HardwareFingerprint;
pub use gpu::{GpuInfo, GpuVendor};
pub use webgl::WebGLInfo;

#[derive(Debug, Clone)]
pub struct HardwareInfo {
    pub gpu: GpuInfo,
    pub webgl: WebGLInfo,
}

impl HardwareInfo {
    pub fn new() -> Self {
        Self::default()
    }

    // 根据策略名称选择硬件配置
    pub fn by_strategy(strategy: &str) -> Self {
        match strategy {
            "firefox_high_end" => Self::nvidia_rtx4090_firefox(),
            "firefox_workstation" => Self::amd_rx7900xtx_firefox(),
            "gaming" => Self::nvidia_rtx4070ti(),
            "workstation" => Self::amd_rx7800xt(),
            "budget" => Self::intel_arc_a770(),
            _ => Self::default(),
        }
    }

    pub fn nvidia_rtx4070ti() -> Self {
        Self {
            gpu: GpuInfo::nvidia_rtx4070ti(),
            webgl: WebGLInfo::chrome_default(),
        }
    }

    pub fn amd_rx7800xt() -> Self {
        Self {
            gpu: GpuInfo::amd_rx7800xt(),
            webgl: WebGLInfo::chrome_default(),
        }
    }

    pub fn intel_arc_a770() -> Self {
        Self {
            gpu: GpuInfo::intel_arc_a770(),
            webgl: WebGLInfo::chrome_default(),
        }
    }

    // 新增高端GPU配置
    pub fn nvidia_rtx4090() -> Self {
        Self {
            gpu: GpuInfo::nvidia_rtx4090(),
            webgl: WebGLInfo::chrome_default(),
        }
    }

    pub fn nvidia_rtx4070() -> Self {
        Self {
            gpu: GpuInfo::nvidia_rtx4070(),
            webgl: WebGLInfo::chrome_default(),
        }
    }

    pub fn amd_rx7900xtx() -> Self {
        Self {
            gpu: GpuInfo::amd_rx7900xtx(),
            webgl: WebGLInfo::chrome_default(),
        }
    }

    pub fn amd_rx7700xt() -> Self {
        Self {
            gpu: GpuInfo::amd_rx7700xt(),
            webgl: WebGLInfo::chrome_default(),
        }
    }

    pub fn intel_arc_a750() -> Self {
        Self {
            gpu: GpuInfo::intel_arc_a750(),
            webgl: WebGLInfo::chrome_default(),
        }
    }

    // Firefox环境配置
    pub fn nvidia_rtx4070ti_firefox() -> Self {
        Self {
            gpu: GpuInfo::nvidia_rtx4070ti(),
            webgl: WebGLInfo::firefox_default(),
        }
    }

    pub fn nvidia_rtx4090_firefox() -> Self {
        Self {
            gpu: GpuInfo::nvidia_rtx4090(),
            webgl: WebGLInfo::firefox_default(),
        }
    }

    pub fn amd_rx7800xt_firefox() -> Self {
        Self {
            gpu: GpuInfo::amd_rx7800xt(),
            webgl: WebGLInfo::firefox_default(),
        }
    }

    pub fn amd_rx7900xtx_firefox() -> Self {
        Self {
            gpu: GpuInfo::amd_rx7900xtx(),
            webgl: WebGLInfo::firefox_default(),
        }
    }

    pub fn generate_dm_img_str(&self) -> String {
        self.webgl.to_dm_img_str()
    }

    pub fn generate_dm_cover_img_str(&self) -> String {
        self.gpu.to_dm_cover_img_str()
    }
}

impl Default for HardwareInfo {
    fn default() -> Self {
        Self::nvidia_rtx4070ti()
    }
}
