//! The status icon behind `crate::platform::tray`: the macOS menu bar item and
//! the Windows notification area icon share one menu.

use crate::platform::tray::Command;
#[cfg(updates)]
use crate::platform::tray::UpdateStatus;
#[cfg(target_os = "macos")]
use cocoa::appkit::{NSApplication, NSApplicationActivationPolicy};
#[cfg(target_os = "macos")]
use cocoa::base::nil;
use gpui_kit::App;
use gpui_kit::Global;

use crate::i18n::t;
#[cfg(updates)]
use crate::i18n::tf;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

struct MenuBar {
    _icon: TrayIcon,
    show: MenuItem,
    hide: MenuItem,
    settings: MenuItem,
    /// Updates are offered where the updater runs.
    #[cfg(updates)]
    update: MenuItem,
    quit: MenuItem,
    /// The last update state, so the item can be relabelled.
    #[cfg(updates)]
    update_label: UpdateLabel,
}

#[cfg(updates)]
#[derive(Clone)]
enum UpdateLabel {
    Check,
    Checking,
    Available(String),
    UpToDate,
    Error,
}

#[cfg(updates)]
impl UpdateLabel {
    fn text(&self) -> String {
        match self {
            Self::Check if cfg!(feature = "licensing") => t("Check for Updates…").to_string(),
            Self::Check => t("Release notes on GitHub ↗").to_string(),
            Self::Checking => t("Checking for Updates…").to_string(),
            Self::Available(version) => tf("Whisple v{} Ready…", &[version]),
            Self::UpToDate => t("Whisple is Up to Date").to_string(),
            Self::Error => t("Update Check Failed — Retry").to_string(),
        }
    }
}

impl Global for MenuBar {}

pub fn install(cx: &mut App) -> Result<(), String> {
    let menu = Menu::new();
    let show = MenuItem::with_id("show", t("Show Whisple"), true, None);
    let hide = MenuItem::with_id("hide", t("Hide Whisple"), false, None);
    let settings = MenuItem::with_id("settings", t("Settings…"), true, None);
    #[cfg(updates)]
    let update = MenuItem::with_id("update", UpdateLabel::Check.text(), true, None);
    let quit = MenuItem::with_id("quit", t("Quit Whisple"), true, None);
    menu.append_items(&[&show, &hide, &settings])
        .map_err(|err| err.to_string())?;
    #[cfg(updates)]
    menu.append(&update).map_err(|err| err.to_string())?;
    menu.append_items(&[&PredefinedMenuItem::separator(), &quit])
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
    #[cfg(target_os = "macos")]
    unsafe {
        NSApplication::sharedApplication(nil).setActivationPolicy_(
            NSApplicationActivationPolicy::NSApplicationActivationPolicyAccessory,
        );
    }
    cx.set_global(MenuBar {
        _icon: icon,
        show,
        hide,
        settings,
        #[cfg(updates)]
        update,
        quit,
        #[cfg(updates)]
        update_label: UpdateLabel::Check,
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

/// Puts the menu in the current interface language.
pub fn relabel(cx: &App) {
    if let Some(menu) = cx.try_global::<MenuBar>() {
        menu.show.set_text(t("Show Whisple"));
        menu.hide.set_text(t("Hide Whisple"));
        menu.settings.set_text(t("Settings…"));
        #[cfg(updates)]
        menu.update.set_text(menu.update_label.text());
        menu.quit.set_text(t("Quit Whisple"));
    }
}

#[cfg(updates)]
pub fn set_update(status: UpdateStatus<'_>, cx: &mut App) {
    if cx.try_global::<MenuBar>().is_some() {
        let label = match status {
            UpdateStatus::Checking => UpdateLabel::Checking,
            UpdateStatus::Available(version) => UpdateLabel::Available(version.to_string()),
            UpdateStatus::UpToDate => UpdateLabel::UpToDate,
            UpdateStatus::Error => UpdateLabel::Error,
        };
        cx.global_mut::<MenuBar>().update_label = label;
        let menu = cx.global::<MenuBar>();
        let update_ready = matches!(status, UpdateStatus::Available(_));
        menu.update.set_text(menu.update_label.text());
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
    // Windows has no template icons, so the bars take the colour that
    // contrasts with the taskbar.
    let ink: u8 = if cfg!(target_os = "windows") && !light_taskbar() {
        255
    } else {
        0
    };
    let mut rgba = [ink, ink, ink, 0].repeat(36 * 36);
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

#[cfg(target_os = "windows")]
fn light_taskbar() -> bool {
    super::windows::light_taskbar()
}

#[cfg(not(target_os = "windows"))]
fn light_taskbar() -> bool {
    false
}
