use base64::{engine::general_purpose, Engine as _};

#[derive(Debug, Clone)]
pub struct WebGLInfo {
    pub version: String,
    pub shading_language_version: String,
    pub vendor: String,
    pub renderer: String,
    pub extensions: Vec<String>,
}

impl WebGLInfo {
    pub fn chrome_default() -> Self {
        Self {
            version: "WebGL 1.0".to_string(),
            shading_language_version: "WebGL GLSL ES 1.0".to_string(),
            vendor: "WebKit".to_string(),
            renderer: "WebKit WebGL".to_string(),
            extensions: vec![
                "ANGLE_instanced_arrays".to_string(),
                "EXT_blend_minmax".to_string(),
                "EXT_color_buffer_half_float".to_string(),
                "EXT_disjoint_timer_query".to_string(),
                "EXT_float_blend".to_string(),
                "EXT_frag_depth".to_string(),
                "EXT_shader_texture_lod".to_string(),
                "EXT_texture_compression_bptc".to_string(),
                "EXT_texture_compression_rgtc".to_string(),
                "EXT_texture_filter_anisotropic".to_string(),
                "WEBKIT_EXT_texture_filter_anisotropic".to_string(),
                "EXT_sRGB".to_string(),
                "KHR_parallel_shader_compile".to_string(),
                "OES_element_index_uint".to_string(),
                "OES_fbo_render_mipmap".to_string(),
                "OES_standard_derivatives".to_string(),
                "OES_texture_float".to_string(),
                "OES_texture_float_linear".to_string(),
                "OES_texture_half_float".to_string(),
                "OES_texture_half_float_linear".to_string(),
                "OES_vertex_array_object".to_string(),
                "WEBGL_color_buffer_float".to_string(),
                "WEBGL_compressed_texture_s3tc".to_string(),
                "WEBGL_compressed_texture_s3tc_srgb".to_string(),
                "WEBGL_debug_renderer_info".to_string(),
                "WEBGL_debug_shaders".to_string(),
                "WEBGL_depth_texture".to_string(),
                "WEBGL_draw_buffers".to_string(),
                "WEBGL_lose_context".to_string(),
                "WEBGL_multi_draw".to_string(),
            ],
        }
    }

    pub fn firefox_default() -> Self {
        Self {
            version: "WebGL 1.0".to_string(),
            shading_language_version: "WebGL GLSL ES 1.0".to_string(),
            vendor: "Mozilla".to_string(),
            renderer: "Mozilla".to_string(),
            extensions: vec![
                "ANGLE_instanced_arrays".to_string(),
                "EXT_blend_minmax".to_string(),
                "EXT_color_buffer_half_float".to_string(),
                "EXT_frag_depth".to_string(),
                "EXT_shader_texture_lod".to_string(),
                "EXT_texture_filter_anisotropic".to_string(),
                "MOZ_EXT_texture_filter_anisotropic".to_string(),
                "EXT_sRGB".to_string(),
                "OES_element_index_uint".to_string(),
                "OES_standard_derivatives".to_string(),
                "OES_texture_float".to_string(),
                "OES_texture_float_linear".to_string(),
                "OES_texture_half_float".to_string(),
                "OES_texture_half_float_linear".to_string(),
                "OES_vertex_array_object".to_string(),
                "WEBGL_color_buffer_float".to_string(),
                "WEBGL_compressed_texture_s3tc".to_string(),
                "WEBGL_depth_texture".to_string(),
                "WEBGL_draw_buffers".to_string(),
                "WEBGL_lose_context".to_string(),
            ],
        }
    }

    pub fn to_dm_img_str(&self) -> String {
        // 生成包含完整WebGL上下文信息的指纹字符串
        let full_webgl_info = format!(
            "{} | {} | {} | GLSL: {} | Extensions: {}",
            self.version,
            self.get_full_context_info(),
            self.renderer,
            self.shading_language_version,
            self.get_extensions_string()
        );
        general_purpose::STANDARD.encode(full_webgl_info)
    }

    pub fn get_full_context_info(&self) -> String {
        format!(
            "Version: {}, Vendor: {}, Renderer: {}, GLSL: {}",
            self.version, self.vendor, self.renderer, self.shading_language_version
        )
    }

    pub fn get_extensions_string(&self) -> String {
        self.extensions.join(" ")
    }
}
