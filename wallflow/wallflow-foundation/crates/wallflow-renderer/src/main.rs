use anyhow::Result;
use clap::Parser;
use std::io::{Read as StdRead, Write as StdWrite};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use uuid::Uuid;
use wallflow_common::{RendererId, WallpaperId};
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

    /// How many seconds the desktop-attached renderer window should stay alive
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
    /// Core ↔ Renderer communication.
    #[arg(long, default_value_t = false)]
    ipc_stdio: bool,
}

// ---------------------------------------------------------------------------
// Loaded static image state (replaces AppliedWallpaperState)
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
    // In --ipc-stdio mode, stdout is reserved for IPC frames.
    // All diagnostic output must go to stderr.
    let use_stderr_only = std::env::args().any(|a| a == "--ipc-stdio");

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

    if args.ipc_stdio {
        return run_ipc_stdio(renderer_id, args.heartbeat_interval_ms, args.timeout_secs);
    }

    if args.headless_heartbeat {
        return run_headless_heartbeat(renderer_id, args.heartbeat_interval_ms, args.timeout_secs);
    }

    if args.desktop_attach {
        return run_desktop_attach_renderer(args.timeout_secs);
    }

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
// IPC stdio mode
// ---------------------------------------------------------------------------

/// IPC stdio mode: the primary communication mode for Core ↔ Renderer.
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
    // Simple ISO 8601-like timestamp using std::time
    // Format: YYYY-MM-DDTHH:MM:SSZ (UTC approximation)
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = elapsed.as_secs();
    // Calculate date components from unix timestamp
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year/month/day from days since epoch
    let (year, month, day) = days_to_date(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(days_since_epoch: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
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
/// Sends decoded commands to the main loop via the channel.
fn stdin_reader_loop(cmd_tx: std::sync::mpsc::Sender<RendererCommand>) -> Result<()> {
    use wallflow_ipc::{IpcMessage, LENGTH_PREFIX_SIZE, MAX_FRAME_SIZE};

    let mut stdin = std::io::stdin().lock();
    loop {
        // Read the 4-byte length prefix
        let mut len_buf = [0u8; LENGTH_PREFIX_SIZE];
        match stdin.read_exact(&mut len_buf) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // stdin closed — exit the reader thread
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
                    // Receiver dropped — main loop has exited
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
    // SAFETY: We write the entire encoded frame to stdout in one call.
    // Stdout is locked for the duration of this write to prevent interleaving.
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&bytes)?;
    stdout.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Legacy headless heartbeat mode
// ---------------------------------------------------------------------------

/// Headless heartbeat mode for cloud testing (legacy).
///
/// Runs without any GUI or Win32 dependencies. Periodically emits a heartbeat
/// event (printed as JSON to stdout) and exits cleanly after the specified
/// timeout. This mode is suitable for testing the renderer supervisor on
/// Linux/CI without a real Windows desktop.
///
/// Prefer `--ipc-stdio` for typed IPC communication.
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

    // Emit the "Started" event
    let started_event = serde_json::json!({
        "type": "RendererEvent",
        "event": "Started",
        "renderer_id": renderer_id.0.to_string(),
        "timestamp_ms": start.elapsed().as_millis() as u64,
    });
    println!("{}", serde_json::to_string(&started_event)?);
    info!(?renderer_id, "headless renderer started");

    // Emit the "Ready" event
    let ready_event = serde_json::json!({
        "type": "RendererEvent",
        "event": "Ready",
        "renderer_id": renderer_id.0.to_string(),
        "timestamp_ms": start.elapsed().as_millis() as u64,
    });
    println!("{}", serde_json::to_string(&ready_event)?);
    info!(?renderer_id, "headless renderer ready");

    loop {
        // Check timeout
        if let Some(dur) = timeout {
            if start.elapsed() >= dur {
                info!(
                    ?renderer_id,
                    heartbeat_count, "headless renderer timeout reached, exiting"
                );
                break;
            }
        }

        // Sleep for the heartbeat interval
        std::thread::sleep(interval);

        heartbeat_count += 1;
        let uptime_ms = start.elapsed().as_millis() as u64;

        // Emit a heartbeat event as JSON on stdout
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

    // Emit the "Exited" event
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

    // SAFETY: RegisterClassW and CreateWindowExW are standard Win32 window
    // creation APIs. The WNDCLASSW struct is properly initialized.
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

        // SAFETY: GetClientRect and MoveWindow are standard Win32 APIs with valid params.
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

            // SAFETY: PeekMessageW with null HWND checks the thread message queue.
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
        // SAFETY: wParam is the HDC passed by WM_ERASEBKGND. GetClientRect
        // and FillRect are standard GDI APIs with valid parameters.
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
