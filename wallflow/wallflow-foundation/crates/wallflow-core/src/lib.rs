//! WallFlow core orchestration.

pub mod app;
pub mod renderer_process;
pub mod watchdog;

pub use app::{CoreApp, CoreError};
pub use renderer_process::{RendererLaunchSpec, RendererProcessManager};
pub use watchdog::{RendererHeartbeat, WatchdogDecision, WatchdogPolicy};
