use gpui_kit::{div, prelude::*, px, Context, Div, FontWeight, IntoElement, Styled};

use crate::i18n::{t, tf};
use crate::ui::settings::widgets::*;
use crate::ui::settings::{SettingsWindow, RELEASES_URL};
use crate::ui::theme;

impl SettingsWindow {
    pub(in crate::ui::settings) fn about(&self, cx: &mut Context<Self>) -> Div {
        let (update_summary, ready) = {
            let hud = self.hud.read(cx);
            (hud.update_summary(), hud.ready_update().is_some())
        };
        let mut update_rows = vec![div()
            .h(px(58.0))
            .px(px(16.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(label_stack(
                &update_summary,
                Some(if ready {
                    t("Whisple restarts in a few seconds after installing.")
                } else {
                    if cfg!(feature = "licensing") {
                        t("Updates are checked against GitHub releases.")
                    } else {
                        t("Release notes on GitHub ↗")
                    }
                }),
            ))
            .child(
                div()
                    .id("settings-update-action")
                    .role(gpui_kit::Role::Button)
                    .aria_label(if ready {
                        t("Install and restart")
                    } else {
                        if cfg!(feature = "licensing") {
                            t("Check for updates")
                        } else {
                            t("Release notes on GitHub ↗")
                        }
                    })
                    .h(px(28.0))
                    .px(px(12.0))
                    .rounded(px(8.0))
                    .bg(if ready { theme::AMBER } else { theme::RAISED })
                    .flex()
                    .items_center()
                    .text_size(px(12.0))
                    .when(ready, |button| button.font_weight(FontWeight::SEMIBOLD))
                    .text_color(if ready { theme::HUD } else { theme::LABEL })
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.hud.update(cx, |hud, cx| {
                            if ready {
                                hud.install_update(cx)
                            } else {
                                hud.check_for_updates_now(cx)
                            }
                        })
                    }))
                    .child(if ready {
                        t("Install & restart")
                    } else {
                        if cfg!(feature = "licensing") {
                            t("Check now")
                        } else {
                            t("Release notes on GitHub ↗")
                        }
                    }),
            )
            .into_any_element()];
        if ready {
            update_rows.push(link_row(
                "settings-release-notes",
                t("What’s new"),
                t("Release notes on GitHub ↗"),
                RELEASES_URL,
                true,
                cx,
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(24.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .size(px(72.0))
                            .rounded_full()
                            .bg(theme::AMBER_SOFT)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().flex().items_center().gap(px(3.0)).children(
                                [13.0, 23.0, 30.0, 20.0, 10.0].map(|height| {
                                    div()
                                        .w(px(4.0))
                                        .h(px(height))
                                        .rounded_full()
                                        .bg(theme::AMBER)
                                }),
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::LABEL)
                            .child("Whisple"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme::SECONDARY)
                            .child(tf(
                                "Version {} · Local-first voice dictation",
                                &[&env!("CARGO_PKG_VERSION")],
                            )),
                    ),
            )
            .child(section(t("Updates"), update_rows))
            .child(section(
                t("Setup"),
                vec![div()
                    .h(px(58.0))
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(label_stack(
                        t("Set up Whisple again"),
                        Some(t("Walk through the features, microphone and model steps.")),
                    ))
                    .child(
                        div()
                            .id("settings-restart-onboarding")
                            .role(gpui_kit::Role::Button)
                            .aria_label(t("Restart onboarding"))
                            .h(px(28.0))
                            .px(px(12.0))
                            .rounded(px(8.0))
                            .bg(theme::RAISED)
                            .flex()
                            .items_center()
                            .text_size(px(12.0))
                            .text_color(theme::LABEL)
                            .cursor_pointer()
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.hud.update(cx, |hud, cx| hud.restart_onboarding(cx))
                            }))
                            .child(t("Restart onboarding")),
                    )
                    .into_any_element()],
            ))
            .child(section(
                t("More"),
                vec![
                    link_row(
                        "settings-website",
                        t("Website"),
                        "whisple.app ↗",
                        "https://whisple.app",
                        false,
                        cx,
                    ),
                    link_row(
                        "settings-source",
                        t("Source code"),
                        "GitHub ↗",
                        "https://github.com/lassejlv/whisple",
                        true,
                        cx,
                    ),
                    link_row(
                        "settings-acknowledgements",
                        t("Acknowledgements"),
                        "whisper.cpp, GPUI ›",
                        "https://github.com/ggerganov/whisper.cpp",
                        true,
                        cx,
                    ),
                ],
            ))
            .child(
                div()
                    .pl(px(4.0))
                    .text_size(px(11.0))
                    .text_color(theme::TERTIARY)
                    .child(t("MIT licensed · Made with GPUI")),
            )
    }
}
