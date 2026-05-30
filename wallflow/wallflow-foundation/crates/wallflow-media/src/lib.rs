//! Media backend abstraction for WallFlow.
//!
//! The MVP production target is Windows Media Foundation. This crate intentionally exposes
//! a small trait so the backend can be replaced or extended without rewriting Core.

use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("unsupported backend: {0}")]
    UnsupportedBackend(String),

    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),

    #[error("invalid source: {0}")]
    InvalidSource(PathBuf),

    #[error("backend error: {0}")]
    Backend(String),
}

pub trait VideoBackend: Send {
    fn load(&mut self, source: &Path) -> Result<(), MediaError>;
    fn play(&mut self) -> Result<(), MediaError>;
    fn pause(&mut self) -> Result<(), MediaError>;
    fn stop(&mut self) -> Result<(), MediaError>;
    fn set_looping(&mut self, looping: bool) -> Result<(), MediaError>;
    fn set_volume(&mut self, volume: f32) -> Result<(), MediaError>;
}

/// Non-rendering backend useful for testing Core state transitions.
pub struct NullVideoBackend {
    source: Option<PathBuf>,
    playing: bool,
    looping: bool,
    volume: f32,
}

impl Default for NullVideoBackend {
    fn default() -> Self {
        Self {
            source: None,
            playing: false,
            looping: true,
            volume: 0.0,
        }
    }
}

impl VideoBackend for NullVideoBackend {
    fn load(&mut self, source: &Path) -> Result<(), MediaError> {
        if source.as_os_str().is_empty() {
            return Err(MediaError::InvalidSource(source.to_path_buf()));
        }
        self.source = Some(source.to_path_buf());
        Ok(())
    }

    fn play(&mut self) -> Result<(), MediaError> {
        if self.source.is_none() {
            return Err(MediaError::Backend("no media source loaded".to_owned()));
        }
        self.playing = true;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), MediaError> {
        self.playing = false;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), MediaError> {
        self.playing = false;
        Ok(())
    }

    fn set_looping(&mut self, looping: bool) -> Result<(), MediaError> {
        self.looping = looping;
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), MediaError> {
        self.volume = volume.clamp(0.0, 1.0);
        Ok(())
    }
}

/// Creates the production video backend for the current platform.
pub fn platform_video_backend() -> Result<Box<dyn VideoBackend>, MediaError> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(
            windows_media_foundation::MediaFoundationBackend::new()?,
        ))
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(MediaError::UnsupportedPlatform(
            std::env::consts::OS.to_owned(),
        ))
    }
}

#[cfg(target_os = "windows")]
mod windows_media_foundation {
    use super::*;

    /// Placeholder for the Windows Media Foundation implementation.
    ///
    /// Next hardening step for the agent:
    /// - initialize COM/MF once per renderer process;
    /// - create Source Reader with hardware transforms enabled;
    /// - upload frames into renderer texture path;
    /// - handle Windows N Media Feature Pack absence explicitly.
    pub struct MediaFoundationBackend {
        inner: NullVideoBackend,
    }

    impl MediaFoundationBackend {
        pub fn new() -> Result<Self, MediaError> {
            Ok(Self {
                inner: NullVideoBackend::default(),
            })
        }
    }

    impl VideoBackend for MediaFoundationBackend {
        fn load(&mut self, source: &Path) -> Result<(), MediaError> {
            self.inner.load(source)
        }

        fn play(&mut self) -> Result<(), MediaError> {
            self.inner.play()
        }

        fn pause(&mut self) -> Result<(), MediaError> {
            self.inner.pause()
        }

        fn stop(&mut self) -> Result<(), MediaError> {
            self.inner.stop()
        }

        fn set_looping(&mut self, looping: bool) -> Result<(), MediaError> {
            self.inner.set_looping(looping)
        }

        fn set_volume(&mut self, volume: f32) -> Result<(), MediaError> {
            self.inner.set_volume(volume)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_backend_requires_source_before_play() {
        let mut backend = NullVideoBackend::default();
        assert!(backend.play().is_err());
    }

    #[test]
    fn null_backend_accepts_valid_source() {
        let mut backend = NullVideoBackend::default();
        backend.load(Path::new("demo.mp4")).unwrap();
        backend.play().unwrap();
        backend.pause().unwrap();
        backend.stop().unwrap();
    }
}
