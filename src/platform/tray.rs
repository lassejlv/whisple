//! The menu bar (macOS) or notification area (Windows) owns its icon for the
//! lifetime of the application. Commands are consumed on GPUI's main thread by
//! the same listener as the shortcut.

use gpui_kit::App;

#[derive(Clone, Copy)]
pub enum Command {
    Show,
    Hide,
    Settings,
    Update,
    Quit,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
pub enum UpdateStatus<'a> {
    Checking,
    Available(&'a str),
    UpToDate,
    Error,
}

pub fn install(cx: &mut App) -> bool {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    match super::system_tray::install(cx) {
        Ok(()) => return crate::settings::load().show_in_menu_bar,
        Err(err) => eprintln!("could not create the tray icon: {err}"),
    }
    let _ = cx;
    false
}

pub fn take_command() -> Option<Command> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    return super::system_tray::take_command();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    None
}

pub fn set_visible(visible: bool, cx: &App) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    super::system_tray::set_visible(visible, cx);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (visible, cx);
}

pub fn set_icon_visible(visible: bool, cx: &App) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    return super::system_tray::set_icon_visible(visible, cx);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (visible, cx);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn set_update(status: UpdateStatus<'_>, cx: &mut App) {
    super::system_tray::set_update(status, cx);
}

/// Puts the menu bar menu in the current interface language.
pub fn relabel(cx: &App) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    super::system_tray::relabel(cx);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = cx;
}
