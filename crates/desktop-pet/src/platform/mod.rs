//! Platform-neutral window capability boundary.

use std::sync::Arc;

use thiserror::Error;
use winit::window::{Window, WindowAttributes};

use crate::display::DesktopPosition;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as implementation;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as implementation;

pub(crate) trait PlatformBackend {
    fn set_always_on_top(&mut self, enabled: bool) -> Result<(), PlatformError>;
    fn window_position(&self) -> Result<DesktopPosition, PlatformError>;
    fn set_window_position(&mut self, position: DesktopPosition) -> Result<(), PlatformError>;
}

#[derive(Debug, Error)]
pub(crate) enum PlatformError {
    #[error("failed to read the native window position: {0}")]
    ReadWindowPosition(#[source] winit::error::NotSupportedError),
    #[error("window position must contain finite logical coordinates, got ({x}, {y})")]
    InvalidWindowPosition { x: f64, y: f64 },
}

pub(crate) fn configure_window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    implementation::configure_window_attributes(attributes)
}

pub(crate) fn create_backend(window: Arc<Window>) -> Box<dyn PlatformBackend> {
    Box::new(implementation::NativePlatformBackend::new(window))
}
