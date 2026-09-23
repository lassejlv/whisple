use std::time::Duration;

use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, Animation, AnimationExt, Context,
    IntoElement, ParentElement, Render, SharedString, Styled, Window,
};

use crate::app::{
    Phase, Reveal, SettingsPage, Whisp, COLLAPSED_HEIGHT, ERROR_EXTRA, PICKER_EXTRA, RESULT_EXTRA,
    SETTINGS_EXTRA,
};
use crate::hotkey;
use crate::models;
use crate::motion::{self, Spring};
use crate::settings;
use crate::theme;

impl Render for Whisp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick(window, cx);
        let error = self.error.clone();

        div()
            .id("whisp")
            .key_context("Whisp")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &crate::app::ToggleListen, _, cx| {
                this.toggle_listen(cx);
            }))
            .on_action(cx.listener(|this, _: &crate::app::CloseOverlay, _, cx| {
                this.close_overlay(cx);
            }))
            .relative()
            .size_full()
            .font_family("Inter")
            .flex()
            .flex_col()
            .justify_end()
            .overflow_hidden()
            .bg(theme::MATERIAL)
            .border_1()
            .border_color(theme::HAIRLINE)
            .text_color(theme::LABEL)
            .child(sheen())
            .when(self.reveal.is_some(), |column| column.child(self.panel(cx)))
            .child(self.pill(cx))
            .when(error.is_some(), |column| {
                column.child(error_line(error.unwrap_or_default()))
            })
    }
}

impl Whisp {
    fn panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let extra = match self.reveal {
            Some(Reveal::Picker) => PICKER_EXTRA,
            Some(Reveal::Result) => RESULT_EXTRA,
            Some(Reveal::Settings) => SETTINGS_EXTRA,
            None => 0.0,
        };
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
                .transcript_card(self.result_text(), cx)
                .into_any_element(),
            Some(Reveal::Settings) => self.settings_panel(cx).into_any_element(),
            None => div().into_any_element(),
        };

        div()
            .w_full()
            .h(px(shown))
            .overflow_hidden()
            .flex()
            .flex_col()
            .justify_end()
            .child(div().opacity(fade).child(body))
    }

    fn result_text(&self) -> String {
        match &self.phase {
            Phase::Result(text) => text.clone(),
            _ => String::new(),
        }
    }

    fn pill(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = match &self.phase {
            Phase::Listening(_) => None,
            Phase::Transcribing => Some("Transcribing"),
            Phase::Idle | Phase::Result(_) if self.selected_ready() => Some("Tap to speak"),
            Phase::Idle | Phase::Result(_) => Some("Choose a model"),
        };
        let chip = if self.selected_ready() {
            self.selected_spec().chip
        } else {
            "Models"
        };

        div()
            .h(px(COLLAPSED_HEIGHT))
            .w_full()
            .px(px(10.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .child(self.voice_button(cx))
            .child(self.pill_center(label, cx))
            .child(self.settings_button(cx))
            .child(self.model_chip(chip, cx))
    }

    fn voice_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.press_scale("voice");
        let listening = matches!(self.phase, Phase::Listening(_));
        let busy = matches!(self.phase, Phase::Transcribing);
        let circle = div()
            .size(px(36.0 * scale))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(if listening { theme::BLUE } else { theme::FILL })
            .child(voice_mark(listening));
        let circle = if busy {
            circle
                .with_animation(
                    "transcribing",
                    Animation::new(Duration::from_millis(640)).repeat(),
                    |mark, delta| {
                        let wave = (delta * std::f32::consts::PI).sin();
                        mark.opacity(0.55 + 0.45 * wave)
                    },
                )
                .into_any_element()
        } else {
            circle.into_any_element()
        };

        press_handlers(
            div()
                .id("voice")
                .size(px(44.0))
                .flex()
                .items_center()
                .justify_center(),
            "voice",
            cx,
            |this, cx| this.toggle_listen(cx),
        )
        .child(circle)
    }

    fn pill_center(&self, label: Option<&str>, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if matches!(self.phase, Phase::Listening(_)) {
            waveform(&self.bars).into_any_element()
        } else {
            div()
                .text_size(px(15.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .line_height(px(20.0))
                .text_color(if matches!(self.phase, Phase::Transcribing) {
                    theme::SECONDARY
                } else {
                    theme::LABEL
                })
                .whitespace_nowrap()
                .text_ellipsis()
                .child(label.unwrap_or_default().to_string())
                .into_any_element()
        };

        press_handlers(
            div().id("speak").flex_1().overflow_hidden(),
            "voice",
            cx,
            |this, cx| this.toggle_listen(cx),
        )
        .child(body)
    }

    fn settings_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.press_scale("settings");
        let open = self.settings_open;
        let color = if open { theme::BLUE } else { theme::LABEL };
        press_handlers(
            div()
                .id("settings")
                .size(px(32.0))
                .flex()
                .items_center()
                .justify_center(),
            "settings",
            cx,
            |this, cx| this.toggle_settings(cx),
        )
        .child(
            div()
                .size(px(28.0 * scale))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(if open { theme::BLUE_SOFT } else { theme::CLEAR })
                .child(icon("icons/gear.svg", color, 15.0 * scale)),
        )
    }

    fn model_chip(&self, label: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.press_scale("models");
        let open = self.picker_open;
        press_handlers(
            div()
                .id("models")
                .h(px(32.0))
                .flex()
                .items_center()
                .justify_center(),
            "models",
            cx,
            |this, cx| this.toggle_picker(cx),
        )
        .child(
            div()
                .h(px(30.0 * scale))
                .px(px(11.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .rounded_full()
                .bg(if open { theme::BLUE_SOFT } else { theme::FILL })
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(if open { theme::BLUE } else { theme::LABEL })
                .child(label.to_string())
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(if open { theme::BLUE } else { theme::SECONDARY })
                        .child("▾"),
                ),
        )
    }

    fn transcript_card(&self, text: String, cx: &mut Context<Self>) -> impl IntoElement {
        let copied = self.copied;
        div()
            .h(px(RESULT_EXTRA))
            .w_full()
            .flex()
            .flex_col()
            .justify_end()
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(16.0))
                    .pb(px(12.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .h(px(60.0))
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .line_height(px(22.0))
                            .text_color(theme::LABEL)
                            .whitespace_normal()
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
                                    .text_color(if copied {
                                        theme::GREEN
                                    } else {
                                        theme::SECONDARY
                                    })
                                    .child(if copied {
                                        "Copied".to_string()
                                    } else {
                                        "Ready to paste".to_string()
                                    }),
                            )
                            .child(self.copy_button(cx)),
                    ),
            )
            .child(separator())
    }

    fn copy_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.press_scale("copy");
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
                .h(px(26.0 * scale))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded_full()
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::BLUE)
                .child("Copy"),
        )
    }

    fn settings_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let page = if self.settings_page == SettingsPage::Language {
            self.language_page(cx).into_any_element()
        } else {
            self.settings_main(cx).into_any_element()
        };
        div()
            .h(px(SETTINGS_EXTRA))
            .w_full()
            .flex()
            .flex_col()
            .justify_end()
            .child(
                div()
                    .px(px(12.0))
                    .pt(px(14.0))
                    .pb(px(10.0))
                    .opacity(self.page_fade.value)
                    .child(page),
            )
            .child(separator())
    }

    fn settings_main(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let opened = self.settings_opened_at;
        let hotkey_value = if self.recording_hotkey {
            "Press shortcut".to_string()
        } else {
            hotkey::label(&self.show_hotkey)
        };
        let hotkey_color = if self.recording_hotkey {
            theme::BLUE
        } else {
            theme::SECONDARY
        };
        div()
            .h(px(282.0))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(settings_header(
                "Settings",
                "Shortcut, language, and notes.",
            ))
            .child(
                div()
                    .h(px(226.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(self.setting_row(
                        "hotkey",
                        "Show bar",
                        hotkey_value,
                        hotkey_color,
                        self.recording_hotkey,
                        entrance(0, opened),
                        cx,
                        |this, cx| this.begin_hotkey_capture(cx),
                    ))
                    .child(self.setting_row(
                        "language",
                        "Language",
                        settings::language_name(&self.language),
                        theme::SECONDARY,
                        false,
                        entrance(1, opened),
                        cx,
                        |this, cx| this.open_languages(cx),
                    ))
                    .child(self.switch_row(
                        "copy",
                        "Copy to clipboard",
                        self.copy_notes,
                        entrance(2, opened),
                        cx,
                        |this, cx| this.toggle_copy_notes(cx),
                    ))
                    .child(self.switch_row(
                        "clean",
                        "Clean up notes",
                        self.clean_fillers,
                        entrance(3, opened),
                        cx,
                        |this, cx| this.toggle_clean_fillers(cx),
                    )),
            )
    }

    fn setting_row(
        &self,
        id: &str,
        title: &str,
        value: impl Into<SharedString>,
        value_color: gpui::Rgba,
        highlighted: bool,
        opacity: f32,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Whisp, &mut Context<Whisp>) + 'static,
    ) -> impl IntoElement {
        let scale = self.press_scale(id);
        let title = title.to_string();
        let value = value.into();
        let show_chevron = id == "language";
        press_handlers(
            div()
                .id(SharedString::from(id.to_string()))
                .w_full()
                .h(px(52.0))
                .opacity(opacity),
            id,
            cx,
            action,
        )
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(376.0 * scale))
                .h(px(48.0 * scale))
                .px(px(12.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .rounded(px(12.0))
                .bg(if highlighted {
                    theme::BLUE_SOFT
                } else {
                    theme::FILL
                })
                .child(
                    div()
                        .flex_1()
                        .text_size(px(14.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme::LABEL)
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(title),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            div()
                                .max_w(px(168.0))
                                .text_size(px(13.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(value_color)
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(value),
                        )
                        .when(show_chevron, |row| {
                            row.child(icon("icons/chevron.svg", theme::TERTIARY, 12.0))
                        }),
                ),
        )
    }

    fn switch_row(
        &self,
        id: &str,
        title: &str,
        on: bool,
        opacity: f32,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Whisp, &mut Context<Whisp>) + 'static,
    ) -> impl IntoElement {
        let scale = self.press_scale(id);
        let title = title.to_string();
        press_handlers(
            div()
                .id(SharedString::from(id.to_string()))
                .w_full()
                .h(px(52.0))
                .opacity(opacity),
            id,
            cx,
            action,
        )
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(376.0 * scale))
                .h(px(48.0 * scale))
                .px(px(12.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .rounded(px(12.0))
                .bg(theme::FILL)
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme::LABEL)
                        .child(title),
                )
                .child(switch(on)),
        )
    }

    fn language_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.language.clone();
        let rows = settings::Preferences::languages()
            .iter()
            .map(|language| {
                let id = language.id.to_string();
                let name = language.name.to_string();
                let on = selected == language.id;
                let press_id = format!("lang-{id}");
                let scale = self.press_scale(&press_id);
                press_handlers(
                    div().id(SharedString::from(press_id.clone())).w_full(),
                    &press_id,
                    cx,
                    move |this, cx| this.choose_language(&id, cx),
                )
                .child(
                    div()
                        .w(px(376.0 * scale))
                        .h(px(36.0))
                        .px(px(12.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .rounded(px(10.0))
                        .bg(if on { theme::BLUE_SOFT } else { theme::CLEAR })
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(if on { theme::BLUE } else { theme::LABEL })
                                .child(name),
                        )
                        .when(on, |row| {
                            row.child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme::BLUE)
                                    .child("✓"),
                            )
                        }),
                )
                .into_any_element()
            })
            .collect::<Vec<_>>();

        div()
            .h(px(282.0))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(self.language_header(cx))
            .child(
                div()
                    .id("language-list")
                    .h(px(226.0))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .children(rows),
            )
    }

    fn language_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.press_scale("lang-back");
        press_handlers(
            div().id("lang-back").h(px(48.0)).w_full(),
            "lang-back",
            cx,
            |this, cx| {
                this.settings_page = SettingsPage::Main;
                this.page_fade.snap(0.0);
                this.page_fade.set(1.0);
                cx.notify();
            },
        )
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(icon("icons/chevron-left.svg", theme::BLUE, 14.0 * scale))
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::BLUE)
                .child("Language"),
        )
    }

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
                    entrance(index, self.picker_opened_at),
                    cx,
                )
            })
            .collect::<Vec<_>>();

        div()
            .h(px(PICKER_EXTRA))
            .w_full()
            .flex()
            .flex_col()
            .justify_end()
            .child(
                div()
                    .px(px(12.0))
                    .pt(px(12.0))
                    .pb(px(10.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .h(px(44.0))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .line_height(px(20.0))
                                    .text_color(theme::LABEL)
                                    .child("Voice models"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .line_height(px(16.0))
                                    .text_color(theme::SECONDARY)
                                    .child("Free Whisper models. They stay on this machine."),
                            ),
                    )
                    .child(
                        div()
                            .id("model-list")
                            .h(px(236.0))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .children(rows),
                    )
                    .child(self.sample_button(cx)),
            )
            .child(separator())
    }

    fn sample_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.press_scale("sample");
        press_handlers(
            div()
                .id("sample")
                .h(px(36.0))
                .w_full()
                .flex()
                .items_center()
                .justify_center(),
            "sample",
            cx,
            |this, cx| this.transcribe_sample(cx),
        )
        .child(
            div()
                .w(px(376.0 * scale))
                .h(px(34.0 * scale))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(10.0))
                .bg(theme::FILL)
                .text_size(px(13.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme::LABEL)
                .child("Transcribe sample"),
        )
    }
}

fn model_row(
    app: &Whisp,
    spec: &'static models::ModelSpec,
    ready: bool,
    opacity: f32,
    cx: &mut Context<Whisp>,
) -> gpui::AnyElement {
    let selected = app.selected == spec.id && ready;
    let progress = app
        .download
        .as_ref()
        .filter(|download| download.id == spec.id);
    let (status, status_color) = if let Some(download) = progress {
        let got = download.received.load(std::sync::atomic::Ordering::Relaxed);
        let pct = ((got as f64 / download.total.max(1) as f64) * 100.0).clamp(0.0, 99.0) as u8;
        (format!("{pct}%"), theme::BLUE)
    } else if selected {
        ("In use".to_string(), theme::BLUE)
    } else if ready {
        ("Use".to_string(), theme::SECONDARY)
    } else {
        ("Download".to_string(), theme::SECONDARY)
    };
    let id = spec.id.to_string();
    let press_id = format!("model-{id}");
    let scale = app.press_scale(&press_id);
    let fraction = progress.map(|download| {
        let got = download.received.load(std::sync::atomic::Ordering::Relaxed);
        (got as f32 / download.total.max(1) as f32).clamp(0.02, 1.0)
    });

    press_handlers(
        div()
            .id(SharedString::from(press_id.clone()))
            .w_full()
            .opacity(opacity),
        &press_id,
        cx,
        move |this, cx| this.choose_model(&id, cx),
    )
    .child(
        div()
            .w(px(376.0 * scale))
            .px(px(10.0))
            .py(px(8.0))
            .flex()
            .flex_col()
            .gap(px(3.0))
            .rounded(px(10.0))
            .bg(if selected {
                theme::BLUE_SOFT
            } else {
                theme::CLEAR
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme::LABEL)
                                    .child(spec.name.to_string()),
                            )
                            .when(spec.recommended, |row| {
                                row.child(
                                    div()
                                        .text_size(px(11.0))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(theme::BLUE)
                                        .child("Best"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(status_color)
                            .child(status),
                    ),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .line_height(px(16.0))
                    .text_color(theme::TERTIARY)
                    .whitespace_normal()
                    .child(format!(
                        "{} · {}",
                        models::format_size(spec.bytes),
                        spec.blurb
                    )),
            )
            .when(fraction.is_some(), |row| {
                let fraction = fraction.unwrap_or(0.0);
                row.child(
                    div()
                        .h(px(2.0))
                        .w_full()
                        .rounded_full()
                        .bg(theme::FILL)
                        .child(
                            div()
                                .h(px(2.0))
                                .w(px(340.0 * fraction))
                                .rounded_full()
                                .bg(theme::BLUE),
                        ),
                )
            }),
    )
    .into_any_element()
}

fn press_handlers(
    el: gpui::Stateful<gpui::Div>,
    id: &str,
    cx: &mut Context<Whisp>,
    action: impl Fn(&mut Whisp, &mut Context<Whisp>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let down = id.to_string();
    let up = id.to_string();
    let out = id.to_string();
    el.on_mouse_down(
        gpui::MouseButton::Left,
        cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.press_down(&down);
            action(this, cx);
        }),
    )
    .on_mouse_up(
        gpui::MouseButton::Left,
        cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.press_up(&up);
        }),
    )
    .on_mouse_up_out(
        gpui::MouseButton::Left,
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

fn settings_header(title: &str, subtitle: &str) -> gpui::Div {
    div()
        .h(px(48.0))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .line_height(px(20.0))
                .text_color(theme::LABEL)
                .child(title.to_string()),
        )
        .child(
            div()
                .text_size(px(12.0))
                .line_height(px(16.0))
                .text_color(theme::SECONDARY)
                .child(subtitle.to_string()),
        )
}

fn icon(path: &'static str, color: gpui::Rgba, size: f32) -> gpui::Svg {
    gpui::svg().path(path).size(px(size)).text_color(color)
}

fn switch(on: bool) -> gpui::Div {
    div()
        .w(px(36.0))
        .h(px(22.0))
        .rounded_full()
        .bg(if on { theme::GREEN } else { theme::MATERIAL })
        .border_1()
        .border_color(if on { theme::GREEN } else { theme::HAIRLINE })
        .relative()
        .child(
            div()
                .absolute()
                .top(px(2.0))
                .left(px(if on { 16.0 } else { 2.0 }))
                .size(px(18.0))
                .rounded_full()
                .bg(theme::LABEL),
        )
}

fn separator() -> gpui::Div {
    div().h(px(1.0)).w_full().bg(theme::SEPARATOR)
}

fn error_line(message: String) -> gpui::Div {
    div()
        .h(px(ERROR_EXTRA))
        .w_full()
        .px(px(16.0))
        .flex()
        .items_center()
        .text_size(px(12.0))
        .text_color(theme::RED)
        .child(message)
}

fn sheen() -> gpui::Div {
    div()
        .absolute()
        .top_0()
        .left(px(10.0))
        .right(px(10.0))
        .h(px(18.0))
        .bg(linear_gradient(
            180.0,
            linear_color_stop(theme::SHEEN, 0.0),
            linear_color_stop(theme::CLEAR, 1.0),
        ))
}

fn waveform(bars: &[Spring]) -> gpui::Div {
    let marks = bars.iter().map(|bar| {
        let height = 4.0 + bar.value.clamp(0.0, 1.0) * 18.0;
        div()
            .w(px(3.0))
            .h(px(height))
            .rounded_full()
            .bg(theme::LABEL)
    });
    div()
        .h(px(28.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(3.0))
        .children(marks)
}

fn voice_mark(listening: bool) -> gpui::Div {
    if listening {
        return div().size(px(11.0)).rounded(px(3.0)).bg(theme::LABEL);
    }

    div()
        .flex()
        .flex_col()
        .items_center()
        .child(
            div()
                .w(px(8.0))
                .h(px(12.0))
                .rounded_full()
                .border_1()
                .border_color(theme::LABEL),
        )
        .child(div().w(px(1.5)).h(px(3.0)).bg(theme::LABEL))
        .child(div().w(px(8.0)).h(px(1.5)).rounded_full().bg(theme::LABEL))
}
