//! Platform-neutral window capability boundary.

use std::sync::Arc;

use thiserror::Error;
use winit::window::{Window, WindowAttributes};

use crate::display::{DesktopPosition, MonitorInfo};

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
    fn reassert_window_order(&mut self) -> Result<(), PlatformError>;
    fn set_click_through(&mut self, enabled: bool) -> Result<(), PlatformError>;
    fn cursor_position(&self) -> Result<Option<DesktopPosition>, PlatformError>;
    fn window_position(&self) -> Result<DesktopPosition, PlatformError>;
    fn set_window_position(&mut self, position: DesktopPosition) -> Result<(), PlatformError>;
    fn monitors(&self) -> Result<Vec<MonitorInfo>, PlatformError>;
}

#[derive(Debug, Error)]
pub(crate) enum PlatformError {
    #[error("failed to read the native window position: {0}")]
    ReadWindowPosition(#[source] winit::error::NotSupportedError),
    #[error("window position must contain finite logical coordinates, got ({x}, {y})")]
    InvalidWindowPosition { x: f64, y: f64 },
    #[error("failed to enumerate monitor work areas: {0}")]
    EnumerateMonitors(String),
    #[error("failed to configure native mouse handling: {0}")]
    ConfigureMouseHandling(String),
    #[cfg(target_os = "macos")]
    #[error("failed to configure native window ordering: {0}")]
    ConfigureWindowOrdering(String),
    #[error("failed to read the global cursor position: {0}")]
    ReadCursorPosition(String),
}

#[derive(Debug, Default)]
pub(super) struct IdempotentBool {
    current: Option<bool>,
}

impl IdempotentBool {
    pub fn apply<E>(
        &mut self,
        requested: bool,
        apply_native: impl FnOnce(bool) -> Result<(), E>,
    ) -> Result<bool, E> {
        if self.current == Some(requested) {
            return Ok(false);
        }
        apply_native(requested)?;
        self.current = Some(requested);
        Ok(true)
    }
}

pub(crate) fn configure_window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    implementation::configure_window_attributes(attributes)
}

pub(crate) fn create_backend(window: Arc<Window>) -> Box<dyn PlatformBackend> {
    Box::new(implementation::NativePlatformBackend::new(window))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::IdempotentBool;

    #[test]
    fn native_boolean_updates_are_idempotent() {
        let calls = Cell::new(0);
        let mut state = IdempotentBool::default();
        for requested in [false, false, true, true, false] {
            state
                .apply(requested, |_| {
                    calls.set(calls.get() + 1);
                    Ok::<_, ()>(())
                })
                .unwrap();
        }
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn failed_native_update_is_retried() {
        let mut state = IdempotentBool::default();
        assert!(state.apply(true, |_| Err::<(), _>(())).is_err());
        assert_eq!(state.apply(true, |_| Ok::<_, ()>(())), Ok(true));
    }
}
