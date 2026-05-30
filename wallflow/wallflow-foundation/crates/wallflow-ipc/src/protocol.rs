use serde::{Deserialize, Serialize};
use uuid::Uuid;
use wallflow_common::{MonitorId, MonitorInfo, RendererId, RendererState, WallpaperAssignment};

pub const PROTOCOL_VERSION: u16 = 1;

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CoreCommand {
    GetMonitors,
    ApplyWallpaper(WallpaperAssignment),
    PauseMonitor { monitor_id: MonitorId },
    ResumeMonitor { monitor_id: MonitorId },
    StopMonitor { monitor_id: MonitorId },
    EnterSafeMode { reason: String },
    ExitSafeMode,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CoreEvent {
    Ready,
    MonitorsSnapshot(Vec<MonitorInfo>),
    RendererStateChanged {
        renderer_id: RendererId,
        state: RendererState,
    },
    Error {
        message: String,
    },
    SafeModeChanged {
        enabled: bool,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RendererCommand {
    Load(WallpaperAssignment),
    Play,
    Pause,
    Stop,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RendererEvent {
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
