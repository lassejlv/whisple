//! A separate update window remains discoverable when the voice bar is hidden.
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    text::TextView,
    ActiveTheme, Disableable, Root,
};
use gpui_kit::{
    div, point, prelude::*, px, size, AnyWindowHandle, App, AppContext, Bounds, Context, Entity,
    FocusHandle, FontWeight, IntoElement, Render, Styled, TitlebarOptions, Window, WindowBounds,
    WindowKind, WindowOptions,
};

use crate::app::Whisp;
use crate::i18n::{t, tf};

pub(crate) fn open(
    cx: &mut App,
    hud: Entity<Whisp>,
    existing: Option<AnyWindowHandle>,
) -> gpui_kit::Result<AnyWindowHandle> {
    if let Some(existing) = existing {
        if existing
            .update(cx, |_, window, cx| {
                window.activate_window();
                cx.activate(true);
            })
            .is_ok()
        {
            return Ok(existing);
        }
    }
    // Native window geometry is in logical pixels; content uses the rem scale.
    let window_size = size(px(580.0), px(470.0));
    let bounds = cx
        .primary_display()
        .map(|display| {
            let screen = display.bounds();
            Bounds {
                origin: point(
                    screen.origin.x + (screen.size.width - window_size.width) * 0.5,
                    screen.origin.y + (screen.size.height - window_size.height) * 0.5,
                ),
                size: window_size,
            }
        })
        .unwrap_or(Bounds {
            origin: point(px(100.0), px(100.0)),
            size: window_size,
        });
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(t("Whisple Update").into()),
                ..Default::default()
            }),
            kind: WindowKind::Normal,
            focus: true,
            show: true,
            is_resizable: true,
            is_minimizable: false,
            window_min_size: Some(size(px(520.0), px(420.0))),
            app_id: Some("whisple-update".into()),
            ..Default::default()
        },
        |window, cx| {
            let closing = hud.downgrade();
            window.on_window_should_close(cx, move |_, cx| {
                closing.update(cx, |hud, cx| hud.dismiss_update(cx)).ok();
                true
            });
            let view = cx.new(|cx| UpdateWindow::new(hud, cx));
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
            cx.new(|cx| Root::new(view, window, cx).bordered(false))
        },
    )?;
    cx.activate(true);
    Ok(*handle)
}

struct UpdateWindow {
    hud: Entity<Whisp>,
    focus: FocusHandle,
}

impl UpdateWindow {
    fn new(hud: Entity<Whisp>, cx: &mut Context<Self>) -> Self {
        cx.observe(&hud, |_, _, cx| cx.notify()).detach();
        Self {
            hud,
            focus: cx.focus_handle(),
        }
    }

    fn later(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.dismiss_update(cx));
        window.remove_window();
    }

    fn install(&mut self, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.install_update(cx));
    }
}

impl Render for UpdateWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hud = self.hud.read(cx);
        let (version, notes) = hud.update_details().unwrap_or_default();
        let version_label = tf(
            "Whisple {} is available. You have {}.",
            &[&version, &env!("CARGO_PKG_VERSION")],
        );
        let notes = if notes.trim().is_empty() {
            t("Release notes are not available for this version.").to_string()
        } else {
            notes
        };
        let ready = hud.update_install_ready();
        let error = hud.update_error.clone();
        let foreground = cx.theme().foreground;
        let secondary = crate::ui::theme::SECONDARY;
        div()
            .id("update-window")
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|view, event: &gpui_kit::KeyDownEvent, window, cx| {
                    match event.keystroke.key.as_str() {
                        "escape" => {
                            view.later(window, cx);
                            cx.stop_propagation();
                        }
                        "enter" | "return" if view.focus.is_focused(window) => {
                            view.install(cx);
                            cx.stop_propagation();
                        }
                        _ => {}
                    }
                }),
            )
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(foreground)
            .child(
                div()
                    .p_6()
                    .flex()
                    .gap_4()
                    .items_start()
                    .child(
                        div()
                            .size(gpui_kit::rems(4.0))
                            .flex_shrink_0()
                            .rounded(cx.theme().radius_lg)
                            .bg(cx.theme().accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().flex().items_center().gap_1().children(
                                [0.65, 1.15, 1.65, 1.05, 0.55].map(|height| {
                                    div()
                                        .w_1()
                                        .h(gpui_kit::rems(height))
                                        .rounded_full()
                                        .bg(cx.theme().primary)
                                }),
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .id("update-heading")
                                    .role(gpui_kit::Role::Heading)
                                    .aria_label(t("New update available"))
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t("New update available")),
                            )
                            .child(
                                div()
                                    .id("update-version")
                                    .role(gpui_kit::Role::Label)
                                    .aria_label(version_label.clone())
                                    .text_sm()
                                    .text_color(secondary)
                                    .child(version_label),
                            ),
                    ),
            )
            .child(
                div()
                    .px_6()
                    .pb_3()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("What’s new")),
            )
            .child(
                div()
                    .id("update-release-notes")
                    .role(gpui_kit::Role::Document)
                    .aria_label(notes.clone())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .border_y_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().muted)
                    .child(div().p_6().text_sm().child(
                        TextView::markdown("update-notes", notes).on_link_click(|url, _, _, cx| {
                            if url.starts_with("https://") || url.starts_with("http://") {
                                cx.open_url(url);
                            }
                        }),
                    )),
            )
            .child(
                div()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .when_some(error, |footer, error| {
                        footer.child(
                            div()
                                .id("update-error")
                                .role(gpui_kit::Role::Alert)
                                .aria_label(error.clone())
                                .text_sm()
                                .text_color(cx.theme().danger)
                                .child(error),
                        )
                    })
                    .child(div().text_sm().text_color(secondary).child(if ready {
                        t("Whisple will restart to finish updating.")
                    } else {
                        t("Finish recording before installing the update.")
                    }))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(Button::new("update-later").label(t("Later")).on_click(
                                cx.listener(|view, _, window, cx| view.later(window, cx)),
                            ))
                            .child(
                                Button::new("update-install")
                                    .primary()
                                    .label(t("Install and restart"))
                                    .when(ready, |button| {
                                        button
                                            .bg(cx.theme().primary)
                                            .text_color(cx.theme().primary_foreground)
                                    })
                                    .disabled(!ready)
                                    .on_click(cx.listener(|view, _, _, cx| view.install(cx))),
                            ),
                    ),
            )
    }
}
