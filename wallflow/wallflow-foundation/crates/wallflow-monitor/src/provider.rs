use thiserror::Error;
use wallflow_common::{MonitorId, MonitorInfo};

#[derive(Debug, Error)]
pub enum MonitorError {
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),

    #[error("OS monitor query failed: {0}")]
    OsQueryFailed(String),

    #[error("no primary monitor found")]
    NoPrimaryMonitor,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MonitorEvent {
    Added(MonitorInfo),
    Removed { id: MonitorId },
    Changed(MonitorInfo),
    TopologyChanged(Vec<MonitorInfo>),
}

pub trait MonitorProvider: Send + Sync {
    fn snapshot(&self) -> Result<Vec<MonitorInfo>, MonitorError>;

    fn primary(&self) -> Result<MonitorInfo, MonitorError> {
        self.snapshot()?
            .into_iter()
            .find(|m| m.is_primary)
            .ok_or(MonitorError::NoPrimaryMonitor)
    }

    fn find_by_id(&self, id: &MonitorId) -> Result<Option<MonitorInfo>, MonitorError> {
        Ok(self.snapshot()?.into_iter().find(|m| &m.id == id))
    }
}

pub fn platform_monitor_provider() -> Box<dyn MonitorProvider> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows_impl::WindowsMonitorProvider)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedMonitorProvider)
    }
}

#[cfg(not(target_os = "windows"))]
struct UnsupportedMonitorProvider;

#[cfg(not(target_os = "windows"))]
impl MonitorProvider for UnsupportedMonitorProvider {
    fn snapshot(&self) -> Result<Vec<MonitorInfo>, MonitorError> {
        Err(MonitorError::UnsupportedPlatform(
            std::env::consts::OS.to_owned(),
        ))
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use std::mem;
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

    pub struct WindowsMonitorProvider;

    impl MonitorProvider for WindowsMonitorProvider {
        fn snapshot(&self) -> Result<Vec<MonitorInfo>, MonitorError> {
            let mut monitors: Vec<MonitorInfo> = Vec::new();
            let lparam = LPARAM((&mut monitors as *mut Vec<MonitorInfo>) as isize);

            // SAFETY: callback receives the same pointer passed in lparam and only mutates it
            // during the synchronous EnumDisplayMonitors call.
            let ok = unsafe {
                EnumDisplayMonitors(
                    HDC(std::ptr::null_mut()),
                    None,
                    Some(enum_monitor_proc),
                    lparam,
                )
            };
            if ok.as_bool() {
                Ok(monitors)
            } else {
                Err(MonitorError::OsQueryFailed(
                    "EnumDisplayMonitors failed".to_owned(),
                ))
            }
        }
    }

    /// SAFETY: This is an EnumDisplayMonitors callback. The OS calls it synchronously
    /// with valid HMONITOR/HDC/RECT values. lparam contains the same pointer passed
    /// by the caller, which remains valid for the duration of the enumeration.
    unsafe extern "system" fn enum_monitor_proc(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        // SAFETY: lparam.0 is the pointer we created from &mut Vec<MonitorInfo> in snapshot().
        // It remains valid because EnumDisplayMonitors is synchronous.
        let monitors = &mut *(lparam.0 as *mut Vec<MonitorInfo>);

        if let Some(info) = query_monitor(monitor) {
            monitors.push(info);
        }

        BOOL(1)
    }

    /// SAFETY: Caller must supply a valid HMONITOR obtained from the OS (e.g. via
    /// EnumDisplayMonitors). Internally calls GetMonitorInfoW with a properly sized
    /// MONITORINFOEXW buffer.
    unsafe fn query_monitor(monitor: HMONITOR) -> Option<MonitorInfo> {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;

        // SAFETY: info points to a valid MONITORINFOEXW buffer whose first field is MONITORINFO.
        // The cast to *mut MONITORINFO is valid because MONITORINFOEXW starts with MONITORINFO.
        let ok = GetMonitorInfoW(monitor, &mut info as *mut _ as *mut _).as_bool();
        if !ok {
            return None;
        }

        let device_name = utf16_to_string(&info.szDevice);
        let bounds = info.monitorInfo.rcMonitor;
        let work = info.monitorInfo.rcWork;
        let primary = (info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;

        let scale_factor = effective_scale_factor(monitor).unwrap_or(1.0);

        Some(MonitorInfo {
            id: MonitorId(device_name.clone()),
            name: device_name,
            is_primary: primary,
            position: wallflow_common::MonitorPosition {
                x: bounds.left,
                y: bounds.top,
            },
            size_px: wallflow_common::MonitorSize {
                width: (bounds.right - bounds.left).max(0) as u32,
                height: (bounds.bottom - bounds.top).max(0) as u32,
            },
            work_area_px: wallflow_common::MonitorSize {
                width: (work.right - work.left).max(0) as u32,
                height: (work.bottom - work.top).max(0) as u32,
            },
            scale_factor,
            refresh_rate_millihz: None,
        })
    }

    /// SAFETY: Caller must supply a valid HMONITOR. Output pointers dpi_x and dpi_y
    /// are stack-allocated and valid for the call duration.
    unsafe fn effective_scale_factor(monitor: HMONITOR) -> Option<f64> {
        let mut dpi_x = 0_u32;
        let mut dpi_y = 0_u32;
        // SAFETY: output pointers are valid for the duration of the call.
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() {
            Some(dpi_x as f64 / 96.0)
        } else {
            None
        }
    }

    fn utf16_to_string(buf: &[u16]) -> String {
        let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }
}
