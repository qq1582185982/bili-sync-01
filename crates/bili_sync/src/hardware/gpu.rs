use base64::{engine::general_purpose, Engine as _};

#[derive(Debug, Clone)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
}

#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub vendor: GpuVendor,
    pub model: String,
    pub device_id: String,
    pub driver_version: String,
    pub directx_version: String,
    pub angle_info: String,
}

impl GpuInfo {
    pub fn nvidia_rtx4070ti() -> Self {
        Self {
            vendor: GpuVendor::Nvidia,
            model: "NVIDIA GeForce RTX 4070 Ti SUPER".to_string(),
            device_id: "0x00002705".to_string(),
            driver_version: "vs_5_0 ps_5_0".to_string(),
            directx_version: "Direct3D11".to_string(),
            angle_info: "ANGLE (NVIDIA, NVIDIA GeForce RTX 4070 Ti SUPER (0x00002705) Direct3D11 vs_5_0 ps_5_0, D3D11)Google Inc. (NVIDIA)".to_string(),
        }
    }

    pub fn nvidia_rtx4080() -> Self {
        Self {
            vendor: GpuVendor::Nvidia,
            model: "NVIDIA GeForce RTX 4080".to_string(),
            device_id: "0x00002782".to_string(),
            driver_version: "vs_5_0 ps_5_0".to_string(),
            directx_version: "Direct3D11".to_string(),
            angle_info: "ANGLE (NVIDIA, NVIDIA GeForce RTX 4080 (0x00002782) Direct3D11 vs_5_0 ps_5_0, D3D11)Google Inc. (NVIDIA)".to_string(),
        }
    }

    pub fn amd_rx7800xt() -> Self {
        Self {
            vendor: GpuVendor::Amd,
            model: "AMD Radeon RX 7800 XT".to_string(),
            device_id: "0x0000747E".to_string(),
            driver_version: "vs_5_0 ps_5_0".to_string(),
            directx_version: "Direct3D11".to_string(),
            angle_info:
                "ANGLE (AMD, AMD Radeon RX 7800 XT (0x0000747E) Direct3D11 vs_5_0 ps_5_0, D3D11)ATI Technologies Inc."
                    .to_string(),
        }
    }

    pub fn intel_arc_a770() -> Self {
        Self {
            vendor: GpuVendor::Intel,
            model: "Intel Arc A770 Graphics".to_string(),
            device_id: "0x000056A0".to_string(),
            driver_version: "vs_5_0 ps_5_0".to_string(),
            directx_version: "Direct3D11".to_string(),
            angle_info: "ANGLE (Intel, Intel Arc A770 Graphics (0x000056A0) Direct3D11 vs_5_0 ps_5_0, D3D11)Intel Inc."
                .to_string(),
        }
    }

    // 新增高端GPU型号
    pub fn nvidia_rtx4090() -> Self {
        Self {
            vendor: GpuVendor::Nvidia,
            model: "NVIDIA GeForce RTX 4090".to_string(),
            device_id: "0x00002684".to_string(),
            driver_version: "vs_5_0 ps_5_0".to_string(),
            directx_version: "Direct3D11".to_string(),
            angle_info: "ANGLE (NVIDIA, NVIDIA GeForce RTX 4090 (0x00002684) Direct3D11 vs_5_0 ps_5_0, D3D11)Google Inc. (NVIDIA)".to_string(),
        }
    }

    pub fn nvidia_rtx4070() -> Self {
        Self {
            vendor: GpuVendor::Nvidia,
            model: "NVIDIA GeForce RTX 4070".to_string(),
            device_id: "0x00002786".to_string(),
            driver_version: "vs_5_0 ps_5_0".to_string(),
            directx_version: "Direct3D11".to_string(),
            angle_info: "ANGLE (NVIDIA, NVIDIA GeForce RTX 4070 (0x00002786) Direct3D11 vs_5_0 ps_5_0, D3D11)Google Inc. (NVIDIA)".to_string(),
        }
    }

    pub fn amd_rx7900xtx() -> Self {
        Self {
            vendor: GpuVendor::Amd,
            model: "AMD Radeon RX 7900 XTX".to_string(),
            device_id: "0x0000744C".to_string(),
            driver_version: "vs_5_0 ps_5_0".to_string(),
            directx_version: "Direct3D11".to_string(),
            angle_info:
                "ANGLE (AMD, AMD Radeon RX 7900 XTX (0x0000744C) Direct3D11 vs_5_0 ps_5_0, D3D11)ATI Technologies Inc."
                    .to_string(),
        }
    }

    pub fn amd_rx7700xt() -> Self {
        Self {
            vendor: GpuVendor::Amd,
            model: "AMD Radeon RX 7700 XT".to_string(),
            device_id: "0x000074E8".to_string(),
            driver_version: "vs_5_0 ps_5_0".to_string(),
            directx_version: "Direct3D11".to_string(),
            angle_info:
                "ANGLE (AMD, AMD Radeon RX 7700 XT (0x000074E8) Direct3D11 vs_5_0 ps_5_0, D3D11)ATI Technologies Inc."
                    .to_string(),
        }
    }

    pub fn intel_arc_a750() -> Self {
        Self {
            vendor: GpuVendor::Intel,
            model: "Intel Arc A750 Graphics".to_string(),
            device_id: "0x000056A1".to_string(),
            driver_version: "vs_5_0 ps_5_0".to_string(),
            directx_version: "Direct3D11".to_string(),
            angle_info: "ANGLE (Intel, Intel Arc A750 Graphics (0x000056A1) Direct3D11 vs_5_0 ps_5_0, D3D11)Intel Inc."
                .to_string(),
        }
    }

    pub fn to_dm_cover_img_str(&self) -> String {
        // 生成包含完整GPU信息的硬件指纹字符串
        let full_gpu_info = format!(
            "{} | {} | Device: {} | Driver: {} | DirectX: {}",
            self.angle_info,
            self.get_full_info(),
            self.device_id,
            self.driver_version,
            self.directx_version
        );
        general_purpose::STANDARD.encode(full_gpu_info)
    }

    pub fn get_vendor_name(&self) -> &'static str {
        match self.vendor {
            GpuVendor::Nvidia => "NVIDIA",
            GpuVendor::Amd => "AMD",
            GpuVendor::Intel => "Intel",
        }
    }

    pub fn get_full_info(&self) -> String {
        format!("{} {} {}", self.get_vendor_name(), self.model, self.device_id)
    }
}
