//! Win32-backed platform implementation.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::{GetMonitorInfoW, MONITORINFO},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetCursorPos, HTCLIENT, HTTRANSPARENT, MONITORINFOF_PRIMARY, WM_NCHITTEST,
        },
    },
};
use winit::{
    dpi::LogicalPosition,
    event_loop::EventLoop,
    platform::windows::MonitorHandleExtWindows,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{Window, WindowAttributes, WindowLevel},
};

use super::{IdempotentBool, PlatformBackend, PlatformError};
use crate::display::{DesktopPosition, MonitorId, MonitorInfo};

const CLICK_THROUGH_SUBCLASS_ID: usize = 0x3d50_6574;

fn hit_test_result(click_through: bool) -> LRESULT {
    if click_through {
        HTTRANSPARENT as LRESULT
    } else {
        HTCLIENT as LRESULT
    }
}

unsafe extern "system" fn click_through_subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    reference_data: usize,
) -> LRESULT {
    if message == WM_NCHITTEST {
        // SAFETY: reference_data points to the boxed AtomicBool owned by the backend. The
        // subclass is removed before that box is dropped, and AtomicBool supports shared access.
        let click_through = unsafe { &*(reference_data as *const AtomicBool) };
        return hit_test_result(click_through.load(Ordering::Relaxed));
    }
    // SAFETY: this callback was installed with SetWindowSubclass and unhandled messages must
    // continue through the comctl32 subclass chain with their original parameters.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

pub(super) struct NativePlatformBackend {
    window: Arc<Window>,
    always_on_top: Option<bool>,
    click_through: IdempotentBool,
    click_through_native: Box<AtomicBool>,
    subclass_installed: bool,
}

impl NativePlatformBackend {
    pub(super) fn new(window: Arc<Window>) -> Self {
        Self {
            window,
            always_on_top: None,
            click_through: IdempotentBool::default(),
            click_through_native: Box::new(AtomicBool::new(false)),
            subclass_installed: false,
        }
    }

    fn hwnd(&self) -> Result<HWND, PlatformError> {
        let handle = self.window.window_handle().map_err(|error| {
            PlatformError::ConfigureMouseHandling(format!(
                "winit did not expose a Win32 window handle: {error}"
            ))
        })?;
        match handle.as_raw() {
            RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get() as HWND),
            _ => Err(PlatformError::ConfigureMouseHandling(
                "winit exposed a non-Win32 window handle on Windows".to_owned(),
            )),
        }
    }

    fn install_click_through_subclass(&mut self) -> Result<(), PlatformError> {
        if self.subclass_installed {
            return Ok(());
        }
        let hwnd = self.hwnd()?;
        let reference_data = self.click_through_native.as_ref() as *const AtomicBool as usize;
        // SAFETY: hwnd is the live winit HWND. The callback and reference_data remain valid until
        // Drop removes this exact subclass before the boxed AtomicBool is released.
        let installed = unsafe {
            SetWindowSubclass(
                hwnd,
                Some(click_through_subclass),
                CLICK_THROUGH_SUBCLASS_ID,
                reference_data,
            )
        };
        if installed == 0 {
            return Err(PlatformError::ConfigureMouseHandling(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        self.subclass_installed = true;
        Ok(())
    }
}

impl Drop for NativePlatformBackend {
    fn drop(&mut self) {
        if !self.subclass_installed {
            return;
        }
        if let Ok(hwnd) = self.hwnd() {
            // SAFETY: hwnd and callback identify the subclass installed by this backend.
            unsafe {
                RemoveWindowSubclass(
                    hwnd,
                    Some(click_through_subclass),
                    CLICK_THROUGH_SUBCLASS_ID,
                );
            }
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

    fn reassert_window_order(&mut self) -> Result<(), PlatformError> {
        if let Some(enabled) = self.always_on_top {
            let level = if enabled {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            };
            self.window.set_window_level(level);
        }
        Ok(())
    }

    fn set_click_through(&mut self, enabled: bool) -> Result<(), PlatformError> {
        self.install_click_through_subclass()?;
        let native = self.click_through_native.as_ref();
        self.click_through.apply(enabled, |requested| {
            native.store(requested, Ordering::Relaxed);
            Ok::<_, PlatformError>(())
        })?;
        Ok(())
    }

    fn cursor_position(&self) -> Result<Option<DesktopPosition>, PlatformError> {
        let mut point = POINT::default();
        // SAFETY: point is a valid writable POINT for the duration of the call.
        if unsafe { GetCursorPos(&mut point) } == 0 {
            return Err(PlatformError::ReadCursorPosition(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        let scale_factor = self.window.scale_factor();
        Ok(Some(DesktopPosition::new(
            f64::from(point.x) / scale_factor,
            f64::from(point.y) / scale_factor,
        )))
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

pub(super) fn create_event_loop() -> Result<EventLoop<()>, winit::error::EventLoopError> {
    EventLoop::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_test_maps_pet_and_transparent_regions() {
        assert_eq!(hit_test_result(false), HTCLIENT as LRESULT);
        assert_eq!(hit_test_result(true), HTTRANSPARENT as LRESULT);
    }
}
