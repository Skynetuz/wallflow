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
}

#[derive(Debug, Clone)]
pub struct RendererLaunchSpec {
    pub renderer_exe: PathBuf,
    pub renderer_id: RendererId,
    pub monitor_id: MonitorId,
    pub wallpaper: WallpaperKind,
}

pub struct RendererProcessManager {
    child: Option<Child>,
}

impl RendererProcessManager {
    pub fn new() -> Self {
        Self { child: None }
    }

    pub async fn launch(&mut self, spec: &RendererLaunchSpec) -> Result<(), RendererProcessError> {
        let mut cmd = Command::new(&spec.renderer_exe);
        cmd.arg("--renderer-id")
            .arg(spec.renderer_id.0.to_string())
            .arg("--monitor")
            .arg(&spec.monitor_id.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

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

        let child = cmd.spawn().map_err(RendererProcessError::Spawn)?;
        self.child = Some(child);
        Ok(())
    }

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
}

impl Default for RendererProcessManager {
    fn default() -> Self {
        Self::new()
    }
}
