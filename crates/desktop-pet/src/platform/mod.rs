//! Platform-neutral window capability boundary.

use std::sync::Arc;

use thiserror::Error;
use winit::window::{Window, WindowAttributes};

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
}

#[derive(Debug, Error)]
#[error("native window operation failed")]
pub(crate) struct PlatformError;

pub(crate) fn configure_window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    implementation::configure_window_attributes(attributes)
}

pub(crate) fn create_backend(window: Arc<Window>) -> Box<dyn PlatformBackend> {
    Box::new(implementation::NativePlatformBackend::new(window))
}
