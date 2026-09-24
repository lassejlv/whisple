use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::{
    div, prelude::*, px, Animation, AnimationExt, AnyElement, BoxShadow, Context, Div, Entity,
    Focusable, FontWeight, IntoElement, ParentElement, Render, Rgba, SharedString, Stateful,
    Styled, Window,
};

use crate::app::{
    Phase, Reveal, SettingsPage, Whisp, BAR_HEIGHT, COLLAPSED_HEIGHT, ERROR_EXTRA, WINDOW_RADIUS,
};
use crate::hotkey;
use crate::models;
use crate::motion;
use crate::settings;
use crate::theme;

/// Language rows visible at once; the list scrolls past this.
const LANGUAGE_LIST_H: f32 = 280.0;
/// The microphone page has no search field, so its list takes that room too.
const MICROPHONE_LIST_H: f32 = 324.0;

impl Render for Whisp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick(window, cx);
        self.sync_language_search(window, cx);
        let error = self.error.clone();

        div()
            .id("whisp")
            .key_context("Whisp")
            .track_focus(&self.focus_handle)
            .on_action(
                cx.listener(|this, _: &crate::app::ToggleListen, window, cx| {
                    // Space belongs to the search field while it has focus.
                    if let Some(search) = this.focused_search(window, cx) {
                        search.update(cx, |search, cx| search.insert(" ", window, cx));
                        return;
                    }
                    this.toggle_listen(cx);
                }),
            )
            .on_action(cx.listener(|this, _: &crate::app::CloseOverlay, _, cx| {
                this.close_overlay(cx);
            }))
            .on_action(cx.listener(|this, _: &crate::app::CopyResult, _, cx| {
                this.copy_result(cx);
            }))
            .size_full()
            .font_family(theme::UI_FONT)
            .text_color(theme::LABEL)
            .flex()
            .flex_col()
            .justify_end()
            // The window can be taller than the HUD while a panel animates
            // (see `window_height`), so the HUD carries its own height and the
            // space above it stays transparent.
            .child(
                div()
                    .h(px(self.chrome.value))
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .overflow_hidden()
                    .rounded(px(WINDOW_RADIUS))
                    .bg(theme::HUD)
                    .border_1()
                    .border_color(theme::HAIRLINE)
                    .shadow(vec![theme::top_edge(theme::EDGE)])
                    .when(self.reveal.is_some(), |column| column.child(self.panel(cx)))
                    .child(self.bar(cx))
                    .when(error.is_some(), |column| {
                        column.child(error_line(error.unwrap_or_default()))
                    }),
            )
    }
}

impl Whisp {
    /// Keep the language search field alive only while its page shows, so it
    /// always opens empty.
    fn sync_language_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let showing = self.settings_open && self.settings_page == SettingsPage::Language;
        if !showing {
            // Dropping a focused field would leave nothing focused, and the
            // bar's Space and Escape bindings would go quiet.
            if self.focused_search(window, cx).is_some() {
                window.focus(&self.focus_handle, cx);
            }
            self.language_search = None;
            return;
        }
        if self.language_search.is_some() {
            return;
        }
        let count = settings::Preferences::languages()
            .iter()
            .filter(|language| language.id != "auto")
            .count();
        let state = cx
            .new(|cx| InputState::new(window, cx).placeholder(format!("Search {count} languages")));
        cx.subscribe(&state, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        self.language_search = Some(state);
    }

    fn focused_search(&self, window: &Window, cx: &gpui_kit::App) -> Option<Entity<InputState>> {
        self.language_search
            .clone()
            .filter(|search| search.focus_handle(cx).is_focused(window))
    }

    fn panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let extra = self.panel_height(self.reveal);
        let error_h = if self.error.is_some() {
            ERROR_EXTRA
        } else {
            0.0
        };
        let shown = (self.chrome.value - COLLAPSED_HEIGHT - error_h).clamp(0.0, extra);
        let fade = if extra > 0.0 {
            (shown / extra).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let body = match self.reveal {
            Some(Reveal::Picker) => self.picker(cx).into_any_element(),
            Some(Reveal::Result) => self
                .transcript_card(self.last_text.clone(), cx)
                .into_any_element(),
            Some(Reveal::Settings) => self.settings_panel(cx).into_any_element(),
            None => div().into_any_element(),
        };

        div()
            .w_full()
            .h(px(shown))
            .flex_shrink_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .justify_end()
            .child(
                div()
                    .h(px(extra))
                    .w_full()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .opacity(fade)
                    .child(body)
                    .child(separator()),
            )
    }

    // MARK: Bar

    fn bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let listening = matches!(self.phase, Phase::Listening(_));
        let center = if listening {
            waveform(&self.bars).into_any_element()
        } else {
            let (label, color) = match &self.phase {
                Phase::Transcribing => ("Transcribing…", theme::SECONDARY),
                Phase::Idle if self.selected_ready() => ("Start recording", theme::LABEL),
                Phase::Result(_) if self.selected_ready() => ("Record again", theme::LABEL),
                _ => ("Choose a model", theme::LABEL),
            };
            div()
                .text_size(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .line_height(px(20.0))
                .text_color(color)
                .whitespace_nowrap()
                .text_ellipsis()
                .child(label)
                .into_any_element()
        };
        let hint = match &self.phase {
            Phase::Listening(_) => self.listening_for().map(|elapsed| {
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .font_features(theme::tabular())
                    .text_color(theme::SECONDARY)
                    .child(clock(elapsed))
            }),
            Phase::Transcribing => Some(hint_text("On device".to_string())),
            _ => Some(hint_text(hotkey::symbols(&self.show_hotkey))),
        };

        div()
            .h(px(BAR_HEIGHT))
            .flex_shrink_0()
            .w_full()
            .px(px(10.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .child(self.voice_button(cx))
            .child(
                press_handlers(
                    div().id("speak").flex_1().min_w_0().overflow_hidden(),
                    "voice",
                    cx,
                    |this, cx| this.toggle_listen(cx),
                )
                .child(center),
            )
            .children(hint)
            .child(self.settings_button(cx))
            .child(self.model_capsule(cx))
    }

    fn voice_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let size = 36.0 * self.press_scale("voice");
        let circle = div()
            .size(px(size))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center();
        let circle = match self.phase {
            Phase::Listening(_) => circle
                .bg(theme::RED)
                .shadow(vec![
                    BoxShadow::new(px(0.0), px(0.0), theme::tone(theme::RED_RING))
                        .spread_radius(px(4.0)),
                    BoxShadow::new(px(0.0), px(4.0), theme::tone(theme::RED_GLOW))
                        .blur_radius(px(14.0)),
                ])
                .child(div().size(px(11.0)).rounded(px(3.0)).bg(theme::KNOB))
                .into_any_element(),
            Phase::Transcribing => circle
                .relative()
                .bg(theme::AMBER_SOFT)
                .child(div().absolute().top_0().left_0().child(asset_icon(
                    "ring-track",
                    theme::AMBER_TRACK,
                    size,
                )))
                .child(div().absolute().top_0().left_0().child(
                    asset_icon("ring-arc", theme::AMBER, size).with_animation(
                        "transcribing",
                        Animation::new(Duration::from_millis(900)).repeat(),
                        |icon, delta| icon.rotate(gpui_kit::radians(delta * std::f32::consts::TAU)),
                    ),
                ))
                .child(voice_glyph().opacity(0.55))
                .into_any_element(),
            _ => circle
                .bg(theme::AMBER_SOFT)
                .child(voice_glyph())
                .into_any_element(),
        };

        press_handlers(
            div()
                .id("voice")
                .size(px(36.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center(),
            "voice",
            cx,
            |this, cx| this.toggle_listen(cx),
        )
        .child(circle)
    }

    fn settings_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let size = 28.0 * self.press_scale("settings");
        let open = self.settings_open;
        press_handlers(
            div()
                .id("settings")
                .size(px(28.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center(),
            "settings",
            cx,
            |this, cx| this.toggle_settings(cx),
        )
        .child(
            div()
                .size(px(size))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .when(open, |circle| circle.bg(theme::AMBER_SOFT))
                .child(
                    Icon::empty()
                        .path("icons/gear.svg")
                        .text_color(if open { theme::AMBER } else { theme::SECONDARY })
                        .with_size(px(15.0)),
                ),
        )
    }

    fn model_capsule(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.press_scale("models");
        let open = self.picker_open;
        let label = if self.selected_ready() {
            self.selected_spec().chip
        } else {
            "Models"
        };
        let capsule = div()
            .h(px(28.0 * scale))
            .pl(px(11.0))
            .pr(px(8.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(5.0))
            .rounded_full()
            .text_size(px(12.0))
            .font_weight(FontWeight::SEMIBOLD)
            .child(label)
            .child(asset_icon(
                "chevrons-up-down",
                if open { theme::AMBER } else { theme::SECONDARY },
                11.0,
            ));
        let capsule = if open {
            capsule
                .bg(theme::AMBER_SOFT)
                .shadow(vec![theme::inner_ring(theme::AMBER_RING)])
                .text_color(theme::AMBER)
        } else {
            capsule
                .bg(theme::RAISED)
                .shadow(vec![theme::top_edge(theme::SHEEN)])
                .text_color(theme::LABEL)
        };

        press_handlers(
            div()
                .id("models")
                .h(px(28.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center(),
            "models",
            cx,
            |this, cx| this.toggle_picker(cx),
        )
        .child(capsule)
    }

    // MARK: Result

    fn transcript_card(&self, text: String, cx: &mut Context<Self>) -> impl IntoElement {
        let words = text.split_whitespace().count();
        let meta = format!(
            "{words} {} · {}",
            if words == 1 { "word" } else { "words" },
            clock(self.recorded)
        );
        div()
            .px(px(18.0))
            .pt(px(18.0))
            .pb(px(12.0))
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .h(px(44.0))
                    .text_size(px(15.0))
                    .line_height(px(22.0))
                    .text_color(theme::LABEL)
                    .whitespace_normal()
                    .line_clamp(2)
                    .child(text),
            )
            .child(
                div()
                    .h(px(28.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .font_features(theme::tabular())
                            .text_color(theme::TERTIARY)
                            .child(meta),
                    )
                    .child(self.copy_button(cx)),
            )
    }

    fn copy_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.press_scale("copy");
        let copied = self.copied;
        press_handlers(
            div()
                .id("copy")
                .h(px(28.0))
                .flex()
                .items_center()
                .justify_center(),
            "copy",
            cx,
            |this, cx| this.copy_result(cx),
        )
        .child(
            div()
                .h(px(28.0 * scale))
                .pl(px(10.0))
                .pr(px(12.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .rounded_full()
                .bg(theme::RAISED)
                .shadow(vec![theme::top_edge(theme::SHEEN)])
                .text_size(px(12.0))
                .child(if copied {
                    asset_icon("check-bold", theme::AMBER, 13.0)
                } else {
                    lucide(Lucide::Copy, theme::LABEL, 13.0)
                })
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::LABEL)
                        .child(if copied { "Copied" } else { "Copy" }),
                )
                .when(!copied, |pill| {
                    pill.child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::TERTIARY)
                            .child("⌘C"),
                    )
                }),
        )
    }

    // MARK: Model picker

    fn picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self
            .models
            .iter()
            .enumerate()
            .map(|(index, model)| {
                model_row(
                    self,
                    model.spec,
                    model.ready,
                    index == 0,
                    entrance(index, self.picker_opened_at),
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let used = self.storage_used();

        div()
            .px(px(10.0))
            .pt(px(18.0))
            .pb(px(12.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(panel_header(
                "Voice models",
                Some("Runs on your Mac. Nothing leaves it."),
            ))
            .child(group().children(rows))
            .child(
                div()
                    .h(px(28.0))
                    .px(px(8.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        press_handlers(
                            div()
                                .id("sample")
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .opacity(if self.press_id.as_deref() == Some("sample") {
                                    0.6
                                } else {
                                    1.0
                                }),
                            "sample",
                            cx,
                            |this, cx| this.transcribe_sample(cx),
                        )
                        .child(asset_icon("play", theme::AMBER, 12.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::AMBER)
                                .child("Transcribe a sample"),
                        ),
                    )
                    .when(used > 0, |row| {
                        row.child(
                            div()
                                .text_size(px(12.0))
                                .text_color(theme::TERTIARY)
                                .child(format!("{} used", models::format_size(used))),
                        )
                    }),
            )
    }

    // MARK: Settings

    fn settings_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let page = match self.settings_page {
            SettingsPage::Language => self.language_page(cx).into_any_element(),
            SettingsPage::Microphone => self.microphone_page(cx).into_any_element(),
            SettingsPage::Main => self.settings_main(cx).into_any_element(),
        };
        div().opacity(self.page_fade.value).child(page)
    }

    fn settings_main(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let opened = self.settings_opened_at;
        let shortcut = if self.recording_hotkey {
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::AMBER)
                .child("Press shortcut")
                .into_any_element()
        } else {
            keycaps(hotkey::keycaps(&self.show_hotkey)).into_any_element()
        };
        let quit_view = cx.weak_entity();

        div()
            .px(px(10.0))
            .pt(px(18.0))
            .pb(px(14.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .px(px(8.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(title("Settings"))
                    .child(
                        div()
                            .id("quit")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                quit_view.update(cx, |this, cx| this.quit(cx)).ok();
                            })
                            .child(div().text_color(theme::SECONDARY).child("Quit Whisp"))
                            .child(div().text_color(theme::TERTIARY).child("⌘Q")),
                    ),
            )
            .child(
                group()
                    .child(self.settings_row(
                        "hotkey",
                        asset_icon("keyboard", theme::LABEL, 15.0),
                        true,
                        entrance(0, opened),
                        row_title("Show Whisp"),
                        shortcut,
                        cx,
                        |this, cx| this.begin_hotkey_capture(cx),
                    ))
                    .child(self.settings_row(
                        "startup",
                        lucide(Lucide::Power, theme::LABEL, 14.0),
                        false,
                        entrance(1, opened),
                        row_title("Open at login"),
                        switch(self.open_on_startup).into_any_element(),
                        cx,
                        |this, cx| this.toggle_open_on_startup(cx),
                    )),
            )
            .child(
                group()
                    .child(self.settings_row(
                        "microphone",
                        lucide(Lucide::Mic, theme::LABEL, 14.0),
                        true,
                        entrance(2, opened),
                        row_title("Microphone"),
                        drill_value(settings::microphone_label(&self.input_device).to_string()),
                        cx,
                        |this, cx| this.open_microphones(cx),
                    ))
                    .child(self.settings_row(
                        "language",
                        lucide(Lucide::Globe, theme::LABEL, 14.0),
                        false,
                        entrance(3, opened),
                        row_title("Language"),
                        drill_value(settings::language_name(&self.language).to_string()),
                        cx,
                        |this, cx| this.open_languages(cx),
                    )),
            )
            .child(
                group()
                    .child(self.settings_row(
                        "copy-notes",
                        lucide(Lucide::Clipboard, theme::LABEL, 14.0),
                        true,
                        entrance(4, opened),
                        row_title("Copy to clipboard"),
                        switch(self.copy_notes).into_any_element(),
                        cx,
                        |this, cx| this.toggle_copy_notes(cx),
                    ))
                    .child(
                        self.settings_row(
                            "clean",
                            lucide(Lucide::Sparkles, theme::LABEL, 14.0),
                            false,
                            entrance(5, opened),
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(1.0))
                                .child(row_title("Clean up notes"))
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .line_height(px(16.0))
                                        .text_color(theme::SECONDARY)
                                        .child("Removes “um”, “uh” and repeats"),
                                )
                                .into_any_element(),
                            switch(self.clean_fillers).into_any_element(),
                            cx,
                            |this, cx| this.toggle_clean_fillers(cx),
                        ),
                    ),
            )
    }

    /// A System Settings row: icon tile, then a content lane whose hairline
    /// starts past the tile.
    #[allow(clippy::too_many_arguments)]
    fn settings_row(
        &self,
        id: &str,
        icon: Icon,
        first: bool,
        opacity: f32,
        label: AnyElement,
        trailing: AnyElement,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Whisp, &mut Context<Whisp>) + 'static,
    ) -> impl IntoElement {
        let pressed = self.press_scale(id) < 1.0;
        press_handlers(
            div()
                .id(SharedString::from(id.to_string()))
                .w_full()
                .pl(px(12.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .opacity(opacity)
                .when(pressed, |row| row.bg(theme::HAIRLINE)),
            id,
            cx,
            action,
        )
        .child(tile(icon))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .min_h(px(46.0))
                .py(px(9.0))
                .pr(px(12.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .when(!first, |lane| {
                    lane.border_t_1().border_color(theme::HAIRLINE)
                })
                .child(label)
                .child(trailing),
        )
    }

    fn language_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self
            .language_search
            .as_ref()
            .map(|search| search.read(cx).value().to_lowercase())
            .unwrap_or_default();
        let selected = self.language.clone();
        let rows = settings::Preferences::languages()
            .iter()
            .filter(|language| {
                query.is_empty()
                    || language.name.to_lowercase().contains(&query)
                    || language.native.to_lowercase().contains(&query)
            })
            .enumerate()
            .map(|(index, language)| {
                let id = language.id.to_string();
                choice_row(
                    self,
                    format!("lang-{id}"),
                    language.name.to_string(),
                    language.native,
                    selected == language.id,
                    index == 0,
                    cx,
                    move |this, cx| this.choose_language(&id, cx),
                )
            })
            .collect::<Vec<_>>();

        let search = self.language_search.as_ref().map(|state| {
            div()
                .h(px(32.0))
                .px(px(10.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(7.0))
                .rounded(px(9.0))
                .bg(theme::INSET)
                .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
                .text_size(px(13.0))
                .child(asset_icon("search", theme::TERTIARY, 13.0))
                .child(
                    div().flex_1().child(
                        Input::new(state)
                            .appearance(false)
                            .px_0()
                            .py_0()
                            .h(px(18.0))
                            .text_size(px(13.0))
                            .line_height(px(18.0)),
                    ),
                )
        });

        div()
            .px(px(10.0))
            .pt(px(14.0))
            .pb(px(14.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(self.nav_bar("lang-back", "Language", cx))
            .children(search)
            .child(scroll_group("language-list", LANGUAGE_LIST_H, rows))
    }

    fn microphone_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.input_device.clone();
        let mut choices = vec![(String::new(), "System default".to_string())];
        for name in &self.microphones {
            choices.push((name.clone(), name.clone()));
        }
        let rows = choices
            .into_iter()
            .enumerate()
            .map(|(index, (name, label))| {
                let on = selected == name;
                choice_row(
                    self,
                    format!("mic-{index}"),
                    label,
                    "",
                    on,
                    index == 0,
                    cx,
                    move |this, cx| this.choose_microphone(&name, cx),
                )
            })
            .collect::<Vec<_>>();

        div()
            .px(px(10.0))
            .pt(px(14.0))
            .pb(px(14.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(self.nav_bar("mic-back", "Microphone", cx))
            .child(scroll_group("microphone-list", MICROPHONE_LIST_H, rows))
    }

    /// A centered title with a "‹ Settings" back control on the left.
    fn nav_bar(&self, id: &'static str, heading: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let dim = self.press_scale(id) < 1.0;
        div()
            .relative()
            .h(px(28.0))
            .flex()
            .items_center()
            .justify_center()
            .child(title(heading))
            .child(
                press_handlers(
                    div()
                        .id(id)
                        .absolute()
                        .left(px(2.0))
                        .top(px(4.0))
                        .h(px(20.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(2.0))
                        .opacity(if dim { 0.6 } else { 1.0 }),
                    id,
                    cx,
                    |this, cx| {
                        this.show_main_page();
                        cx.notify();
                    },
                )
                .child(asset_icon("chevron-left-bold", theme::AMBER, 16.0))
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::AMBER)
                        .child("Settings"),
                ),
            )
    }
}

fn model_row(
    app: &Whisp,
    spec: &'static models::ModelSpec,
    ready: bool,
    first: bool,
    opacity: f32,
    cx: &mut Context<Whisp>,
) -> AnyElement {
    let selected = app.selected == spec.id && ready;
    let download = app
        .download
        .as_ref()
        .filter(|download| download.id == spec.id);
    let received = download.map(|download| {
        download
            .received
            .load(std::sync::atomic::Ordering::Relaxed)
            .min(download.total)
    });
    let meta = match received {
        Some(got) => format!(
            "{} of {} · {}",
            models::format_size(got),
            models::format_size(spec.bytes),
            spec.blurb
        ),
        None => format!("{} · {}", models::format_size(spec.bytes), spec.blurb),
    };
    let id = spec.id.to_string();
    let press_id = format!("model-{id}");

    let trailing = if let (Some(download), Some(got)) = (download, received) {
        let fraction = got as f32 / download.total.max(1) as f32;
        press_handlers(
            div()
                .id("cancel-download")
                .relative()
                .size(px(26.0))
                .flex()
                .items_center()
                .justify_center(),
            "cancel-download",
            cx,
            |this, cx| this.cancel_download(cx),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .child(ring(1.0, theme::EDGE)),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .child(ring(fraction, theme::AMBER)),
        )
        .child(div().size(px(8.0)).rounded(px(2.0)).bg(theme::AMBER))
        .into_any_element()
    } else if selected {
        asset_icon("check-bold", theme::AMBER, 16.0).into_any_element()
    } else if !ready {
        div()
            .w(px(56.0))
            .h(px(26.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(theme::RAISED)
            .shadow(vec![theme::top_edge(theme::SHEEN)])
            .text_size(px(12.0))
            .font_weight(FontWeight::BOLD)
            .text_color(theme::AMBER)
            .child("GET")
            .into_any_element()
    } else {
        div().into_any_element()
    };

    let pressed = app.press_scale(&press_id) < 1.0;
    press_handlers(
        div()
            .id(SharedString::from(press_id.clone()))
            .h(px(54.0))
            .w_full()
            .px(px(14.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .opacity(opacity)
            .when(!first, |row| row.border_t_1().border_color(theme::HAIRLINE))
            .when(selected, |row| row.bg(theme::AMBER_WASH))
            .when(pressed && !selected, |row| row.bg(theme::HAIRLINE)),
        &press_id,
        cx,
        move |this, cx| this.choose_model(&id, cx),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(1.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(7.0))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .line_height(px(19.0))
                            .font_weight(if selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .text_color(theme::LABEL)
                            .child(spec.name),
                    )
                    .when(spec.recommended, |name| {
                        name.child(
                            div()
                                .h(px(16.0))
                                .px(px(6.0))
                                .flex()
                                .items_center()
                                .rounded_full()
                                .bg(theme::AMBER_BADGE)
                                .text_size(px(10.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::AMBER)
                                .child("RECOMMENDED"),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .line_height(px(16.0))
                    .font_features(theme::tabular())
                    .text_color(theme::SECONDARY)
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(meta),
            ),
    )
    .child(
        div()
            .w(px(60.0))
            .flex_shrink_0()
            .flex()
            .justify_end()
            .child(trailing),
    )
    .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn choice_row(
    app: &Whisp,
    press_id: String,
    label: String,
    native: &'static str,
    on: bool,
    first: bool,
    cx: &mut Context<Whisp>,
    action: impl Fn(&mut Whisp, &mut Context<Whisp>) + 'static,
) -> AnyElement {
    let pressed = app.press_scale(&press_id) < 1.0;
    let trailing = if on {
        Some(asset_icon("check-bold", theme::AMBER, 15.0).into_any_element())
    } else if !native.is_empty() {
        Some(
            div()
                .text_size(px(12.0))
                .text_color(theme::TERTIARY)
                .child(native)
                .into_any_element(),
        )
    } else {
        None
    };
    press_handlers(
        div()
            .id(SharedString::from(press_id.clone()))
            .w_full()
            .flex_shrink_0()
            .pl(px(14.0))
            .flex()
            .when(pressed, |row| row.bg(theme::HAIRLINE)),
        &press_id,
        cx,
        action,
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .h(px(40.0))
            .pr(px(14.0))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .when(!first, |lane| {
                lane.border_t_1().border_color(theme::HAIRLINE)
            })
            .child(
                div()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(row_title(&label)),
            )
            .children(trailing),
    )
    .into_any_element()
}

fn press_handlers(
    el: Stateful<Div>,
    id: &str,
    cx: &mut Context<Whisp>,
    action: impl Fn(&mut Whisp, &mut Context<Whisp>) + 'static,
) -> Stateful<Div> {
    let down = id.to_string();
    let up = id.to_string();
    let out = id.to_string();
    el.on_mouse_down(
        gpui_kit::MouseButton::Left,
        cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.press_down(&down);
            action(this, cx);
        }),
    )
    .on_mouse_up(
        gpui_kit::MouseButton::Left,
        cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.press_up(&up);
        }),
    )
    .on_mouse_up_out(
        gpui_kit::MouseButton::Left,
        cx.listener(move |this, _, _, _cx| {
            this.press_up(&out);
        }),
    )
}

fn entrance(index: usize, opened: Option<std::time::Instant>) -> f32 {
    let Some(opened) = opened else {
        return 1.0;
    };
    let t = (opened.elapsed().as_secs_f32() - index as f32 * 0.03) / 0.18;
    motion::ease_out(t.clamp(0.0, 1.0))
}

/// `m:ss`, the way a recording timer reads.
fn clock(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn title(text: &str) -> Div {
    div()
        .text_size(px(15.0))
        .font_weight(FontWeight::SEMIBOLD)
        .line_height(px(20.0))
        .text_color(theme::LABEL)
        .child(text.to_string())
}

fn row_title(text: &str) -> AnyElement {
    div()
        .text_size(px(14.0))
        .font_weight(FontWeight::MEDIUM)
        .line_height(px(19.0))
        .text_color(theme::LABEL)
        .child(text.to_string())
        .into_any_element()
}

fn hint_text(text: String) -> Div {
    div()
        .flex_shrink_0()
        .text_size(px(12.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::TERTIARY)
        .whitespace_nowrap()
        .child(text)
}

fn panel_header(heading: &str, subtitle: Option<&str>) -> Div {
    div()
        .px(px(8.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(title(heading))
        .children(subtitle.map(|subtitle| {
            div()
                .text_size(px(12.0))
                .line_height(px(16.0))
                .text_color(theme::SECONDARY)
                .child(subtitle.to_string())
        }))
}

/// A grouped inset section, as in System Settings.
fn group() -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(12.0))
        .bg(theme::INSET)
        .overflow_hidden()
}

/// A grouped section of fixed height whose rows scroll.
fn scroll_group(id: &'static str, height: f32, rows: Vec<AnyElement>) -> impl IntoElement {
    div()
        .h(px(height))
        .w_full()
        .rounded(px(12.0))
        .bg(theme::INSET)
        .overflow_hidden()
        .child(
            div()
                .id(id)
                .size_full()
                .overflow_y_scrollbar()
                .flex()
                .flex_col()
                .children(rows),
        )
}

fn tile(icon: Icon) -> Div {
    div()
        .size(px(26.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.0))
        .bg(theme::RAISED)
        .shadow(vec![theme::top_edge(theme::TILE_SHEEN)])
        .child(icon)
}

fn keycaps(caps: Vec<String>) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .children(caps.into_iter().map(|cap| {
            let wide = cap.chars().count() > 1;
            div()
                .h(px(22.0))
                .min_w(px(22.0))
                .px(px(if wide { 8.0 } else { 6.0 }))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .bg(theme::RAISED)
                .shadow(vec![
                    theme::top_edge(theme::TILE_SHEEN),
                    BoxShadow::new(px(0.0), px(-1.0), theme::tone(theme::KEY_BASE)).inset(),
                ])
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::SECONDARY)
                .child(cap)
        }))
}

fn drill_value(value: String) -> AnyElement {
    div()
        .min_w_0()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .mr(px(-2.0))
        .child(
            div()
                .max_w(px(180.0))
                .text_size(px(13.0))
                .text_color(theme::SECONDARY)
                .whitespace_nowrap()
                .text_ellipsis()
                .child(value),
        )
        .child(asset_icon("chevron-right-bold", theme::TERTIARY, 13.0))
        .into_any_element()
}

/// A macOS switch: amber when on, the knob riding to the lit side.
fn switch(on: bool) -> Div {
    div()
        .w(px(34.0))
        .h(px(20.0))
        .flex_shrink_0()
        .p(px(2.0))
        .flex()
        .when(on, |track| track.justify_end())
        .rounded_full()
        .bg(if on { theme::AMBER } else { theme::TRACK })
        .child(
            div()
                .size(px(16.0))
                .rounded_full()
                .bg(theme::KNOB)
                .shadow(vec![BoxShadow::new(
                    px(0.0),
                    px(1.0),
                    theme::tone(gpui_kit::rgba(0x00000059)),
                )
                .blur_radius(px(3.0))]),
        )
}

fn lucide(name: Lucide, color: Rgba, size: f32) -> Icon {
    Icon::new(name).text_color(color).with_size(px(size))
}

/// One of the app's own glyphs under `icons/whisp/`.
fn asset_icon(name: &str, color: Rgba, size: f32) -> Icon {
    Icon::empty()
        .path(format!("icons/whisp/{name}.svg"))
        .text_color(color)
        .with_size(px(size))
}

/// A 26px progress ring. The arc starts at twelve o'clock and runs clockwise.
fn ring(fraction: f32, color: Rgba) -> Icon {
    // Whole percents, so a download redraws a bounded set of shapes.
    let fraction = (fraction.clamp(0.0, 1.0) * 100.0).round() / 100.0;
    let (c, r) = (13.0_f32, 11.5_f32);
    let shape = if fraction >= 1.0 {
        format!(r#"<circle cx="{c}" cy="{c}" r="{r}"/>"#)
    } else {
        let angle = fraction.max(0.02) * std::f32::consts::TAU;
        let (x, y) = (c + r * angle.sin(), c - r * angle.cos());
        let large = u8::from(fraction > 0.5);
        format!(
            r#"<path d="M{c} {top} A{r} {r} 0 {large} 1 {x:.2} {y:.2}"/>"#,
            top = c - r
        )
    };
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 26 26" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round">{shape}</svg>"##
    );
    Icon::default()
        .data(svg.as_bytes())
        .text_color(color)
        .with_size(px(26.0))
}

fn separator() -> Div {
    div()
        .h(px(1.0))
        .w_full()
        .flex_shrink_0()
        .bg(theme::HAIRLINE)
}

fn error_line(message: String) -> Div {
    div()
        .h(px(ERROR_EXTRA))
        .w_full()
        .flex_shrink_0()
        .px(px(16.0))
        .flex()
        .items_center()
        .text_size(px(12.0))
        .text_color(theme::RED)
        .whitespace_nowrap()
        .text_ellipsis()
        .child(message)
}

/// Live input level, oldest on the left fading in toward the newest.
fn waveform(bars: &[motion::Spring]) -> Div {
    let last = bars.len().saturating_sub(1).max(1) as f32;
    let marks = bars.iter().enumerate().map(|(index, bar)| {
        let height = 3.0 + bar.value.clamp(0.0, 1.0) * 19.0;
        div()
            .w(px(3.0))
            .h(px(height))
            .flex_shrink_0()
            .rounded_full()
            .bg(theme::LABEL)
            .opacity(0.25 + 0.75 * index as f32 / last)
    });
    div()
        .h(px(24.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(3.0))
        .children(marks)
}

/// The resting voice mark: five amber bars.
fn voice_glyph() -> Div {
    div()
        .h(px(16.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .children([6.0, 12.0, 16.0, 10.0, 5.0].into_iter().map(|height| {
            div()
                .w(px(2.0))
                .h(px(height))
                .rounded_full()
                .bg(theme::AMBER)
        }))
}
