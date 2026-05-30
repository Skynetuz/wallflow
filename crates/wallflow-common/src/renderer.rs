use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique renderer process identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RendererId(pub Uuid);

impl fmt::Display for RendererId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl RendererId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for RendererId {
    fn default() -> Self {
        Self::new()
    }
}

/// Group of renderers sharing one timeline, useful for future span wallpapers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RendererGroupId(pub Uuid);

impl RendererGroupId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for RendererGroupId {
    fn default() -> Self {
        Self::new()
    }
}

/// Lifecycle state of a single renderer process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RendererState {
    Starting,
    Running,
    Paused,
    Stopping,
    Stopped,
    Crashed,
}

impl RendererState {
    /// Returns `true` if the renderer is in a terminal state (stopped or crashed).
    pub fn is_terminal(self) -> bool {
        matches!(self, RendererState::Stopped | RendererState::Crashed)
    }

    /// Returns `true` if the renderer is actively running (not stopped, crashed, or starting).
    pub fn is_alive(self) -> bool {
        matches!(self, RendererState::Running | RendererState::Paused)
    }
}

/// Health status of a renderer process, based on heartbeat recency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RendererHealth {
    /// Renderer is sending heartbeats within the expected interval.
    Healthy,
    /// No heartbeat received for a while, but within the restart window.
    Stale,
    /// Renderer has exceeded the maximum restart count and needs safe mode.
    Unhealthy,
}

/// Restart policy for a renderer process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RendererRestartPolicy {
    /// Never restart the renderer automatically.
    Never,
    /// Restart up to `max_attempts` times within the policy window.
    Limited { max_attempts: u32 },
    /// Always restart the renderer on failure.
    Always,
}

impl Default for RendererRestartPolicy {
    fn default() -> Self {
        RendererRestartPolicy::Limited { max_attempts: 3 }
    }
}

/// Assignment of a renderer to a specific monitor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererAssignment {
    pub renderer_id: RendererId,
    pub monitor_id: crate::monitor::MonitorId,
}

impl RendererAssignment {
    pub fn new(renderer_id: RendererId, monitor_id: crate::monitor::MonitorId) -> Self {
        Self {
            renderer_id,
            monitor_id,
        }
    }
}
