//! First-run setup, translated from Whisple's four Paper onboarding frames.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::input::{Input, InputContentType, InputEvent, InputState};
use gpui_kit::component::Root;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::{
    div, prelude::*, px, size, App, AppContext, Bounds, BoxShadow, Context, Div, Entity,
    FocusHandle, Focusable, FontWeight, IntoElement, Render, SharedString, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowKind, WindowOptions,
};

use crate::cloud::{self, Provider};
use crate::hotkey;
use crate::license;
use crate::microphone_permission;
use crate::models::{self, ModelSpec};
use crate::settings;
use crate::theme;
use crate::tray;

const WIDTH: f32 = 720.0;
const HEIGHT: f32 = 540.0;

struct Download {
    received: Arc<AtomicU64>,
    total: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModelSource {
    OnDevice,
    Cloud,
}

pub(crate) struct Onboarding {
    focus_handle: FocusHandle,
    step: usize,
    selected: &'static ModelSpec,
    source: ModelSource,
    cloud_provider: Provider,
    cloud_config: Option<Provider>,
    key_input: Option<Entity<InputState>>,
    key_visible: bool,
    microphone_allowed: bool,
    requesting_microphone: bool,
    download: Option<Download>,
    error: Option<String>,
}

pub(crate) fn open(cx: &mut App) {
    // The trial begins when onboarding opens, not when setup finishes. The
    // credential store can block, so do not access it on the UI thread.
    cx.background_executor()
        .spawn(async {
            let _ = license::start_trial();
        })
        .detach();
    let window_size = size(px(WIDTH), px(HEIGHT));
    let bounds = cx
        .primary_display()
        .map(|display| {
            let screen = display.bounds();
            Bounds {
                origin: gpui_kit::point(
                    screen.origin.x + (screen.size.width - window_size.width) * 0.5,
                    screen.origin.y + (screen.size.height - window_size.height) * 0.5,
                ),
                size: window_size,
            }
        })
        .unwrap_or(Bounds {
            origin: gpui_kit::point(px(100.0), px(100.0)),
            size: window_size,
        });
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            is_movable: true,
            app_owns_titlebar_drag: false,
            inactive_frame_interval: None,
            is_resizable: false,
            is_minimizable: false,
            display_id: None,
            window_background: WindowBackgroundAppearance::Transparent,
            icon: None,
            app_id: Some("whisple-onboarding".into()),
            window_min_size: Some(window_size),
            window_decorations: Some(WindowDecorations::Client),
            tabbing_identifier: None,
        },
        |window, cx| {
            let view = cx.new(|cx| Onboarding::new(window, cx));
            window.focus(&view.focus_handle(cx), cx);
            cx.new(|cx| {
                Root::new(view, window, cx)
                    .bordered(false)
                    .bg(gpui_kit::transparent_black())
            })
        },
    )
    .expect("open Whisple onboarding");
    cx.activate(true);
}

impl Onboarding {
    fn new(window: &Window, cx: &mut Context<Self>) -> Self {
        let prefs = settings::load();
        let saved_cloud = Provider::from_id(&prefs.selected);
        let selected = models::spec(&prefs.selected)
            .filter(|spec| ["preview", "turbo-q5", "turbo-q8"].contains(&spec.id))
            .unwrap_or_else(|| models::spec(models::recommended_id()).unwrap());
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            let alive = handle
                .update(cx, |_, window, cx| {
                    this.update(cx, |view: &mut Self, cx| {
                        if view.download.is_some() {
                            cx.notify();
                        }
                        if hotkey::take_press() {
                            window.activate_window();
                            cx.activate(true);
                        }
                        while let Some(command) = tray::take_command() {
                            match command {
                                tray::Command::Quit => cx.quit(),
                                tray::Command::Hide => cx.hide(),
                                tray::Command::Show
                                | tray::Command::Settings
                                | tray::Command::Update => {
                                    window.activate_window();
                                    cx.activate(true);
                                }
                            }
                        }
                    })
                    .is_ok()
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
        })
        .detach();
        Self {
            focus_handle: cx.focus_handle(),
            step: 0,
            selected,
            source: if saved_cloud.is_some() {
                ModelSource::Cloud
            } else {
                ModelSource::OnDevice
            },
            cloud_provider: saved_cloud.unwrap_or(Provider::OpenAi),
            cloud_config: None,
            key_input: None,
            key_visible: false,
            microphone_allowed: microphone_permission::is_allowed(),
            requesting_microphone: false,
            download: None,
            error: None,
        }
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.download.is_none() && !self.requesting_microphone {
            if self.cloud_config.take().is_some() {
                self.key_input = None;
                self.key_visible = false;
                window.focus(&self.focus_handle, cx);
            } else {
                self.step = self.step.saturating_sub(1);
            }
            self.error = None;
            cx.notify();
        }
    }

    fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.step {
            0 => self.step = 1,
            1 if self.microphone_allowed => self.step = 2,
            1 => self.request_microphone(cx),
            2 if self.cloud_config.is_some() => self.save_cloud_key(window, cx),
            2 if self.source == ModelSource::Cloud => self.prepare_cloud(window, cx),
            2 => self.prepare_model(cx),
            3 => self.finish(window, cx),
            _ => {}
        }
        cx.notify();
    }

    fn request_microphone(&mut self, cx: &mut Context<Self>) {
        if self.requesting_microphone {
            return;
        }
        self.requesting_microphone = true;
        self.error = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async { microphone_permission::request() })
                .await;
            this.update(cx, |view, cx| {
                view.requesting_microphone = false;
                match result {
                    Ok(()) => {
                        view.microphone_allowed = true;
                        view.step = 2;
                    }
                    Err(err) => {
                        view.error = Some(format!("Microphone access is unavailable: {err}"))
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn prepare_model(&mut self, cx: &mut Context<Self>) {
        if self.download.is_some() {
            return;
        }
        let spec = self.selected;
        if models::is_downloaded(spec) {
            models::save_selected(spec.id);
            self.step = 3;
            self.error = None;
            return;
        }
        let received = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        self.download = Some(Download {
            received: Arc::clone(&received),
            total: spec.bytes,
        });
        self.error = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { models::download(spec, &received, &cancel) })
                .await;
            this.update(cx, |view, cx| {
                view.download = None;
                match result {
                    Ok(_) => {
                        models::save_selected(spec.id);
                        view.step = 3;
                    }
                    Err(err) => view.error = Some(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn prepare_cloud(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let provider = self.cloud_provider;
        match cloud::has_key(provider) {
            Ok(true) => {
                models::save_selected(provider.id());
                self.step = 3;
                self.error = None;
            }
            Ok(false) => {
                self.cloud_config = Some(provider);
                self.error = None;
                self.key_visible = false;
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Paste API key")
                        .masked(true)
                });
                cx.subscribe(&input, |_, _, _: &InputEvent, cx| cx.notify())
                    .detach();
                window.focus(&input.focus_handle(cx), cx);
                self.key_input = Some(input);
            }
            Err(err) => self.error = Some(err),
        }
    }

    fn save_cloud_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(provider) = self.cloud_config else {
            return;
        };
        let key = self
            .key_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        match cloud::save_key(provider, &key) {
            Ok(()) => {
                models::save_selected(provider.id());
                self.cloud_config = None;
                self.key_input = None;
                self.key_visible = false;
                window.focus(&self.focus_handle, cx);
                self.step = 3;
                self.error = None;
            }
            Err(err) => self.error = Some(err),
        }
    }

    fn selected_id(&self) -> &'static str {
        match self.source {
            ModelSource::OnDevice => self.selected.id,
            ModelSource::Cloud => self.cloud_provider.id(),
        }
    }

    fn selected_name(&self) -> &'static str {
        match self.source {
            ModelSource::OnDevice => self.selected.name,
            ModelSource::Cloud => self.cloud_provider.name(),
        }
    }

    fn selected_chip(&self) -> &'static str {
        match self.source {
            ModelSource::OnDevice => self.selected.chip,
            ModelSource::Cloud => self.cloud_provider.name(),
        }
    }

    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut prefs = settings::load();
        prefs.selected = self.selected_id().to_string();
        prefs.onboarding_complete = true;
        settings::save(&prefs);
        crate::open_hud(cx, true);
        window.remove_window();
    }

    fn content(&self, cx: &mut Context<Self>) -> Div {
        let inner = match self.step {
            0 => self.welcome().into_any_element(),
            1 => self.microphone().into_any_element(),
            2 if self.cloud_config.is_some() => self.cloud_key_panel(cx).into_any_element(),
            2 => self.model_choice(cx).into_any_element(),
            _ => self.try_it().into_any_element(),
        };
        div()
            .flex_1()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px(px(72.0))
            .child(inner)
    }

    fn welcome(&self) -> Div {
        let bars = [15.0, 30.0, 40.0, 24.0, 12.0];
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(26.0))
            .pb(px(20.0))
            .child(
                div()
                    .size(px(104.0))
                    .rounded_full()
                    .bg(theme::AMBER_SOFT)
                    .shadow(vec![
                        BoxShadow::new(px(0.0), px(0.0), theme::tone(theme::AMBER_HALO_INNER))
                            .spread_radius(px(10.0)),
                        BoxShadow::new(px(0.0), px(0.0), theme::tone(theme::AMBER_HALO_OUTER))
                            .spread_radius(px(22.0)),
                        BoxShadow::new(px(0.0), px(20.0), theme::tone(theme::AMBER_WASH))
                            .blur_radius(px(60.0)),
                    ])
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(bars.into_iter().map(|height| {
                        div()
                            .w(px(5.0))
                            .h(px(height))
                            .ml(px(5.0))
                            .rounded_full()
                            .bg(theme::AMBER)
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.0))
                    .child(heading("Welcome to Whisple", 34.0))
                    .child(description(
                        "A voice bar for your whole Mac. Three quick steps and you’re talking instead of typing.",
                        420.0,
                        16.0,
                    )),
            )
            .child(
                div()
                    .h(px(28.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded_full()
                    .bg(theme::INSET)
                    .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
                    .child(div().size(px(6.0)).rounded_full().bg(theme::AMBER))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::SECONDARY)
                            .child("On-device or cloud · your choice"),
                    ),
            )
    }

    fn microphone(&self) -> Div {
        let status = if self.microphone_allowed {
            "Allowed"
        } else if self.requesting_microphone {
            "Waiting for macOS"
        } else {
            "Not allowed yet"
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(28.0))
            .pb(px(12.0))
            .child(
                div()
                    .size(px(72.0))
                    .rounded(px(20.0))
                    .bg(theme::RAISED)
                    .shadow(vec![theme::top_edge(theme::TILE_SHEEN)])
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::empty()
                            .path("icons/whisp/onboarding-mic.svg")
                            .text_color(theme::AMBER)
                            .with_size(px(32.0)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.0))
                    .child(heading("Let Whisple hear you", 30.0))
                    .child(description(
                        "macOS will ask for microphone access. Whisple only listens while recording. With a local model, audio stays on your Mac.",
                        440.0,
                        15.0,
                    )),
            )
            .child(
                div()
                    .w(px(440.0))
                    .flex()
                    .flex_col()
                    .rounded(px(12.0))
                    .overflow_hidden()
                    .bg(theme::INSET)
                    .child(
                        div()
                            .h(px(52.0))
                            .px(px(14.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .size(px(26.0))
                                    .rounded(px(7.0))
                                    .bg(theme::RAISED)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        Icon::empty()
                                            .path("icons/whisp/onboarding-mic.svg")
                                            .text_color(theme::LABEL)
                                            .with_size(px(14.0)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme::LABEL)
                                    .child("Microphone"),
                            )
                            .child(
                                div()
                                    .h(px(24.0))
                                    .px(px(10.0))
                                    .flex()
                                    .items_center()
                                    .rounded_full()
                                    .bg(theme::RAISED)
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if self.microphone_allowed {
                                        theme::AMBER
                                    } else {
                                        theme::SECONDARY
                                    })
                                    .child(status),
                            ),
                    )
                    .child(
                        div()
                            .h(px(44.0))
                            .px(px(14.0))
                            .border_t_1()
                            .border_color(theme::HAIRLINE)
                            .flex()
                            .items_center()
                            .text_size(px(12.0))
                            .text_color(theme::TERTIARY)
                            .child("You can change this later in System Settings › Privacy & Security."),
                    ),
            )
            .when_some(self.error.as_ref(), |this, error| {
                this.child(
                    div()
                        .w(px(440.0))
                        .text_center()
                        .text_size(px(12.0))
                        .text_color(theme::RED)
                        .child(error.clone()),
                )
            })
    }

    fn model_choice(&self, cx: &mut Context<Self>) -> Div {
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(18.0))
            .pb(px(8.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .child(heading("Pick a voice model", 30.0))
                    .child(description(if self.source == ModelSource::OnDevice {
                        "It downloads once and runs entirely on your Mac. Switch models any time from the bar."
                    } else {
                        "Use an online model with your own API key. No model download needed."
                    }, 440.0, 15.0)),
            )
            .child(self.model_source_switch(cx))
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap(px(12.0))
                    .justify_center()
                    .when(self.source == ModelSource::OnDevice, |this| {
                        this.children(
                            ["preview", "turbo-q5", "turbo-q8"]
                                .into_iter()
                                .filter_map(models::spec)
                                .map(|spec| self.model_card(spec, cx)),
                        )
                    })
                    .when(self.source == ModelSource::Cloud, |this| {
                        this.children(Provider::ALL.map(|provider| self.cloud_card(provider, cx)))
                    }),
            )
            .child(
                div()
                    .h(px(18.0))
                    .text_size(px(12.0))
                    .text_color(theme::TERTIARY)
                    .child(if self.source == ModelSource::OnDevice {
                        "More models are in Settings › Models, from 142 MB to 1.5 GB."
                    } else {
                        "Recordings are sent to the selected provider. Provider charges may apply."
                    }),
            )
            .when_some(self.error.as_ref(), |this, error| {
                this.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme::RED)
                        .child(error.clone()),
                )
            })
    }

    fn model_source_switch(&self, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(34.0))
            .p(px(3.0))
            .flex()
            .gap(px(3.0))
            .rounded_full()
            .bg(theme::INSET)
            .children(
                [
                    (ModelSource::OnDevice, "On device"),
                    (ModelSource::Cloud, "Cloud"),
                ]
                .map(|(source, label)| {
                    div()
                        .id(SharedString::from(format!("onboarding-source-{label}")))
                        .h_full()
                        .px(px(18.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(if self.source == source {
                            theme::RAISED
                        } else {
                            theme::INSET
                        })
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if self.source == source {
                            theme::LABEL
                        } else {
                            theme::SECONDARY
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.source = source;
                            view.error = None;
                            cx.notify();
                        }))
                        .child(label)
                }),
            )
    }

    fn cloud_card(&self, provider: Provider, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.cloud_provider == provider;
        div()
            .id(SharedString::from(format!(
                "onboarding-cloud-{}",
                provider.id()
            )))
            .w(px(250.0))
            .h(px(168.0))
            .p(px(18.0))
            .flex()
            .flex_col()
            .gap(px(7.0))
            .rounded(px(14.0))
            .bg(if selected {
                theme::AMBER_SOFT
            } else {
                theme::INSET
            })
            .shadow(vec![theme::inner_ring(if selected {
                theme::AMBER
            } else {
                theme::HAIRLINE
            })])
            .cursor_pointer()
            .on_click(cx.listener(move |view, _, _, cx| {
                view.cloud_provider = provider;
                view.error = None;
                cx.notify();
            }))
            .child(
                div()
                    .h(px(28.0))
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(Icon::empty().path(provider.icon()).with_size(px(23.0)))
                    .child(
                        div()
                            .size(px(18.0))
                            .rounded_full()
                            .border_2()
                            .border_color(if selected {
                                theme::AMBER
                            } else {
                                theme::TERTIARY
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(selected, |this| {
                                this.bg(theme::AMBER).text_color(theme::HUD).child("✓")
                            }),
                    ),
            )
            .child(
                div()
                    .pt(px(5.0))
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::LABEL)
                    .child(provider.name()),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if selected {
                        theme::AMBER
                    } else {
                        theme::TERTIARY
                    })
                    .child(provider.model()),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .line_height(px(19.0))
                    .text_color(theme::SECONDARY)
                    .child(provider.description()),
            )
    }

    fn cloud_key_panel(&self, cx: &mut Context<Self>) -> Div {
        let provider = self.cloud_config.unwrap();
        let input = self.key_input.clone();
        div()
            .w(px(440.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(20.0))
            .child(Icon::empty().path(provider.icon()).with_size(px(42.0)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.0))
                    .child(heading("Connect your cloud model", 28.0))
                    .child(description("Enter your API key to use this model in Whisple.", 440.0, 15.0)),
            )
            .child(
                div()
                    .w_full()
                    .p(px(18.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .rounded(px(12.0))
                    .bg(theme::INSET)
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::LABEL)
                            .child(format!("{} · {}", provider.name(), provider.model())),
                    )
                    .child(
                        div()
                            .h(px(42.0))
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .rounded(px(9.0))
                            .bg(theme::HUD)
                            .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
                            .children(input.map(|state| {
                                div().flex_1().min_w_0().child(
                                    Input::new(&state)
                                        .content_type(InputContentType::Password)
                                        .appearance(false)
                                        .px_0()
                                        .py_0()
                                        .h(px(20.0))
                                        .text_size(px(13.0)),
                                )
                            }))
                            .child(
                                div()
                                    .id("onboarding-key-visibility")
                                    .text_size(px(12.0))
                                    .text_color(theme::SECONDARY)
                                    .cursor_pointer()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.key_visible = !view.key_visible;
                                        if let Some(input) = &view.key_input {
                                            input.update(cx, |input, cx| {
                                                input.set_masked(!view.key_visible, window, cx)
                                            });
                                        }
                                        cx.notify();
                                    }))
                                    .child(if self.key_visible { "Hide" } else { "Show" }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .line_height(px(18.0))
                            .text_color(theme::SECONDARY)
                            .child(format!("Your key is saved in the system credential store. Recordings are sent to {} for transcription. Provider charges may apply.", provider.name())),
                    ),
            )
            .when_some(self.error.as_ref(), |this, error| {
                this.child(div().text_size(px(12.0)).text_color(theme::RED).child(error.clone()))
            })
    }

    fn model_card(&self, spec: &'static ModelSpec, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected.id == spec.id;
        div()
            .id(SharedString::from(format!("onboarding-model-{}", spec.id)))
            .w(px(184.0))
            .h(px(168.0))
            .p(px(18.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .rounded(px(14.0))
            .bg(if selected {
                theme::AMBER_SOFT
            } else {
                theme::INSET
            })
            .shadow(vec![theme::inner_ring(if selected {
                theme::AMBER
            } else {
                theme::HAIRLINE
            })])
            .cursor_pointer()
            .on_click(cx.listener(move |view, _, _, cx| {
                if view.download.is_none() {
                    view.selected = spec;
                    view.error = None;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .h(px(32.0))
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(
                        div()
                            .size(px(18.0))
                            .rounded_full()
                            .border_2()
                            .border_color(if selected {
                                theme::AMBER
                            } else {
                                theme::TERTIARY
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(selected, |this| {
                                this.bg(theme::AMBER).text_color(theme::HUD).child("✓")
                            }),
                    )
                    .when(spec.recommended, |this| {
                        this.child(
                            div()
                                .px(px(7.0))
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
                    .pt(px(10.0))
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::LABEL)
                    .child(spec.name),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if selected {
                        theme::AMBER
                    } else {
                        theme::TERTIARY
                    })
                    .child(models::format_size(spec.bytes)),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .line_height(px(19.0))
                    .text_color(theme::SECONDARY)
                    .child(spec.blurb),
            )
    }

    fn try_it(&self) -> Div {
        let caps = hotkey::keycaps(&settings::load().show_hotkey);
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(24.0))
            .child(
                div()
                    .h(px(28.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded_full()
                    .bg(theme::AMBER_SOFT)
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::AMBER)
                    .child(format!("✓  {} is ready", self.selected_name())),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.0))
                    .child(heading("Now try it", 30.0))
                    .child(description(
                        {
                            #[cfg(target_os = "macos")]
                            let instructions = "Press the shortcut, say a sentence, then press Space. Your words go into the selected text field. Clipboard copying is optional.";
                            #[cfg(not(target_os = "macos"))]
                            let instructions = "Press the shortcut, say a sentence, then press Space. Your words land on the clipboard.";
                            instructions
                        },
                        440.0,
                        15.0,
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .children(caps.into_iter().map(|cap| {
                        div()
                            .h(px(44.0))
                            .min_w(px(44.0))
                            .px(px(if cap.chars().count() > 1 { 20.0 } else { 12.0 }))
                            .rounded(px(11.0))
                            .bg(theme::RAISED)
                            .shadow(vec![theme::top_edge(theme::TILE_SHEEN)])
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(if cap.chars().count() > 1 { 15.0 } else { 20.0 }))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::LABEL)
                            .child(cap)
                    })),
            )
            .child(
                div()
                    .w(px(400.0))
                    .h(px(58.0))
                    .px(px(10.0))
                    .rounded(px(20.0))
                    .bg(theme::HUD)
                    .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .size(px(36.0))
                            .rounded_full()
                            .bg(theme::RED)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().size(px(11.0)).rounded(px(3.0)).bg(theme::LABEL)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap(px(3.0))
                            .children([3., 5., 4., 8., 12., 7., 10., 16., 20., 13., 9., 15., 22., 18., 11., 14., 19., 8., 4.].into_iter().map(|height| {
                                div().w(px(3.0)).h(px(height)).rounded_full().bg(theme::LABEL)
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(theme::SECONDARY)
                            .child("0:07"),
                    )
                    .child(
                        div()
                            .size(px(28.0))
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
                    .child(
                        div()
                            .px(px(11.0))
                            .h(px(28.0))
                            .rounded_full()
                            .bg(theme::RAISED)
                            .flex()
                            .items_center()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::LABEL)
                            .child(self.selected_chip()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::AMBER)
                            .child(format!(
                                "Your 3-day free trial has started. After that, Whisple is {} once.",
                                license::PRICE
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(theme::TERTIARY)
                            .child("You can change the shortcut any time in Settings."),
                    ),
            )
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let label = match self.step {
            0 => "Get started".to_string(),
            1 if self.requesting_microphone => "Waiting for macOS…".to_string(),
            1 if self.microphone_allowed => "Continue".to_string(),
            1 => "Allow microphone".to_string(),
            2 if self.download.is_some() => {
                let download = self.download.as_ref().unwrap();
                let fraction =
                    download.received.load(Ordering::Relaxed) as f64 / download.total.max(1) as f64;
                format!(
                    "Downloading {}%",
                    (fraction * 100.0).clamp(0.0, 100.0) as u32
                )
            }
            2 if self.cloud_config.is_some() => {
                format!("Save and use {}", self.cloud_provider.name())
            }
            2 if self.source == ModelSource::Cloud => format!("Use {}", self.cloud_provider.name()),
            2 if models::is_downloaded(self.selected) => format!("Use {}", self.selected.name),
            2 => format!("Download {}", self.selected.name),
            _ => "Start using Whisple".to_string(),
        };
        let busy = self.download.is_some() || self.requesting_microphone;
        div()
            .h(px(72.0))
            .w_full()
            .px(px(24.0))
            .border_t_1()
            .border_color(theme::HAIRLINE)
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .children((0..4).map(|index| {
                        div()
                            .w(px(if self.step == index { 20.0 } else { 6.0 }))
                            .h(px(6.0))
                            .rounded_full()
                            .bg(if self.step == index {
                                theme::AMBER
                            } else {
                                theme::RAISED
                            })
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .when(self.step > 0, |this| {
                        this.child(
                            div()
                                .id("onboarding-back")
                                .h(px(36.0))
                                .px(px(16.0))
                                .flex()
                                .items_center()
                                .rounded_full()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::SECONDARY)
                                .cursor_pointer()
                                .on_click(cx.listener(|view, _, window, cx| view.back(window, cx)))
                                .child("Back"),
                        )
                    })
                    .child(
                        div()
                            .id("onboarding-next")
                            .h(px(36.0))
                            .px(px(20.0))
                            .rounded_full()
                            .bg(theme::AMBER)
                            .opacity(if busy { 0.55 } else { 1.0 })
                            .flex()
                            .items_center()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::HUD)
                            .cursor_pointer()
                            .on_click(cx.listener(|view, _, window, cx| view.next(window, cx)))
                            .child(label),
                    ),
            )
    }
}

impl Focusable for Onboarding {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Onboarding {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("onboarding")
            .key_context("Onboarding")
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(
                cx.listener(|view, event: &gpui_kit::KeyDownEvent, window, cx| {
                    match event.keystroke.key.to_lowercase().as_str() {
                        "enter" | "return" => view.next(window, cx),
                        "escape" => view.back(window, cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                }),
            )
            .flex()
            .flex_col()
            .rounded(px(16.0))
            .overflow_hidden()
            .bg(theme::HUD)
            .border_1()
            .border_color(theme::HAIRLINE)
            .shadow(vec![theme::top_edge(theme::EDGE)])
            .font_family(theme::UI_FONT)
            .child(
                div()
                    .h(px(44.0))
                    .w_full()
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .children(
                        [0xff5f57ff_u32, 0xfebc2eff, 0x28c840ff]
                            .into_iter()
                            .map(|color| {
                                div().size(px(12.0)).rounded_full().bg(gpui_kit::Rgba {
                                    r: ((color >> 24) & 0xff) as f32 / 255.0,
                                    g: ((color >> 16) & 0xff) as f32 / 255.0,
                                    b: ((color >> 8) & 0xff) as f32 / 255.0,
                                    a: 1.0,
                                })
                            }),
                    ),
            )
            .child(self.content(cx))
            .child(self.footer(cx))
    }
}

fn heading(label: &'static str, size: f32) -> Div {
    div()
        .text_center()
        .text_size(px(size))
        .line_height(px(size + 6.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::LABEL)
        .child(label)
}

fn description(label: &'static str, width: f32, size: f32) -> Div {
    div()
        .w(px(width))
        .text_center()
        .text_size(px(size))
        .line_height(px(size + 8.0))
        .text_color(theme::SECONDARY)
        .child(label)
}
