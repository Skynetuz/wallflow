//! Persistent configuration for WallFlow.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;
use wallflow_common::{PerformanceProfile, WallpaperAssignment};

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config from {path}: {source}")]
    Read { path: String, source: io::Error },

    #[error("failed to write config to {path}: {source}")]
    Write { path: String, source: io::Error },

    #[error("failed to parse config from {path}: {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },

    #[error("failed to serialize config: {0}")]
    Serialize(serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub schema_version: u32,
    pub safe_mode: bool,
    pub autostart_enabled: bool,
    pub default_profile: PerformanceProfile,
    pub pause_on_fullscreen: bool,
    pub stop_on_battery: bool,
    pub assignments: Vec<WallpaperAssignment>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            safe_mode: false,
            autostart_enabled: false,
            default_profile: PerformanceProfile::Balanced,
            pause_on_fullscreen: true,
            stop_on_battery: true,
            assignments: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        serde_json::from_str(&raw).map_err(|source| ConfigError::Parse {
            path: path.display().to_string(),
            source,
        })
    }

    pub fn load_or_default(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        if path.exists() {
            Self::load(path)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
                path: parent.display().to_string(),
                source,
            })?;
        }

        let raw = serde_json::to_string_pretty(self).map_err(ConfigError::Serialize)?;
        fs::write(path, raw).map_err(|source| ConfigError::Write {
            path: path.display().to_string(),
            source,
        })
    }
}

pub fn default_config_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("WallFlow").join("config.json");
    }

    PathBuf::from(".").join("wallflow.config.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_uses_schema_version_1() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.schema_version, 1);
        assert!(cfg.pause_on_fullscreen);
    }

    #[test]
    fn config_roundtrip_json() {
        let cfg = AppConfig::default();
        let raw = serde_json::to_string(&cfg).expect("config serialization should succeed");
        let decoded: AppConfig =
            serde_json::from_str(&raw).expect("config deserialization should succeed");
        assert_eq!(cfg, decoded);
    }
}
