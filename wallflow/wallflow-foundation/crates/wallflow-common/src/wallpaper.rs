use crate::monitor::MonitorId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Unique wallpaper identity inside local library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WallpaperId(pub Uuid);

impl WallpaperId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for WallpaperId {
    fn default() -> Self {
        Self::new()
    }
}

/// Supported wallpaper kinds for the MVP and near-future versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WallpaperKind {
    None,
    StaticImage {
        path: PathBuf,
    },
    Video {
        path: PathBuf,
        muted: bool,
        looping: bool,
    },
    WebPackage {
        manifest_path: PathBuf,
    },
}

/// Resource behavior profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PerformanceProfile {
    BatterySaver,
    #[default]
    Balanced,
    Quality,
}

/// Assignment of a wallpaper to one monitor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallpaperAssignment {
    pub monitor_id: MonitorId,
    pub wallpaper_id: WallpaperId,
    pub kind: WallpaperKind,
    pub profile: PerformanceProfile,
}
