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

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use cocoa::appkit::{NSApplication, NSApplicationActivationPolicy};
    use cocoa::base::nil;
    use gpui_kit::Global;
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
        Icon, TrayIcon, TrayIconBuilder,
    };

    struct MenuBar {
        _icon: TrayIcon,
        hide: MenuItem,
        update: MenuItem,
    }

    impl Global for MenuBar {}

    pub fn install(cx: &mut App) -> Result<(), String> {
        let menu = Menu::new();
        let show = MenuItem::with_id("show", "Show Whisple", true, None);
        let hide = MenuItem::with_id("hide", "Hide Whisple", false, None);
        let settings = MenuItem::with_id("settings", "Settings…", true, None);
        let update = MenuItem::with_id("update", "Check for Updates…", true, None);
        let quit = MenuItem::with_id("quit", "Quit Whisple", true, None);
        menu.append_items(&[
            &show,
            &hide,
            &settings,
            &update,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|err| err.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("Whisple")
            .with_icon(waveform_icon(false))
            .with_icon_as_template(true)
            .with_menu(Box::new(menu))
            .build()
            .map_err(|err| err.to_string())?;
        icon.set_visible(crate::settings::load().show_in_menu_bar)
            .map_err(|err| err.to_string())?;
        // GPUI sets Regular before running the launch callback. Change it
        // only after the status item exists, so Whisple is always reachable.
        unsafe {
            NSApplication::sharedApplication(nil).setActivationPolicy_(
                NSApplicationActivationPolicy::NSApplicationActivationPolicyAccessory,
            );
        }
        cx.set_global(MenuBar {
            _icon: icon,
            hide,
            update,
        });
        Ok(())
    }

    pub fn take_command() -> Option<Command> {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            match event.id.0.as_str() {
                "show" => return Some(Command::Show),
                "hide" => return Some(Command::Hide),
                "settings" => return Some(Command::Settings),
                "update" => return Some(Command::Update),
                "quit" => return Some(Command::Quit),
                _ => {}
            }
        }
        None
    }

    pub fn set_visible(visible: bool, cx: &App) {
        if let Some(menu) = cx.try_global::<MenuBar>() {
            menu.hide.set_enabled(visible);
        }
    }

    pub fn set_icon_visible(visible: bool, cx: &App) -> Result<(), String> {
        if let Some(menu) = cx.try_global::<MenuBar>() {
            menu._icon
                .set_visible(visible)
                .map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    pub fn set_update(status: UpdateStatus<'_>, cx: &App) {
        if let Some(menu) = cx.try_global::<MenuBar>() {
            let update_ready = matches!(status, UpdateStatus::Available(_));
            let label = match status {
                UpdateStatus::Checking => "Checking for Updates…".to_string(),
                UpdateStatus::Available(version) => format!("Install Whisple v{version}…"),
                UpdateStatus::UpToDate => "Whisple is Up to Date".to_string(),
                UpdateStatus::Error => "Update Check Failed — Retry".to_string(),
            };
            menu.update.set_text(label);
            menu.update
                .set_enabled(!matches!(status, UpdateStatus::Checking));
            if let Err(err) = menu._icon.set_icon(Some(waveform_icon(update_ready))) {
                eprintln!("could not refresh the menu bar icon: {err}");
            }
        }
    }

    fn waveform_icon(update_ready: bool) -> Icon {
        // Paper's five-bar Whisple mark as an 18pt monochrome template.
        // AppKit supplies light/dark appearance, including the update dot.
        let mut rgba = vec![0; 36 * 36 * 4];
        for (bar, height) in [10.0_f32, 18.0, 24.0, 16.0, 8.0].into_iter().enumerate() {
            let center_x = 4.0 + bar as f32 * 7.0;
            let half_line = height * 0.5 - 2.0;
            for y in 0..36 {
                for x in 0..36 {
                    let dx = (x as f32 + 0.5 - center_x).abs();
                    let dy = ((y as f32 + 0.5 - 18.0).abs() - half_line).max(0.0);
                    let alpha = ((2.5 - dx.hypot(dy)).clamp(0.0, 1.0) * 255.0) as u8;
                    let offset = (y * 36 + x) * 4 + 3;
                    rgba[offset] = rgba[offset].max(alpha);
                }
            }
        }
        if update_ready {
            for y in 4..12 {
                for x in 29..36 {
                    let distance =
                        ((x as f32 + 0.5 - 32.5).powi(2) + (y as f32 + 0.5 - 7.5).powi(2)).sqrt();
                    let alpha = ((3.5 - distance).clamp(0.0, 1.0) * 255.0) as u8;
                    let offset = (y * 36 + x) * 4 + 3;
                    rgba[offset] = rgba[offset].max(alpha);
                }
            }
        }
        Icon::from_rgba(rgba, 36, 36).expect("36px RGBA menu bar icon")
    }
}

pub fn install(cx: &mut App) -> bool {
    #[cfg(target_os = "macos")]
    match macos::install(cx) {
        Ok(()) => return crate::settings::load().show_in_menu_bar,
        Err(err) => eprintln!("could not create the menu bar icon: {err}"),
    }
    let _ = cx;
    false
}

pub fn take_command() -> Option<Command> {
    #[cfg(target_os = "macos")]
    return macos::take_command();
    #[cfg(not(target_os = "macos"))]
    None
}

pub fn set_visible(visible: bool, cx: &App) {
    #[cfg(target_os = "macos")]
    macos::set_visible(visible, cx);
    #[cfg(not(target_os = "macos"))]
    let _ = (visible, cx);
}

pub fn set_icon_visible(visible: bool, cx: &App) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return macos::set_icon_visible(visible, cx);
    #[cfg(not(target_os = "macos"))]
    let _ = (visible, cx);
    #[cfg(not(target_os = "macos"))]
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn set_update(status: UpdateStatus<'_>, cx: &App) {
    macos::set_update(status, cx);
}
