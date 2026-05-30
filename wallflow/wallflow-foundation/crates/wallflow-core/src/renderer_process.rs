use std::path::PathBuf;
use std::process::Stdio;
use thiserror::Error;
use tokio::process::{Child, Command};
use wallflow_common::{MonitorId, RendererId, WallpaperKind};

#[derive(Debug, Error)]
pub enum RendererProcessError {
    #[error("failed to spawn renderer: {0}")]
    Spawn(std::io::Error),

    #[error("renderer process is not running")]
    NotRunning,

    #[error("renderer process already running (pid: {0})")]
    AlreadyRunning(u32),
}

/// Specification for launching a renderer subprocess.
#[derive(Debug, Clone)]
pub struct RendererLaunchSpec {
    pub renderer_exe: PathBuf,
    pub renderer_id: RendererId,
    pub monitor_id: MonitorId,
    pub wallpaper: WallpaperKind,
    /// Optional headless heartbeat mode for cloud testing.
    pub headless_heartbeat: bool,
    /// Heartbeat interval in milliseconds (only used if `headless_heartbeat` is true).
    pub heartbeat_interval_ms: u64,
    /// Timeout in seconds (0 = no timeout). Only used if `headless_heartbeat` is true.
    pub timeout_secs: u64,
}

impl RendererLaunchSpec {
    /// Create a minimal launch spec for headless heartbeat smoke testing.
    pub fn headless_smoke(renderer_exe: PathBuf) -> Self {
        Self {
            renderer_exe,
            renderer_id: RendererId::new(),
            monitor_id: MonitorId("primary".into()),
            wallpaper: WallpaperKind::None,
            headless_heartbeat: true,
            heartbeat_interval_ms: 500,
            timeout_secs: 5,
        }
    }
}

/// Manages a single renderer subprocess.
///
/// `RendererProcessManager` handles spawning and killing the renderer process.
/// It does not own the supervision logic — that lives in `RendererSupervisor`.
pub struct RendererProcessManager {
    child: Option<Child>,
}

impl RendererProcessManager {
    pub fn new() -> Self {
        Self { child: None }
    }

    /// Launch a renderer subprocess according to the given specification.
    ///
    /// Returns an error if a renderer is already running under this manager.
    pub async fn launch(&mut self, spec: &RendererLaunchSpec) -> Result<(), RendererProcessError> {
        // Check if we already have a running child.
        if let Some(ref child) = self.child {
            // Try to get the PID — if the child has already exited, we can reuse.
            if let Some(pid) = child.id() {
                return Err(RendererProcessError::AlreadyRunning(pid));
            }
        }

        let mut cmd = Command::new(&spec.renderer_exe);
        cmd.arg("--renderer-id")
            .arg(spec.renderer_id.0.to_string())
            .arg("--monitor")
            .arg(&spec.monitor_id.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if spec.headless_heartbeat {
            cmd.arg("--headless-heartbeat")
                .arg("--heartbeat-interval-ms")
                .arg(spec.heartbeat_interval_ms.to_string())
                .arg("--timeout-secs")
                .arg(spec.timeout_secs.to_string());
        } else {
            match &spec.wallpaper {
                WallpaperKind::None => {
                    cmd.arg("--wallpaper").arg("none");
                }
                WallpaperKind::StaticImage { path } => {
                    cmd.arg("--wallpaper")
                        .arg("static")
                        .arg("--source")
                        .arg(path);
                }
                WallpaperKind::Video {
                    path,
                    muted,
                    looping,
                } => {
                    cmd.arg("--wallpaper")
                        .arg("video")
                        .arg("--source")
                        .arg(path)
                        .arg("--muted")
                        .arg(muted.to_string())
                        .arg("--looping")
                        .arg(looping.to_string());
                }
                WallpaperKind::WebPackage { manifest_path } => {
                    cmd.arg("--wallpaper")
                        .arg("web")
                        .arg("--source")
                        .arg(manifest_path);
                }
            }
        }

        let child = cmd.spawn().map_err(RendererProcessError::Spawn)?;
        self.child = Some(child);
        Ok(())
    }

    /// Send SIGKILL (or the platform equivalent) to the renderer process.
    pub async fn kill(&mut self) -> Result<(), RendererProcessError> {
        match self.child.as_mut() {
            Some(child) => {
                child.kill().await.map_err(RendererProcessError::Spawn)?;
                self.child = None;
                Ok(())
            }
            None => Err(RendererProcessError::NotRunning),
        }
    }

    /// Check whether the renderer process is still running.
    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_status)) => false, // exited
                Ok(None) => true,           // still running
                Err(_) => false,            // error checking, assume dead
            }
        } else {
            false
        }
    }

    /// Get the process ID if the renderer is still running.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(|c| c.id())
    }
}

impl Default for RendererProcessManager {
    fn default() -> Self {
        Self::new()
    }
}
