//! Models: local downloads and cloud providers with their API keys.
use std::sync::atomic::Ordering;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputContentType, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::{
    div, prelude::*, px, AnyElement, AppContext, Context, Div, Focusable, FontWeight, IntoElement,
    SharedString, Styled, Window,
};

use crate::cloud::{self, Provider};
use crate::models;
use crate::settings_window::widgets::*;
use crate::settings_window::SettingsWindow;
use crate::theme;

impl SettingsWindow {
    pub(in crate::settings_window) fn models(&self, cx: &mut Context<Self>) -> Div {
        let (selected, cloud_keys, used) = {
            let hud = self.hud.read(cx);
            (hud.selected.clone(), hud.cloud_keys, hud.storage_used())
        };
        let local = models::CATALOG
            .iter()
            .enumerate()
            .map(|(index, spec)| self.model_row(spec, index > 0, selected == spec.id, cx))
            .collect();
        let cloud = Provider::ALL
            .map(|provider| {
                self.cloud_row(
                    provider,
                    cloud_keys[provider.index()],
                    selected == provider.id(),
                    cx,
                )
            })
            .into();
        div()
            .flex()
            .flex_col()
            .gap(px(22.0))
            .child(section_with_detail(
                "On this Mac",
                format!("{} used", models::format_size(used)),
                local,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(section_with_detail("Cloud", "Uses your own API key".into(), cloud))
                    .child(
                        div()
                            .pl(px(4.0))
                            .text_size(px(12.0))
                            .text_color(theme::TERTIARY)
                            .child("Cloud recordings are sent to the selected provider. Keys are stored in your Mac’s Keychain."),
                    ),
            )
    }

    pub(in crate::settings_window) fn model_row(
        &self,
        spec: &'static models::ModelSpec,
        divider: bool,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ready = models::is_downloaded(spec);
        let (pending, progress) = {
            let hud = self.hud.read(cx);
            (
                hud.pending_uninstall.as_deref() == Some(spec.id),
                hud.download
                    .as_ref()
                    .filter(|download| download.id == spec.id)
                    .map(|download| {
                        (
                            download
                                .received
                                .load(Ordering::Relaxed)
                                .min(download.total),
                            download.total,
                        )
                    }),
            )
        };
        let subtitle = if let Some((received, total)) = progress {
            format!(
                "{} of {} · {}",
                models::format_size(received),
                models::format_size(total),
                spec.blurb
            )
        } else {
            format!("{} · {}", models::format_size(spec.bytes), spec.blurb)
        };
        let id = spec.id;
        let trailing = if progress.is_some() {
            div()
                .id(SharedString::from(format!("settings-cancel-{id}")))
                .role(gpui_kit::Role::Button)
                .aria_label(format!("Cancel {} download", spec.name))
                .text_size(px(12.0))
                .text_color(theme::AMBER)
                .cursor_pointer()
                .on_click(cx.listener(|view, _, _, cx| {
                    cx.stop_propagation();
                    view.hud.update(cx, |hud, cx| hud.cancel_download(cx));
                }))
                .child("Cancel")
                .into_any_element()
        } else if ready && selected {
            Icon::new(Lucide::Check)
                .text_color(theme::AMBER)
                .with_size(px(16.0))
                .into_any_element()
        } else if ready {
            div()
                .id(SharedString::from(format!("settings-remove-{id}")))
                .role(gpui_kit::Role::Button)
                .aria_label(if pending {
                    format!("Confirm remove {}", spec.name)
                } else {
                    format!("Remove {}", spec.name)
                })
                .px(px(10.0))
                .py(px(5.0))
                .rounded_full()
                .text_size(px(12.0))
                .text_color(if pending {
                    theme::RED
                } else {
                    theme::SECONDARY
                })
                .cursor_pointer()
                .on_click(cx.listener(move |view, _, _, cx| {
                    cx.stop_propagation();
                    view.hud.update(cx, |hud, cx| hud.uninstall_model(id, cx));
                }))
                .child(if pending { "Confirm remove" } else { "Remove" })
                .into_any_element()
        } else {
            div()
                .px(px(11.0))
                .py(px(4.0))
                .rounded_full()
                .bg(theme::RAISED)
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::AMBER)
                .child("GET")
                .into_any_element()
        };
        div()
            .id(SharedString::from(format!("settings-model-{id}")))
            .role(gpui_kit::Role::Button)
            .aria_label(format!("Use {} model", spec.name))
            .h(px(54.0))
            .px(px(16.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .when(divider, |row| {
                row.border_t_1().border_color(theme::HAIRLINE)
            })
            .when(selected && ready, |row| row.bg(theme::AMBER_WASH))
            .when(!(selected && ready), |row| {
                row.hover(|row| row.bg(theme::HOVER))
            })
            .cursor_pointer()
            .on_click(cx.listener(move |view, _, _, cx| {
                view.hud.update(cx, |hud, cx| hud.choose_model(id, cx));
            }))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(7.0))
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme::LABEL)
                                    .child(spec.name),
                            )
                            .when(spec.recommended, |title| {
                                title.child(
                                    div()
                                        .px(px(6.0))
                                        .py(px(2.0))
                                        .rounded_full()
                                        .bg(theme::AMBER_BADGE)
                                        .text_size(px(9.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme::AMBER)
                                        .child("RECOMMENDED"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme::SECONDARY)
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(subtitle),
                    ),
            )
            .child(trailing)
            .into_any_element()
    }

    pub(in crate::settings_window) fn cloud_row(
        &self,
        provider: Provider,
        connected: bool,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(SharedString::from(format!(
                "settings-cloud-{}",
                provider.id()
            )))
            .role(gpui_kit::Role::Button)
            .aria_label(format!("Use {} cloud model", provider.name()))
            .h(px(59.0))
            .px(px(14.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .when(provider == Provider::Groq, |row| {
                row.border_t_1().border_color(theme::HAIRLINE)
            })
            .when(selected, |row| row.bg(theme::AMBER_WASH))
            .when(!selected, |row| row.hover(|row| row.bg(theme::HOVER)))
            .cursor_pointer()
            .on_click(cx.listener(move |view, _, window, cx| {
                if connected {
                    view.hud
                        .update(cx, |hud, cx| hud.choose_cloud(provider, cx));
                } else {
                    view.open_cloud(provider, window, cx);
                }
            }))
            .child(provider_tile(provider, 30.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme::LABEL)
                                    .child(provider.name()),
                            )
                            .when(connected, |row| {
                                row.child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(theme::GREEN)
                                        .child("● Key saved"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme::SECONDARY)
                            .child(provider.description()),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "settings-key-{}",
                        provider.id()
                    )))
                    .role(gpui_kit::Role::Button)
                    .aria_label(format!(
                        "{} {} API key",
                        if connected { "Edit" } else { "Add" },
                        provider.name()
                    ))
                    .px(px(12.0))
                    .py(px(5.0))
                    .rounded_full()
                    .bg(theme::RAISED)
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::AMBER)
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _, window, cx| {
                        cx.stop_propagation();
                        view.open_cloud(provider, window, cx);
                    }))
                    .child(if connected { "EDIT" } else { "ADD KEY" }),
            )
            .into_any_element()
    }
    pub(in crate::settings_window) fn open_cloud(
        &mut self,
        provider: Provider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cloud_config = Some(provider);
        self.key_visible = false;
        self.error = None;
        let connected = self.hud.read(cx).cloud_keys[provider.index()];
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(if connected {
                    "Paste a new key to replace it"
                } else {
                    "Paste API key"
                })
                .masked(true)
        });
        cx.subscribe(&input, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        window.focus(&input.focus_handle(cx), cx);
        self.key_input = Some(input);
        cx.notify();
    }

    pub(in crate::settings_window) fn save_cloud(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = self.cloud_config else {
            return;
        };
        let connected = self.hud.read(cx).cloud_keys[provider.index()];
        let key = self
            .key_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        if !key.trim().is_empty() || !connected {
            if let Err(err) = cloud::save_key(provider, &key) {
                self.error = Some(err);
                cx.notify();
                return;
            }
        }
        self.hud
            .update(cx, |hud, cx| hud.use_saved_cloud_key(provider, cx));
        self.close_modal(window, cx);
    }

    pub(in crate::settings_window) fn remove_cloud(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = self.cloud_config else {
            return;
        };
        match cloud::delete_key(provider) {
            Ok(()) => {
                self.hud
                    .update(cx, |hud, cx| hud.forget_cloud_key(provider, cx));
                self.close_modal(window, cx);
            }
            Err(err) => {
                self.error = Some(err);
                cx.notify();
            }
        }
    }

    pub(in crate::settings_window) fn close_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cloud_config = None;
        self.key_input = None;
        self.key_visible = false;
        self.error = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub(in crate::settings_window) fn cloud_modal(&self, cx: &mut Context<Self>) -> Div {
        let provider = self
            .cloud_config
            .expect("the dialog shows only for a provider");
        let connected = self.hud.read(cx).cloud_keys[provider.index()];
        modal_backdrop().child(
            div()
                .w(px(480.0))
                .flex()
                .flex_col()
                .rounded(px(14.0))
                .overflow_hidden()
                .bg(theme::INSET)
                .border_1()
                .border_color(theme::EDGE)
                .child(
                    div()
                        .p(px(22.0))
                        .flex()
                        .flex_col()
                        .gap(px(18.0))
                        .child(cloud_modal_header(provider, connected))
                        .child(self.cloud_key_field(provider, connected, cx))
                        .when_some(self.error.as_ref(), |body, error| {
                            body.child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme::RED)
                                    .child(error.clone()),
                            )
                        }),
                )
                .child(cloud_modal_footer(provider, connected, cx)),
        )
    }

    fn cloud_key_field(&self, provider: Provider, connected: bool, cx: &mut Context<Self>) -> Div {
        let (url, link) = match provider {
            Provider::OpenAi => (
                "https://platform.openai.com/api-keys",
                "Manage keys at platform.openai.com ↗",
            ),
            Provider::Groq => (
                "https://console.groq.com/keys",
                "Get a key at console.groq.com ↗",
            ),
        };
        let note = if connected {
            format!(
                "A key is saved on this device. Paste a new one to replace it. Audio goes to {} only when selected.",
                provider.name()
            )
        } else {
            format!(
                "Your key stays on this device, in your Keychain. Audio goes to {} only when this model is selected.",
                provider.name()
            )
        };
        let input = self.key_input.clone().map(|state| {
            div().flex_1().min_w_0().child(
                Input::new(&state)
                    .content_type(InputContentType::Password)
                    .appearance(false)
                    .px_0()
                    .py_0()
                    .h(px(20.0))
                    .text_size(px(13.0)),
            )
        });
        let show_hide = div()
            .id("settings-show-key")
            .role(gpui_kit::Role::Button)
            .aria_label(if self.key_visible {
                "Hide API key"
            } else {
                "Show API key"
            })
            .text_size(px(12.0))
            .text_color(theme::SECONDARY)
            .cursor_pointer()
            .hover(|button| button.text_color(theme::LABEL))
            .on_click(cx.listener(|view, _, window, cx| {
                view.key_visible = !view.key_visible;
                if let Some(input) = &view.key_input {
                    input.update(cx, |input, cx| {
                        input.set_masked(!view.key_visible, window, cx)
                    });
                }
                cx.notify();
            }))
            .child(if self.key_visible { "Hide" } else { "Show" });
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_label("API KEY"))
                    .child(
                        div()
                            .id("settings-provider-key-url")
                            .role(gpui_kit::Role::Button)
                            .aria_label(format!("Open {} API keys", provider.name()))
                            .text_size(px(12.0))
                            .text_color(theme::AMBER)
                            .cursor_pointer()
                            .on_click(move |_, _, cx| cx.open_url(url))
                            .child(link),
                    ),
            )
            .child(
                div()
                    .h(px(40.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(8.0))
                    .bg(theme::HUD)
                    .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
                    .children(input)
                    .child(show_hide),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .line_height(px(17.0))
                    .text_color(theme::SECONDARY)
                    .child(note),
            )
    }
}

fn cloud_modal_header(provider: Provider, connected: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(14.0))
        .child(provider_tile(provider, 44.0))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_size(px(16.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::LABEL)
                        .child(if connected {
                            provider.name().to_string()
                        } else {
                            format!("Connect {}", provider.name())
                        }),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme::SECONDARY)
                        .child(provider.model()),
                ),
        )
        .when(connected, |header| {
            header.child(
                div()
                    .px(px(8.0))
                    .py(px(5.0))
                    .rounded_full()
                    .bg(theme::GREEN_SOFT)
                    .text_size(px(11.0))
                    .text_color(theme::GREEN)
                    .child("● Key saved"),
            )
        })
}

fn cloud_modal_footer(
    provider: Provider,
    connected: bool,
    cx: &mut Context<SettingsWindow>,
) -> Div {
    let leading = if connected {
        div()
            .id("settings-remove-key")
            .role(gpui_kit::Role::Button)
            .aria_label(format!("Remove {} API key", provider.name()))
            .text_size(px(12.0))
            .text_color(theme::RED)
            .cursor_pointer()
            .on_click(cx.listener(|view, _, window, cx| view.remove_cloud(window, cx)))
            .child("Remove key")
            .into_any_element()
    } else {
        div()
            .text_size(px(12.0))
            .text_color(theme::TERTIARY)
            .child(format!("Billed by {}", provider.name()))
            .into_any_element()
    };
    div()
        .h(px(60.0))
        .px(px(22.0))
        .border_t_1()
        .border_color(theme::HAIRLINE)
        .flex()
        .items_center()
        .justify_between()
        .child(leading)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    modal_button("settings-cloud-cancel", "Cancel", false)
                        .on_click(cx.listener(|view, _, window, cx| view.close_modal(window, cx))),
                )
                .child(
                    modal_button(
                        "settings-cloud-save",
                        if connected { "Use model" } else { "Save & use" },
                        true,
                    )
                    .on_click(cx.listener(|view, _, window, cx| view.save_cloud(window, cx))),
                ),
        )
}
