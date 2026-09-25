//! License: the free trial, buying, and activating a key on this device.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputContentType};
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::{
    div, prelude::*, px, Context, Div, FontWeight, IntoElement, Stateful, Styled, Window,
};

use crate::i18n::{t, tf};
use crate::license::{self, Access};
use crate::settings_window::widgets::*;
use crate::settings_window::SettingsWindow;
use crate::theme;

impl SettingsWindow {
    pub(in crate::settings_window) fn start_talking(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hud = self.hud.clone();
        let hud_window = hud.read(cx).hud_window;
        self.close(window, cx);
        cx.defer(move |cx| {
            let _ = hud_window.update(cx, |_, window, cx| {
                hud.update(cx, |view, cx| view.set_visible(true, window, cx));
            });
        });
    }

    pub(in crate::settings_window) fn activate_license(&mut self, cx: &mut Context<Self>) {
        if self.license_busy {
            return;
        }
        let key = self.license_input.read(cx).value().to_string();
        if key.trim().is_empty() {
            self.error = Some(t("Paste your license key first.").into());
            cx.notify();
            return;
        }
        self.license_busy = true;
        self.error = None;
        let hud = self.hud.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { license::activate(&key) })
                .await;
            if let Ok(access) = &result {
                hud.update(cx, |view, cx| view.set_license_access(access.clone(), cx));
            }
            this.update(cx, |view: &mut Self, cx| {
                view.license_busy = false;
                view.just_activated = matches!(result, Ok(Access::Active(_)));
                view.error = result.err();
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(in crate::settings_window) fn refresh_license(&mut self, cx: &mut Context<Self>) {
        if self.license_busy {
            return;
        }
        self.license_busy = true;
        self.error = None;
        let hud = self.hud.clone();
        cx.spawn(async move |this, cx| {
            let access = cx
                .background_executor()
                .spawn(async { license::check_saved() })
                .await;
            hud.update(cx, |view, cx| view.set_license_access(access, cx));
            this.update(cx, |view: &mut Self, cx| {
                view.license_busy = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(in crate::settings_window) fn deactivate_license(&mut self, cx: &mut Context<Self>) {
        if self.license_busy {
            return;
        }
        self.license_busy = true;
        self.error = None;
        self.just_activated = false;
        let hud = self.hud.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async {
                    license::deactivate()?;
                    Ok::<_, String>(license::check_saved())
                })
                .await;
            if let Ok(access) = &result {
                hud.update(cx, |view, cx| view.set_license_access(access.clone(), cx));
            }
            this.update(cx, |view: &mut Self, cx| {
                view.license_busy = false;
                view.error = result.err();
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(in crate::settings_window) fn license(&self, cx: &mut Context<Self>) -> Div {
        let status = self.hud.read(cx).license_access.clone();
        let has_key = status.display_key().is_some_and(|key| !key.is_empty());
        let trial_detail = status.trial_remaining().map(|remaining| {
            tf(
                "{} of your 3-day free trial. Then {} once to keep Whisple.",
                &[&license::trial_left(remaining, false), &license::PRICE],
            )
        });
        let ended_detail = tf(
            "Your 3-day free trial has ended. Buy Whisple for {} once to keep dictating.",
            &[&license::PRICE],
        );
        let trial_license_issue = match &status {
            Access::Trial {
                license_issue: Some(reason),
                ..
            } => Some(tf("Saved license needs attention: {}", &[reason])),
            _ => None,
        };
        let (title, detail) = match &status {
            Access::Checking => (
                t("Checking license"),
                t("Contacting Polar to verify access."),
            ),
            Access::Trial { .. } if status.allowed() => {
                (t("Free trial"), trial_detail.as_deref().unwrap_or_default())
            }
            Access::Trial { .. } | Access::TrialExpired => {
                (t("Free trial ended"), ended_detail.as_str())
            }
            Access::Active(_) => (
                t("License active"),
                t("Whisple is ready to use on this device."),
            ),
            Access::Offline(_) => (
                t("License active offline"),
                t("A recent verification allows temporary offline use."),
            ),
            Access::Blocked { .. } => (
                t("License needs attention"),
                t("Check the message below or enter another key."),
            ),
            Access::Unavailable { .. } => (
                t("Could not verify access"),
                t("Connect to the internet and check your license again."),
            ),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(24.0))
            .child(
                div()
                    .p(px(20.0))
                    .flex()
                    .items_center()
                    .gap(px(16.0))
                    .rounded(px(12.0))
                    .bg(theme::INSET)
                    .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
                    .child(
                        div()
                            .size(px(44.0))
                            .rounded(px(10.0))
                            .bg(theme::RAISED)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(Lucide::KeyRound)
                                    .text_color(theme::AMBER)
                                    .with_size(px(20.0)),
                            ),
                    )
                    .child(label_stack(title, Some(detail))),
            )
            .when_some(trial_license_issue, |pane, issue| {
                pane.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme::RED)
                        .child(issue),
                )
            })
            .when(
                self.just_activated && matches!(&status, Access::Active(_)),
                |pane| {
                    pane.child(
                        div()
                            .p(px(18.0))
                            .rounded(px(12.0))
                            .bg(theme::AMBER_SOFT)
                            .flex()
                            .flex_col()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .text_size(px(16.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::LABEL)
                                    .child(t("Whisple is yours")),
                            )
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(theme::SECONDARY)
                                    .child(t("Your lifetime license is active. Thanks for supporting Whisple.")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme::SECONDARY)
                                    .child(tf(
                                        "Active on this Mac · {}",
                                        &[&status.display_key().unwrap_or_default()],
                                    )),
                            )
                            .child(
                                div()
                                    .id("license-start-talking")
                                    .role(gpui_kit::Role::Button)
                                    .aria_label(t("Start talking"))
                                    .h(px(34.0))
                                    .px(px(14.0))
                                    .rounded(px(8.0))
                                    .bg(theme::AMBER)
                                    .text_color(theme::HUD)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(px(12.0))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.start_talking(window, cx)
                                    }))
                                    .child(t("Start talking")),
                            ),
                    )
                },
            )
            .child(section(
                t("Lifetime license"),
                vec![link_row(
                    "license-buy",
                    t("Buy Whisple"),
                    tf("{} once · opens Polar checkout ↗", &[&license::PRICE]),
                    license::CHECKOUT_URL,
                    false,
                    cx,
                )],
            ))
            .child(section(
                t("Activate on this device"),
                vec![div()
                    .p(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme::SECONDARY)
                            .child(
                                status
                                    .display_key()
                                    .filter(|key| !key.is_empty())
                                    .unwrap_or(
                                        t("Paste the key from your purchase email or Polar account."),
                                    )
                                    .to_string(),
                            ),
                    )
                    .child(
                        div()
                            .h(px(40.0))
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .rounded(px(8.0))
                            .bg(theme::HUD)
                            .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
                            .child(
                                Input::new(&self.license_input)
                                    .content_type(InputContentType::Password)
                                    .appearance(false)
                                    .px_0()
                                    .py_0()
                                    .h(px(22.0))
                                    .text_size(px(13.0)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(16.0))
                                    .when(has_key, |links| {
                                        links.child(
                                            text_button("license-deactivate", t("Deactivate this device"))
                                                .aria_label(t("Deactivate license on this device"))
                                                .on_click(cx.listener(|view, _, _, cx| {
                                                    view.deactivate_license(cx)
                                                })),
                                        )
                                    })
                                    .when(!has_key, |links| {
                                        links.child(
                                            text_button("license-portal", t("Find your key ↗"))
                                                .aria_label(t("Open Polar purchases"))
                                                .on_click(|_, _, cx| {
                                                    cx.open_url(license::CUSTOMER_PORTAL_URL)
                                                }),
                                        )
                                    })
                                    .child(
                                        text_button("license-refresh", t("Check status"))
                                            .aria_label(t("Check license status"))
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.refresh_license(cx)
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .id("license-activate")
                                    .role(gpui_kit::Role::Button)
                                    .aria_label(t("Activate license"))
                                    .h(px(34.0))
                                    .px(px(14.0))
                                    .rounded(px(8.0))
                                    .bg(theme::AMBER)
                                    .flex()
                                    .items_center()
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::HUD)
                                    .cursor_pointer()
                                    .on_click(
                                        cx.listener(|view, _, _, cx| view.activate_license(cx)),
                                    )
                                    .child(if self.license_busy {
                                        t("Working…")
                                    } else {
                                        t("Activate key")
                                    }),
                            ),
                    )
                    .into_any_element()],
            ))
    }
}

/// A quiet text action for the license card's footer.
fn text_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .text_size(px(12.0))
        .text_color(theme::SECONDARY)
        .cursor_pointer()
        .hover(|button| button.text_color(theme::LABEL))
        .child(label)
}
