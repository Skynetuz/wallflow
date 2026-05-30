use anyhow::Result;
use clap::{Parser, Subcommand};
use wallflow_config::{default_config_path, AppConfig};
use wallflow_core::CoreApp;
use wallflow_monitor::platform_monitor_provider;

#[derive(Debug, Parser)]
#[command(name = "wallflow")]
#[command(about = "WallFlow developer CLI")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print detected monitors as JSON.
    Monitors,

    /// Print the default config path.
    ConfigPath,

    /// Create default config if it does not exist.
    InitConfig,

    /// Run a minimal core smoke check.
    Smoke,

    /// Probe the desktop window hierarchy and print diagnostics.
    DesktopProbe,

    /// Create a dummy renderer window, attach it behind desktop icons, then clean up.
    DesktopAttachSmoke,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    match args.command {
        Command::Monitors => {
            let provider = platform_monitor_provider();
            let monitors = provider.snapshot()?;
            println!("{}", serde_json::to_string_pretty(&monitors)?);
        }
        Command::ConfigPath => {
            println!("{}", default_config_path().display());
        }
        Command::InitConfig => {
            let path = default_config_path();
            let cfg = AppConfig::load_or_default(&path)?;
            cfg.save(&path)?;
            println!("wrote {}", path.display());
        }
        Command::Smoke => {
            let cfg = AppConfig::default();
            let app = CoreApp::new(cfg);
            let monitors = app.monitors()?;
            println!("core ok; monitors={}", monitors.len());
        }
        Command::DesktopProbe => {
            run_desktop_probe()?;
        }
        Command::DesktopAttachSmoke => {
            run_desktop_attach_smoke()?;
        }
    }

    Ok(())
}

fn run_desktop_probe() -> Result<()> {
    use wallflow_desktop::probe_desktop;

    let report = probe_desktop();
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");

    if !report.platform_supported {
        eprintln!(
            "Desktop integration is not supported on this platform ({}).",
            std::env::consts::OS
        );
    } else if report.attach_feasible {
        println!("Desktop attach is feasible.");
    } else {
        eprintln!("Desktop attach is NOT feasible.");
        if let Some(err) = &report.error {
            eprintln!("Reason: {err}");
        }
    }

    Ok(())
}

fn run_desktop_attach_smoke() -> Result<()> {
    run_desktop_attach_smoke_impl()
}

#[cfg(not(target_os = "windows"))]
fn run_desktop_attach_smoke_impl() -> Result<()> {
    eprintln!(
        "desktop-attach-smoke is only supported on Windows. Current platform: {}",
        std::env::consts::OS
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_desktop_attach_smoke_impl() -> Result<()> {
    use std::time::{Duration, Instant};
    use wallflow_desktop::{
        attach_window_to_desktop, detach_window_from_desktop, find_desktop_worker, probe_desktop,
        NativeWindowHandle,
    };
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{CreateSolidBrush, FillRect, GetDC, ReleaseDC};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    // Step 1: Probe first
    let probe = probe_desktop();
    println!("=== Desktop Probe ===");
    println!("{}", serde_json::to_string_pretty(&probe)?);

    if !probe.attach_feasible {
        eprintln!("Desktop attach is not feasible. Aborting smoke test.");
        if let Some(err) = &probe.error {
            eprintln!("Reason: {err}");
        }
        return Ok(());
    }

    // Step 2: Find desktop worker
    let worker = find_desktop_worker()?;
    println!("Found desktop worker: HWND {:#x}", worker.0 as usize);

    // Step 3: Create a dummy window
    let class_name = wide("WallFlowSmokeWndClass");
    let window_title = wide("WallFlow Smoke Test");

    // SAFETY: RegisterClassW and CreateWindowExW are standard Win32 window
    // creation APIs. The WNDCLASSW struct is properly initialized.
    unsafe {
        let module = GetModuleHandleW(PCWSTR::null())
            .map_err(|e| anyhow::anyhow!("GetModuleHandleW failed: {e}"))?;

        let wnd_class = WNDCLASSW {
            lpfnWndProc: Some(smoke_wnd_proc),
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

        // Create as a popup window — no title bar or borders. WS_POPUP | WS_VISIBLE
        // is the correct style for a wallpaper background window.
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
        println!("Created dummy renderer window: HWND {:#x}", hwnd.0 as usize);

        // Step 4: Attach to desktop
        let attach_report = attach_window_to_desktop(native_handle)?;
        println!(
            "=== Attach Result ===\n{}",
            serde_json::to_string_pretty(&attach_report)?
        );

        // After SetParent, reposition the window to fill the WorkerW client area.
        // SAFETY: GetClientRect and MoveWindow are standard Win32 APIs with valid parameters.
        let mut rect = RECT::default();
        let _ = GetClientRect(HWND(worker.0 as *mut _), &mut rect);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let _ = MoveWindow(hwnd, 0, 0, width, height, true);

        println!(
            "Window is now behind desktop icons (sized {}x{}).",
            width, height
        );

        // Step 5: Run a short message loop so the window can paint itself.
        // Using PeekMessageW + sleep instead of GetMessageW so we can check the timeout.
        let start = Instant::now();
        let mut msg = MSG::default();
        loop {
            if start.elapsed() >= Duration::from_secs(5) {
                break;
            }
            // SAFETY: PeekMessageW with null HWND checks the thread message queue.
            // PM_REMOVE removes the message after retrieval.
            let has_msg = PeekMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0, PM_REMOVE);
            if has_msg.as_bool() {
                if msg.message == WM_QUIT {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                // No messages — sleep briefly to avoid busy-waiting
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        // Step 6: Detach
        let detach_report = detach_window_from_desktop(native_handle)?;
        println!(
            "=== Detach Result ===\n{}",
            serde_json::to_string_pretty(&detach_report)?
        );

        // Step 7: Destroy the window
        let destroy_result = DestroyWindow(hwnd);
        if let Err(e) = destroy_result {
            eprintln!("Warning: DestroyWindow failed: {e}");
        }
        println!("Dummy window destroyed. Smoke test complete.");
    }

    Ok(())
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn smoke_wnd_proc(
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
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        // Paint a dark blue background (BGR: 0x804040) so the window is
        // clearly visible against the default desktop.
        let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00804040));
        let _ = FillRect(hdc, &rect, brush);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush);
        LRESULT(1) // background was erased
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
