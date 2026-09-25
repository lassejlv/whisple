//! General: startup, the show shortcut, what happens to a transcript, and
//! the assistant's voice commands and screen context.
use gpui_kit::{div, prelude::*, px, Context, Div, Styled};

use crate::hotkey::Shortcut;
use crate::i18n::t;
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

    pub(in crate::settings_window) fn toggle_commands(&mut self, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.toggle_voice_commands(cx));
        cx.notify();
    }

    pub(in crate::settings_window) fn toggle_screen(&mut self, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.toggle_screen_context(cx));
        cx.notify();
    }

    pub(in crate::settings_window) fn begin_shortcut(
        &mut self,
        slot: Shortcut,
        cx: &mut Context<Self>,
    ) {
        self.hud
            .update(cx, |hud, cx| hud.begin_hotkey_capture(slot, cx));
        cx.notify();
    }

    pub(in crate::settings_window) fn general(&self, cx: &mut Context<Self>) -> Div {
        let (startup, shortcut, record, copy, clean, capturing, commands, screen) = {
            let hud = self.hud.read(cx);
            (
                hud.open_on_startup,
                hud.show_hotkey.clone(),
                hud.record_hotkey.clone(),
                hud.copy_notes,
                hud.clean_fillers,
                hud.recording_hotkey,
                hud.voice_commands,
                hud.screen_context,
            )
        };
        let menu_bar = settings::load().show_in_menu_bar;
        div()
            .flex()
            .flex_col()
            .gap(px(26.0))
            .child(section(
                t("Language"),
                vec![selector_row(
                    t("App language"),
                    Some(t("Menus, buttons and settings. Dictation is not affected.")),
                    selector_control(&self.app_language_select, t("App language"))
                        .into_any_element(),
                    false,
                )],
            ))
            .child(section(
                t("Startup"),
                vec![
                    setting_row(
                        "open-at-login",
                        t("Open at login"),
                        None,
                        self.switch("open-at-login", startup),
                        false,
                        cx,
                        |view, cx| view.toggle_startup(cx),
                    ),
                    setting_row(
                        "show-in-menu-bar",
                        t("Show in menu bar"),
                        Some(t("Keep the Whisple icon next to the clock.")),
                        self.switch("show-in-menu-bar", menu_bar),
                        true,
                        cx,
                        |view, cx| view.toggle_menu_bar(cx),
                    ),
                ],
            ))
            .child(section(
                t("Shortcuts"),
                vec![
                    setting_row(
                        "show-whisple-hotkey",
                        t("Show Whisple"),
                        Some(t("Opens the bar from any app. Press again to hide it.")),
                        shortcut_control(&shortcut, capturing == Some(Shortcut::Show)),
                        false,
                        cx,
                        |view, cx| view.begin_shortcut(Shortcut::Show, cx),
                    ),
                    setting_row(
                        "record-hotkey",
                        t("Start recording"),
                        Some(t("Opens the bar and starts recording. Press again to finish.")),
                        shortcut_control(&record, capturing == Some(Shortcut::Record)),
                        true,
                        cx,
                        |view, cx| view.begin_shortcut(Shortcut::Record, cx),
                    ),
                ],
            ))
            .child(section(
                t("Output"),
                vec![
                    setting_row(
                        "copy-to-clipboard",
                        t("Copy to clipboard when done"),
                        None,
                        self.switch("copy-to-clipboard", copy),
                        false,
                        cx,
                        |view, cx| view.toggle_copy(cx),
                    ),
                    setting_row(
                        "clean-up-notes",
                        t("Clean up notes"),
                        Some(t("Removes “um”, “uh” and repeated words.")),
                        self.switch("clean-up-notes", clean),
                        true,
                        cx,
                        |view, cx| view.toggle_clean(cx),
                    ),
                ],
            ))
            .child(section(
                t("Assistant"),
                vec![
                    setting_row(
                        "voice-commands",
                        t("Voice commands"),
                        Some(t("Say “Open Spotify” or “Go to github.com” to open it.")),
                        self.switch("voice-commands", commands),
                        false,
                        cx,
                        |view, cx| view.toggle_commands(cx),
                    ),
                    setting_row(
                        "screen-context",
                        t("Share screen with Whisple"),
                        Some(
                            t("Start with “Hey Whisple” to ask about your screen. It sends the app, window title, selection and a screenshot to your cloud provider."),
                        ),
                        self.switch("screen-context", screen),
                        true,
                        cx,
                        |view, cx| view.toggle_screen(cx),
                    ),
                ],
            ))
    }
}
