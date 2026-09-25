//! The menu bar owns its icon for the lifetime of the application. Commands
//! are consumed on GPUI's main thread by the same listener as the shortcut.

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
    #[cfg(target_os = "macos")]
    match super::macos::tray::install(cx) {
        Ok(()) => return crate::settings::load().show_in_menu_bar,
        Err(err) => eprintln!("could not create the menu bar icon: {err}"),
    }
    let _ = cx;
    false
}

pub fn take_command() -> Option<Command> {
    #[cfg(target_os = "macos")]
    return super::macos::tray::take_command();
    #[cfg(not(target_os = "macos"))]
    None
}

pub fn set_visible(visible: bool, cx: &App) {
    #[cfg(target_os = "macos")]
    super::macos::tray::set_visible(visible, cx);
    #[cfg(not(target_os = "macos"))]
    let _ = (visible, cx);
}

pub fn set_icon_visible(visible: bool, cx: &App) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return super::macos::tray::set_icon_visible(visible, cx);
    #[cfg(not(target_os = "macos"))]
    let _ = (visible, cx);
    #[cfg(not(target_os = "macos"))]
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn set_update(status: UpdateStatus<'_>, cx: &mut App) {
    super::macos::tray::set_update(status, cx);
}

/// Puts the menu bar menu in the current interface language.
pub fn relabel(cx: &App) {
    #[cfg(target_os = "macos")]
    super::macos::tray::relabel(cx);
    #[cfg(not(target_os = "macos"))]
    let _ = cx;
}
