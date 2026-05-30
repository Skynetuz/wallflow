//! WallFlow static render output layer.
//!
//! This crate provides CPU/reference rendering of static image wallpapers.
//! It takes a decoded image, a layout (from the layout engine), a fit mode,
//! and a background color, and produces an RGBA frame or PNG output.
//!
//! The CPU reference renderer is the cloud-testable baseline before adding
//! wgpu GPU presentation. It uses nearest-neighbor scaling (temporary;
//! bilinear/Lanczos can be added later).

mod color;
mod error;
mod output;
mod renderer;

pub use color::RgbaColor;
pub use error::StaticRenderError;
pub use output::{RenderOutput, RenderOutputMetadata};
pub use renderer::{render_static_image_cpu, StaticRenderInput};

/// Which backend to use for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderBackend {
    /// CPU reference renderer: pure Rust, no GPU, cloud-testable.
    CpuReference,
    // WgpuExperimental will be added in a future stage.
}

use serde::{Deserialize, Serialize};
