use serde::{Deserialize, Serialize};
use uuid::Uuid;
use wallflow_common::{MonitorId, MonitorInfo, RendererId, WallpaperId};

/// IPC protocol version. Increment on breaking changes.
/// Version 2 added the new RendererCommand/RendererEvent/CoreCommand/CoreEvent types.
/// Version 3 adds the IpcMessage tagged union for unambiguous frame decoding.
/// Version 4 adds ApplyWallpaperRequest, StaticImagePayload, WallpaperApplyError.
pub const PROTOCOL_VERSION: u16 = 4;

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

// ---------------------------------------------------------------------------
// IpcMessage — tagged union for unambiguous frame decoding
// ---------------------------------------------------------------------------

/// Top-level IPC message that wraps all possible message types.
///
/// Every frame on the IPC transport is an `IpcMessage`. The `direction` tag
/// makes it unambiguous which type of payload is inside, even for a reader
/// that does not know what to expect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "direction", content = "payload")]
pub enum IpcMessage {
    /// A command sent from Core to a Renderer process.
    #[serde(rename = "core_to_renderer")]
    Command(CommandEnvelope<RendererCommand>),
    /// An event sent from a Renderer process to Core.
    #[serde(rename = "renderer_to_core")]
    Event(EventEnvelope<RendererEvent>),
    /// A command sent from an external source (UI, CLI) to Core.
    #[serde(rename = "external_to_core")]
    CoreCommand(CommandEnvelope<CoreCommand>),
    /// An event broadcast by Core to listeners.
    #[serde(rename = "core_broadcast")]
    CoreEvent(EventEnvelope<CoreEvent>),
}

// ---------------------------------------------------------------------------
// Typed envelopes
// ---------------------------------------------------------------------------

/// Envelope for commands (which carry a request_id for correlation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandEnvelope<T> {
    pub protocol_version: u16,
    pub request_id: RequestId,
    pub payload: T,
}

impl<T> CommandEnvelope<T> {
    pub fn new(payload: T) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: RequestId::new(),
            payload,
        }
    }

    pub fn with_request_id(request_id: RequestId, payload: T) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            payload,
        }
    }
}

/// Envelope for events (which do not require a request_id by default).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    pub protocol_version: u16,
    pub request_id: Option<RequestId>,
    pub payload: T,
}

impl<T> EventEnvelope<T> {
    pub fn new(payload: T) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: None,
            payload,
        }
    }

    /// Create an event envelope that correlates with a specific request.
    pub fn with_correlation(request_id: RequestId, payload: T) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: Some(request_id),
            payload,
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy generic envelope (kept for backward compatibility)
// ---------------------------------------------------------------------------

/// Generic envelope wrapping every IPC message with metadata.
/// Prefer `CommandEnvelope` or `EventEnvelope` for new code.
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
// Wallpaper apply types (IPC payloads)
// ---------------------------------------------------------------------------

/// How a static image should be fitted to the screen.
/// Mirrors `wallflow_package::FitMode` but defined independently in the IPC
/// layer so that the renderer does not need to depend on wallflow-package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    #[default]
    Cover,
    Contain,
    Stretch,
    Center,
    Tile,
}

impl std::fmt::Display for FitMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FitMode::Cover => write!(f, "cover"),
            FitMode::Contain => write!(f, "contain"),
            FitMode::Stretch => write!(f, "stretch"),
            FitMode::Center => write!(f, "center"),
            FitMode::Tile => write!(f, "tile"),
        }
    }
}

/// Payload for a static image wallpaper apply request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticImagePayload {
    /// Absolute or package-relative path to the image file.
    pub image_path: String,
    /// How the image should be fitted to the screen.
    #[serde(default)]
    pub fit: FitMode,
    /// Background color as a CSS color string (e.g., "#000000").
    #[serde(default = "default_background_color")]
    pub background: String,
    /// Optional opacity (0-255). None means fully opaque.
    #[serde(default)]
    pub opacity: Option<u8>,
}

fn default_background_color() -> String {
    "#000000".into()
}

/// The kind of wallpaper payload being applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum WallpaperPayload {
    /// Static image wallpaper.
    #[serde(rename = "static_image")]
    StaticImage(StaticImagePayload),
}

/// A request to apply a wallpaper to a renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyWallpaperRequest {
    /// The wallpaper ID being applied.
    pub wallpaper_id: WallpaperId,
    /// The wallpaper payload (kind + data).
    pub payload: WallpaperPayload,
    /// The target monitor for this wallpaper.
    pub target_monitor: MonitorId,
}

/// Options for applying a wallpaper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WallpaperApplyOptions {
    /// Whether to transition immediately or wait for the next frame.
    #[serde(default)]
    pub immediate: bool,
}

/// Error that can occur when applying a wallpaper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WallpaperApplyError {
    /// The wallpaper kind is not supported by this renderer.
    UnsupportedKind { kind: String },
    /// The image path is empty or invalid.
    InvalidImagePath { path: String },
    /// The image file could not be loaded.
    ImageLoadFailed { path: String, reason: String },
    /// The renderer is not in a state that allows applying wallpapers.
    InvalidRendererState { state: String },
    /// A generic error occurred.
    Other { message: String },
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
    /// Apply a new wallpaper to this renderer with full typed payload.
    ApplyWallpaper(ApplyWallpaperRequest),
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
        wallpaper_id: WallpaperId,
        monitor_id: MonitorId,
    },
    /// An error occurred while applying a wallpaper.
    WallpaperApplyFailed {
        renderer_id: RendererId,
        wallpaper_id: WallpaperId,
        error: WallpaperApplyError,
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
    ApplyWallpaperToMonitor(ApplyWallpaperRequest),
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
// Helper functions for creating typed IPC messages
// ---------------------------------------------------------------------------

/// Create a `RendererCommand` IPC message ready for framing.
pub fn renderer_command(cmd: RendererCommand) -> IpcMessage {
    IpcMessage::Command(CommandEnvelope::new(cmd))
}

/// Create a `RendererEvent` IPC message ready for framing.
pub fn renderer_event(event: RendererEvent) -> IpcMessage {
    IpcMessage::Event(EventEnvelope::new(event))
}

/// Create a `CoreCommand` IPC message ready for framing.
pub fn core_command(cmd: CoreCommand) -> IpcMessage {
    IpcMessage::CoreCommand(CommandEnvelope::new(cmd))
}

/// Create a `CoreEvent` IPC message ready for framing.
pub fn core_event(event: CoreEvent) -> IpcMessage {
    IpcMessage::CoreEvent(EventEnvelope::new(event))
}

/// Extract the renderer_id from a RendererEvent, if present.
pub fn renderer_event_id(event: &RendererEvent) -> RendererId {
    match event {
        RendererEvent::Started { renderer_id } => *renderer_id,
        RendererEvent::Ready { renderer_id } => *renderer_id,
        RendererEvent::Heartbeat { renderer_id, .. } => *renderer_id,
        RendererEvent::Paused { renderer_id } => *renderer_id,
        RendererEvent::Resumed { renderer_id } => *renderer_id,
        RendererEvent::WallpaperApplied { renderer_id, .. } => *renderer_id,
        RendererEvent::WallpaperApplyFailed { renderer_id, .. } => *renderer_id,
        RendererEvent::Error { renderer_id, .. } => *renderer_id,
        RendererEvent::Exited { renderer_id, .. } => *renderer_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_command_apply_wallpaper_roundtrip() {
        let cmd = RendererCommand::ApplyWallpaper(ApplyWallpaperRequest {
            wallpaper_id: WallpaperId::new(),
            payload: WallpaperPayload::StaticImage(StaticImagePayload {
                image_path: "/path/to/wallpaper.png".into(),
                fit: FitMode::Cover,
                background: "#000000".into(),
                opacity: None,
            }),
            target_monitor: MonitorId("mon-1".into()),
        });
        let json = serde_json::to_string(&cmd).expect("serialize");
        let decoded: RendererCommand = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn renderer_event_wallpaper_applied_roundtrip() {
        let event = RendererEvent::WallpaperApplied {
            renderer_id: RendererId::new(),
            wallpaper_id: WallpaperId::new(),
            monitor_id: MonitorId("mon-1".into()),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let decoded: RendererEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(event, decoded);
    }

    #[test]
    fn renderer_event_wallpaper_apply_failed_roundtrip() {
        let event = RendererEvent::WallpaperApplyFailed {
            renderer_id: RendererId::new(),
            wallpaper_id: WallpaperId::new(),
            error: WallpaperApplyError::UnsupportedKind {
                kind: "video".into(),
            },
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let decoded: RendererEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(event, decoded);
    }

    #[test]
    fn wallpaper_apply_error_serialization() {
        let errors = vec![
            WallpaperApplyError::UnsupportedKind { kind: "web".into() },
            WallpaperApplyError::InvalidImagePath { path: "".into() },
            WallpaperApplyError::ImageLoadFailed {
                path: "/missing.png".into(),
                reason: "not found".into(),
            },
            WallpaperApplyError::InvalidRendererState {
                state: "starting".into(),
            },
            WallpaperApplyError::Other {
                message: "something went wrong".into(),
            },
        ];
        for error in errors {
            let json = serde_json::to_string(&error).expect("serialize");
            let decoded: WallpaperApplyError = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(error, decoded);
        }
    }

    #[test]
    fn fit_mode_roundtrip() {
        let modes = vec![
            FitMode::Cover,
            FitMode::Contain,
            FitMode::Stretch,
            FitMode::Center,
            FitMode::Tile,
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).expect("serialize");
            let decoded: FitMode = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(mode, decoded);
        }
    }

    #[test]
    fn static_image_payload_roundtrip() {
        let payload = StaticImagePayload {
            image_path: "content/wallpaper.png".into(),
            fit: FitMode::Contain,
            background: "#1a1a2e".into(),
            opacity: Some(200),
        };
        let json = serde_json::to_string(&payload).expect("serialize");
        let decoded: StaticImagePayload = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(payload, decoded);
    }

    #[test]
    fn wallpaper_payload_roundtrip() {
        let payload = WallpaperPayload::StaticImage(StaticImagePayload {
            image_path: "test.png".into(),
            fit: FitMode::default(),
            background: "#000000".into(),
            opacity: None,
        });
        let json = serde_json::to_string(&payload).expect("serialize");
        let decoded: WallpaperPayload = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(payload, decoded);
    }

    #[test]
    fn apply_wallpaper_request_roundtrip() {
        let req = ApplyWallpaperRequest {
            wallpaper_id: WallpaperId::new(),
            payload: WallpaperPayload::StaticImage(StaticImagePayload {
                image_path: "/test/wallpaper.png".into(),
                fit: FitMode::Cover,
                background: "#000000".into(),
                opacity: None,
            }),
            target_monitor: MonitorId("primary".into()),
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let decoded: ApplyWallpaperRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(req, decoded);
    }

    #[test]
    fn renderer_command_roundtrip_json() {
        let cmd = RendererCommand::ApplyWallpaper(ApplyWallpaperRequest {
            wallpaper_id: WallpaperId::new(),
            payload: WallpaperPayload::StaticImage(StaticImagePayload {
                image_path: "/path/to/image.png".into(),
                fit: FitMode::Cover,
                background: "#000000".into(),
                opacity: None,
            }),
            target_monitor: MonitorId("mon-1".into()),
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
        let cmd = CoreCommand::ApplyWallpaperToMonitor(ApplyWallpaperRequest {
            wallpaper_id: WallpaperId::new(),
            payload: WallpaperPayload::StaticImage(StaticImagePayload {
                image_path: "/test.mp4".into(),
                fit: FitMode::Stretch,
                background: "#111111".into(),
                opacity: None,
            }),
            target_monitor: MonitorId("mon-2".into()),
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

    #[test]
    fn ipc_message_command_roundtrip() {
        let msg = renderer_command(RendererCommand::Pause);
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: IpcMessage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn ipc_message_event_roundtrip() {
        let rid = RendererId::new();
        let msg = renderer_event(RendererEvent::Ready { renderer_id: rid });
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: IpcMessage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn command_envelope_has_request_id() {
        let env = CommandEnvelope::new(RendererCommand::Start);
        assert_eq!(env.protocol_version, PROTOCOL_VERSION);
        // request_id is always present in CommandEnvelope
    }

    #[test]
    fn event_envelope_no_request_id_by_default() {
        let env = EventEnvelope::new(RendererEvent::Started {
            renderer_id: RendererId::new(),
        });
        assert_eq!(env.protocol_version, PROTOCOL_VERSION);
        assert!(env.request_id.is_none());
    }

    #[test]
    fn event_envelope_with_correlation() {
        let rid = RequestId::new();
        let env = EventEnvelope::with_correlation(
            rid,
            RendererEvent::Started {
                renderer_id: RendererId::new(),
            },
        );
        assert!(env.request_id.is_some());
        assert_eq!(env.request_id.as_ref().expect("id"), &rid);
    }

    #[test]
    fn renderer_event_id_extraction() {
        let rid = RendererId::new();
        let event = RendererEvent::Heartbeat {
            renderer_id: rid,
            uptime_ms: 100,
        };
        assert_eq!(renderer_event_id(&event), rid);
    }

    #[test]
    fn ipc_message_core_command_roundtrip() {
        let msg = core_command(CoreCommand::PauseAll);
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: IpcMessage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn ipc_message_core_event_roundtrip() {
        let msg = core_event(CoreEvent::Ready);
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: IpcMessage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(msg, decoded);
    }
}
