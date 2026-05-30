use anyhow::Result;
use clap::Parser;
use std::io::{Read as StdRead, Write as StdWrite};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use uuid::Uuid;
use wallflow_common::{
    RenderSimLayoutReport, RenderSimReport, RendererId, RendererRuntimeMode, RendererRuntimeState,
    RendererViewport, WallpaperId, WindowRuntimeConfig,
};
use wallflow_ipc::{
    encode_to_bytes, AppliedWallpaperReport, ApplyWallpaperRequest, EventEnvelope,
    IpcImageMetadata, IpcMessage, RendererCommand, RendererEvent, StaticImageApplyReport,
    StaticImageLayoutReport, WallpaperApplyError, WallpaperPayload,
};
use wallflow_media::{platform_video_backend, NullVideoBackend, VideoBackend};

#[derive(Debug, Parser)]
#[command(name = "wallflow-renderer")]
#[command(about = "WallFlow isolated renderer process")]
struct Args {
    #[arg(long)]
    renderer_id: Option<Uuid>,

    #[arg(long, default_value = "primary")]
    monitor: String,

    #[arg(long, default_value = "none")]
    wallpaper: String,

    #[arg(long)]
    source: Option<PathBuf>,

    #[arg(long, default_value_t = true)]
    muted: bool,

    #[arg(long, default_value_t = true)]
    looping: bool,

    /// Create a dummy desktop-attached renderer window (Windows only).
    /// The window will be placed behind desktop icons.
    #[arg(long, default_value_t = false)]
    desktop_attach: bool,

    /// How many seconds the renderer window should stay alive
    /// before automatically exiting. 0 = run until Ctrl+C.
    #[arg(long, default_value_t = 0)]
    timeout_secs: u64,

    /// Run in headless heartbeat mode (no GUI, no Win32).
    /// Periodically prints a heartbeat event and exits after timeout.
    /// Suitable for cloud testing on Linux.
    /// This is the LEGACY mode — prefer --ipc-stdio for typed IPC.
    #[arg(long, default_value_t = false)]
    headless_heartbeat: bool,

    /// Heartbeat interval in milliseconds (used with --headless-heartbeat and --ipc-stdio).
    #[arg(long, default_value_t = 500)]
    heartbeat_interval_ms: u64,

    /// Run in IPC stdio mode: read commands from stdin, write events to stdout.
    /// All events use typed IPC frames (length-prefixed JSON).
    /// Diagnostic logs go to stderr only. This is the preferred mode for
    /// Core <-> Renderer communication.
    #[arg(long, default_value_t = false)]
    ipc_stdio: bool,

    /// Run in windowed static mode: opens a winit window for static wallpaper
    /// display. The window runs for the specified timeout or until closed.
    /// Requires a display server (Wayland/X11 on Linux, Desktop on Windows).
    #[arg(long, default_value_t = false)]
    windowed_static: bool,

    /// Run in headless render simulation mode: no window, synthetic viewport,
    /// produces a structured JSON report. Suitable for CI and cloud testing.
    #[arg(long, default_value_t = false)]
    headless_render_sim: bool,

    /// Window/viewport width in pixels (used with --windowed-static and --headless-render-sim).
    #[arg(long, default_value_t = 800)]
    width: u32,

    /// Window/viewport height in pixels (used with --windowed-static and --headless-render-sim).
    #[arg(long, default_value_t = 450)]
    height: u32,
}

// ---------------------------------------------------------------------------
// Loaded static image state
// ---------------------------------------------------------------------------

/// Tracks a loaded static image wallpaper with decoded metadata and layout.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LoadedStaticImageState {
    wallpaper_id: WallpaperId,
    metadata: wallflow_package::ImageMetadata,
    layout: wallflow_package::StaticImageLayout,
    applied_at: Instant,
}

fn main() -> Result<()> {
    // Determine if stderr-only logging is needed
    let use_stderr_only =
        std::env::args().any(|a| a == "--ipc-stdio" || a == "--headless-render-sim");

    if use_stderr_only {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
    }

    let args = Args::parse();
    let renderer_id = RendererId(args.renderer_id.unwrap_or_else(Uuid::new_v4));

    info!(?renderer_id, monitor = %args.monitor, wallpaper = %args.wallpaper, "renderer starting");

    // Dispatch to the correct runtime mode
    if args.ipc_stdio {
        return run_ipc_stdio(renderer_id, args.heartbeat_interval_ms, args.timeout_secs);
    }

    if args.headless_render_sim {
        return run_headless_render_sim(
            renderer_id,
            args.width,
            args.height,
            args.timeout_secs,
            args.source.as_deref(),
        );
    }

    if args.windowed_static {
        return run_windowed_static(
            renderer_id,
            args.width,
            args.height,
            args.timeout_secs,
            args.source.as_deref(),
        );
    }

    if args.headless_heartbeat {
        return run_headless_heartbeat(renderer_id, args.heartbeat_interval_ms, args.timeout_secs);
    }

    if args.desktop_attach {
        return run_desktop_attach_renderer(args.timeout_secs);
    }

    // Default: no specific mode — keep process alive for smoke testing
    match args.wallpaper.as_str() {
        "none" => {
            info!("no wallpaper requested; renderer stays alive for smoke testing");
        }
        "video" => {
            let Some(source) = args.source.as_deref() else {
                warn!("video wallpaper requested without --source");
                return Ok(());
            };

            let mut backend: Box<dyn VideoBackend> = match platform_video_backend() {
                Ok(backend) => backend,
                Err(err) => {
                    warn!(error = %err, "platform backend unavailable; falling back to null backend");
                    Box::new(NullVideoBackend::default())
                }
            };
            backend.load(source)?;
            backend.set_looping(args.looping)?;
            backend.set_volume(if args.muted { 0.0 } else { 1.0 })?;
            backend.play()?;
            info!(source = %source.display(), "video backend started");
        }
        "static" => {
            let Some(source) = args.source.as_deref() else {
                warn!("static wallpaper requested without --source");
                return Ok(());
            };
            info!(source = %source.display(), "static renderer path is reserved for the next winit/wgpu pass");
        }
        other => {
            warn!(wallpaper = other, "unsupported wallpaper kind");
        }
    }

    // MVP smoke behavior: keep process alive until manually stopped.
    std::thread::park();
    Ok(())
}

// ---------------------------------------------------------------------------
// Headless render simulation mode
// ---------------------------------------------------------------------------

/// Headless render simulation: no window, synthetic viewport, structured report.
///
/// This mode simulates the renderer lifecycle without requiring a display server.
/// It goes through the state transitions Starting → Ready → Running → ShuttingDown → Exited,
/// optionally applying a static wallpaper and calculating its layout for the given viewport.
/// The result is a structured JSON report printed to stdout.
fn run_headless_render_sim(
    renderer_id: RendererId,
    width: u32,
    height: u32,
    timeout_secs: u64,
    source: Option<&std::path::Path>,
) -> Result<()> {
    let start = Instant::now();
    let timeout = if timeout_secs > 0 {
        Duration::from_secs(timeout_secs)
    } else {
        Duration::from_secs(5) // Default 5s for sim mode
    };

    let viewport = RendererViewport::new(width, height);
    if !viewport.is_valid() {
        let report = RenderSimReport {
            mode: RendererRuntimeMode::HeadlessRenderSim,
            viewport,
            state_transitions: vec![RendererRuntimeState::Starting, RendererRuntimeState::Failed],
            wallpaper_applied: false,
            layout_report: None,
            total_sim_time_ms: start.elapsed().as_millis() as u64,
            exit_code: 1,
        };
        eprintln!("invalid viewport dimensions: {}x{}", width, height);
        let json = serde_json::to_string(&report)?;
        println!("{json}");
        return Ok(());
    }

    let mut state_transitions = vec![RendererRuntimeState::Starting];
    info!(?renderer_id, %viewport, "headless render sim starting");

    state_transitions.push(RendererRuntimeState::Ready);
    info!(?renderer_id, "headless render sim ready");

    state_transitions.push(RendererRuntimeState::Running);

    // Try to apply a static wallpaper if a source image path was provided
    let mut wallpaper_applied = false;
    let mut layout_report: Option<RenderSimLayoutReport> = None;

    if let Some(image_path) = source {
        match apply_static_wallpaper_for_sim(image_path, &viewport) {
            Ok((metadata, layout)) => {
                wallpaper_applied = true;
                layout_report = Some(RenderSimLayoutReport {
                    image_width: metadata.width,
                    image_height: metadata.height,
                    viewport_width: viewport.width,
                    viewport_height: viewport.height,
                    destination_x: layout.destination_rect.x,
                    destination_y: layout.destination_rect.y,
                    destination_width: layout.destination_rect.width,
                    destination_height: layout.destination_rect.height,
                });
                info!(
                    ?renderer_id,
                    image_path = %image_path.display(),
                    image_width = metadata.width,
                    image_height = metadata.height,
                    "wallpaper applied in render sim"
                );
            }
            Err(e) => {
                warn!(?renderer_id, error = %e, "failed to apply wallpaper in render sim");
            }
        }
    }

    // Simulate the running period
    while start.elapsed() < timeout {
        std::thread::sleep(Duration::from_millis(100));
    }

    state_transitions.push(RendererRuntimeState::ShuttingDown);
    state_transitions.push(RendererRuntimeState::Exited);

    let total_sim_time_ms = start.elapsed().as_millis() as u64;

    let report = RenderSimReport {
        mode: RendererRuntimeMode::HeadlessRenderSim,
        viewport,
        state_transitions,
        wallpaper_applied,
        layout_report,
        total_sim_time_ms,
        exit_code: 0,
    };

    let json = serde_json::to_string(&report)?;
    println!("{json}");

    info!(
        ?renderer_id,
        total_sim_time_ms, "headless render sim completed"
    );
    Ok(())
}

/// Apply a static wallpaper for render simulation (no IPC, direct return).
fn apply_static_wallpaper_for_sim(
    image_path: &std::path::Path,
    viewport: &RendererViewport,
) -> Result<(
    wallflow_package::ImageMetadata,
    wallflow_package::StaticImageLayout,
)> {
    let metadata = wallflow_package::load_image_metadata(image_path)?;

    let image_size = wallflow_package::ImageSize {
        width: metadata.width,
        height: metadata.height,
    };
    let vp = wallflow_package::Viewport {
        width: viewport.width,
        height: viewport.height,
    };

    let layout = wallflow_package::calculate_static_image_layout(
        image_size,
        vp,
        wallflow_common::FitMode::Cover,
        "#000000".into(),
    )?;

    Ok((metadata, layout))
}

// ---------------------------------------------------------------------------
// Windowed static mode (winit)
// ---------------------------------------------------------------------------

/// Application handler for the windowed static renderer using winit 0.30 ApplicationHandler API.
struct WindowedStaticApp {
    renderer_id: RendererId,
    viewport: RendererViewport,
    loaded_state: Option<LoadedStaticImageState>,
    timeout: Option<Duration>,
    start: Instant,
    window: Option<winit::window::Window>,
}

impl winit::application::ApplicationHandler for WindowedStaticApp {
    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            winit::event::WindowEvent::CloseRequested => {
                info!(?self.renderer_id, "window close requested, exiting");
                event_loop.exit();
            }
            winit::event::WindowEvent::Resized(physical_size) => {
                let new_w = physical_size.width;
                let new_h = physical_size.height;
                if new_w > 0 && new_h > 0 {
                    self.viewport = RendererViewport::new(new_w, new_h);

                    // Recalculate layout if wallpaper is applied
                    if let Some(ref state) = self.loaded_state {
                        let vp = wallflow_package::Viewport {
                            width: new_w,
                            height: new_h,
                        };
                        match wallflow_package::calculate_static_image_layout(
                            wallflow_package::ImageSize {
                                width: state.metadata.width,
                                height: state.metadata.height,
                            },
                            vp,
                            state.layout.fit,
                            state.layout.background.clone(),
                        ) {
                            Ok(new_layout) => {
                                info!(
                                    ?self.renderer_id,
                                    viewport = %self.viewport,
                                    dest_w = new_layout.destination_rect.width,
                                    dest_h = new_layout.destination_rect.height,
                                    "layout recalculated after resize"
                                );
                                let wallpaper_id = state.wallpaper_id;
                                let metadata = state.metadata.clone();
                                let applied_at = state.applied_at;
                                self.loaded_state = Some(LoadedStaticImageState {
                                    wallpaper_id,
                                    metadata,
                                    layout: new_layout,
                                    applied_at,
                                });
                            }
                            Err(e) => {
                                warn!(?self.renderer_id, error = %e, "failed to recalculate layout after resize");
                            }
                        }
                    }

                    info!(
                        ?self.renderer_id,
                        width = new_w,
                        height = new_h,
                        "window resized"
                    );
                }
            }
            winit::event::WindowEvent::Destroyed => {
                info!(?self.renderer_id, "window destroyed, exiting");
                event_loop.exit();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        // Check timeout
        if let Some(dur) = self.timeout {
            if self.start.elapsed() >= dur {
                info!(
                    ?self.renderer_id,
                    elapsed_ms = self.start.elapsed().as_millis() as u64,
                    "windowed static renderer timeout reached, exiting"
                );
                event_loop.exit();
            }
        }
        // Request a redraw so the window remains responsive
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        // Create the window when the event loop is ready (on first resume)
        if self.window.is_none() {
            let window_config = WindowRuntimeConfig::new(self.viewport.width, self.viewport.height);
            let window_attrs = winit::window::WindowAttributes::default()
                .with_title(&window_config.title)
                .with_inner_size(winit::dpi::LogicalSize::new(
                    window_config.width,
                    window_config.height,
                ));

            match event_loop.create_window(window_attrs) {
                Ok(w) => {
                    info!(?self.renderer_id, "winit window created");
                    self.window = Some(w);
                }
                Err(e) => {
                    warn!(?self.renderer_id, error = %e, "failed to create winit window");
                    event_loop.exit();
                }
            }
        }
    }
}

/// Windowed static mode: creates a winit window, optionally loads a static wallpaper,
/// runs the event loop until timeout or window close.
///
/// This mode requires a display server. On Linux this means Wayland or X11.
/// If no display server is available, it returns a graceful error without panicking.
fn run_windowed_static(
    renderer_id: RendererId,
    width: u32,
    height: u32,
    timeout_secs: u64,
    source: Option<&std::path::Path>,
) -> Result<()> {
    let start = Instant::now();
    let timeout = if timeout_secs > 0 {
        Some(Duration::from_secs(timeout_secs))
    } else {
        None
    };

    let viewport = RendererViewport::new(width, height);
    let mut loaded_state: Option<LoadedStaticImageState> = None;

    info!(?renderer_id, %viewport, "windowed static renderer starting");

    // Try to apply a static wallpaper if source was provided
    if let Some(image_path) = source {
        match apply_static_wallpaper_for_windowed(image_path, &viewport) {
            Ok(state) => {
                info!(
                    ?renderer_id,
                    image_path = %image_path.display(),
                    image_width = state.metadata.width,
                    image_height = state.metadata.height,
                    "wallpaper applied in windowed mode"
                );
                loaded_state = Some(state);
            }
            Err(e) => {
                warn!(?renderer_id, error = %e, "failed to apply wallpaper in windowed mode");
            }
        }
    }

    // Create the winit event loop
    let event_loop = match winit::event_loop::EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            // No display server available — return a clear error without panic
            let msg = format!("failed to create winit event loop (no display server?): {e}");
            warn!(?renderer_id, error = %msg, "cannot open window");
            eprintln!("{msg}");
            return Err(anyhow::anyhow!("{msg}"));
        }
    };

    let mut app = WindowedStaticApp {
        renderer_id,
        viewport,
        loaded_state,
        timeout,
        start,
        window: None,
    };

    event_loop.run_app(&mut app)?;

    info!(?renderer_id, "windowed static renderer exited");
    Ok(())
}

/// Apply a static wallpaper for windowed mode (no IPC, direct return).
fn apply_static_wallpaper_for_windowed(
    image_path: &std::path::Path,
    viewport: &RendererViewport,
) -> Result<LoadedStaticImageState> {
    let metadata = wallflow_package::load_image_metadata(image_path)?;

    let image_size = wallflow_package::ImageSize {
        width: metadata.width,
        height: metadata.height,
    };
    let vp = wallflow_package::Viewport {
        width: viewport.width,
        height: viewport.height,
    };

    let layout = wallflow_package::calculate_static_image_layout(
        image_size,
        vp,
        wallflow_common::FitMode::Cover,
        "#000000".into(),
    )?;

    Ok(LoadedStaticImageState {
        wallpaper_id: WallpaperId::new(),
        metadata,
        layout,
        applied_at: Instant::now(),
    })
}

// ---------------------------------------------------------------------------
// IPC stdio mode
// ---------------------------------------------------------------------------

/// IPC stdio mode: the primary communication mode for Core <-> Renderer.
///
/// - Reads `RendererCommand` frames from stdin on a background thread.
/// - Writes `RendererEvent` frames to stdout from the main thread.
/// - All diagnostic logs go to stderr.
/// - Heartbeats are sent periodically via IPC frames.
/// - Exits cleanly on `Shutdown` command or timeout.
fn run_ipc_stdio(renderer_id: RendererId, interval_ms: u64, timeout_secs: u64) -> Result<()> {
    let timeout = if timeout_secs > 0 {
        Some(Duration::from_secs(timeout_secs))
    } else {
        None
    };

    let start = Instant::now();
    let interval = Duration::from_millis(interval_ms);
    let mut is_paused = false;
    let mut heartbeat_count: u64 = 0;
    let mut loaded_state: Option<LoadedStaticImageState> = None;

    // Channel for receiving commands from the stdin reader thread
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<RendererCommand>();

    // Spawn a background thread to read commands from stdin
    std::thread::Builder::new()
        .name("ipc-stdin-reader".into())
        .spawn(move || {
            if let Err(e) = stdin_reader_loop(cmd_tx) {
                info!("stdin reader thread exited: {e}");
            }
        })?;

    // Send Started event
    send_ipc_event(RendererEvent::Started { renderer_id })?;
    info!(?renderer_id, "IPC stdio renderer started");

    // Send Ready event
    send_ipc_event(RendererEvent::Ready { renderer_id })?;
    info!(?renderer_id, "IPC stdio renderer ready");

    loop {
        // Check timeout
        if let Some(dur) = timeout {
            if start.elapsed() >= dur {
                info!(
                    ?renderer_id,
                    heartbeat_count, "IPC stdio renderer timeout reached, exiting"
                );
                send_ipc_event(RendererEvent::Exited {
                    renderer_id,
                    exit_code: Some(0),
                })?;
                break;
            }
        }

        // Check for commands (non-blocking with short timeout)
        match cmd_rx.recv_timeout(interval) {
            Ok(cmd) => {
                match cmd {
                    RendererCommand::Pause => {
                        is_paused = true;
                        send_ipc_event(RendererEvent::Paused { renderer_id })?;
                        info!(?renderer_id, "renderer paused via IPC command");
                    }
                    RendererCommand::Resume => {
                        is_paused = false;
                        send_ipc_event(RendererEvent::Resumed { renderer_id })?;
                        info!(?renderer_id, "renderer resumed via IPC command");
                    }
                    RendererCommand::Shutdown => {
                        send_ipc_event(RendererEvent::Exited {
                            renderer_id,
                            exit_code: Some(0),
                        })?;
                        info!(?renderer_id, "renderer shutting down via IPC command");
                        break;
                    }
                    RendererCommand::Start => {
                        // Already started; acknowledge
                        send_ipc_event(RendererEvent::Ready { renderer_id })?;
                    }
                    RendererCommand::Stop => {
                        send_ipc_event(RendererEvent::Exited {
                            renderer_id,
                            exit_code: Some(0),
                        })?;
                        info!(?renderer_id, "renderer stopped via IPC command");
                        break;
                    }
                    RendererCommand::ApplyWallpaper(request) => {
                        handle_apply_wallpaper(renderer_id, request, &mut loaded_state)?;
                    }
                    RendererCommand::SetMonitor { monitor_id } => {
                        info!(?renderer_id, ?monitor_id, "monitor changed via IPC command");
                        let uptime_ms = start.elapsed().as_millis() as u64;
                        send_ipc_event(RendererEvent::Heartbeat {
                            renderer_id,
                            uptime_ms,
                        })?;
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // No command received in this interval — send heartbeat if not paused
                if !is_paused {
                    heartbeat_count += 1;
                    let uptime_ms = start.elapsed().as_millis() as u64;
                    send_ipc_event(RendererEvent::Heartbeat {
                        renderer_id,
                        uptime_ms,
                    })?;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // Stdin reader thread exited (EOF on stdin) — keep running until timeout
                // but stop trying to read commands
                info!(?renderer_id, "stdin closed, no more commands expected");
                std::thread::sleep(interval);
            }
        }
    }

    Ok(())
}

/// Handle an ApplyWallpaper command: validate, decode metadata, calculate layout, respond.
fn handle_apply_wallpaper(
    renderer_id: RendererId,
    request: ApplyWallpaperRequest,
    loaded_state: &mut Option<LoadedStaticImageState>,
) -> Result<()> {
    let wallpaper_id = request.wallpaper_id;
    let monitor_id = request.target_monitor.clone();

    match &request.payload {
        WallpaperPayload::StaticImage(static_payload) => {
            // Validate image path is not empty
            if static_payload.image_path.trim().is_empty() {
                send_ipc_event(RendererEvent::WallpaperApplyFailed {
                    renderer_id,
                    wallpaper_id,
                    error: WallpaperApplyError::InvalidImagePath {
                        path: String::new(),
                    },
                })?;
                warn!(?renderer_id, "apply wallpaper rejected: empty image path");
                return Ok(());
            }

            let image_path = std::path::Path::new(&static_payload.image_path);

            // Decode image metadata using wallflow-package
            let metadata = match wallflow_package::load_image_metadata(image_path) {
                Ok(m) => m,
                Err(e) => {
                    let reason = format!("{e}");
                    warn!(
                        ?renderer_id,
                        ?wallpaper_id,
                        error = %reason,
                        "failed to decode image metadata"
                    );
                    send_ipc_event(RendererEvent::WallpaperApplyFailed {
                        renderer_id,
                        wallpaper_id,
                        error: WallpaperApplyError::ImageLoadFailed {
                            path: static_payload.image_path.clone(),
                            reason,
                        },
                    })?;
                    return Ok(());
                }
            };

            // Calculate layout using wallflow-package
            let image_size = wallflow_package::ImageSize {
                width: metadata.width,
                height: metadata.height,
            };
            // Default viewport: 1920x1080 (synthetic viewport for cloud testing)
            let viewport = wallflow_package::Viewport {
                width: 1920,
                height: 1080,
            };

            let layout = match wallflow_package::calculate_static_image_layout(
                image_size,
                viewport,
                static_payload.fit,
                static_payload.background.clone(),
            ) {
                Ok(l) => l,
                Err(e) => {
                    let reason = format!("{e}");
                    warn!(
                        ?renderer_id,
                        ?wallpaper_id,
                        error = %reason,
                        "failed to calculate layout"
                    );
                    send_ipc_event(RendererEvent::WallpaperApplyFailed {
                        renderer_id,
                        wallpaper_id,
                        error: WallpaperApplyError::Other { message: reason },
                    })?;
                    return Ok(());
                }
            };

            info!(
                ?renderer_id,
                ?wallpaper_id,
                image_path = %static_payload.image_path,
                image_width = metadata.width,
                image_height = metadata.height,
                ?static_payload.fit,
                "static wallpaper applied with decoded metadata and layout"
            );

            // Store the loaded state
            *loaded_state = Some(LoadedStaticImageState {
                wallpaper_id,
                metadata: metadata.clone(),
                layout: layout.clone(),
                applied_at: Instant::now(),
            });

            // Build the report
            let ipc_metadata: IpcImageMetadata = metadata.into();
            let ipc_layout: StaticImageLayoutReport = layout.into();

            let report = AppliedWallpaperReport {
                wallpaper_id,
                renderer_id,
                applied_at: Some(chrono_now_iso8601()),
                static_image: Some(StaticImageApplyReport {
                    image_metadata: ipc_metadata,
                    layout: ipc_layout,
                }),
            };

            send_ipc_event(RendererEvent::WallpaperApplied {
                renderer_id,
                wallpaper_id,
                monitor_id,
                report: Some(report),
            })?;
        }
    }

    Ok(())
}

/// Get current time as ISO 8601 string without depending on chrono.
fn chrono_now_iso8601() -> String {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = elapsed.as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_date(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(days_since_epoch: u64) -> (u64, u64, u64) {
    let z = days_since_epoch + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Background thread that reads IPC command frames from stdin.
fn stdin_reader_loop(cmd_tx: std::sync::mpsc::Sender<RendererCommand>) -> Result<()> {
    use wallflow_ipc::{IpcMessage, LENGTH_PREFIX_SIZE, MAX_FRAME_SIZE};

    let mut stdin = std::io::stdin().lock();
    loop {
        let mut len_buf = [0u8; LENGTH_PREFIX_SIZE];
        match stdin.read_exact(&mut len_buf) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => {
                warn!("error reading stdin length prefix: {e}");
                break;
            }
        }

        let len = u32::from_le_bytes(len_buf) as usize;
        if len == 0 || len > MAX_FRAME_SIZE {
            warn!(len, "invalid IPC frame length on stdin");
            continue;
        }

        let mut payload = vec![0u8; len];
        if let Err(e) = stdin.read_exact(&mut payload) {
            warn!("error reading stdin payload: {e}");
            break;
        }

        let msg: IpcMessage = match serde_json::from_slice(&payload) {
            Ok(m) => m,
            Err(e) => {
                warn!("error decoding IPC message from stdin: {e}");
                continue;
            }
        };

        match msg {
            IpcMessage::Command(env) => {
                if env.protocol_version != wallflow_ipc::PROTOCOL_VERSION {
                    warn!(
                        expected = wallflow_ipc::PROTOCOL_VERSION,
                        got = env.protocol_version,
                        "protocol version mismatch"
                    );
                    continue;
                }
                if cmd_tx.send(env.payload).is_err() {
                    break;
                }
            }
            _ => {
                warn!("expected Command message on stdin, got different message type");
            }
        }
    }

    Ok(())
}

/// Send a RendererEvent as an IPC frame to stdout.
fn send_ipc_event(event: RendererEvent) -> Result<()> {
    let msg = IpcMessage::Event(EventEnvelope::new(event));
    let bytes = encode_to_bytes(&msg)?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&bytes)?;
    stdout.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Legacy headless heartbeat mode
// ---------------------------------------------------------------------------

fn run_headless_heartbeat(
    renderer_id: RendererId,
    interval_ms: u64,
    timeout_secs: u64,
) -> Result<()> {
    let timeout = if timeout_secs > 0 {
        Some(Duration::from_secs(timeout_secs))
    } else {
        None
    };

    let start = Instant::now();
    let interval = Duration::from_millis(interval_ms);
    let mut heartbeat_count: u64 = 0;

    let started_event = serde_json::json!({
        "type": "RendererEvent",
        "event": "Started",
        "renderer_id": renderer_id.0.to_string(),
        "timestamp_ms": start.elapsed().as_millis() as u64,
    });
    println!("{}", serde_json::to_string(&started_event)?);
    info!(?renderer_id, "headless renderer started");

    let ready_event = serde_json::json!({
        "type": "RendererEvent",
        "event": "Ready",
        "renderer_id": renderer_id.0.to_string(),
        "timestamp_ms": start.elapsed().as_millis() as u64,
    });
    println!("{}", serde_json::to_string(&ready_event)?);
    info!(?renderer_id, "headless renderer ready");

    loop {
        if let Some(dur) = timeout {
            if start.elapsed() >= dur {
                info!(
                    ?renderer_id,
                    heartbeat_count, "headless renderer timeout reached, exiting"
                );
                break;
            }
        }

        std::thread::sleep(interval);

        heartbeat_count += 1;
        let uptime_ms = start.elapsed().as_millis() as u64;

        let heartbeat_event = serde_json::json!({
            "type": "RendererEvent",
            "event": "Heartbeat",
            "renderer_id": renderer_id.0.to_string(),
            "uptime_ms": uptime_ms,
            "heartbeat_count": heartbeat_count,
            "timestamp_ms": start.elapsed().as_millis() as u64,
        });
        println!("{}", serde_json::to_string(&heartbeat_event)?);
    }

    let exited_event = serde_json::json!({
        "type": "RendererEvent",
        "event": "Exited",
        "renderer_id": renderer_id.0.to_string(),
        "exit_code": 0,
        "timestamp_ms": start.elapsed().as_millis() as u64,
    });
    println!("{}", serde_json::to_string(&exited_event)?);

    Ok(())
}

// ---------------------------------------------------------------------------
// Desktop attach mode (Windows only)
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "windows"))]
fn run_desktop_attach_renderer(_timeout_secs: u64) -> Result<()> {
    warn!("--desktop-attach is only supported on Windows");
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_desktop_attach_renderer(timeout_secs: u64) -> Result<()> {
    use wallflow_desktop::{
        attach_window_to_desktop, detach_window_from_desktop, probe_desktop, NativeWindowHandle,
    };
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{CreateSolidBrush, FillRect};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    // REQUIRES_REAL_WINDOWS_VALIDATION
    let probe = probe_desktop();
    info!(
        platform_supported = probe.platform_supported,
        progman = probe.progman_hwnd,
        workerw = probe.workerw_hwnd,
        attach_feasible = probe.attach_feasible,
        "desktop probe result"
    );

    if !probe.attach_feasible {
        let err = probe.error.as_deref().unwrap_or("unknown reason");
        anyhow::bail!("desktop attach not feasible: {err}");
    }

    let class_name = wide("WallFlowRendererClass");
    let window_title = wide("WallFlow Renderer");

    unsafe {
        let module = GetModuleHandleW(PCWSTR::null())
            .map_err(|e| anyhow::anyhow!("GetModuleHandleW failed: {e}"))?;

        let wnd_class = WNDCLASSW {
            lpfnWndProc: Some(renderer_wnd_proc),
            hInstance: module,
            hbrBackground: COLOR_WINDOW,
            lpszClassName: PCWSTR(class_name.0.as_ptr()),
            ..Default::default()
        };

        let atom = RegisterClassW(&wnd_class);
        if atom == 0 {
            let err = windows::Win32::Foundation::GetLastError();
            anyhow::bail!("RegisterClassW failed (error: {})", err.0);
        }

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name.0.as_ptr()),
            PCWSTR(window_title.0.as_ptr()),
            WINDOW_STYLE::WS_POPUP | WINDOW_STYLE::WS_VISIBLE,
            0,
            0,
            screen_w,
            screen_h,
            None,
            None,
            module,
            None,
        )
        .map_err(|e| anyhow::anyhow!("CreateWindowExW failed: {e}"))?;

        let native_handle = NativeWindowHandle(hwnd.0 as isize);
        info!(hwnd = hwnd.0 as isize, "renderer window created");

        let attach_report = attach_window_to_desktop(native_handle)?;
        info!(
            worker = attach_report.worker_handle.0,
            previous_parent = attach_report.previous_parent_hwnd,
            "renderer window attached to desktop"
        );

        let mut rect = RECT::default();
        let _ = GetClientRect(HWND(attach_report.worker_handle.0 as *mut _), &mut rect);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let _ = MoveWindow(hwnd, 0, 0, width, height, true);

        println!(
            "Renderer attached to desktop (WorkerW HWND: {:#x}, sized {}x{}). \
             Window is behind desktop icons.",
            attach_report.worker_handle.0 as usize, width, height
        );

        let start = Instant::now();
        let timeout = if timeout_secs > 0 {
            Some(Duration::from_secs(timeout_secs))
        } else {
            None
        };

        let mut msg = MSG::default();
        loop {
            if let Some(dur) = timeout {
                if start.elapsed() >= dur {
                    info!("timeout reached, exiting");
                    break;
                }
            }

            let has_msg = PeekMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0, PM_REMOVE);
            if has_msg.as_bool() {
                if msg.message == WM_QUIT {
                    info!("WM_QUIT received, exiting message loop");
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        let detach_report = detach_window_from_desktop(native_handle)?;
        info!(
            success = detach_report.success,
            "renderer window detached from desktop"
        );

        let _ = DestroyWindow(hwnd);
        info!("renderer window destroyed");
    }

    Ok(())
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn renderer_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_DESTROY {
        PostQuitMessage(0);
        LRESULT(0)
    } else if msg == WM_ERASEBKGND {
        let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _);
        let mut rect = windows::Win32::Foundation::RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00804040));
        let _ = FillRect(hdc, &rect, brush);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush);
        LRESULT(1)
    } else {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

#[cfg(target_os = "windows")]
struct WideString(Vec<u16>);

#[cfg(target_os = "windows")]
fn wide(value: &str) -> WideString {
    WideString(value.encode_utf16().chain(std::iter::once(0)).collect())
}
