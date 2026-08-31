//! Win32-backed platform implementation.

use std::sync::Arc;

use winit::window::{Window, WindowAttributes, WindowLevel};

use super::{PlatformBackend, PlatformError};

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
}

pub(super) fn configure_window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes
}
