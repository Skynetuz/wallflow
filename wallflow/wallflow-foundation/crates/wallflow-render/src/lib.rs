//! WallFlow static render output layer.
//!
//! This crate provides CPU/reference rendering and experimental wgpu GPU rendering
//! of static image wallpapers. It takes a decoded image, a layout (from the layout
//! engine), a fit mode, and a background color, and produces an RGBA frame or PNG output.
//!
//! The CPU reference renderer is the cloud-testable baseline. The wgpu experimental
//! renderer is optional and gracefully degrades when no GPU is available.
//!
//! Nearest-neighbor scaling is used by the CPU renderer (temporary;
//! bilinear/Lanczos can be added later).

mod color;
mod error;
mod output;
mod presenter;
mod renderer;
mod wgpu_backend;
mod wgpu_error;

pub use color::RgbaColor;
pub use error::StaticRenderError;
pub use output::{RenderOutput, RenderOutputMetadata};
pub use presenter::{
    rgba_to_softbuffer_u32, rgba_to_softbuffer_u32_with_surface_size, PresenterBackend,
    PresenterReport, PresenterState, PresenterViewport, SoftbufferPresenterConfig,
};
pub use renderer::{render_static_image_cpu, StaticRenderInput};
pub use wgpu_backend::{
    probe_wgpu_capabilities, render_static_image_wgpu_offscreen, WgpuRenderCapabilities,
};
pub use wgpu_error::WgpuRenderError;

use serde::{Deserialize, Serialize};

/// Which backend to use for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RenderBackend {
    /// CPU reference renderer: pure Rust, no GPU, cloud-testable.
    #[default]
    CpuReference,
    /// Experimental wgpu GPU renderer: requires GPU adapter, may be unavailable.
    WgpuExperimental,
}

impl std::fmt::Display for RenderBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderBackend::CpuReference => write!(f, "cpu"),
            RenderBackend::WgpuExperimental => write!(f, "wgpu"),
        }
    }
}

impl std::str::FromStr for RenderBackend {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "cpu" => Ok(RenderBackend::CpuReference),
            "wgpu" => Ok(RenderBackend::WgpuExperimental),
            other => Err(format!(
                "unknown render backend '{}'; expected 'cpu' or 'wgpu'",
                other
            )),
        }
    }
}
