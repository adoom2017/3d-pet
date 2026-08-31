//! AppKit-backed platform implementation.

use std::sync::Arc;

use objc2_app_kit::NSScreen;
use objc2_foundation::MainThreadMarker;
use winit::{
    dpi::LogicalPosition,
    platform::macos::{MonitorHandleExtMacOS, WindowAttributesExtMacOS, WindowExtMacOS},
    window::{Window, WindowAttributes, WindowLevel},
};

use super::{PlatformBackend, PlatformError};
use crate::display::{DesktopPosition, LogicalSize, MonitorId, MonitorInfo};

pub(super) struct NativePlatformBackend {
    window: Arc<Window>,
    always_on_top: Option<bool>,
}

impl NativePlatformBackend {
    pub(super) fn new(window: Arc<Window>) -> Self {
        window.set_has_shadow(false);
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
        let marker = MainThreadMarker::new().ok_or_else(|| {
            PlatformError::EnumerateMonitors(
                "NSScreen access must occur on the macOS main thread".to_owned(),
            )
        })?;
        let main_screen = NSScreen::mainScreen(marker).ok_or_else(|| {
            PlatformError::EnumerateMonitors("macOS did not report a main screen".to_owned())
        })?;
        let main_height = main_screen.frame().size.height;
        let primary_id = self
            .window
            .primary_monitor()
            .map(|monitor| monitor.native_id());

        Ok(self
            .window
            .available_monitors()
            .filter_map(|monitor| {
                let screen_pointer = monitor.ns_screen()?;
                // SAFETY: winit returns the live NSScreen backing this MonitorHandle. This
                // method runs on the main thread and the handle keeps the screen reachable.
                let screen = unsafe { &*screen_pointer.cast::<NSScreen>() };
                let visible = screen.visibleFrame();
                let origin = DesktopPosition::new(
                    visible.origin.x,
                    main_height - visible.origin.y - visible.size.height,
                );
                MonitorInfo::new(
                    MonitorId(u64::from(monitor.native_id())),
                    origin,
                    LogicalSize::new(visible.size.width, visible.size.height),
                    monitor.scale_factor(),
                    primary_id == Some(monitor.native_id()),
                )
            })
            .collect())
    }
}

pub(super) fn configure_window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes.with_has_shadow(false)
}
