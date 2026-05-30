//! Platform-specific desktop integration.
//!
//! On Windows, live wallpapers usually require attaching a renderer window into the
//! desktop window hierarchy. This layer is intentionally isolated because Windows updates
//! may change WorkerW/Progman behavior.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DesktopError {
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),

    #[error("desktop worker window was not found")]
    WorkerWindowNotFound,

    #[error("desktop attach failed: {0}")]
    AttachFailed(String),
}

/// Opaque native window handle represented as an integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeWindowHandle(pub isize);

/// Handle to the desktop worker window that should parent the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopWorkerHandle(pub isize);

/// Finds the desktop worker surface used for wallpaper rendering.
pub fn find_desktop_worker() -> Result<DesktopWorkerHandle, DesktopError> {
    platform::find_desktop_worker()
}

/// Parents a renderer window into the desktop worker window.
pub fn attach_window_to_desktop(
    window: NativeWindowHandle,
) -> Result<DesktopWorkerHandle, DesktopError> {
    platform::attach_window_to_desktop(window)
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub fn find_desktop_worker() -> Result<DesktopWorkerHandle, DesktopError> {
        Err(DesktopError::UnsupportedPlatform(
            std::env::consts::OS.to_owned(),
        ))
    }

    pub fn attach_window_to_desktop(
        _window: NativeWindowHandle,
    ) -> Result<DesktopWorkerHandle, DesktopError> {
        Err(DesktopError::UnsupportedPlatform(
            std::env::consts::OS.to_owned(),
        ))
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowExW, FindWindowW, SendMessageTimeoutW, SetParent, SMTO_NORMAL,
    };

    const WM_SPAWN_WORKERW: u32 = 0x052C;

    pub fn find_desktop_worker() -> Result<DesktopWorkerHandle, DesktopError> {
        // SAFETY: Uses Win32 window discovery APIs with constant class-name strings.
        unsafe {
            spawn_workerw();

            let mut result = HWND(std::ptr::null_mut());
            let mut data = FindWorkerData {
                workerw: &mut result,
            };
            let data_ptr = &mut data as *mut FindWorkerData as isize;

            // EnumWindows returns Result<()>. Failure or empty result means no worker found.
            let enum_result = EnumWindows(Some(enum_windows_proc), LPARAM(data_ptr));
            if enum_result.is_err() || result.0.is_null() {
                return Err(DesktopError::WorkerWindowNotFound);
            }

            Ok(DesktopWorkerHandle(result.0 as isize))
        }
    }

    pub fn attach_window_to_desktop(
        window: NativeWindowHandle,
    ) -> Result<DesktopWorkerHandle, DesktopError> {
        let worker = find_desktop_worker()?;
        // SAFETY: HWND values are supplied by the OS or caller. SetParent result is checked.
        let set_parent_result =
            unsafe { SetParent(HWND(window.0 as *mut _), HWND(worker.0 as *mut _)) };
        if set_parent_result.is_err() {
            // SetParent can fail; report the error instead of silently ignoring it.
            return Err(DesktopError::AttachFailed(
                "SetParent returned an error".to_owned(),
            ));
        }
        Ok(worker)
    }

    /// SAFETY: Sends a private Windows message (0x052C) to the Progman window to
    /// spawn a WorkerW child window. This is a well-known technique used by wallpaper
    /// engines. The Progman HWND is validated before use.
    unsafe fn spawn_workerw() {
        let progman = FindWindowW(wide("Progman").as_pcwstr(), PCWSTR::null());
        let progman = match progman {
            Ok(hwnd) if !hwnd.0.is_null() => hwnd,
            _ => return,
        };

        let mut result = 0_usize;
        let _ = SendMessageTimeoutW(
            progman,
            WM_SPAWN_WORKERW,
            WPARAM(0),
            LPARAM(0),
            SMTO_NORMAL,
            1000,
            Some(&mut result as *mut usize),
        );
    }

    struct FindWorkerData<'a> {
        workerw: &'a mut HWND,
    }

    /// SAFETY: This is an EnumWindows callback. The OS supplies valid HWND values.
    /// lparam contains the same pointer created by the caller, which remains valid
    /// for the synchronous duration of the enumeration.
    unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam.0 is the pointer we created from &mut FindWorkerData in
        // find_desktop_worker(). It remains valid because EnumWindows is synchronous.
        let data = &mut *(lparam.0 as *mut FindWorkerData);
        let shell_view = FindWindowExW(
            hwnd,
            HWND(std::ptr::null_mut()),
            wide("SHELLDLL_DefView").as_pcwstr(),
            PCWSTR::null(),
        );

        if let Ok(shell_view) = shell_view {
            if !shell_view.0.is_null() {
                let workerw = FindWindowExW(
                    HWND(std::ptr::null_mut()),
                    hwnd,
                    wide("WorkerW").as_pcwstr(),
                    PCWSTR::null(),
                );
                if let Ok(workerw) = workerw {
                    if !workerw.0.is_null() {
                        *data.workerw = workerw;
                        return BOOL(0);
                    }
                }
            }
        }

        BOOL(1)
    }

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
