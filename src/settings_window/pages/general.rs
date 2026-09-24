//! General: startup, the show shortcut, and what happens to a transcript.
use gpui_kit::{div, prelude::*, px, Context, Div, Styled};

use crate::settings::{self};
use crate::settings_window::widgets::*;
use crate::settings_window::SettingsWindow;
use crate::tray;

impl SettingsWindow {
    pub(in crate::settings_window) fn toggle_menu_bar(&mut self, cx: &mut Context<Self>) {
        let mut prefs = settings::load();
        let visible = !prefs.show_in_menu_bar;
        match tray::set_icon_visible(visible, cx) {
            Ok(()) => {
                prefs.show_in_menu_bar = visible;
                settings::save(&prefs);
                self.error = None;
            }
            Err(err) => self.error = Some(err),
        }
        cx.notify();
    }

    pub(in crate::settings_window) fn toggle_startup(&mut self, cx: &mut Context<Self>) {
        self.hud
            .update(cx, |hud, cx| hud.toggle_open_on_startup(cx));
        cx.notify();
    }

    pub(in crate::settings_window) fn toggle_copy(&mut self, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.toggle_copy_notes(cx));
        cx.notify();
    }

    pub(in crate::settings_window) fn toggle_clean(&mut self, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.toggle_clean_fillers(cx));
        cx.notify();
    }

    pub(in crate::settings_window) fn begin_shortcut(&mut self, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.begin_hotkey_capture(cx));
        cx.notify();
    }

    pub(in crate::settings_window) fn general(&self, cx: &mut Context<Self>) -> Div {
        let (startup, shortcut, copy, clean, capturing) = {
            let hud = self.hud.read(cx);
            (
                hud.open_on_startup,
                hud.show_hotkey.clone(),
                hud.copy_notes,
                hud.clean_fillers,
                hud.recording_hotkey,
            )
        };
        let menu_bar = settings::load().show_in_menu_bar;
        div()
            .flex()
            .flex_col()
            .gap(px(26.0))
            .child(section(
                "Startup",
                vec![
                    setting_row(
                        "open-at-login",
                        "Open at login",
                        None,
                        self.switch("open-at-login", startup),
                        false,
                        cx,
                        |view, cx| view.toggle_startup(cx),
                    ),
                    setting_row(
                        "show-in-menu-bar",
                        "Show in menu bar",
                        Some("Keep the Whisple icon next to the clock."),
                        self.switch("show-in-menu-bar", menu_bar),
                        true,
                        cx,
                        |view, cx| view.toggle_menu_bar(cx),
                    ),
                ],
            ))
            .child(section(
                "Shortcut",
                vec![setting_row(
                    "show-whisple-hotkey",
                    "Show Whisple",
                    Some("Opens the bar from any app. Press again to stop."),
                    shortcut_control(&shortcut, capturing),
                    false,
                    cx,
                    |view, cx| view.begin_shortcut(cx),
                )],
            ))
            .child(section(
                "Output",
                vec![
                    setting_row(
                        "copy-to-clipboard",
                        "Copy to clipboard when done",
                        None,
                        self.switch("copy-to-clipboard", copy),
                        false,
                        cx,
                        |view, cx| view.toggle_copy(cx),
                    ),
                    setting_row(
                        "clean-up-notes",
                        "Clean up notes",
                        Some("Removes “um”, “uh” and repeated words."),
                        self.switch("clean-up-notes", clean),
                        true,
                        cx,
                        |view, cx| view.toggle_clean(cx),
                    ),
                ],
            ))
    }
}
