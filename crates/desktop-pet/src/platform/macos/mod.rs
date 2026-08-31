//! AppKit-backed platform implementation.

use std::sync::Arc;

use objc2_app_kit::{NSEvent, NSScreen, NSView};
use objc2_foundation::MainThreadMarker;
use winit::{
    dpi::LogicalPosition,
    platform::macos::{MonitorHandleExtMacOS, WindowAttributesExtMacOS, WindowExtMacOS},
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{Window, WindowAttributes, WindowLevel},
};

use super::{IdempotentBool, PlatformBackend, PlatformError};
use crate::display::{DesktopPosition, LogicalSize, MonitorId, MonitorInfo};

pub(super) struct NativePlatformBackend {
    window: Arc<Window>,
    always_on_top: Option<bool>,
    click_through: IdempotentBool,
}

impl NativePlatformBackend {
    pub(super) fn new(window: Arc<Window>) -> Self {
        window.set_has_shadow(false);
        Self {
            window,
            always_on_top: None,
            click_through: IdempotentBool::default(),
        }
    }

    fn primary_screen_top() -> Result<f64, PlatformError> {
        let marker = MainThreadMarker::new().ok_or_else(|| {
            PlatformError::ReadCursorPosition(
                "AppKit cursor access must occur on the macOS main thread".to_owned(),
            )
        })?;
        let screens = NSScreen::screens(marker);
        // SAFETY: AppKit returns an immutable NSArray here; firstObject retains the primary
        // NSScreen, which is documented as the first element of NSScreen.screens.
        let primary = unsafe { screens.firstObject() }.ok_or_else(|| {
            PlatformError::ReadCursorPosition(
                "macOS did not report a primary screen for cursor normalization".to_owned(),
            )
        })?;
        let frame = primary.frame();
        Ok(frame.origin.y + frame.size.height)
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

    fn set_click_through(&mut self, enabled: bool) -> Result<(), PlatformError> {
        let window = Arc::clone(&self.window);
        self.click_through.apply(enabled, move |requested| {
            if MainThreadMarker::new().is_none() {
                return Err(PlatformError::ConfigureMouseHandling(
                    "NSWindow mouse handling must be configured on the macOS main thread"
                        .to_owned(),
                ));
            }
            let handle = window.window_handle().map_err(|error| {
                PlatformError::ConfigureMouseHandling(format!(
                    "winit did not expose an AppKit window handle: {error}"
                ))
            })?;
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                return Err(PlatformError::ConfigureMouseHandling(
                    "winit exposed a non-AppKit window handle on macOS".to_owned(),
                ));
            };
            // SAFETY: raw-window-handle guarantees ns_view is a live NSView owned by the winit
            // window, whose Arc remains alive throughout this backend call.
            let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
            let native_window = view.window().ok_or_else(|| {
                PlatformError::ConfigureMouseHandling(
                    "the winit NSView is not attached to an NSWindow".to_owned(),
                )
            })?;
            native_window.setIgnoresMouseEvents(requested);
            // SAFETY: the retained NSWindow is live on the AppKit main thread. Reading the
            // property immediately verifies that the window server accepted the requested state.
            let applied = unsafe { native_window.ignoresMouseEvents() };
            if applied != requested {
                return Err(PlatformError::ConfigureMouseHandling(format!(
                    "NSWindow reported ignoresMouseEvents={applied} after requesting {requested}"
                )));
            }
            Ok::<_, PlatformError>(())
        })?;
        Ok(())
    }

    fn cursor_position(&self) -> Result<Option<DesktopPosition>, PlatformError> {
        let primary_top = Self::primary_screen_top()?;
        // SAFETY: NSEvent's class cursor query is called from winit's AppKit main thread.
        let native = unsafe { NSEvent::mouseLocation() };
        let position = DesktopPosition::new(native.x, primary_top - native.y);
        if !position.is_finite() {
            return Err(PlatformError::ReadCursorPosition(format!(
                "AppKit returned non-finite coordinates ({}, {})",
                native.x, native.y
            )));
        }
        Ok(Some(position))
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
        let screens = NSScreen::screens(marker);
        // SAFETY: AppKit returns an immutable NSArray and documents its first item as the
        // primary screen. Retaining that item keeps it alive through this enumeration.
        let primary = unsafe { screens.firstObject() }.ok_or_else(|| {
            PlatformError::EnumerateMonitors("macOS did not report a primary screen".to_owned())
        })?;
        let primary_frame = primary.frame();
        let primary_top = primary_frame.origin.y + primary_frame.size.height;
        let primary_id = self
            .window
            .primary_monitor()
            .map(|monitor| monitor.native_id());

        let monitors: Vec<_> = self
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
                    primary_top - visible.origin.y - visible.size.height,
                );
                MonitorInfo::new(
                    MonitorId(u64::from(monitor.native_id())),
                    origin,
                    LogicalSize::new(visible.size.width, visible.size.height),
                    monitor.scale_factor(),
                    primary_id == Some(monitor.native_id()),
                )
            })
            .collect();
        if !monitors.is_empty() {
            return Ok(monitors);
        }

        tracing::warn!(
            screen_count = screens.len(),
            "winit returned no monitors; using the AppKit screen snapshot"
        );
        Ok((0..screens.len())
            .filter_map(|index| {
                let screen = screens.get(index)?;
                let visible = screen.visibleFrame();
                let origin = DesktopPosition::new(
                    visible.origin.x,
                    primary_top - visible.origin.y - visible.size.height,
                );
                MonitorInfo::new(
                    MonitorId((1_u64 << 63) | index as u64),
                    origin,
                    LogicalSize::new(visible.size.width, visible.size.height),
                    screen.backingScaleFactor(),
                    index == 0,
                )
            })
            .collect())
    }
}

pub(super) fn configure_window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes
        .with_has_shadow(false)
        .with_accepts_first_mouse(true)
}
