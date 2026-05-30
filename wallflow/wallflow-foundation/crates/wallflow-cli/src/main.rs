use anyhow::Result;
use clap::{Parser, Subcommand};
use std::time::{Duration, Instant};
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

    /// Launch a headless renderer, monitor its process, and report results.
    /// This is the LEGACY cloud-testable supervisor smoke test (uses stdout text).
    SupervisorSmoke {
        /// How many seconds to let the renderer run before declaring success.
        #[arg(long, default_value_t = 5)]
        timeout_secs: u64,

        /// Renderer heartbeat interval in milliseconds.
        #[arg(long, default_value_t = 500)]
        heartbeat_interval_ms: u64,
    },

    /// Launch a renderer in --ipc-stdio mode, exchange typed IPC frames,
    /// exercise the full command/event lifecycle, and report results.
    /// This is the PRIMARY cloud-testable IPC integration test.
    IpcSupervisorSmoke {
        /// How many seconds to let the renderer run before declaring success.
        #[arg(long, default_value_t = 10)]
        timeout_secs: u64,

        /// Renderer heartbeat interval in milliseconds.
        #[arg(long, default_value_t = 500)]
        heartbeat_interval_ms: u64,
    },
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
        Command::SupervisorSmoke {
            timeout_secs,
            heartbeat_interval_ms,
        } => {
            run_supervisor_smoke(timeout_secs, heartbeat_interval_ms)?;
        }
        Command::IpcSupervisorSmoke {
            timeout_secs,
            heartbeat_interval_ms,
        } => {
            run_ipc_supervisor_smoke(timeout_secs, heartbeat_interval_ms)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Desktop probe
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Desktop attach smoke
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Supervisor smoke
// ---------------------------------------------------------------------------

/// Supervisor smoke test: launches a headless renderer, monitors it, and
/// prints a structured report. This is the cloud-testable integration test
/// that validates the renderer lifecycle without requiring a real Windows
/// desktop or GUI.
///
/// The test works by:
/// 1. Finding the renderer executable in the current target directory.
/// 2. Spawning it with `--headless-heartbeat` mode.
/// 3. Reading stdout lines as heartbeat events.
/// 4. Waiting for the renderer to exit (or timeout).
/// 5. Printing a structured JSON report.
fn run_supervisor_smoke(timeout_secs: u64, heartbeat_interval_ms: u64) -> Result<()> {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;
    use tokio::time;

    // Build a minimal runtime for the async smoke test
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let start = Instant::now();

        // Find the renderer executable
        let renderer_exe = find_renderer_exe()?;

        println!("=== Supervisor Smoke Test ===");
        println!("Renderer exe: {}", renderer_exe.display());
        println!(
            "Timeout: {}s, Heartbeat interval: {}ms",
            timeout_secs, heartbeat_interval_ms
        );

        // Spawn the renderer process
        let mut child = Command::new(&renderer_exe)
            .arg("--headless-heartbeat")
            .arg("--heartbeat-interval-ms")
            .arg(heartbeat_interval_ms.to_string())
            .arg("--timeout-secs")
            .arg(timeout_secs.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn renderer: {e}"))?;

        let pid = child.id();
        println!("Renderer spawned (PID: {:?})", pid);

        // Read stdout lines as heartbeat events
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture renderer stdout"))?;
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        let mut heartbeat_count: u64 = 0;
        let mut started_received = false;
        let mut ready_received = false;
        let mut exited_received = false;
        let mut first_heartbeat_at: Option<Duration> = None;
        let mut last_heartbeat_at: Option<Duration> = None;

        let deadline = time::sleep(Duration::from_secs(timeout_secs + 10));
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                _ = &mut deadline => {
                    eprintln!("Supervisor smoke timed out waiting for renderer output");
                    let _ = child.kill().await;
                    break;
                }
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            // Try to parse as JSON event
                            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                                let event_type = event.get("event").and_then(|v| v.as_str()).unwrap_or("unknown");
                                let elapsed = start.elapsed();

                                match event_type {
                                    "Started" => {
                                        started_received = true;
                                        println!("  [{:.1}s] Received Started event", elapsed.as_secs_f64());
                                    }
                                    "Ready" => {
                                        ready_received = true;
                                        println!("  [{:.1}s] Received Ready event", elapsed.as_secs_f64());
                                    }
                                    "Heartbeat" => {
                                        heartbeat_count += 1;
                                        if first_heartbeat_at.is_none() {
                                            first_heartbeat_at = Some(elapsed);
                                        }
                                        last_heartbeat_at = Some(elapsed);
                                        if heartbeat_count <= 3 || heartbeat_count % 5 == 0 {
                                            println!(
                                                "  [{:.1}s] Heartbeat #{} (uptime: {}ms)",
                                                elapsed.as_secs_f64(),
                                                heartbeat_count,
                                                event.get("uptime_ms").and_then(|v| v.as_u64()).unwrap_or(0)
                                            );
                                        }
                                    }
                                    "Exited" => {
                                        exited_received = true;
                                        println!(
                                            "  [{:.1}s] Received Exited event (exit_code: {:?})",
                                            elapsed.as_secs_f64(),
                                            event.get("exit_code")
                                        );
                                    }
                                    other => {
                                        println!("  [{:.1}s] Unknown event: {}", elapsed.as_secs_f64(), other);
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            // EOF — renderer closed stdout
                            break;
                        }
                        Err(e) => {
                            eprintln!("Error reading renderer stdout: {e}");
                            break;
                        }
                    }
                }
            }
        }

        // Wait for the process to exit
        let exit_status = match time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(status)) => Some(status),
            Ok(Err(e)) => {
                eprintln!("Error waiting for renderer: {e}");
                None
            }
            Err(_) => {
                eprintln!("Renderer did not exit in time, killing");
                let _ = child.kill().await;
                None
            }
        };

        let total_elapsed = start.elapsed();
        let exit_code = exit_status.as_ref().and_then(|s| s.code());
        let success = exit_code == Some(0)
            && started_received
            && ready_received
            && heartbeat_count > 0
            && exited_received;

        // Build the structured report
        let report = serde_json::json!({
            "test": "supervisor-smoke",
            "success": success,
            "total_elapsed_ms": total_elapsed.as_millis() as u64,
            "renderer_pid": pid,
            "exit_code": exit_code,
            "events": {
                "started_received": started_received,
                "ready_received": ready_received,
                "heartbeat_count": heartbeat_count,
                "exited_received": exited_received,
                "first_heartbeat_at_ms": first_heartbeat_at.map(|d| d.as_millis() as u64),
                "last_heartbeat_at_ms": last_heartbeat_at.map(|d| d.as_millis() as u64),
            },
            "config": {
                "timeout_secs": timeout_secs,
                "heartbeat_interval_ms": heartbeat_interval_ms,
            }
        });

        println!("\n=== Supervisor Smoke Report ===");
        println!("{}", serde_json::to_string_pretty(&report)?);

        if success {
            println!("\nSupervisor smoke test PASSED.");
        } else {
            eprintln!("\nSupervisor smoke test FAILED.");
            if !started_received {
                eprintln!("  Missing: Started event");
            }
            if !ready_received {
                eprintln!("  Missing: Ready event");
            }
            if heartbeat_count == 0 {
                eprintln!("  Missing: heartbeat events");
            }
            if !exited_received {
                eprintln!("  Missing: Exited event");
            }
            if exit_code != Some(0) {
                eprintln!("  Non-zero exit code: {:?}", exit_code);
            }
        }

        Ok(())
    })
}

/// Find the wallflow-renderer executable in the current build output.
fn find_renderer_exe() -> Result<std::path::PathBuf> {
    // Try the standard target/debug location relative to the workspace
    let candidates = [
        "target/debug/wallflow-renderer",
        "../target/debug/wallflow-renderer",
        "../../target/debug/wallflow-renderer",
    ];

    // First, try to find the exe relative to the current executable
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let candidate = parent.join("wallflow-renderer");
            if candidate.exists() {
                return Ok(candidate);
            }
            // Go up to target/
            if let Some(grandparent) = parent.parent() {
                let candidate = grandparent.join("wallflow-renderer");
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
    }

    // Try CWD-based paths
    for candidate in &candidates {
        let path = std::path::PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    // On Windows, try with .exe extension
    #[cfg(target_os = "windows")]
    {
        for candidate in &candidates {
            let path = std::path::PathBuf::format!("{}.exe", candidate);
            if path.exists() {
                return Ok(path);
            }
        }
    }

    Err(anyhow::anyhow!(
        "could not find wallflow-renderer executable. Build it first with: cargo build -p wallflow-renderer"
    ))
}

// ---------------------------------------------------------------------------
// IPC supervisor smoke
// ---------------------------------------------------------------------------

/// IPC supervisor smoke test: launches a renderer in `--ipc-stdio` mode,
/// exchanges typed IPC frames, and validates the full command/event lifecycle.
///
/// The test flow:
/// 1. Spawn renderer with `--ipc-stdio`
/// 2. Wait for Started + Ready events
/// 3. Wait for at least 2 Heartbeat events
/// 4. Send Pause command, wait for Paused event
/// 5. Send Resume command, wait for Resumed event
/// 6. Send Shutdown command, wait for Exited event
/// 7. Print structured JSON report
fn run_ipc_supervisor_smoke(timeout_secs: u64, heartbeat_interval_ms: u64) -> Result<()> {
    use std::process::Stdio;
    use tokio::process::Command;
    use tokio::time;
    use wallflow_ipc::{RendererCommand, RendererEvent};

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let start = Instant::now();
        let renderer_exe = find_renderer_exe()?;

        println!("=== IPC Supervisor Smoke Test ===");
        println!("Renderer exe: {}", renderer_exe.display());
        println!(
            "Timeout: {}s, Heartbeat interval: {}ms",
            timeout_secs, heartbeat_interval_ms
        );

        // Spawn renderer with --ipc-stdio
        let mut child = Command::new(&renderer_exe)
            .arg("--ipc-stdio")
            .arg("--heartbeat-interval-ms")
            .arg(heartbeat_interval_ms.to_string())
            .arg("--timeout-secs")
            .arg(timeout_secs.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // renderer logs go to our stderr
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn renderer: {e}"))?;

        let pid = child.id();
        println!("Renderer spawned (PID: {:?})", pid);

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture renderer stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture renderer stdout"))?;

        let mut stdin_writer = tokio::io::BufWriter::new(stdin);
        let mut stdout_reader = tokio::io::BufReader::new(stdout);

        let mut started_received = false;
        let mut ready_received = false;
        let mut heartbeat_count: u64 = 0;
        let mut paused_received = false;
        let mut resumed_received = false;
        let mut exited_received = false;
        let mut exit_code_received: Option<i32> = None;
        let mut pause_sent = false;
        let mut resume_sent = false;
        let mut shutdown_sent = false;

        let deadline = time::sleep(Duration::from_secs(timeout_secs + 10));
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                _ = &mut deadline => {
                    eprintln!("IPC supervisor smoke timed out");
                    let _ = child.kill().await;
                    break;
                }
                result = read_ipc_frame(&mut stdout_reader) => {
                    match result {
                        Ok(Some(event)) => {
                            let elapsed = start.elapsed();
                            match &event {
                                RendererEvent::Started { renderer_id } => {
                                    started_received = true;
                                    println!("  [{:.1}s] Started (renderer_id: {})", elapsed.as_secs_f64(), renderer_id);
                                }
                                RendererEvent::Ready { renderer_id } => {
                                    ready_received = true;
                                    println!("  [{:.1}s] Ready (renderer_id: {})", elapsed.as_secs_f64(), renderer_id);
                                }
                                RendererEvent::Heartbeat { renderer_id, uptime_ms } => {
                                    heartbeat_count += 1;
                                    if heartbeat_count <= 3 {
                                        println!("  [{:.1}s] Heartbeat #{} (uptime: {}ms, renderer_id: {})",
                                            elapsed.as_secs_f64(), heartbeat_count, uptime_ms, renderer_id);
                                    }
                                    // After 2 heartbeats, send Pause
                                    if heartbeat_count == 2 && !pause_sent {
                                        println!("  [{:.1}s] Sending Pause command...", elapsed.as_secs_f64());
                                        send_ipc_command(&mut stdin_writer, RendererCommand::Pause).await?;
                                        pause_sent = true;
                                    }
                                }
                                RendererEvent::Paused { renderer_id } => {
                                    paused_received = true;
                                    println!("  [{:.1}s] Paused (renderer_id: {})", elapsed.as_secs_f64(), renderer_id);
                                    // Now send Resume
                                    if !resume_sent {
                                        println!("  [{:.1}s] Sending Resume command...", elapsed.as_secs_f64());
                                        send_ipc_command(&mut stdin_writer, RendererCommand::Resume).await?;
                                        resume_sent = true;
                                    }
                                }
                                RendererEvent::Resumed { renderer_id } => {
                                    resumed_received = true;
                                    println!("  [{:.1}s] Resumed (renderer_id: {})", elapsed.as_secs_f64(), renderer_id);
                                    // Now send Shutdown
                                    if !shutdown_sent {
                                        println!("  [{:.1}s] Sending Shutdown command...", elapsed.as_secs_f64());
                                        send_ipc_command(&mut stdin_writer, RendererCommand::Shutdown).await?;
                                        shutdown_sent = true;
                                    }
                                }
                                RendererEvent::Exited { renderer_id, exit_code } => {
                                    exited_received = true;
                                    exit_code_received = *exit_code;
                                    println!("  [{:.1}s] Exited (exit_code: {:?}, renderer_id: {})", elapsed.as_secs_f64(), exit_code, renderer_id);
                                    break;
                                }
                                RendererEvent::Error { renderer_id, message } => {
                                    eprintln!("  [{:.1}s] Error from renderer {}: {}", elapsed.as_secs_f64(), renderer_id, message);
                                }
                                RendererEvent::WallpaperApplied { renderer_id, monitor_id } => {
                                    println!("  [{:.1}s] WallpaperApplied (renderer_id: {}, monitor_id: {})", elapsed.as_secs_f64(), renderer_id, monitor_id.0);
                                }
                            }
                        }
                        Ok(None) => {
                            // EOF
                            break;
                        }
                        Err(e) => {
                            eprintln!("Error reading IPC frame: {e}");
                            break;
                        }
                    }
                }
            }
        }

        // Drop the stdin writer to close the pipe
        drop(stdin_writer);

        // Wait for the process to exit
        let exit_status = match time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(status)) => Some(status),
            Ok(Err(e)) => {
                eprintln!("Error waiting for renderer: {e}");
                None
            }
            Err(_) => {
                eprintln!("Renderer did not exit in time, killing");
                let _ = child.kill().await;
                None
            }
        };

        let total_elapsed = start.elapsed();
        let process_exit_code = exit_status.as_ref().and_then(|s| s.code());

        let success = process_exit_code == Some(0)
            && started_received
            && ready_received
            && heartbeat_count >= 2
            && paused_received
            && resumed_received
            && exited_received
            && exit_code_received == Some(0);

        let report = serde_json::json!({
            "test": "ipc-supervisor-smoke",
            "success": success,
            "total_elapsed_ms": total_elapsed.as_millis() as u64,
            "renderer_pid": pid,
            "process_exit_code": process_exit_code,
            "ipc_exit_code": exit_code_received,
            "events": {
                "started_received": started_received,
                "ready_received": ready_received,
                "heartbeat_count": heartbeat_count,
                "paused_received": paused_received,
                "resumed_received": resumed_received,
                "exited_received": exited_received,
            },
            "commands_sent": {
                "pause_sent": pause_sent,
                "resume_sent": resume_sent,
                "shutdown_sent": shutdown_sent,
            },
            "config": {
                "timeout_secs": timeout_secs,
                "heartbeat_interval_ms": heartbeat_interval_ms,
            }
        });

        println!("\n=== IPC Supervisor Smoke Report ===");
        println!("{}", serde_json::to_string_pretty(&report)?);

        if success {
            println!("\nIPC supervisor smoke test PASSED.");
        } else {
            eprintln!("\nIPC supervisor smoke test FAILED.");
            if !started_received { eprintln!("  Missing: Started event"); }
            if !ready_received { eprintln!("  Missing: Ready event"); }
            if heartbeat_count < 2 { eprintln!("  Missing: at least 2 heartbeats (got {})", heartbeat_count); }
            if !paused_received { eprintln!("  Missing: Paused event"); }
            if !resumed_received { eprintln!("  Missing: Resumed event"); }
            if !exited_received { eprintln!("  Missing: Exited event"); }
            if exit_code_received != Some(0) { eprintln!("  Non-zero IPC exit code: {:?}", exit_code_received); }
            if process_exit_code != Some(0) { eprintln!("  Non-zero process exit code: {:?}", process_exit_code); }
        }

        Ok(())
    })
}

/// Read one IPC frame from the renderer's stdout and extract the RendererEvent.
async fn read_ipc_frame<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<wallflow_ipc::RendererEvent>> {
    use tokio::io::AsyncReadExt;
    use wallflow_ipc::{IpcMessage, LENGTH_PREFIX_SIZE, MAX_FRAME_SIZE};

    let mut len_buf = [0u8; LENGTH_PREFIX_SIZE];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }

    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_SIZE {
        return Err(anyhow::anyhow!("invalid IPC frame length: {len}"));
    }

    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;

    let msg: IpcMessage = serde_json::from_slice(&payload)?;

    match msg {
        IpcMessage::Event(env) => Ok(Some(env.payload)),
        _ => {
            eprintln!("expected Event message from renderer, got different message type");
            Ok(None)
        }
    }
}

/// Send one IPC command frame to the renderer's stdin.
async fn send_ipc_command<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    cmd: wallflow_ipc::RendererCommand,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    use wallflow_ipc::{encode_to_bytes, CommandEnvelope, IpcMessage};

    let msg = IpcMessage::Command(CommandEnvelope::new(cmd));
    let bytes = encode_to_bytes(&msg)?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}
