use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique renderer process identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RendererId(pub Uuid);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RendererState {
    Starting,
    Running,
    Paused,
    Stopping,
    Stopped,
    Crashed,
}
