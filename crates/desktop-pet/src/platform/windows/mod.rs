//! Win32-backed platform implementation.

use std::sync::Arc;

use windows_sys::Win32::{
    Graphics::Gdi::{GetMonitorInfoW, MONITORINFO},
    UI::WindowsAndMessaging::MONITORINFOF_PRIMARY,
};
use winit::{
    dpi::LogicalPosition,
    platform::windows::MonitorHandleExtWindows,
    window::{Window, WindowAttributes, WindowLevel},
};

use super::{PlatformBackend, PlatformError};
use crate::display::{DesktopPosition, MonitorId, MonitorInfo};

pub(super) struct NativePlatformBackend {
    window: Arc<Window>,
    always_on_top: Option<bool>,
}

impl NativePlatformBackend {
    pub(super) fn new(window: Arc<Window>) -> Self {
        Self {
            window,
            always_on_top: None,
        }
    }
}

impl PlatformBackend for NativePlatformBackend {
    fn set_always_on_top(&mut self, enabled: bool) -> Result<(), PlatformError> {
        if self.always_on_top != Some(enabled) {
            let level = if enabled {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            };
            self.window.set_window_level(level);
            self.always_on_top = Some(enabled);
        }
        Ok(())
    }

    fn window_position(&self) -> Result<DesktopPosition, PlatformError> {
        let physical = self
            .window
            .outer_position()
            .map_err(PlatformError::ReadWindowPosition)?;
        let logical = physical.to_logical::<f64>(self.window.scale_factor());
        Ok(DesktopPosition::new(logical.x, logical.y))
    }

    fn set_window_position(&mut self, position: DesktopPosition) -> Result<(), PlatformError> {
        if !position.is_finite() {
            return Err(PlatformError::InvalidWindowPosition {
                x: position.x,
                y: position.y,
            });
        }
        let rounded = position.rounded();
        self.window
            .set_outer_position(LogicalPosition::new(rounded.x, rounded.y));
        Ok(())
    }

    fn monitors(&self) -> Result<Vec<MonitorInfo>, PlatformError> {
        self.window
            .available_monitors()
            .map(|monitor| {
                let mut native = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    rcMonitor: Default::default(),
                    rcWork: Default::default(),
                    dwFlags: 0,
                };
                // SAFETY: hmonitor is supplied by winit and native points to a correctly
                // initialized MONITORINFO whose lifetime covers this call.
                let succeeded = unsafe {
                    GetMonitorInfoW(monitor.hmonitor() as *mut std::ffi::c_void, &mut native)
                };
                if succeeded == 0 {
                    return Err(PlatformError::EnumerateMonitors(
                        std::io::Error::last_os_error().to_string(),
                    ));
                }
                let scale = monitor.scale_factor();
                let work = native.rcWork;
                MonitorInfo::from_physical_work_area(
                    MonitorId(monitor.hmonitor() as usize as u64),
                    work.left,
                    work.top,
                    work.right,
                    work.bottom,
                    scale,
                    native.dwFlags & MONITORINFOF_PRIMARY != 0,
                )
                .ok_or_else(|| {
                    PlatformError::EnumerateMonitors(format!(
                        "monitor {:?} returned invalid work-area metrics",
                        monitor.native_id()
                    ))
                })
            })
            .collect()
    }
}

pub(super) fn configure_window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes
}
