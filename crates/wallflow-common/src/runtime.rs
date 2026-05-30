//! Renderer runtime abstraction types.
//!
//! These types describe how the renderer process runs — in which mode,
//! what state it is in, and what viewport it targets. They are pure data
//! types with no I/O or platform dependencies.

use serde::{Deserialize, Serialize};

/// How the renderer process runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RendererRuntimeMode {
    /// Headless IPC mode: no window, communicates over stdin/stdout.
    HeadlessIpc,
    /// Headless render simulation: no window, synthetic viewport, produces
    /// a structured report. Suitable for CI and cloud testing.
    HeadlessRenderSim,
    /// Windowed static image mode: opens a winit window, displays a static
    /// wallpaper image (no GPU pipeline yet). Requires a display server.
    WindowedStatic,
}

impl std::fmt::Display for RendererRuntimeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RendererRuntimeMode::HeadlessIpc => write!(f, "headless-ipc"),
            RendererRuntimeMode::HeadlessRenderSim => write!(f, "headless-render-sim"),
            RendererRuntimeMode::WindowedStatic => write!(f, "windowed-static"),
        }
    }
}

/// Lifecycle state of the renderer runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RendererRuntimeState {
    /// Runtime is initializing (loading config, setting up).
    Starting,
    /// Runtime is ready and waiting for commands or content.
    Ready,
    /// Runtime is actively running (wallpaper displayed or simulating).
    Running,
    /// Runtime is paused (content visible but not animating).
    Paused,
    /// Runtime is shutting down gracefully.
    ShuttingDown,
    /// Runtime has exited.
    Exited,
    /// Runtime has failed.
    Failed,
}

impl RendererRuntimeState {
    /// Returns true if the runtime is in a terminal state.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Exited | Self::Failed)
    }

    /// Returns true if the runtime is actively alive.
    pub fn is_alive(self) -> bool {
        matches!(self, Self::Ready | Self::Running | Self::Paused)
    }
}

impl std::fmt::Display for RendererRuntimeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RendererRuntimeState::Starting => write!(f, "starting"),
            RendererRuntimeState::Ready => write!(f, "ready"),
            RendererRuntimeState::Running => write!(f, "running"),
            RendererRuntimeState::Paused => write!(f, "paused"),
            RendererRuntimeState::ShuttingDown => write!(f, "shutting-down"),
            RendererRuntimeState::Exited => write!(f, "exited"),
            RendererRuntimeState::Failed => write!(f, "failed"),
        }
    }
}

/// Viewport dimensions tracked by the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererViewport {
    /// Viewport width in logical pixels.
    pub width: u32,
    /// Viewport height in logical pixels.
    pub height: u32,
    /// Optional DPI scale factor (1.0 = standard, 2.0 = HiDPI).
    pub scale_factor: Option<u32>,
}

impl RendererViewport {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            scale_factor: None,
        }
    }

    pub fn with_scale_factor(mut self, factor: u32) -> Self {
        self.scale_factor = Some(factor);
        self
    }

    /// Returns true if the viewport has valid (non-zero) dimensions.
    pub fn is_valid(self) -> bool {
        self.width > 0 && self.height > 0
    }
}

impl std::fmt::Display for RendererViewport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.scale_factor {
            Some(sf) => write!(f, "{}x{}@{}x", self.width, self.height, sf),
            None => write!(f, "{}x{}", self.width, self.height),
        }
    }
}

/// Configuration for the windowed renderer runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowRuntimeConfig {
    /// Window width in logical pixels.
    pub width: u32,
    /// Window height in logical pixels.
    pub height: u32,
    /// Window title.
    pub title: String,
    /// Whether the window should be visible.
    pub visible: bool,
    /// Whether the window should be borderless (decorated = false).
    #[serde(default = "default_borderless")]
    pub borderless: bool,
}

fn default_borderless() -> bool {
    false
}

impl Default for WindowRuntimeConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            title: "WallFlow Renderer".into(),
            visible: true,
            borderless: false,
        }
    }
}

impl WindowRuntimeConfig {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ..Default::default()
        }
    }
}

/// Result of a headless render simulation run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderSimReport {
    /// The runtime mode used (should always be HeadlessRenderSim).
    pub mode: RendererRuntimeMode,
    /// The viewport used for the simulation.
    pub viewport: RendererViewport,
    /// The runtime state transitions that occurred.
    pub state_transitions: Vec<RendererRuntimeState>,
    /// Whether a wallpaper was applied during simulation.
    pub wallpaper_applied: bool,
    /// Layout report, if a wallpaper was applied.
    pub layout_report: Option<RenderSimLayoutReport>,
    /// Total simulation time in milliseconds.
    pub total_sim_time_ms: u64,
    /// Exit code (0 = success).
    pub exit_code: i32,
}

/// Layout report from a render simulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderSimLayoutReport {
    /// Image width in pixels.
    pub image_width: u32,
    /// Image height in pixels.
    pub image_height: u32,
    /// Viewport width used for layout.
    pub viewport_width: u32,
    /// Viewport height used for layout.
    pub viewport_height: u32,
    /// Destination rectangle X.
    pub destination_x: f64,
    /// Destination rectangle Y.
    pub destination_y: f64,
    /// Destination rectangle width.
    pub destination_width: f64,
    /// Destination rectangle height.
    pub destination_height: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_mode_display() {
        assert_eq!(RendererRuntimeMode::HeadlessIpc.to_string(), "headless-ipc");
        assert_eq!(
            RendererRuntimeMode::HeadlessRenderSim.to_string(),
            "headless-render-sim"
        );
        assert_eq!(
            RendererRuntimeMode::WindowedStatic.to_string(),
            "windowed-static"
        );
    }

    #[test]
    fn runtime_state_transitions() {
        // Starting is not yet alive (still initializing) but not terminal
        assert!(!RendererRuntimeState::Starting.is_alive());
        assert!(!RendererRuntimeState::Starting.is_terminal());

        assert!(RendererRuntimeState::Running.is_alive());
        assert!(RendererRuntimeState::Paused.is_alive());
        assert!(RendererRuntimeState::Ready.is_alive());

        assert!(RendererRuntimeState::Exited.is_terminal());
        assert!(RendererRuntimeState::Failed.is_terminal());
        assert!(!RendererRuntimeState::Running.is_terminal());
        assert!(!RendererRuntimeState::ShuttingDown.is_terminal());
    }

    #[test]
    fn viewport_validity() {
        let valid = RendererViewport::new(1920, 1080);
        assert!(valid.is_valid());

        let zero_width = RendererViewport::new(0, 1080);
        assert!(!zero_width.is_valid());

        let zero_height = RendererViewport::new(1920, 0);
        assert!(!zero_height.is_valid());
    }

    #[test]
    fn viewport_display() {
        let vp = RendererViewport::new(1920, 1080);
        assert_eq!(vp.to_string(), "1920x1080");

        let vp_scaled = RendererViewport::new(1920, 1080).with_scale_factor(2);
        assert_eq!(vp_scaled.to_string(), "1920x1080@2x");
    }

    #[test]
    fn window_config_default() {
        let config = WindowRuntimeConfig::default();
        assert_eq!(config.width, 1920);
        assert_eq!(config.height, 1080);
        assert_eq!(config.title, "WallFlow Renderer");
        assert!(config.visible);
        assert!(!config.borderless);
    }

    #[test]
    fn window_config_new() {
        let config = WindowRuntimeConfig::new(800, 450);
        assert_eq!(config.width, 800);
        assert_eq!(config.height, 450);
        assert_eq!(config.title, "WallFlow Renderer");
    }

    #[test]
    fn render_sim_report_serialization() {
        let report = RenderSimReport {
            mode: RendererRuntimeMode::HeadlessRenderSim,
            viewport: RendererViewport::new(800, 450),
            state_transitions: vec![
                RendererRuntimeState::Starting,
                RendererRuntimeState::Ready,
                RendererRuntimeState::Running,
                RendererRuntimeState::ShuttingDown,
                RendererRuntimeState::Exited,
            ],
            wallpaper_applied: true,
            layout_report: Some(RenderSimLayoutReport {
                image_width: 2,
                image_height: 2,
                viewport_width: 800,
                viewport_height: 450,
                destination_x: 0.0,
                destination_y: -337.5,
                destination_width: 800.0,
                destination_height: 1125.0,
            }),
            total_sim_time_ms: 2000,
            exit_code: 0,
        };
        let json = serde_json::to_string(&report).expect("serialize");
        let decoded: RenderSimReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report, decoded);
    }

    #[test]
    fn runtime_state_display() {
        assert_eq!(RendererRuntimeState::Starting.to_string(), "starting");
        assert_eq!(RendererRuntimeState::Running.to_string(), "running");
        assert_eq!(
            RendererRuntimeState::ShuttingDown.to_string(),
            "shutting-down"
        );
        assert_eq!(RendererRuntimeState::Failed.to_string(), "failed");
    }
}
