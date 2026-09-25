use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputContentType, InputEvent, InputState};
use gpui_kit::component::Root;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::{
    div, prelude::*, px, size, App, AppContext, Bounds, BoxShadow, Context, Div, Entity,
    FocusHandle, Focusable, FontWeight, IntoElement, Render, SharedString, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowKind, WindowOptions,
};

use crate::i18n::{self, t, tf, Lang};
#[cfg(feature = "licensing")]
use crate::licensing as license;
use crate::platform::hotkey;
use crate::platform::microphone_permission;
use crate::platform::tray;
use crate::settings;
use crate::transcription::cloud::{self, Provider};
use crate::transcription::models::{self, ModelSpec};
use crate::ui::theme;

const WIDTH: f32 = 720.0;
/// Height of the transparent macOS title bar, which the window content runs
/// under so the native traffic lights sit on the onboarding surface. Other
/// systems draw their title bar above the content instead.
const TITLEBAR_HEIGHT: f32 = if cfg!(target_os = "macos") { 44.0 } else { 0.0 };
const HEIGHT: f32 = 496.0 + TITLEBAR_HEIGHT;

const WELCOME: usize = 0;
const FEATURES: usize = 1;
const MICROPHONE: usize = 2;
const MODEL: usize = 3;
const TRY_IT: usize = 4;
const STEPS: usize = 5;

struct Download {
    received: Arc<AtomicU64>,
    total: u64,
}

#[derive(Clone, Copy)]
enum Choice {
    Local(&'static ModelSpec),
    Cloud(Provider),
}

impl Choice {
    /// Every choice: the cloud providers, then the recommended download and
    /// the other downloads from smallest to largest.
    fn all() -> Vec<Self> {
        let mut local: Vec<&'static ModelSpec> = models::CATALOG.iter().collect();
        local.sort_by_key(|spec| (!spec.recommended, spec.bytes));
        Provider::ALL
            .map(Self::Cloud)
            .into_iter()
            .chain(local.into_iter().map(Self::Local))
            .collect()
    }

    fn id(self) -> &'static str {
        match self {
            Self::Local(spec) => spec.id,
            Self::Cloud(provider) => provider.id(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Local(spec) => t(spec.name),
            Self::Cloud(provider) => provider.name(),
        }
    }

    fn chip(self) -> &'static str {
        match self {
            Self::Local(spec) => spec.chip,
            Self::Cloud(provider) => provider.name(),
        }
    }
}

pub(crate) struct Onboarding {
    focus_handle: FocusHandle,
    step: usize,
    choice: Choice,
    cloud_config: Option<Provider>,
    key_input: Option<Entity<InputState>>,
    key_visible: bool,
    microphone_allowed: bool,
    requesting_microphone: bool,
    key_busy: bool,
    download: Option<Download>,
    error: Option<String>,
}

pub(crate) fn open(cx: &mut App) {
    #[cfg(feature = "licensing")]
    {
        // The trial begins when onboarding opens, not when setup finishes. The
        // credential store can block, so do not access it on the UI thread.
        cx.background_executor()
            .spawn(async {
                let _ = license::start_trial();
            })
            .detach();
    }
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
            // The system draws the window controls: macOS traffic lights over
            // the top of the onboarding surface, and the regular title bar on
            // Windows and Linux.
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some("Whisple".into()),
                appears_transparent: cfg!(target_os = "macos"),
                traffic_light_position: Some(gpui_kit::point(px(16.0), px(16.0))),
            }),
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            is_movable: true,
            app_owns_titlebar_drag: false,
            inactive_frame_interval: None,
            is_resizable: false,
            is_minimizable: false,
            display_id: None,
            window_background: WindowBackgroundAppearance::Opaque,
            icon: None,
            app_id: Some("whisple-onboarding".into()),
            window_min_size: Some(window_size),
            window_decorations: Some(WindowDecorations::Server),
            tabbing_identifier: None,
        },
        |window, cx| {
            let view = cx.new(|cx| Onboarding::new(window, cx));
            window.focus(&view.focus_handle(cx), cx);
            // Onboarding is the only window until setup finishes, so closing
            // it with the system close button quits instead of leaving an
            // invisible process behind.
            window.on_window_should_close(cx, |_, cx| {
                cx.quit();
                true
            });
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

#[cfg(feature = "licensing")]
fn trial_message() -> Option<Div> {
    Some(
        div()
            .text_size(px(13.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::AMBER)
            .child(tf(
                "Your 3-day free trial has started. After that, Whisple is {} once.",
                &[&license::PRICE],
            )),
    )
}

#[cfg(not(feature = "licensing"))]
fn trial_message() -> Option<Div> {
    None
}

impl Onboarding {
    fn new(window: &Window, cx: &mut Context<Self>) -> Self {
        let prefs = settings::load();
        let choice = Provider::from_id(&prefs.selected)
            .map(Choice::Cloud)
            .or_else(|| models::spec(&prefs.selected).map(Choice::Local))
            .unwrap_or_else(|| {
                Choice::Local(models::spec(models::recommended_id()).expect("recommended model"))
            });
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
                        if hotkey::take_presses().any() {
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
            step: WELCOME,
            choice,
            cloud_config: None,
            key_input: None,
            key_visible: false,
            microphone_allowed: microphone_permission::is_allowed(),
            requesting_microphone: false,
            key_busy: false,
            download: None,
            error: None,
        }
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.download.is_none() && !self.requesting_microphone && !self.key_busy {
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
        if self.download.is_some() || self.requesting_microphone || self.key_busy {
            return;
        }
        match self.step {
            WELCOME => self.step = FEATURES,
            FEATURES => self.step = MICROPHONE,
            MICROPHONE if self.microphone_allowed => self.step = MODEL,
            MICROPHONE => self.request_microphone(window, cx),
            MODEL if self.cloud_config.is_some() => self.save_cloud_key(window, cx),
            MODEL => match self.choice {
                Choice::Cloud(provider) => self.prepare_cloud(provider, window, cx),
                Choice::Local(spec) => self.prepare_model(spec, cx),
            },
            TRY_IT => self.finish(window, cx),
            _ => {}
        }
        cx.notify();
    }

    fn request_microphone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.requesting_microphone {
            return;
        }
        self.requesting_microphone = true;
        self.error = None;
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async { microphone_permission::request() })
                .await;
            this.update_in(cx, |view, window, cx| {
                // macOS asks in its own dialog and then returns focus to the
                // app that was in front before. Whisple has no Dock icon, so
                // onboarding would stay buried behind that app's windows.
                window.activate_window();
                cx.activate(true);
                view.requesting_microphone = false;
                match result {
                    Ok(()) => {
                        view.microphone_allowed = true;
                        view.step = MODEL;
                    }
                    Err(err) => {
                        view.error = Some(tf("Microphone access is unavailable: {}", &[&err]))
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn prepare_model(&mut self, spec: &'static ModelSpec, cx: &mut Context<Self>) {
        if self.download.is_some() {
            return;
        }
        if models::is_downloaded(spec) {
            models::save_selected(spec.id);
            self.step = TRY_IT;
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
                        view.step = TRY_IT;
                    }
                    Err(err) => view.error = Some(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn prepare_cloud(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        self.key_busy = true;
        self.error = None;
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { cloud::has_key(provider) })
                .await;
            this.update_in(cx, |view, window, cx| {
                view.key_busy = false;
                match result {
                    Ok(true) => {
                        models::save_selected(provider.id());
                        view.step = TRY_IT;
                    }
                    Ok(false) => {
                        view.cloud_config = Some(provider);
                        view.key_visible = false;
                        let input = cx.new(|cx| {
                            InputState::new(window, cx)
                                .placeholder(t("Paste API key"))
                                .masked(true)
                        });
                        cx.subscribe(&input, |_, _, _: &InputEvent, cx| cx.notify())
                            .detach();
                        window.focus(&input.focus_handle(cx), cx);
                        view.key_input = Some(input);
                    }
                    Err(err) => view.error = Some(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
        self.key_busy = true;
        self.error = None;
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { cloud::save_key(provider, &key) })
                .await;
            this.update_in(cx, |view, window, cx| {
                view.key_busy = false;
                match result {
                    Ok(()) => {
                        models::save_selected(provider.id());
                        view.cloud_config = None;
                        view.key_input = None;
                        view.key_visible = false;
                        window.focus(&view.focus_handle, cx);
                        view.step = TRY_IT;
                    }
                    Err(err) => view.error = Some(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut prefs = settings::load();
        prefs.selected = self.choice.id().to_string();
        // Keep the detected language once the user has seen it.
        if prefs.app_language.is_empty() {
            prefs.app_language = i18n::current().code().to_string();
        }
        prefs.onboarding_complete = true;
        if let Err(err) = settings::try_save(&prefs) {
            self.error = Some(tf("Could not save settings: {}", &[&err]));
            cx.notify();
            return;
        }
        crate::open_hud(cx, true);
        window.remove_window();
    }

    fn content(&self, cx: &mut Context<Self>) -> Div {
        let inner = match self.step {
            WELCOME => self.welcome(cx).into_any_element(),
            FEATURES => self.features().into_any_element(),
            MICROPHONE => self.microphone().into_any_element(),
            MODEL if self.cloud_config.is_some() => self.cloud_key_panel(cx).into_any_element(),
            MODEL => self.model_choice(cx).into_any_element(),
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

    fn welcome(&self, cx: &mut Context<Self>) -> Div {
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
                    .child(heading(t("Welcome to Whisple"), 34.0))
                    .child(description(
                        t("A voice bar for your whole computer. A few quick steps and you’re talking instead of typing."),
                        420.0,
                        16.0,
                    )),
            )
            .child(language_picker(cx))
    }

    fn microphone(&self) -> Div {
        let status = if self.microphone_allowed {
            t("Allowed")
        } else if self.requesting_microphone {
            if cfg!(target_os = "windows") {
                t("Checking the microphone")
            } else {
                t("Waiting for macOS")
            }
        } else {
            t("Not allowed yet")
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
                    .child(heading(t("Let Whisple hear you"), 30.0))
                    .child(description(
                        if cfg!(target_os = "windows") { t("Whisple only listens while recording. With a local model, audio stays on your PC.") } else { t("macOS will ask for microphone access. Whisple only listens while recording. With a local model, audio stays on your Mac.") },
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
                                    .child(t("Microphone")),
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
                            .child(if cfg!(target_os = "windows") { t("You can change this later in Settings › Privacy & security › Microphone.") } else { t("You can change this later in System Settings › Privacy & Security.") }),
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

    fn features(&self) -> Div {
        let shortcut = hotkey::symbols(&record_shortcut());
        #[cfg(target_os = "macos")]
        let dictate = tf(
            "Press {} to start talking and again to finish. Your words are typed into the app you were using.",
            &[&shortcut],
        );
        #[cfg(not(target_os = "macos"))]
        let dictate = tf(
            "Press {} to start talking and again to finish. Your words are copied, ready to paste.",
            &[&shortcut],
        );
        let rows = [
            (Lucide::Keyboard, t("Dictate anywhere"), dictate),
            (
                Lucide::AppWindow,
                t("Open apps by voice"),
                t("Say “Open Spotify” or “Go to github.com” and Whisple opens it.").to_string(),
            ),
            (
                Lucide::ScanEye,
                t("Ask about your screen"),
                t("Start with “Hey Whisple” to ask about what you see, or have it write a reply for you. Uses your OpenAI or Groq key.").to_string(),
            ),
            (
                Lucide::ShieldCheck,
                t("Private by default"),
                t("On-device models keep your voice on this computer. Cloud models are optional.")
                    .to_string(),
            ),
        ];
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(22.0))
            .pb(px(8.0))
            .child(heading(t("What Whisple can do"), 30.0))
            .child(
                div()
                    .w(px(520.0))
                    .flex()
                    .flex_col()
                    .rounded(px(12.0))
                    .overflow_hidden()
                    .bg(theme::INSET)
                    .children(rows.into_iter().enumerate().map(
                        |(index, (icon, title, detail))| {
                            div()
                                .px(px(16.0))
                                .py(px(13.0))
                                .flex()
                                .items_start()
                                .gap(px(14.0))
                                .when(index > 0, |row| {
                                    row.border_t_1().border_color(theme::HAIRLINE)
                                })
                                .child(
                                    div()
                                        .size(px(32.0))
                                        .flex_shrink_0()
                                        .rounded(px(9.0))
                                        .bg(theme::AMBER_SOFT)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            Icon::new(icon)
                                                .text_color(theme::AMBER)
                                                .with_size(px(16.0)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.0))
                                        .child(
                                            div()
                                                .text_size(px(14.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme::LABEL)
                                                .child(title),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.0))
                                                .line_height(px(17.0))
                                                .text_color(theme::SECONDARY)
                                                .child(detail),
                                        ),
                                )
                        },
                    )),
            )
    }

    fn model_choice(&self, cx: &mut Context<Self>) -> Div {
        let choices = Choice::all();
        let last = choices.len() - 1;
        let detail = match self.choice {
            Choice::Local(spec) => tf("{} · Runs on this computer", &[&t(spec.blurb)]),
            Choice::Cloud(provider) => {
                tf("{} · Uses your own API key", &[&t(provider.description())])
            }
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.0))
                    .child(heading(t("Pick a voice model"), 30.0))
                    .child(description(
                        t("You can switch any time from the bar."),
                        440.0,
                        14.0,
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
                    .children(
                        choices
                            .into_iter()
                            .enumerate()
                            .map(|(index, choice)| self.choice_row(choice, index == last, cx)),
                    ),
            )
            .child(
                div()
                    .w(px(440.0))
                    .h(px(16.0))
                    .text_center()
                    .text_size(px(12.0))
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(if self.error.is_some() {
                        theme::RED
                    } else {
                        theme::SECONDARY
                    })
                    .child(self.error.clone().unwrap_or(detail)),
            )
    }

    fn choice_row(&self, choice: Choice, last: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.choice.id() == choice.id();
        let (tag, recommended) = match choice {
            Choice::Local(spec) => (models::format_size(spec.bytes), spec.recommended),
            Choice::Cloud(_) => (t("Cloud").to_string(), false),
        };
        div()
            .id(SharedString::from(format!(
                "onboarding-model-{}",
                choice.id()
            )))
            .h(px(31.0))
            .px(px(14.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .when(!last, |row| row.border_b_1().border_color(theme::HAIRLINE))
            .when(selected, |row| row.bg(theme::AMBER_SOFT))
            .role(gpui_kit::Role::RadioButton)
            .aria_label(choice.name())
            .aria_selected(selected)
            .when(selected, |row| row.aria_active_descendant())
            .cursor_pointer()
            .hover(|row| row.bg(theme::RAISED))
            .on_click(cx.listener(move |view, _, _, cx| {
                if view.download.is_none() && !view.key_busy {
                    view.choice = choice;
                    view.error = None;
                    cx.notify();
                }
            }))
            .child(radio(selected))
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(if selected {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(theme::LABEL)
                    .child(choice.name()),
            )
            .when(recommended, |row| {
                row.child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::AMBER)
                        .child(t("Recommended")),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(12.0))
                    .font_features(theme::tabular())
                    .text_color(if selected {
                        theme::AMBER
                    } else {
                        theme::TERTIARY
                    })
                    .child(tag),
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
                    .child(heading(t("Connect your cloud model"), 28.0))
                    .child(description(t("Enter your API key to use this model in Whisple."), 440.0, 15.0)),
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
                    .when(provider == Provider::Vercel, |panel| {
                        panel.child(crate::ui::gateway_model_picker("onboarding-gateway"))
                    })
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
                                    .role(gpui_kit::Role::Button)
                                    .aria_label(if self.key_visible { t("Hide") } else { t("Show") })
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
                                    .child(if self.key_visible { t("Hide") } else { t("Show") }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .line_height(px(18.0))
                            .text_color(theme::SECONDARY)
                            .child(tf("Your key is saved in the system credential store. Recordings are sent to {} for transcription. Provider charges may apply.", &[&provider.name()])),
                    ),
            )
            .when_some(self.error.as_ref(), |this, error| {
                this.child(div().text_size(px(12.0)).text_color(theme::RED).child(error.clone()))
            })
    }

    fn try_it(&self) -> Div {
        let caps = hotkey::keycaps(&record_shortcut());
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
                    .child(tf("✓  {} is ready", &[&self.choice.name()])),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.0))
                    .child(heading(t("Ready to start"), 30.0))
                    .child(description(
                        t("Click Start using Whisple, then press the shortcut to record. Press it again to finish."),
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
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme::TERTIARY)
                            .child(t("Voice bar preview")),
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
                            .child(self.choice.chip()),
                    ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .when_some(trial_message(), |pane, message| pane.child(message))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(theme::TERTIARY)
                            .child(t("You can change the shortcut any time in Settings.")),
                    ),
            )
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let label = match self.step {
            WELCOME => t("Get started").to_string(),
            FEATURES => t("Continue").to_string(),
            MICROPHONE if self.requesting_microphone => if cfg!(target_os = "windows") {
                t("Checking the microphone…")
            } else {
                t("Waiting for macOS…")
            }
            .to_string(),
            MICROPHONE if self.microphone_allowed => t("Continue").to_string(),
            MICROPHONE => t("Allow microphone").to_string(),
            MODEL if self.download.is_some() => {
                let download = self.download.as_ref().unwrap();
                let fraction =
                    download.received.load(Ordering::Relaxed) as f64 / download.total.max(1) as f64;
                tf(
                    "Downloading {}%",
                    &[&((fraction * 100.0).clamp(0.0, 100.0) as u32)],
                )
            }
            MODEL if self.key_busy => t("Working…").to_string(),
            MODEL if self.cloud_config.is_some() => tf("Save and use {}", &[&self.choice.name()]),
            MODEL => match self.choice {
                Choice::Cloud(provider) => tf("Use {}", &[&provider.name()]),
                Choice::Local(spec) if models::is_downloaded(spec) => {
                    tf("Use {}", &[&t(spec.name)])
                }
                Choice::Local(spec) => tf("Download {}", &[&t(spec.name)]),
            },
            _ => t("Start using Whisple").to_string(),
        };
        let busy = self.download.is_some() || self.requesting_microphone || self.key_busy;
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
                    .children((0..STEPS).map(|index| {
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
                                .role(gpui_kit::Role::Button)
                                .aria_label(t("Back"))
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
                                .child(t("Back")),
                        )
                    })
                    .child(
                        div()
                            .id("onboarding-next")
                            .role(gpui_kit::Role::Button)
                            .aria_label(label.clone())
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
                    if view.step == MODEL
                        && view.cloud_config.is_none()
                        && view.download.is_none()
                        && !view.key_busy
                    {
                        let delta = match event.keystroke.key.to_lowercase().as_str() {
                            "up" => Some(-1isize),
                            "down" => Some(1isize),
                            _ => None,
                        };
                        if let Some(delta) = delta {
                            let choices = Choice::all();
                            let current = choices
                                .iter()
                                .position(|choice| choice.id() == view.choice.id())
                                .unwrap_or(0);
                            let next = (current as isize + delta)
                                .clamp(0, choices.len().saturating_sub(1) as isize)
                                as usize;
                            view.choice = choices[next];
                            view.error = None;
                            cx.notify();
                            cx.stop_propagation();
                            return;
                        }
                    }
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
            .overflow_hidden()
            .bg(theme::HUD)
            .font_family(theme::UI_FONT)
            .child(div().h(px(TITLEBAR_HEIGHT)).flex_shrink_0())
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

fn language_picker(cx: &mut Context<Onboarding>) -> Div {
    let current = i18n::current();
    div()
        .flex()
        .flex_wrap()
        .justify_center()
        .gap(px(6.0))
        .children(Lang::ALL.into_iter().map(|lang| {
            let selected = lang == current;
            div()
                .id(SharedString::from(format!(
                    "onboarding-language-{}",
                    lang.code()
                )))
                .role(gpui_kit::Role::Button)
                .aria_label(lang.native_name())
                .h(px(28.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .rounded_full()
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
                .text_size(px(12.0))
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if selected {
                    theme::AMBER
                } else {
                    theme::SECONDARY
                })
                .cursor_pointer()
                .on_click(cx.listener(move |_, _, _, cx| {
                    crate::set_app_language(lang, cx);
                    cx.notify();
                }))
                .child(lang.native_name())
        }))
}

fn record_shortcut() -> String {
    let prefs = settings::load();
    if prefs.record_hotkey.is_empty() {
        prefs.show_hotkey
    } else {
        prefs.record_hotkey
    }
}

fn radio(selected: bool) -> Div {
    div()
        .size(px(16.0))
        .flex_shrink_0()
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
            this.child(div().size(px(8.0)).rounded_full().bg(theme::AMBER))
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_is_offered_in_one_list() {
        let choices = Choice::all();
        assert_eq!(choices.len(), models::CATALOG.len() + Provider::ALL.len());
        let local: Vec<u64> = choices
            .iter()
            .filter_map(|choice| match choice {
                Choice::Local(spec) => Some(spec.bytes),
                Choice::Cloud(_) => None,
            })
            .collect();
        let clouds = Provider::ALL.len();
        assert!(choices[..clouds]
            .iter()
            .all(|choice| matches!(choice, Choice::Cloud(_))));
        assert!(matches!(choices[clouds], Choice::Local(spec) if spec.recommended));
        assert!(local[1..].windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(choices
            .iter()
            .any(|choice| choice.id() == models::recommended_id()));
    }
}
