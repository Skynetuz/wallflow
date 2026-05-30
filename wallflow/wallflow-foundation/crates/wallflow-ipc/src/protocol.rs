use serde::{Deserialize, Serialize};
use uuid::Uuid;
use wallflow_common::{MonitorId, MonitorInfo, RendererId, RendererState, WallpaperAssignment};

/// IPC protocol version. Increment on breaking changes.
pub const PROTOCOL_VERSION: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestId(pub Uuid);

impl RequestId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

/// Generic envelope wrapping every IPC message with metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub protocol_version: u16,
    pub request_id: Option<RequestId>,
    pub payload: T,
}

impl<T> Envelope<T> {
    pub fn event(payload: T) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: None,
            payload,
        }
    }

    pub fn request(request_id: RequestId, payload: T) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: Some(request_id),
            payload,
        }
    }
}

// ---------------------------------------------------------------------------
// Core ↔ Renderer typed protocol
// ---------------------------------------------------------------------------

/// Commands sent from Core to a specific Renderer process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RendererCommand {
    /// Start rendering the previously loaded wallpaper.
    Start,
    /// Pause rendering (keep resources, stop frame output).
    Pause,
    /// Resume rendering from a paused state.
    Resume,
    /// Stop rendering and release resources.
    Stop,
    /// Apply a new wallpaper assignment to this renderer.
    ApplyWallpaper(WallpaperAssignment),
    /// Assign the renderer to a different monitor.
    SetMonitor { monitor_id: MonitorId },
    /// Graceful shutdown request.
    Shutdown,
}

/// Events sent from a Renderer process back to Core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RendererEvent {
    /// Renderer process has started.
    Started { renderer_id: RendererId },
    /// Renderer is ready to accept commands (initialization complete).
    Ready { renderer_id: RendererId },
    /// Periodic heartbeat indicating the renderer is alive.
    Heartbeat {
        renderer_id: RendererId,
        uptime_ms: u64,
    },
    /// Renderer has paused rendering.
    Paused { renderer_id: RendererId },
    /// Renderer has resumed rendering.
    Resumed { renderer_id: RendererId },
    /// Wallpaper has been successfully applied and is rendering.
    WallpaperApplied {
        renderer_id: RendererId,
        monitor_id: MonitorId,
    },
    /// Renderer encountered an error.
    Error {
        renderer_id: RendererId,
        message: String,
    },
    /// Renderer process is exiting.
    Exited {
        renderer_id: RendererId,
        exit_code: Option<i32>,
    },
}

/// Commands sent from external sources (UI, CLI) to the Core process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CoreCommand {
    /// Apply a wallpaper to a specific monitor, launching a renderer if needed.
    ApplyWallpaperToMonitor(WallpaperAssignment),
    /// Stop the wallpaper on a specific monitor.
    StopWallpaper { monitor_id: MonitorId },
    /// Pause all active renderers.
    PauseAll,
    /// Resume all paused renderers.
    ResumeAll,
    /// Query the current state of all renderers.
    QueryState,
    /// Request a monitor snapshot.
    GetMonitors,
    /// Enter safe mode (stops all renderers).
    EnterSafeMode { reason: String },
    /// Exit safe mode.
    ExitSafeMode,
    /// Shut down the core process.
    Shutdown,
}

/// Events broadcast by the Core process to listeners (UI, CLI, log subscribers).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CoreEvent {
    /// Core state has changed (e.g., safe mode toggled).
    StateChanged { safe_mode: bool },
    /// A new renderer process has been started.
    RendererStarted { renderer_id: RendererId },
    /// A renderer process has stopped normally.
    RendererStopped { renderer_id: RendererId },
    /// A renderer process has crashed.
    RendererCrashed {
        renderer_id: RendererId,
        reason: Option<String>,
    },
    /// A renderer process has been recovered after a crash.
    RendererRecovered { renderer_id: RendererId },
    /// Monitor snapshot is available.
    MonitorsSnapshot(Vec<MonitorInfo>),
    /// Core is ready to accept commands.
    Ready,
    /// An error occurred in the core.
    Error { message: String },
    /// Safe mode state changed.
    SafeModeChanged {
        enabled: bool,
        reason: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Legacy protocol types (kept for backward compatibility during migration)
// ---------------------------------------------------------------------------

/// Legacy core command for monitor-level operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LegacyCoreCommand {
    GetMonitors,
    ApplyWallpaper(WallpaperAssignment),
    PauseMonitor { monitor_id: MonitorId },
    ResumeMonitor { monitor_id: MonitorId },
    StopMonitor { monitor_id: MonitorId },
    EnterSafeMode { reason: String },
    ExitSafeMode,
    Shutdown,
}

/// Legacy renderer command set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LegacyRendererCommand {
    Load(WallpaperAssignment),
    Play,
    Pause,
    Stop,
    Shutdown,
}

/// Legacy renderer event set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LegacyRendererEvent {
    Ready {
        renderer_id: RendererId,
    },
    FirstFrame {
        renderer_id: RendererId,
    },
    Heartbeat {
        renderer_id: RendererId,
        uptime_ms: u64,
    },
    StateChanged {
        renderer_id: RendererId,
        state: RendererState,
    },
    Error {
        renderer_id: RendererId,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_command_roundtrip_json() {
        let cmd = RendererCommand::ApplyWallpaper(WallpaperAssignment {
            monitor_id: MonitorId("mon-1".into()),
            wallpaper_id: wallflow_common::WallpaperId::new(),
            kind: wallflow_common::WallpaperKind::None,
            profile: wallflow_common::PerformanceProfile::Balanced,
        });
        let json = serde_json::to_string(&cmd).expect("serialize");
        let decoded: RendererCommand = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn renderer_event_roundtrip_json() {
        let event = RendererEvent::Heartbeat {
            renderer_id: RendererId::new(),
            uptime_ms: 5000,
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let decoded: RendererEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(event, decoded);
    }

    #[test]
    fn core_command_roundtrip_json() {
        let cmd = CoreCommand::ApplyWallpaperToMonitor(WallpaperAssignment {
            monitor_id: MonitorId("mon-2".into()),
            wallpaper_id: wallflow_common::WallpaperId::new(),
            kind: wallflow_common::WallpaperKind::Video {
                path: std::path::PathBuf::from("/test.mp4"),
                muted: true,
                looping: true,
            },
            profile: wallflow_common::PerformanceProfile::Quality,
        });
        let json = serde_json::to_string(&cmd).expect("serialize");
        let decoded: CoreCommand = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn core_event_roundtrip_json() {
        let event = CoreEvent::RendererCrashed {
            renderer_id: RendererId::new(),
            reason: Some("segfault".into()),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let decoded: CoreEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(event, decoded);
    }

    #[test]
    fn envelope_protocol_version() {
        let env = Envelope::event(RendererCommand::Start);
        assert_eq!(env.protocol_version, PROTOCOL_VERSION);
        assert!(env.request_id.is_none());
    }

    #[test]
    fn envelope_request_has_id() {
        let rid = RequestId::new();
        let env = Envelope::request(rid, RendererCommand::Shutdown);
        assert_eq!(env.protocol_version, PROTOCOL_VERSION);
        assert!(env.request_id.is_some());
    }
}
