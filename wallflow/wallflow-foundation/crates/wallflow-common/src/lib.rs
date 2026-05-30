//! Shared domain model for WallFlow.
//!
//! This crate must not depend on UI, Win32, Media Foundation, Tauri or renderer internals.

pub mod error;
pub mod fit_mode;
pub mod monitor;
pub mod renderer;
pub mod wallpaper;

pub use error::{WallFlowError, WallFlowResult};
pub use fit_mode::FitMode;
pub use monitor::{MonitorId, MonitorInfo, MonitorPosition, MonitorSize};
pub use renderer::{
    RendererAssignment, RendererGroupId, RendererHealth, RendererId, RendererRestartPolicy,
    RendererState,
};
pub use wallpaper::{PerformanceProfile, WallpaperAssignment, WallpaperId, WallpaperKind};
