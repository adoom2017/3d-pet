//! Platform-neutral window capability boundary. Implemented beginning in Phase 1.

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;
