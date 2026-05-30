//! WallFlow core orchestration.
//!
//! This crate contains the central `CoreApp`, the `RendererSupervisor` that
//! manages renderer process lifecycle, and the `RendererProcessManager` that
//! spawns/kills renderer subprocesses. All logic is cloud-testable: nothing
//! here depends on a real Windows desktop or a GUI.

pub mod app;
pub mod renderer_process;
pub mod supervisor;
pub mod watchdog;

pub use app::{CoreApp, CoreError};
pub use renderer_process::{RendererLaunchSpec, RendererProcessError, RendererProcessManager};
pub use supervisor::{
    RendererHandle, RendererReport, RendererStatus, SupervisorError, SupervisorReport,
    SupervisorSnapshot,
};
pub use watchdog::{RendererHeartbeat, WatchdogDecision, WatchdogPolicy};
