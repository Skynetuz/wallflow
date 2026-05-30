//! Platform-specific desktop integration for WallFlow.
//!
//! On Windows, live wallpapers require attaching a renderer window into the
//! desktop window hierarchy behind the desktop icons. This is achieved by:
//!
//! 1. Finding the `Progman` window (Program Manager).
//! 2. Sending message `0x052C` to spawn a `WorkerW` window.
//! 3. Enumerating top-level windows to find the `WorkerW` that is a sibling
//!    of `SHELLDLL_DefView`.
//! 4. Using `SetParent` to embed the renderer window into that `WorkerW`.
//!
//! This layer is intentionally isolated because Windows updates may change
//! the Progman/WorkerW behavior. All unsafe Win32 code is confined to this crate.

use serde::{Deserialize, Serialize};
use thiserror::Error;
#[cfg(not(target_os = "windows"))]
use tracing::info;
#[cfg(target_os = "windows")]
use tracing::{debug, info, warn};

// ── Error types ──────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum DesktopError {
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),

    #[error("Progman window not found")]
    ProgmanNotFound,

    #[error("failed to spawn WorkerW via Progman message 0x052C")]
    WorkerWSpawnFailed,

    #[error("WorkerW window not found after enumeration")]
    WorkerWindowNotFound,

    #[error("SHELLDLL_DefView not found during enumeration")]
    ShellDefViewNotFound,

    #[error("desktop attach failed: {0}")]
    AttachFailed(String),

    #[error("desktop detach failed: {0}")]
    DetachFailed(String),

    #[error("window creation failed: {0}")]
    WindowCreationFailed(String),

    #[error("invalid window handle")]
    InvalidHandle,
}

// ── Handle types ─────────────────────────────────────────────────────────────

/// Opaque native window handle represented as an integer.
///
/// On Windows, this stores the `HWND` value. On other platforms it is always zero
/// and operations return `UnsupportedPlatform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeWindowHandle(pub isize);

impl NativeWindowHandle {
    /// Returns `true` if this handle is non-zero (potentially valid).
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

/// Handle to the desktop worker window that should parent the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DesktopWorkerHandle(pub isize);

impl DesktopWorkerHandle {
    /// Returns `true` if this handle is non-zero (potentially valid).
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

// ── Diagnostic report types ──────────────────────────────────────────────────

/// Detailed diagnostic report from probing the desktop window hierarchy.
///
/// This struct is serializable so the CLI can output it as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopProbeReport {
    /// Whether the platform supports desktop integration.
    pub platform_supported: bool,
    /// HWND of the Progman window (0 if not found).
    pub progman_hwnd: isize,
    /// Whether the 0x052C message was sent to Progman.
    pub spawn_message_sent: bool,
    /// HWND of the SHELLDLL_DefView window (0 if not found).
    pub shell_def_view_hwnd: isize,
    /// HWND of the selected WorkerW window (0 if not found).
    pub workerw_hwnd: isize,
    /// Whether the attach operation should be possible.
    pub attach_feasible: bool,
    /// Human-readable error message if something failed.
    pub error: Option<String>,
}

impl DesktopProbeReport {
    /// Creates a report for an unsupported platform.
    pub fn unsupported() -> Self {
        Self {
            platform_supported: false,
            progman_hwnd: 0,
            spawn_message_sent: false,
            shell_def_view_hwnd: 0,
            workerw_hwnd: 0,
            attach_feasible: false,
            error: Some(format!("unsupported platform: {}", std::env::consts::OS)),
        }
    }
}

/// Result of a desktop attach operation with diagnostic information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopAttachReport {
    /// The desktop worker handle used for parenting.
    pub worker_handle: DesktopWorkerHandle,
    /// The renderer window handle that was attached.
    pub renderer_handle: NativeWindowHandle,
    /// The previous parent HWND (0 if the window had no parent).
    pub previous_parent_hwnd: isize,
}

/// Result of a desktop detach operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopDetachReport {
    /// The renderer window that was detached.
    pub renderer_handle: NativeWindowHandle,
    /// Whether the detach succeeded.
    pub success: bool,
}

// ── Internal discovery data ──────────────────────────────────────────────────

/// Intermediate data collected during desktop window enumeration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopDiscoveryData {
    pub progman_hwnd: isize,
    pub spawn_message_sent: bool,
    pub shell_def_view_hwnd: isize,
    pub workerw_hwnd: isize,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Probes the desktop window hierarchy and returns a diagnostic report.
///
/// This function does not modify any windows. It only reads the current state.
/// On non-Windows platforms, returns an `UnsupportedPlatform` error.
pub fn probe_desktop() -> DesktopProbeReport {
    platform::probe_desktop()
}

/// Finds the desktop worker surface used for wallpaper rendering.
///
/// Returns a `DesktopWorkerHandle` that can be used with `attach_window_to_desktop`.
pub fn find_desktop_worker() -> Result<DesktopWorkerHandle, DesktopError> {
    platform::find_desktop_worker()
}

/// Parents a renderer window into the desktop worker window.
///
/// After this call, the renderer window will appear behind the desktop icons.
/// Returns a `DesktopAttachReport` with diagnostic information.
pub fn attach_window_to_desktop(
    window: NativeWindowHandle,
) -> Result<DesktopAttachReport, DesktopError> {
    if !window.is_valid() {
        return Err(DesktopError::InvalidHandle);
    }
    platform::attach_window_to_desktop(window)
}

/// Detaches a renderer window from the desktop worker, restoring its
/// original parent (the desktop).
///
/// This should be called before destroying the renderer window to avoid
/// leaving orphaned windows in the desktop hierarchy.
pub fn detach_window_from_desktop(
    window: NativeWindowHandle,
) -> Result<DesktopDetachReport, DesktopError> {
    if !window.is_valid() {
        return Err(DesktopError::InvalidHandle);
    }
    platform::detach_window_from_desktop(window)
}

// ── Non-Windows stubs ────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub fn probe_desktop() -> DesktopProbeReport {
        info!(
            platform = std::env::consts::OS,
            "desktop probe: platform not supported"
        );
        DesktopProbeReport::unsupported()
    }

    pub fn find_desktop_worker() -> Result<DesktopWorkerHandle, DesktopError> {
        Err(DesktopError::UnsupportedPlatform(
            std::env::consts::OS.to_owned(),
        ))
    }

    pub fn attach_window_to_desktop(
        _window: NativeWindowHandle,
    ) -> Result<DesktopAttachReport, DesktopError> {
        Err(DesktopError::UnsupportedPlatform(
            std::env::consts::OS.to_owned(),
        ))
    }

    pub fn detach_window_from_desktop(
        _window: NativeWindowHandle,
    ) -> Result<DesktopDetachReport, DesktopError> {
        Err(DesktopError::UnsupportedPlatform(
            std::env::consts::OS.to_owned(),
        ))
    }
}

// ── Windows implementation ───────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GetLastError, BOOL, HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowExW, FindWindowW, IsWindow, SendMessageTimeoutW, SetParent,
        SMTO_NORMAL,
    };

    const WM_SPAWN_WORKERW: u32 = 0x052C;

    pub fn probe_desktop() -> DesktopProbeReport {
        // SAFETY: All operations below are read-only window queries.
        // SendMessageTimeoutW sends a well-known message to Progman to
        // ensure WorkerW exists; this is the same technique used by
        // every major wallpaper engine and does not modify user data.
        unsafe { probe_desktop_impl() }
    }

    unsafe fn probe_desktop_impl() -> DesktopProbeReport {
        let mut report = DesktopProbeReport {
            platform_supported: true,
            progman_hwnd: 0,
            spawn_message_sent: false,
            shell_def_view_hwnd: 0,
            workerw_hwnd: 0,
            attach_feasible: false,
            error: None,
        };

        // Step 1: Find Progman
        let progman = match FindWindowW(wide("Progman").as_pcwstr(), PCWSTR::null()) {
            Ok(hwnd) if !hwnd.0.is_null() => {
                report.progman_hwnd = hwnd.0 as isize;
                info!(hwnd = hwnd.0 as isize, "probe: found Progman window");
                hwnd
            }
            Ok(_) => {
                let err = "probe: FindWindowW(\"Progman\") returned null handle";
                warn!(err);
                report.error = Some(err.to_owned());
                return report;
            }
            Err(e) => {
                let msg = format!("probe: FindWindowW(\"Progman\") failed: {e}");
                warn!(error = %e, "probe: FindWindowW Progman failed");
                report.error = Some(msg);
                return report;
            }
        };

        // Step 2: Send 0x052C to spawn WorkerW
        let mut spawn_result = 0_usize;
        let send_result = SendMessageTimeoutW(
            progman,
            WM_SPAWN_WORKERW,
            WPARAM(0),
            LPARAM(0),
            SMTO_NORMAL,
            1000,
            Some(&mut spawn_result as *mut usize),
        );
        report.spawn_message_sent = true;
        info!(lresult = send_result.0, "probe: sent 0x052C to Progman");

        // Step 3: Enumerate windows to find SHELLDLL_DefView and WorkerW
        let mut discovery = EnumDiscoveryData {
            shell_def_view_hwnd: HWND(std::ptr::null_mut()),
            workerw_hwnd: HWND(std::ptr::null_mut()),
        };

        let enum_result = EnumWindows(
            Some(enum_windows_probe_proc),
            LPARAM(&mut discovery as *mut _ as isize),
        );

        if let Err(e) = enum_result {
            let msg = format!("probe: EnumWindows failed: {e}");
            warn!(error = %e, "probe: EnumWindows failed");
            report.error = Some(msg);
            return report;
        }

        report.shell_def_view_hwnd = discovery.shell_def_view_hwnd.0 as isize;
        report.workerw_hwnd = discovery.workerw_hwnd.0 as isize;

        if !discovery.shell_def_view_hwnd.0.is_null() {
            info!(
                hwnd = discovery.shell_def_view_hwnd.0 as isize,
                "probe: found SHELLDLL_DefView"
            );
        } else {
            warn!("probe: SHELLDLL_DefView not found");
        }

        if !discovery.workerw_hwnd.0.is_null() {
            info!(
                hwnd = discovery.workerw_hwnd.0 as isize,
                "probe: found WorkerW"
            );
            report.attach_feasible = true;
        } else {
            let msg = "probe: WorkerW not found after enumeration";
            warn!(msg);
            report.error = Some(msg.to_owned());
        }

        report
    }

    pub fn find_desktop_worker() -> Result<DesktopWorkerHandle, DesktopError> {
        // SAFETY: Same window discovery technique as probe_desktop.
        unsafe { find_desktop_worker_impl() }
    }

    unsafe fn find_desktop_worker_impl() -> Result<DesktopWorkerHandle, DesktopError> {
        // Step 1: Find Progman
        let progman = FindWindowW(wide("Progman").as_pcwstr(), PCWSTR::null()).map_err(|e| {
            let last = GetLastError();
            warn!(
                error = %e,
                last_error = last.0,
                "find_desktop_worker: FindWindowW Progman failed"
            );
            DesktopError::ProgmanNotFound
        })?;

        if progman.0.is_null() {
            warn!("find_desktop_worker: Progman HWND is null");
            return Err(DesktopError::ProgmanNotFound);
        }
        info!(
            hwnd = progman.0 as isize,
            "find_desktop_worker: found Progman"
        );

        // Step 2: Send 0x052C to ensure WorkerW exists
        let mut spawn_result = 0_usize;
        let _ = SendMessageTimeoutW(
            progman,
            WM_SPAWN_WORKERW,
            WPARAM(0),
            LPARAM(0),
            SMTO_NORMAL,
            1000,
            Some(&mut spawn_result as *mut usize),
        );
        debug!("find_desktop_worker: sent 0x052C to Progman");

        // Step 3: Enumerate to find WorkerW
        let mut discovery = EnumDiscoveryData {
            shell_def_view_hwnd: HWND(std::ptr::null_mut()),
            workerw_hwnd: HWND(std::ptr::null_mut()),
        };

        let enum_result = EnumWindows(
            Some(enum_windows_probe_proc),
            LPARAM(&mut discovery as *mut _ as isize),
        );

        if enum_result.is_err() {
            let last = GetLastError();
            warn!(
                last_error = last.0,
                "find_desktop_worker: EnumWindows failed"
            );
            return Err(DesktopError::WorkerWindowNotFound);
        }

        if discovery.workerw_hwnd.0.is_null() {
            warn!("find_desktop_worker: WorkerW not found after enumeration");
            return Err(DesktopError::WorkerWindowNotFound);
        }

        info!(
            hwnd = discovery.workerw_hwnd.0 as isize,
            "find_desktop_worker: found WorkerW"
        );
        Ok(DesktopWorkerHandle(discovery.workerw_hwnd.0 as isize))
    }

    pub fn attach_window_to_desktop(
        window: NativeWindowHandle,
    ) -> Result<DesktopAttachReport, DesktopError> {
        // SAFETY: HWND values are supplied by the OS or validated caller.
        // SetParent is called with valid handles; the result is checked.
        unsafe { attach_impl(window) }
    }

    unsafe fn attach_impl(window: NativeWindowHandle) -> Result<DesktopAttachReport, DesktopError> {
        let worker = find_desktop_worker()?;

        let child_hwnd = HWND(window.0 as *mut _);
        let parent_hwnd = HWND(worker.0 as *mut _);

        // Validate that the child window still exists
        if IsWindow(child_hwnd).as_bool() {
            debug!(hwnd = window.0, "attach: child window is valid");
        } else {
            warn!(hwnd = window.0, "attach: child window handle is not valid");
            return Err(DesktopError::AttachFailed(
                "renderer window handle is not a valid window".to_owned(),
            ));
        }

        let previous = SetParent(child_hwnd, parent_hwnd);
        match previous {
            Ok(prev_hwnd) => {
                info!(
                    renderer_hwnd = window.0,
                    worker_hwnd = worker.0,
                    previous_parent = prev_hwnd.0 as isize,
                    "attach: SetParent succeeded"
                );
                Ok(DesktopAttachReport {
                    worker_handle: worker,
                    renderer_handle: window,
                    previous_parent_hwnd: prev_hwnd.0 as isize,
                })
            }
            Err(e) => {
                let last = GetLastError();
                let msg = format!("SetParent failed: {e} (last error: {})", last.0);
                warn!(error = %e, last_error = last.0, "attach: SetParent failed");
                Err(DesktopError::AttachFailed(msg))
            }
        }
    }

    pub fn detach_window_from_desktop(
        window: NativeWindowHandle,
    ) -> Result<DesktopDetachReport, DesktopError> {
        // SAFETY: We set the parent back to the desktop (null HWND).
        // This is the standard way to unparent a window.
        unsafe { detach_impl(window) }
    }

    unsafe fn detach_impl(window: NativeWindowHandle) -> Result<DesktopDetachReport, DesktopError> {
        let child_hwnd = HWND(window.0 as *mut _);

        // Validate that the child window still exists
        if !IsWindow(child_hwnd).as_bool() {
            warn!(hwnd = window.0, "detach: window handle is not valid");
            return Err(DesktopError::DetachFailed(
                "window handle is not a valid window".to_owned(),
            ));
        }

        // SetParent(null) reparents to the desktop
        // SAFETY: Passing null HWND as parent is the documented way to make
        // a window a top-level window again.
        let result = SetParent(child_hwnd, HWND(std::ptr::null_mut()));
        match result {
            Ok(_) => {
                info!(hwnd = window.0, "detach: SetParent(desktop) succeeded");
                Ok(DesktopDetachReport {
                    renderer_handle: window,
                    success: true,
                })
            }
            Err(e) => {
                let last = GetLastError();
                let msg = format!("SetParent(null) failed: {e} (last error: {})", last.0);
                warn!(error = %e, last_error = last.0, "detach: SetParent failed");
                Err(DesktopError::DetachFailed(msg))
            }
        }
    }

    // ── EnumWindows callback for probing ─────────────────────────────────────

    struct EnumDiscoveryData {
        shell_def_view_hwnd: HWND,
        workerw_hwnd: HWND,
    }

    /// SAFETY: This is an EnumWindows callback. The OS supplies valid HWND values.
    /// lparam contains the same pointer created by the caller, which remains valid
    /// for the synchronous duration of the enumeration.
    unsafe extern "system" fn enum_windows_probe_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam.0 is the pointer we created from &mut EnumDiscoveryData.
        // It remains valid because EnumWindows is synchronous.
        let data = &mut *(lparam.0 as *mut EnumDiscoveryData);

        let shell_view = FindWindowExW(
            hwnd,
            HWND(std::ptr::null_mut()),
            wide("SHELLDLL_DefView").as_pcwstr(),
            PCWSTR::null(),
        );

        if let Ok(shell_view) = shell_view {
            if !shell_view.0.is_null() {
                data.shell_def_view_hwnd = shell_view;
                debug!(
                    parent_hwnd = hwnd.0 as isize,
                    shell_view_hwnd = shell_view.0 as isize,
                    "enum: found SHELLDLL_DefView"
                );

                let workerw = FindWindowExW(
                    HWND(std::ptr::null_mut()),
                    hwnd,
                    wide("WorkerW").as_pcwstr(),
                    PCWSTR::null(),
                );
                if let Ok(workerw) = workerw {
                    if !workerw.0.is_null() {
                        data.workerw_hwnd = workerw;
                        debug!(
                            workerw_hwnd = workerw.0 as isize,
                            "enum: found WorkerW sibling of SHELLDLL_DefView"
                        );
                        // Stop enumeration — we found what we need
                        return BOOL(0);
                    }
                }
            }
        }

        BOOL(1)
    }

    // ── Utility ──────────────────────────────────────────────────────────────

    struct WideString(Vec<u16>);

    impl WideString {
        fn as_pcwstr(&self) -> PCWSTR {
            PCWSTR(self.0.as_ptr())
        }
    }

    fn wide(value: &str) -> WideString {
        WideString(value.encode_utf16().chain(std::iter::once(0)).collect())
    }
}

// ── Tests (platform-independent) ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_window_handle_is_valid() {
        assert!(!NativeWindowHandle(0).is_valid());
        assert!(NativeWindowHandle(1).is_valid());
        assert!(NativeWindowHandle(-1).is_valid());
    }

    #[test]
    fn desktop_worker_handle_is_valid() {
        assert!(!DesktopWorkerHandle(0).is_valid());
        assert!(DesktopWorkerHandle(1).is_valid());
    }

    #[test]
    fn probe_report_unsupported() {
        let report = DesktopProbeReport::unsupported();
        assert!(!report.platform_supported);
        assert_eq!(report.progman_hwnd, 0);
        assert!(!report.attach_feasible);
        assert!(report.error.is_some());
    }

    #[test]
    fn probe_report_serialization() {
        let report = DesktopProbeReport {
            platform_supported: true,
            progman_hwnd: 0x1234,
            spawn_message_sent: true,
            shell_def_view_hwnd: 0x5678,
            workerw_hwnd: 0x9ABC,
            attach_feasible: true,
            error: None,
        };
        let json = serde_json::to_string(&report).expect("serialization should succeed");
        let decoded: DesktopProbeReport =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(decoded.progman_hwnd, 0x1234);
        assert_eq!(decoded.workerw_hwnd, 0x9ABC);
        assert!(decoded.attach_feasible);
    }

    #[test]
    fn attach_report_serialization() {
        let report = DesktopAttachReport {
            worker_handle: DesktopWorkerHandle(0x100),
            renderer_handle: NativeWindowHandle(0x200),
            previous_parent_hwnd: 0,
        };
        let json = serde_json::to_string(&report).expect("serialization should succeed");
        let decoded: DesktopAttachReport =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(decoded.worker_handle.0, 0x100);
        assert_eq!(decoded.renderer_handle.0, 0x200);
    }

    #[test]
    fn detach_report_serialization() {
        let report = DesktopDetachReport {
            renderer_handle: NativeWindowHandle(0x300),
            success: true,
        };
        let json = serde_json::to_string(&report).expect("serialization should succeed");
        let decoded: DesktopDetachReport =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert!(decoded.success);
    }

    #[test]
    fn attach_rejects_invalid_handle() {
        let result = attach_window_to_desktop(NativeWindowHandle(0));
        assert!(result.is_err());
        match result {
            Err(DesktopError::InvalidHandle) => {}
            other => panic!("expected InvalidHandle, got {other:?}"),
        }
    }

    #[test]
    fn detach_rejects_invalid_handle() {
        let result = detach_window_from_desktop(NativeWindowHandle(0));
        assert!(result.is_err());
        match result {
            Err(DesktopError::InvalidHandle) => {}
            other => panic!("expected InvalidHandle, got {other:?}"),
        }
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn non_windows_functions_return_unsupported() {
        assert!(find_desktop_worker().is_err());
        assert!(attach_window_to_desktop(NativeWindowHandle(1)).is_err());
        assert!(detach_window_from_desktop(NativeWindowHandle(1)).is_err());

        let probe = probe_desktop();
        assert!(!probe.platform_supported);
    }

    #[test]
    fn error_display_messages() {
        let err = DesktopError::ProgmanNotFound;
        assert!(err.to_string().contains("Progman"));

        let err = DesktopError::AttachFailed("test reason".to_owned());
        assert!(err.to_string().contains("test reason"));

        let err = DesktopError::InvalidHandle;
        assert!(err.to_string().contains("invalid"));
    }

    #[test]
    fn discovery_data_serialization() {
        let data = DesktopDiscoveryData {
            progman_hwnd: 100,
            spawn_message_sent: true,
            shell_def_view_hwnd: 200,
            workerw_hwnd: 300,
        };
        let json = serde_json::to_string(&data).expect("serialization should succeed");
        let decoded: DesktopDiscoveryData =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(decoded.progman_hwnd, 100);
        assert_eq!(decoded.workerw_hwnd, 300);
    }
}
