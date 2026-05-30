//! Wgpu GPU backend for static image rendering.
//!
//! This module provides:
//! - `probe_wgpu_capabilities()`: detect available GPU adapters and report capabilities
//! - `render_static_image_wgpu_offscreen()`: experimental offscreen GPU render
//!
//! The capability probe is safe to call in any environment — it never panics if
//! no GPU is available, instead returning a structured report with `supported: false`.
//!
//! The offscreen render path is currently **clear-only experimental**: it creates
//! a texture, runs a clear pass with the background color, copies to a CPU buffer,
//! and returns the result. Full textured quad rendering is planned for Stage 010.

use crate::color::RgbaColor;
use crate::output::RenderOutput;
use crate::wgpu_error::WgpuRenderError;
use serde::{Deserialize, Serialize};

/// Capabilities reported by the wgpu GPU backend probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WgpuRenderCapabilities {
    /// Whether a suitable GPU adapter was found and a device could be created.
    pub supported: bool,
    /// Name of the GPU adapter (if found).
    pub adapter_name: Option<String>,
    /// Backend type (e.g., "Vulkan", "Metal", "Dx12", "Gl").
    pub backend: Option<String>,
    /// Device type (e.g., "DiscreteGpu", "IntegratedGpu", "Cpu").
    pub device_type: Option<String>,
    /// Features supported by the device (as string list).
    pub features: Vec<String>,
    /// Key limits summary.
    pub limits: Option<WgpuLimitsSummary>,
    /// If not supported, the reason why.
    pub failure_reason: Option<String>,
}

/// Summary of key GPU limits relevant to static image rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WgpuLimitsSummary {
    /// Maximum 2D texture dimension.
    pub max_texture_dimension_2d: u32,
    /// Maximum buffer size in bytes.
    pub max_buffer_size: u64,
}

/// Probe the system for wgpu GPU capabilities.
///
/// This function is safe to call in any environment. If no GPU adapter is
/// available (common in headless CI environments), it returns a
/// `WgpuRenderCapabilities` with `supported: false` and a descriptive
/// `failure_reason`. It never panics.
pub fn probe_wgpu_capabilities() -> WgpuRenderCapabilities {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        flags: wgpu::InstanceFlags::default(),
        backend_options: wgpu::BackendOptions::default(),
        display: None,
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
    });

    let adapter_result =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: true,
            compatible_surface: None,
        }));

    let adapter = match adapter_result {
        Ok(a) => a,
        Err(e) => {
            return WgpuRenderCapabilities {
                supported: false,
                adapter_name: None,
                backend: None,
                device_type: None,
                features: vec![],
                limits: None,
                failure_reason: Some(format!("no adapter available: {e}")),
            };
        }
    };

    let info = adapter.get_info();
    let adapter_name = Some(info.name.clone());
    let backend = Some(format!("{:?}", info.backend));
    let device_type = Some(format!("{:?}", info.device_type));

    let device_result = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("wallflow-capability-probe"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
    }));

    let (device, _queue) = match device_result {
        Ok(dq) => dq,
        Err(e) => {
            return WgpuRenderCapabilities {
                supported: false,
                adapter_name,
                backend,
                device_type,
                features: vec![],
                limits: None,
                failure_reason: Some(format!("device creation failed: {e}")),
            };
        }
    };

    let features: Vec<String> = device.features().iter().map(|f| format!("{f:?}")).collect();
    let limits = WgpuLimitsSummary {
        max_texture_dimension_2d: device.limits().max_texture_dimension_2d,
        max_buffer_size: device.limits().max_buffer_size,
    };

    WgpuRenderCapabilities {
        supported: true,
        adapter_name,
        backend,
        device_type,
        features,
        limits: Some(limits),
        failure_reason: None,
    }
}

/// Render a static image using the wgpu GPU backend (clear-only experimental).
///
/// This is an experimental offscreen render path. Currently it only performs
/// a clear pass with the background color — it does NOT render the image
/// texture. Full textured quad rendering is planned for Stage 010.
///
/// The function:
/// 1. Creates a wgpu instance and requests an adapter.
/// 2. Creates a device and queue.
/// 3. Creates an output texture of the viewport size.
/// 4. Runs a clear pass with the background color.
/// 5. Copies the texture to a CPU-readable buffer.
/// 6. Returns a `RenderOutput` with the pixel data.
///
/// If no GPU adapter is available, returns `WgpuRenderError::NoAdapter`.
/// This is expected in headless CI environments.
pub fn render_static_image_wgpu_offscreen(
    width: u32,
    height: u32,
    background: &str,
) -> Result<RenderOutput, WgpuRenderError> {
    // Validate dimensions
    if width == 0 || height == 0 {
        return Err(WgpuRenderError::InvalidViewport { width, height });
    }

    // Parse background color
    let bg_color = RgbaColor::parse_hex(background).map_err(WgpuRenderError::InvalidBackground)?;

    // Create instance
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        flags: wgpu::InstanceFlags::default(),
        backend_options: wgpu::BackendOptions::default(),
        display: None,
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
    });

    // Request adapter
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: true,
        compatible_surface: None,
    }))
    .map_err(|e| WgpuRenderError::NoAdapter(format!("{e}")))?;

    // Request device
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("wallflow-offscreen-render"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
    }))
    .map_err(|e| WgpuRenderError::DeviceCreation(format!("{e}")))?;

    // Create output texture
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("wallflow-output-texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Convert background color from u8 to float for wgpu
    let bg_r = bg_color.r as f64 / 255.0;
    let bg_g = bg_color.g as f64 / 255.0;
    let bg_b = bg_color.b as f64 / 255.0;
    let bg_a = bg_color.a as f64 / 255.0;

    // Create command encoder and run clear pass
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("wallflow-clear-encoder"),
    });

    {
        let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("wallflow-clear-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &texture_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: bg_r,
                        g: bg_g,
                        b: bg_b,
                        a: bg_a,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    // Copy texture to buffer
    let bytes_per_pixel = 4u32;
    let unpadded_bytes_per_row = width * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

    let buffer_size = (padded_bytes_per_row * height) as u64;
    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("wallflow-output-buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &output_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    queue.submit(std::iter::once(encoder.finish()));

    // Map buffer and read pixels
    let buffer_slice = output_buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });

    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|e| WgpuRenderError::BufferMap(format!("poll failed: {e}")))?;

    rx.recv()
        .map_err(|e| WgpuRenderError::BufferMap(format!("channel recv failed: {e}")))?
        .map_err(|e| WgpuRenderError::BufferMap(format!("buffer map failed: {e}")))?;

    let data = buffer_slice.get_mapped_range();

    // Convert padded rows to unpadded RGBA pixel data
    let mut pixels_rgba = vec![0u8; (width * height * bytes_per_pixel) as usize];
    for y in 0..height {
        let src_offset = (y * padded_bytes_per_row) as usize;
        let dst_offset = (y * width * bytes_per_pixel) as usize;
        let row_len = (width * bytes_per_pixel) as usize;
        if src_offset + row_len <= data.len() && dst_offset + row_len <= pixels_rgba.len() {
            pixels_rgba[dst_offset..dst_offset + row_len]
                .copy_from_slice(&data[src_offset..src_offset + row_len]);
        }
    }

    drop(data);
    output_buffer.unmap();

    Ok(RenderOutput::new(width, height, pixels_rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wgpu_capabilities_serialization() {
        let caps = WgpuRenderCapabilities {
            supported: true,
            adapter_name: Some("Test GPU".into()),
            backend: Some("Vulkan".into()),
            device_type: Some("DiscreteGpu".into()),
            features: vec!["TextureCompressionBc".into()],
            limits: Some(WgpuLimitsSummary {
                max_texture_dimension_2d: 16384,
                max_buffer_size: 256 * 1024 * 1024,
            }),
            failure_reason: None,
        };
        let json = serde_json::to_string(&caps).expect("serialize");
        let decoded: WgpuRenderCapabilities = serde_json::from_str(&json).expect("deserialize");
        assert!(decoded.supported);
        assert_eq!(decoded.adapter_name, Some("Test GPU".into()));
        assert_eq!(
            decoded.limits.as_ref().map(|l| l.max_texture_dimension_2d),
            Some(16384)
        );
    }

    #[test]
    fn wgpu_capabilities_unsupported_serialization() {
        let caps = WgpuRenderCapabilities {
            supported: false,
            adapter_name: None,
            backend: None,
            device_type: None,
            features: vec![],
            limits: None,
            failure_reason: Some("no adapter available: NotFound".into()),
        };
        let json = serde_json::to_string(&caps).expect("serialize");
        let decoded: WgpuRenderCapabilities = serde_json::from_str(&json).expect("deserialize");
        assert!(!decoded.supported);
        assert!(decoded.failure_reason.is_some());
    }

    #[test]
    fn wgpu_render_error_display() {
        let err = WgpuRenderError::NoAdapter("no GPU found".into());
        let msg = format!("{err}");
        assert!(msg.contains("no suitable GPU adapter"));
        assert!(msg.contains("no GPU found"));

        let err = WgpuRenderError::DeviceCreation("out of memory".into());
        let msg = format!("{err}");
        assert!(msg.contains("failed to create GPU device"));

        let err = WgpuRenderError::InvalidViewport {
            width: 0,
            height: 100,
        };
        let msg = format!("{err}");
        assert!(msg.contains("invalid viewport"));

        let err = WgpuRenderError::NotImplemented("textured quad".into());
        let msg = format!("{err}");
        assert!(msg.contains("experimental"));
        assert!(msg.contains("textured quad"));
    }

    #[test]
    fn render_backend_serialization() {
        let cpu = crate::RenderBackend::CpuReference;
        let json = serde_json::to_string(&cpu).expect("serialize");
        let decoded: crate::RenderBackend = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, crate::RenderBackend::CpuReference);

        let wgpu = crate::RenderBackend::WgpuExperimental;
        let json = serde_json::to_string(&wgpu).expect("serialize");
        let decoded: crate::RenderBackend = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, crate::RenderBackend::WgpuExperimental);
    }

    #[test]
    fn render_backend_from_str() {
        assert_eq!(
            "cpu".parse::<crate::RenderBackend>(),
            Ok(crate::RenderBackend::CpuReference)
        );
        assert_eq!(
            "wgpu".parse::<crate::RenderBackend>(),
            Ok(crate::RenderBackend::WgpuExperimental)
        );
        assert_eq!(
            "WGPU".parse::<crate::RenderBackend>(),
            Ok(crate::RenderBackend::WgpuExperimental)
        );
        assert!("invalid".parse::<crate::RenderBackend>().is_err());
    }

    #[test]
    fn render_backend_display() {
        assert_eq!(format!("{}", crate::RenderBackend::CpuReference), "cpu");
        assert_eq!(
            format!("{}", crate::RenderBackend::WgpuExperimental),
            "wgpu"
        );
    }

    #[test]
    fn wgpu_limits_summary_serialization() {
        let limits = WgpuLimitsSummary {
            max_texture_dimension_2d: 8192,
            max_buffer_size: 128 * 1024 * 1024,
        };
        let json = serde_json::to_string(&limits).expect("serialize");
        let decoded: WgpuLimitsSummary = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.max_texture_dimension_2d, 8192);
    }
}
