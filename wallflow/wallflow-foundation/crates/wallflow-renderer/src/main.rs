use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::{info, warn};
use uuid::Uuid;
use wallflow_common::RendererId;
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
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let renderer_id = RendererId(args.renderer_id.unwrap_or_else(Uuid::new_v4));

    info!(?renderer_id, monitor = %args.monitor, wallpaper = %args.wallpaper, "renderer starting");

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
    // The next agent task should replace this with the winit event loop and IPC reader.
    std::thread::park();
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn run_desktop_attach_renderer(_timeout_secs: u64) -> Result<()> {
    warn!("--desktop-attach is only supported on Windows");
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_desktop_attach_renderer(timeout_secs: u64) -> Result<()> {
    use std::time::{Duration, Instant};
    use wallflow_desktop::{
        attach_window_to_desktop, detach_window_from_desktop, probe_desktop, NativeWindowHandle,
    };
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    // Step 1: Probe desktop
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

    // Step 2: Register window class
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

        // Step 3: Create the renderer window
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name.0.as_ptr()),
            PCWSTR(window_title.0.as_ptr()),
            WINDOW_STYLE::WS_OVERLAPPEDWINDOW,
            0,
            0,
            800,
            600,
            None,
            None,
            module,
            None,
        )
        .map_err(|e| anyhow::anyhow!("CreateWindowExW failed: {e}"))?;

        let native_handle = NativeWindowHandle(hwnd.0 as isize);
        info!(hwnd = hwnd.0 as isize, "renderer window created");

        // Step 4: Show and attach
        ShowWindow(hwnd, SW_SHOW);
        info!("renderer window shown");

        let attach_report = attach_window_to_desktop(native_handle)?;
        info!(
            worker = attach_report.worker_handle.0,
            previous_parent = attach_report.previous_parent_hwnd,
            "renderer window attached to desktop"
        );
        println!(
            "Renderer attached to desktop (WorkerW HWND: {:#x}). \
             Window is behind desktop icons.",
            attach_report.worker_handle.0 as usize
        );

        // Step 5: Run message loop with optional timeout
        let start = Instant::now();
        let timeout = if timeout_secs > 0 {
            Some(Duration::from_secs(timeout_secs))
        } else {
            None
        };

        let mut msg = MSG::default();
        loop {
            // Check timeout
            if let Some(dur) = timeout {
                if start.elapsed() >= dur {
                    info!("timeout reached, exiting");
                    break;
                }
            }

            // SAFETY: GetMessageW with null HWND retrieves messages for all windows
            // on the calling thread. The MSG buffer is stack-allocated and valid.
            let ret = GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0);
            if !ret.as_bool() {
                // WM_QUIT received
                info!("WM_QUIT received, exiting message loop");
                break;
            }

            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Step 6: Detach and cleanup
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
