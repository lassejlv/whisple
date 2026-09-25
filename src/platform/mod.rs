pub(crate) mod hotkey;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub(crate) mod linux;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
pub(crate) mod microphone_permission;
pub(crate) mod placement;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod system_tray;
pub(crate) mod tray;
#[cfg(target_os = "windows")]
pub(crate) mod windows;
