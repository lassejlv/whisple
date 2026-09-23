use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui::{
    div, ease_in_out, prelude::*, px, Animation, AnimationExt, Context, IntoElement, ParentElement,
    Render, Styled, Window,
};

use crate::app::{CloseOverlay, Phase, ToggleListen, Whisp};
use crate::models;
use crate::theme;

const BARS: usize = 22;

impl Render for Whisp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_chrome(window);
        let listening = matches!(self.phase, Phase::Listening(_));
        let working =
            listening || self.download.is_some() || matches!(self.phase, Phase::Transcribing);
        if working {
            window.request_animation_frame();
        }
        if listening {
            self.note_level();
        }

        let transcript = match &self.phase {
            Phase::Result(text) => Some(text.clone()),
            _ => None,
        };

        div()
            .id("whisp")
            .key_context("Whisp")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &ToggleListen, _, cx| this.toggle_listen(cx)))
            .on_action(cx.listener(|this, _: &CloseOverlay, _, cx| this.close_overlay(cx)))
            .size_full()
            .flex()
            .flex_col()
            .justify_end()
            .items_center()
            .bg(theme::INK)
            .p(px(8.0))
            .gap_2()
            .when(self.picker_open, |row| row.child(self.picker(cx)))
            .when(transcript.is_some() && !self.picker_open, |row| {
                row.child(self.transcript_card(transcript.unwrap_or_default(), cx))
            })
            .child(self.pill(cx))
            .when(self.error.is_some(), |row| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(theme::DANGER)
                        .child(self.error.clone().unwrap_or_default()),
                )
            })
    }
}

impl Whisp {
    fn pill(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = match &self.phase {
            Phase::Listening(_) => None,
            Phase::Transcribing => Some("Transcribing"),
            Phase::Result(text) => Some(text.as_str()),
            Phase::Idle if self.selected_ready() => Some("Tap to speak"),
            Phase::Idle => Some("Choose a model"),
        };
        let chip = if self.selected_ready() {
            self.selected_spec().chip
        } else {
            "Models"
        };

        let nested = self.panel_open();
        div()
            .w(px(404.0))
            .h(px(58.0))
            .px(px(7.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .rounded_full()
            .when(nested, |pill| {
                pill.bg(theme::INK_RAISED)
                    .border_1()
                    .border_color(theme::LINE)
            })
            .child(self.voice_button(cx))
            .child(self.pill_center(label, cx))
            .child(self.model_chip(chip, cx))
    }

    fn voice_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let listening = matches!(self.phase, Phase::Listening(_));
        let fill = if listening {
            theme::ACCENT
        } else {
            theme::FAINT
        };

        div()
            .id("voice")
            .size(px(44.0))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(fill)
            .cursor_pointer()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_listen(cx);
                }),
            )
            .child(voice_mark(
                listening,
                matches!(self.phase, Phase::Transcribing),
            ))
    }

    fn pill_center(&self, label: Option<&str>, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if matches!(self.phase, Phase::Listening(_)) {
            waveform(&self.levels).into_any_element()
        } else {
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(if matches!(self.phase, Phase::Result(_)) {
                    theme::TEXT
                } else {
                    theme::MUTED
                })
                .whitespace_nowrap()
                .text_ellipsis()
                .child(label.unwrap_or_default().to_string())
                .into_any_element()
        };

        div()
            .id("speak")
            .w(px(236.0))
            .overflow_hidden()
            .cursor_pointer()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_listen(cx);
                }),
            )
            .child(body)
    }

    fn model_chip(&self, label: &str, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("models")
            .h(px(32.0))
            .px_3()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(if self.picker_open {
                theme::ACCENT_SOFT
            } else {
                theme::FAINT
            })
            .text_xs()
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme::TEXT)
            .cursor_pointer()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_picker(cx);
                }),
            )
            .child(label.to_string())
    }

    fn transcript_card(&self, text: String, cx: &mut Context<Self>) -> impl IntoElement {
        let copied = self.copied;
        div()
            .w(px(400.0))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_xl()
            .bg(theme::INK)
            .border_1()
            .border_color(theme::LINE)
            .shadow_lg()
            .child(
                div()
                    .text_sm()
                    .text_color(theme::TEXT)
                    .whitespace_normal()
                    .child(text),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(if copied { theme::OK } else { theme::MUTED })
                            .child(if copied {
                                "Copied to the clipboard".to_string()
                            } else {
                                "Ready to paste".to_string()
                            }),
                    )
                    .child(
                        div()
                            .id("copy")
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(theme::FAINT)
                            .text_xs()
                            .text_color(theme::TEXT)
                            .cursor_pointer()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.copy_result(cx);
                                }),
                            )
                            .child("Copy"),
                    ),
            )
    }

    fn picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self
            .models
            .iter()
            .map(|model| model_row(self, model.spec, model.ready, cx))
            .collect::<Vec<_>>();

        div()
            .w(px(400.0))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_xl()
            .bg(theme::INK)
            .border_1()
            .border_color(theme::LINE)
            .shadow_lg()
            .child(
                div()
                    .px_2()
                    .pt_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT)
                            .child("Voice models"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::MUTED)
                            .whitespace_normal()
                            .child("Free Whisper models. They stay on this machine."),
                    ),
            )
            .child(
                div()
                    .id("model-list")
                    .h(px(292.0))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(rows),
            )
            .child(self.sample_button(cx))
    }

    fn sample_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("sample")
            .mx_1()
            .mb_1()
            .h(px(34.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_lg()
            .bg(theme::INK_RAISED)
            .text_xs()
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme::TEXT)
            .cursor_pointer()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.transcribe_sample(cx);
                }),
            )
            .child("Transcribe sample")
    }
}

fn model_row(
    app: &Whisp,
    spec: &'static models::ModelSpec,
    ready: bool,
    cx: &mut Context<Whisp>,
) -> gpui::AnyElement {
    let selected = app.selected == spec.id && ready;
    let progress = app
        .download
        .as_ref()
        .filter(|download| download.id == spec.id);
    let status = if let Some(download) = progress {
        let got = download.received.load(Ordering::Relaxed);
        let pct = ((got as f64 / download.total.max(1) as f64) * 100.0).clamp(0.0, 99.0) as u8;
        format!("{pct}%")
    } else if selected {
        "In use".to_string()
    } else if ready {
        "Use".to_string()
    } else {
        "Download".to_string()
    };
    let id = spec.id.to_string();

    div()
        .id(gpui::SharedString::from(format!("model-{id}")))
        .w_full()
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_1()
        .rounded_lg()
        .bg(if selected {
            theme::ACCENT_SOFT
        } else {
            theme::INK_RAISED
        })
        .border_1()
        .border_color(if selected { theme::ACCENT } else { theme::LINE })
        .cursor_pointer()
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.choose_model(&id, cx);
            }),
        )
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
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme::TEXT)
                                .child(spec.name.to_string()),
                        )
                        .when(spec.recommended, |row| {
                            row.child(
                                div()
                                    .px_2()
                                    .rounded_full()
                                    .bg(theme::ACCENT_SOFT)
                                    .text_xs()
                                    .text_color(theme::ACCENT)
                                    .child("Best"),
                            )
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(if selected { theme::OK } else { theme::MUTED })
                        .child(status),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme::MUTED)
                .whitespace_normal()
                .child(format!(
                    "{} · {}",
                    models::format_size(spec.bytes),
                    spec.blurb
                )),
        )
        .into_any_element()
}

fn waveform(levels: &std::collections::VecDeque<f32>) -> gpui::Div {
    let mut bars = Vec::with_capacity(BARS);
    for index in 0..BARS {
        let from_end = BARS - 1 - index;
        let level = levels.iter().rev().nth(from_end).copied().unwrap_or(0.08);
        let height = 4.0 + level * 20.0;
        bars.push(
            div()
                .w(px(3.0))
                .h(px(height))
                .rounded_full()
                .bg(theme::TEXT),
        );
    }
    div()
        .h(px(28.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(3.0))
        .children(bars)
}

fn voice_mark(listening: bool, busy: bool) -> gpui::AnyElement {
    if listening {
        return div()
            .w(px(12.0))
            .h(px(12.0))
            .rounded(px(2.0))
            .bg(theme::TEXT)
            .into_any_element();
    }

    div()
        .size(px(10.0))
        .rounded_full()
        .bg(if busy { theme::ACCENT } else { theme::TEXT })
        .with_animation(
            "voice-breathe",
            Animation::new(Duration::from_millis(if busy { 700 } else { 1600 }))
                .repeat()
                .with_easing(ease_in_out),
            move |dot, delta| {
                let wave = (delta * std::f32::consts::PI).sin();
                dot.opacity(0.45 + 0.55 * wave)
            },
        )
        .into_any_element()
}
