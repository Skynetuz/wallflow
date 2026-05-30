use serde::{Deserialize, Serialize};

/// Stable monitor identity used by WallFlow.
///
/// The value is OS-specific but must remain stable across process restarts when possible.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MonitorId(pub String);

/// Top-left pixel position in the virtual desktop coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorPosition {
    pub x: i32,
    pub y: i32,
}

/// Pixel size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorSize {
    pub width: u32,
    pub height: u32,
}

/// Display metadata consumed by Core, UI and renderers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub id: MonitorId,
    pub name: String,
    pub is_primary: bool,
    pub position: MonitorPosition,
    pub size_px: MonitorSize,
    pub work_area_px: MonitorSize,
    pub scale_factor: f64,
    pub refresh_rate_millihz: Option<u32>,
}

impl MonitorInfo {
    /// Returns true when width and height are non-zero.
    pub fn has_valid_size(&self) -> bool {
        self.size_px.width > 0 && self.size_px.height > 0
    }
}
