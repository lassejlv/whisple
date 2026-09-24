use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::{
    div, prelude::*, px, Animation, AnimationExt, AnyElement, BoxShadow, Context, Div, FontWeight,
    IntoElement, ParentElement, Render, Rgba, SharedString, Stateful, Styled, Window,
};

use crate::app::{
    Phase, Recovery, ResultKind, Reveal, Whisp, BAR_HEIGHT, COLLAPSED_HEIGHT, MENU_DIVIDER,
    MENU_PAD, MENU_ROW, NOTICE_EXTRA, RESULT_LINE, WINDOW_RADIUS,
};
use crate::cloud::Provider;
use crate::hotkey;
use crate::license::{self, Access};
use crate::motion;
use crate::settings_window::SettingsTarget;
use crate::theme;
#[cfg(target_os = "macos")]
use crate::updater::UpdatePrompt;

impl Render for Whisp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick(window, cx);

        div()
            .id("whisp")
            .key_context("Whisp")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &crate::app::ToggleListen, _, cx| {
                this.toggle_listen(cx);
            }))
            .on_action(
                cx.listener(|this, _: &crate::app::CloseOverlay, window, cx| {
                    this.close_overlay(window, cx);
                }),
            )
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
                    .when(self.error.is_some(), |column| {
                        column.child(self.error_notice(cx))
                    })
                    .when(self.update_line_visible(), |column| {
                        column.child(self.update_notice(cx))
                    }),
            )
    }
}

#[derive(Clone, Copy)]
enum NoticeAction {
    Settings(SettingsTarget),
    #[cfg(target_os = "macos")]
    Privacy,
    Models,
    Record,
    Retry,
    Local,
}

impl Whisp {
    /// The single action a notice offers, if any.
    fn notice_action(&self) -> Option<(&'static str, NoticeAction)> {
        let retry = self
            .failed_audio_available()
            .then_some(("Retry", NoticeAction::Retry));
        let local = (self.failed_audio_available()
            && self
                .models
                .iter()
                .any(|model| model.ready && model.spec.id.starts_with("turbo")))
        .then_some(("Use local Turbo", NoticeAction::Local));
        match self.recovery? {
            Recovery::Microphone | Recovery::MicrophoneDisconnected => {
                Some(("Choose mic", NoticeAction::Settings(SettingsTarget::Audio)))
            }
            #[cfg(target_os = "macos")]
            Recovery::MicrophonePermission => Some(("Open Privacy", NoticeAction::Privacy)),
            #[cfg(not(target_os = "macos"))]
            Recovery::MicrophonePermission => None,
            Recovery::Model => Some(("Choose model", NoticeAction::Models)),
            Recovery::NoSpeech => Some(("Try again", NoticeAction::Record)),
            Recovery::CloudKey(provider) => Some((
                "Fix key",
                NoticeAction::Settings(SettingsTarget::CloudKey(provider)),
            )),
            Recovery::CloudOffline => local.or(retry),
            Recovery::CloudRateLimited | Recovery::CloudOther => retry.or(local),
            Recovery::LocalFallback => retry.map(|_| ("Retry cloud", NoticeAction::Retry)),
            Recovery::Command => None,
            Recovery::AssistantKey => {
                Some(("Add key", NoticeAction::Settings(SettingsTarget::Models)))
            }
            Recovery::Assistant => Some(("Try again", NoticeAction::Record)),
        }
    }

    fn error_notice(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let message = self.error.clone().unwrap_or_default();
        let (title, detail) = match self.recovery {
            #[cfg(target_os = "macos")]
            Some(kind @ Recovery::MicrophonePermission) => (
                kind.title().to_string(),
                Some("Allow Whisple under Privacy & Security › Microphone.".to_string()),
            ),
            Some(kind) => (kind.title().to_string(), Some(message)),
            None => (message, None),
        };
        let action = self.notice_action().map(|(label, action)| {
            (label, move |this: &mut Whisp, cx: &mut Context<Whisp>| {
                this.perform_notice_action(action, cx)
            })
        });
        notice_line(
            title,
            detail,
            theme::RED,
            action,
            |this, cx| this.dismiss_notice(cx),
            cx,
        )
    }

    fn update_notice(&self, cx: &mut Context<Self>) -> AnyElement {
        #[cfg(target_os = "macos")]
        if self.update_prompt == Some(UpdatePrompt::JustUpdated) {
            return notice_line(
                format!("Updated to Whisple {}", env!("CARGO_PKG_VERSION")),
                None,
                theme::LABEL,
                None::<(&str, fn(&mut Whisp, &mut Context<Whisp>))>,
                |this, cx| this.dismiss_update(cx),
                cx,
            )
            .into_any_element();
        }
        let version = self.ready_update().unwrap_or_default();
        notice_line(
            format!("Whisple {version} is ready"),
            Some("Installs and restarts in a few seconds.".to_string()),
            theme::LABEL,
            Some(("Install", |this: &mut Whisp, cx: &mut Context<Whisp>| {
                this.install_update(cx)
            })),
            |this, cx| this.dismiss_update(cx),
            cx,
        )
        .into_any_element()
    }

    fn perform_notice_action(&mut self, action: NoticeAction, cx: &mut Context<Self>) {
        match action {
            NoticeAction::Settings(target) => {
                self.open_settings_at(target, cx);
                if !matches!(target, SettingsTarget::CloudKey(_)) {
                    self.dismiss_notice(cx);
                }
            }
            #[cfg(target_os = "macos")]
            NoticeAction::Privacy => {
                cx.open_url(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
                );
                self.dismiss_notice(cx);
            }
            NoticeAction::Models => {
                self.dismiss_notice(cx);
                if !self.menu_open {
                    self.toggle_menu(cx);
                }
            }
            NoticeAction::Record => self.toggle_listen(cx),
            NoticeAction::Retry => self.retry_audio(false, cx),
            NoticeAction::Local => self.retry_audio(true, cx),
        }
    }

    fn panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let extra = self.panel_height(self.reveal);
        let notice_h = if self.error.is_some() || self.update_line_visible() {
            NOTICE_EXTRA
        } else {
            0.0
        };
        let shown = (self.chrome.value - COLLAPSED_HEIGHT - notice_h).clamp(0.0, extra);
        let fade = if extra > 0.0 {
            (shown / extra).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let body = match self.reveal {
            Some(Reveal::Menu) => self.model_menu(extra - 1.0, cx).into_any_element(),
            Some(Reveal::Result) => self
                .transcript_card(self.last_text.clone(), cx)
                .into_any_element(),
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
        let locked = self.locked();
        let trial_over = matches!(
            self.license_access,
            Access::Trial { .. } | Access::TrialExpired
        );
        let center = if listening {
            waveform(&self.bars).into_any_element()
        } else {
            let (label, color) = match &self.phase {
                Phase::Transcribing => (self.working.unwrap_or("Transcribing…"), theme::SECONDARY),
                _ if locked && trial_over => ("Free trial ended", theme::LABEL),
                _ if locked => ("License needs attention", theme::LABEL),
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
            Phase::Transcribing => Some(hint_text(match self.transcribing_provider {
                Some(provider) => provider.name().to_string(),
                None if self.working.is_some() => "Voice command".to_string(),
                None => "On device".to_string(),
            })),
            _ if locked => None,
            _ => Some(match self.trial_ending() {
                Some(left) => hint_text(format!("Trial · {}", license::trial_left(left, true)))
                    .text_color(theme::AMBER),
                None => hint_text(hotkey::symbols(&self.show_hotkey)),
            }),
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
            .child(if locked {
                self.unlock_capsule(trial_over, cx).into_any_element()
            } else {
                self.model_capsule(cx).into_any_element()
            })
    }

    fn unlock_capsule(&self, buy: bool, cx: &mut Context<Self>) -> impl IntoElement {
        press_handlers(
            div()
                .id("unlock-license")
                .h(px(28.0))
                .px(px(11.0))
                .rounded_full()
                .bg(theme::AMBER_SOFT)
                .text_color(theme::AMBER)
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .flex()
                .items_center(),
            "unlock-license",
            cx,
            |this, cx| this.open_license_window(cx),
        )
        .child(if buy {
            format!("Unlock · {}", license::PRICE)
        } else {
            "License".to_string()
        })
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
            |this, cx| this.open_settings_window(cx),
        )
        .child(
            div()
                .size(px(size))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::empty()
                        .path("icons/gear.svg")
                        .text_color(theme::SECONDARY)
                        .with_size(px(15.0)),
                ),
        )
    }

    fn model_capsule(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.press_scale("models");
        let open = self.menu_open;
        let label = if !self.selected_ready() && self.menu_choices().is_empty() {
            "Add a model"
        } else if !self.selected_ready() {
            "Models"
        } else if let Some(provider) = Provider::from_id(&self.selected) {
            provider.name()
        } else {
            self.selected_spec().chip
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
            |this, cx| this.toggle_menu(cx),
        )
        .child(capsule)
    }

    // MARK: Result

    fn transcript_card(&self, text: String, cx: &mut Context<Self>) -> impl IntoElement {
        let words = text.split_whitespace().count();
        let meta = match self.result_kind {
            ResultKind::Dictation => format!(
                "{words} {} · {}",
                if words == 1 { "word" } else { "words" },
                clock(self.recorded)
            ),
            ResultKind::Command => "Voice command".to_string(),
            ResultKind::Answer(provider) => format!("Whisple · {}", provider.name()),
            ResultKind::Typed(provider) => format!("Written by Whisple · {}", provider.name()),
        };
        let lines = self.result_lines();
        div()
            .px(px(18.0))
            .pt(px(18.0))
            .pb(px(12.0))
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .h(px(RESULT_LINE * lines as f32))
                    .text_size(px(15.0))
                    .line_height(px(RESULT_LINE))
                    .text_color(theme::LABEL)
                    .whitespace_normal()
                    .line_clamp(lines)
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
                    .when(self.result_kind != ResultKind::Command, |row| {
                        row.child(self.copy_button(cx))
                    }),
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

    // MARK: Model menu

    /// A short dropdown above the capsule: ready models to switch between,
    /// then Settings for downloads and API keys.
    fn model_menu(&self, height: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let choices = self.menu_choices();
        let any = !choices.is_empty();
        let rows = choices.into_iter().map(|choice| {
            let id = choice.id;
            menu_row(
                self,
                format!("menu-{id}"),
                choice.name,
                Some(choice.detail),
                self.selected == id,
                cx,
                move |this, cx| this.choose_from_menu(id, cx),
            )
        });
        div()
            .h(px(height))
            .px(px(MENU_PAD))
            .py(px(MENU_PAD))
            .flex()
            .flex_col()
            .children(rows)
            .when(any, |menu| {
                menu.child(
                    div()
                        .h(px(MENU_DIVIDER))
                        .flex()
                        .items_center()
                        .px(px(8.0))
                        .child(separator()),
                )
            })
            .child(menu_row(
                self,
                "menu-manage".to_string(),
                "Manage models…",
                None,
                false,
                cx,
                |this, cx| this.open_settings_at(SettingsTarget::Models, cx),
            ))
    }
}

fn menu_row(
    app: &Whisp,
    press_id: String,
    label: &'static str,
    detail: Option<&'static str>,
    selected: bool,
    cx: &mut Context<Whisp>,
    action: impl Fn(&mut Whisp, &mut Context<Whisp>) + 'static,
) -> AnyElement {
    let pressed = app.press_scale(&press_id) < 1.0;
    press_handlers(
        div()
            .id(SharedString::from(press_id.clone()))
            .role(gpui_kit::Role::MenuItem)
            .aria_label(label)
            .h(px(MENU_ROW))
            .w_full()
            .flex_shrink_0()
            .px(px(8.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .rounded(px(8.0))
            .cursor_pointer()
            .hover(|row| row.bg(theme::HAIRLINE))
            .when(pressed, |row| row.bg(theme::HAIRLINE)),
        &press_id,
        cx,
        action,
    )
    .child(
        div()
            .w(px(16.0))
            .flex_shrink_0()
            .flex()
            .justify_center()
            .when(selected, |slot| {
                slot.child(asset_icon("check-bold", theme::AMBER, 14.0))
            }),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .text_size(px(13.0))
            .font_weight(if selected {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::MEDIUM
            })
            .text_color(theme::LABEL)
            .whitespace_nowrap()
            .text_ellipsis()
            .child(label),
    )
    .children(detail.map(|detail| {
        div()
            .flex_shrink_0()
            .text_size(px(12.0))
            .text_color(theme::TERTIARY)
            .child(detail)
    }))
    .into_any_element()
}

/// One line under the bar: a title, an optional detail, at most one action,
/// and a dismiss control.
fn notice_line(
    title: String,
    detail: Option<String>,
    title_color: Rgba,
    action: Option<(
        &'static str,
        impl Fn(&mut Whisp, &mut Context<Whisp>) + 'static,
    )>,
    dismiss: impl Fn(&mut Whisp, &mut Context<Whisp>) + 'static,
    cx: &mut Context<Whisp>,
) -> Div {
    div()
        .h(px(NOTICE_EXTRA))
        .w_full()
        .flex_shrink_0()
        .pl(px(16.0))
        .pr(px(10.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .border_t_1()
        .border_color(theme::HAIRLINE)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(title_color)
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(title),
                )
                .children(detail.map(|detail| {
                    div()
                        .text_size(px(11.0))
                        .text_color(theme::SECONDARY)
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(detail)
                })),
        )
        .children(action.map(|(label, action)| {
            press_handlers(
                div()
                    .id("notice-action")
                    .role(gpui_kit::Role::Button)
                    .aria_label(label)
                    .flex_shrink_0()
                    .h(px(26.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .bg(theme::AMBER_SOFT)
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::AMBER)
                    .cursor_pointer(),
                "notice-action",
                cx,
                action,
            )
            .child(label)
        }))
        .child(
            press_handlers(
                div()
                    .id("notice-dismiss")
                    .role(gpui_kit::Role::Button)
                    .aria_label("Dismiss")
                    .size(px(22.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer(),
                "notice-dismiss",
                cx,
                dismiss,
            )
            .child(lucide(Lucide::X, theme::TERTIARY, 13.0)),
        )
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

/// `m:ss`, the way a recording timer reads.
fn clock(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
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

fn separator() -> Div {
    div()
        .h(px(1.0))
        .w_full()
        .flex_shrink_0()
        .bg(theme::HAIRLINE)
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
