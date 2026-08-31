//! Win32-backed platform implementation.

use std::sync::Arc;

use winit::{
    dpi::LogicalPosition,
    window::{Window, WindowAttributes, WindowLevel},
};

use super::{PlatformBackend, PlatformError};
use crate::display::DesktopPosition;

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
}

pub(super) fn configure_window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes
}
