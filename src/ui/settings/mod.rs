mod pages;
mod widgets;

use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
#[cfg(feature = "licensing")]
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::select::{SearchableVec, SelectEvent, SelectState};
use gpui_kit::component::IndexPath;
use gpui_kit::component::{Icon, Root, Sizable};
use gpui_kit::{
    div, prelude::*, px, size, Animation, AnimationExt, AnyElement, AnyWindowHandle, App,
    AppContext, Bounds, Context, Div, Entity, FocusHandle, Focusable, FontWeight, IntoElement,
    Render, SharedString, Styled, Window, WindowBackgroundAppearance, WindowBounds,
    WindowDecorations, WindowKind, WindowOptions,
};

use pages::audio::{
    app_language_choices, language_choices, microphone_choices, output_language_choices, Choice,
};

use crate::app::Whisp;
use crate::audio::{self, Mic};
use crate::i18n::{t, tf};
use crate::settings::Preferences;
use crate::transcription::cloud::Provider;
use crate::ui::motion;
use crate::ui::theme;

const WIDTH: f32 = 880.0;
const HEIGHT: f32 = 660.0;
const SIDEBAR_WIDTH: f32 = 232.0;
const TRANSITION: Duration = Duration::from_millis(200);
const RELEASES_URL: &str = "https://github.com/lassejlv/whisple/releases";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsTarget {
    General,
    Audio,
    Models,
    CloudKey(Provider),
    #[cfg(feature = "licensing")]
    License,
}

impl SettingsTarget {
    fn page(self) -> Page {
        match self {
            Self::General => Page::General,
            Self::Audio => Page::Audio,
            Self::Models | Self::CloudKey(_) => Page::Models,
            #[cfg(feature = "licensing")]
            Self::License => Page::License,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Audio,
    Models,
    #[cfg(feature = "licensing")]
    License,
    About,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::General => t("General"),
            Self::Audio => t("Audio"),
            Self::Models => t("Models"),
            #[cfg(feature = "licensing")]
            Self::License => t("License"),
            Self::About => t("About"),
        }
    }

    fn icon(self) -> Lucide {
        match self {
            Self::General => Lucide::SlidersHorizontal,
            Self::Audio => Lucide::Mic,
            Self::Models => Lucide::Cpu,
            #[cfg(feature = "licensing")]
            Self::License => Lucide::KeyRound,
            Self::About => Lucide::Info,
        }
    }
}

pub(crate) struct SettingsWindow {
    focus_handle: FocusHandle,
    hud: Entity<Whisp>,
    page: Page,
    microphone_select: Entity<SelectState<SearchableVec<Choice>>>,
    language_select: Entity<SelectState<SearchableVec<Choice>>>,
    output_select: Entity<SelectState<SearchableVec<Choice>>>,
    app_language_select: Entity<SelectState<SearchableVec<Choice>>>,
    cloud_config: Option<Provider>,
    key_input: Option<Entity<InputState>>,
    key_visible: bool,
    #[cfg(feature = "licensing")]
    license_input: Entity<InputState>,
    #[cfg(feature = "licensing")]
    license_busy: bool,
    #[cfg(feature = "licensing")]
    just_activated: bool,
    microphone_names: Vec<String>,
    monitor: Option<Mic>,
    monitor_level: f32,
    flipped: Option<&'static str>,
    error: Option<String>,
}

#[derive(Clone)]
pub(crate) struct SettingsHandle {
    window: AnyWindowHandle,
    view: Entity<SettingsWindow>,
}

impl SettingsHandle {
    pub(crate) fn close(&self, cx: &mut App) {
        self.window
            .update(cx, |_, window, _| window.remove_window())
            .ok();
    }
}

pub(crate) fn open(
    cx: &mut App,
    hud: Entity<Whisp>,
    existing: Option<SettingsHandle>,
    target: SettingsTarget,
) -> SettingsHandle {
    if let Some(existing) = existing {
        if existing
            .window
            .update(cx, |_, window, cx| {
                if target != SettingsTarget::General {
                    existing
                        .view
                        .update(cx, |view, cx| view.show_target(target, window, cx));
                }
                window.activate_window();
                cx.activate(true);
            })
            .is_ok()
        {
            return existing;
        }
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
    let mut settings_view = None;
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // The system draws the window controls: macOS traffic lights
                // over the sidebar, and the regular title bar on Windows and
                // Linux. A transparent title bar on Windows would hide the
                // native caption buttons.
                titlebar: Some(gpui_kit::TitlebarOptions {
                    title: Some(t("Whisple Settings").into()),
                    appears_transparent: cfg!(target_os = "macos"),
                    traffic_light_position: Some(gpui_kit::point(px(18.0), px(18.0))),
                }),
                focus: true,
                show: true,
                kind: WindowKind::Normal,
                is_movable: true,
                app_owns_titlebar_drag: false,
                inactive_frame_interval: None,
                is_resizable: false,
                is_minimizable: true,
                display_id: None,
                window_background: WindowBackgroundAppearance::Opaque,
                icon: None,
                app_id: Some("whisple-settings".into()),
                window_min_size: Some(window_size),
                window_decorations: Some(WindowDecorations::Server),
                tabbing_identifier: None,
            },
            |window, cx| {
                let view = cx.new(|cx| SettingsWindow::new(window, hud, Page::General, cx));
                view.update(cx, |view, cx| view.show_target(target, window, cx));
                settings_view = Some(view.clone());
                let closing = view.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    closing.update(cx, |view, cx| view.release(cx)).ok();
                    true
                });
                window.focus(&view.focus_handle(cx), cx);
                cx.new(|cx| {
                    Root::new(view, window, cx)
                        .bordered(false)
                        .bg(gpui_kit::transparent_black())
                })
            },
        )
        .expect("open Whisple settings");
    cx.activate(true);
    SettingsHandle {
        window: *handle,
        view: settings_view.expect("settings view was created"),
    }
}

fn output_language_position(id: &str) -> usize {
    Preferences::languages()
        .iter()
        .filter(|language| language.id != "auto")
        .position(|language| language.id == id)
        .map_or(0, |index| index + 1)
}

impl SettingsWindow {
    fn new(
        window: &mut Window,
        hud: Entity<Whisp>,
        initial_page: Page,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&hud, |_, _, cx| cx.notify()).detach();
        let (input_device, language, output_language) = {
            let hud = hud.read(cx);
            (
                hud.input_device.clone(),
                hud.language.clone(),
                hud.output_language.clone(),
            )
        };
        // Load devices when Audio is opened, so License and other pages do not
        // touch the microphone subsystem during startup.
        let microphone_names = Vec::new();
        let microphone_index = if input_device.is_empty() {
            Some(0)
        } else {
            microphone_names
                .iter()
                .position(|name| name == &input_device)
                .map(|index| index + 1)
        };
        let microphone_select = cx.new(|cx| {
            SelectState::new(
                microphone_choices(&microphone_names),
                microphone_index.map(|index| IndexPath::default().row(index)),
                window,
                cx,
            )
            .searchable(true)
        });
        let language_index = Preferences::languages()
            .iter()
            .position(|choice| choice.id == language);
        let language_select = cx.new(|cx| {
            SelectState::new(
                language_choices(),
                language_index.map(|index| IndexPath::default().row(index)),
                window,
                cx,
            )
            .searchable(true)
        });
        let output_index = output_language_position(&output_language);
        let output_select = cx.new(|cx| {
            SelectState::new(
                output_language_choices(),
                Some(IndexPath::default().row(output_index)),
                window,
                cx,
            )
            .searchable(true)
        });
        let app_language_index = crate::i18n::Lang::ALL
            .iter()
            .position(|lang| *lang == crate::i18n::current());
        let app_language_select = cx.new(|cx| {
            SelectState::new(
                app_language_choices(),
                app_language_index.map(|index| IndexPath::default().row(index)),
                window,
                cx,
            )
        });
        cx.subscribe_in(
            &app_language_select,
            window,
            |view, _, event, window, cx| {
                let SelectEvent::Confirm(Some(code)) = event else {
                    return;
                };
                view.choose_app_language(code, window, cx);
            },
        )
        .detach();
        cx.subscribe_in(&output_select, window, |view, _, event, _, cx| {
            let SelectEvent::Confirm(Some(language)) = event else {
                return;
            };
            view.choose_output_language(language, cx);
        })
        .detach();
        cx.subscribe_in(&microphone_select, window, |view, _, event, _, cx| {
            let SelectEvent::Confirm(Some(name)) = event else {
                return;
            };
            view.choose_microphone(name, cx);
        })
        .detach();
        cx.subscribe_in(&language_select, window, |view, _, event, _, cx| {
            let SelectEvent::Confirm(Some(language)) = event else {
                return;
            };
            view.choose_language(language, cx);
        })
        .detach();
        #[cfg(feature = "licensing")]
        let license_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("Paste your Polar license key"))
                .masked(true)
        });
        #[cfg(feature = "licensing")]
        cx.subscribe(&license_input, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        let handle = window.window_handle();
        let mut interval = Duration::from_millis(100);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(interval).await;
            let alive = handle
                .update(cx, |_, _, cx| {
                    this.update(cx, |view: &mut Self, cx| {
                        interval = view.poll(cx);
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
            hud,
            page: initial_page,
            microphone_select,
            language_select,
            output_select,
            app_language_select,
            cloud_config: None,
            key_input: None,
            key_visible: false,
            #[cfg(feature = "licensing")]
            license_input,
            #[cfg(feature = "licensing")]
            license_busy: false,
            #[cfg(feature = "licensing")]
            just_activated: false,
            microphone_names,
            monitor: None,
            monitor_level: 0.0,
            flipped: None,
            error: None,
        }
    }

    /// Redraws only what moves on its own: the Audio page's level meter at
    /// 30 fps, and a model download's progress. Returns the next wait.
    fn poll(&mut self, cx: &mut Context<Self>) -> Duration {
        match self.page {
            Page::Audio => {
                let raw = self.monitor.as_ref().map_or(0.0, |mic| mic.level());
                // Rise at once, fall gently, so the meter reads as a level
                // rather than flicker.
                self.monitor_level = raw.max(self.monitor_level * 0.82);
                cx.notify();
                Duration::from_millis(33)
            }
            Page::Models if self.hud.read(cx).download.is_some() => {
                cx.notify();
                Duration::from_millis(100)
            }
            _ => Duration::from_millis(100),
        }
    }

    fn select_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == page {
            return;
        }
        self.hud.update(cx, |hud, cx| hud.cancel_hotkey_capture(cx));
        self.monitor = None;
        self.cloud_config = None;
        self.key_input = None;
        self.error = None;
        #[cfg(feature = "licensing")]
        {
            self.just_activated = false;
        }
        self.flipped = None;
        self.monitor_level = 0.0;
        self.page = page;
        if page == Page::Audio {
            let (language, output_language) = {
                let hud = self.hud.read(cx);
                (hud.language.clone(), hud.output_language.clone())
            };
            self.language_select.update(cx, |state, cx| {
                state.set_selected_value(&language, window, cx);
            });
            self.output_select.update(cx, |state, cx| {
                state.set_selected_value(&output_language, window, cx);
            });
            self.load_microphones(window, cx);
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Listing CoreAudio devices and opening the level monitor take a moment,
    /// so the Audio page appears first and fills in when they are ready.
    fn load_microphones(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            let names = cx
                .background_executor()
                .spawn(async { audio::input_names().unwrap_or_default() })
                .await;
            this.update_in(cx, |view, window, cx| {
                if view.page != Page::Audio {
                    return;
                }
                let input_device = view.hud.read(cx).input_device.clone();
                view.microphone_select.update(cx, |state, cx| {
                    state.set_items(microphone_choices(&names), window, cx);
                    state.set_selected_value(&input_device, window, cx);
                });
                view.microphone_names = names;
                match Mic::monitor(&input_device) {
                    Ok(monitor) => view.monitor = Some(monitor),
                    Err(err) => view.error = Some(tf("Input level unavailable: {}", &[&err])),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn show_target(&mut self, target: SettingsTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.select_page(target.page(), window, cx);
        if let SettingsTarget::CloudKey(provider) = target {
            self.open_cloud(provider, window, cx);
        }
    }

    fn choose_app_language(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lang) = crate::i18n::Lang::from_code(code) else {
            return;
        };
        crate::set_app_language(lang, cx);
        window.set_window_title(t("Whisple Settings"));
        let (language, output_language, input_device) = {
            let hud = self.hud.read(cx);
            (
                hud.language.clone(),
                hud.output_language.clone(),
                hud.input_device.clone(),
            )
        };
        self.language_select.update(cx, |state, cx| {
            state.set_items(language_choices(), window, cx);
            state.set_selected_value(&language, window, cx);
        });
        self.output_select.update(cx, |state, cx| {
            state.set_items(output_language_choices(), window, cx);
            state.set_selected_value(&output_language, window, cx);
        });
        let names = self.microphone_names.clone();
        self.microphone_select.update(cx, |state, cx| {
            state.set_items(microphone_choices(&names), window, cx);
            state.set_selected_value(&input_device, window, cx);
        });
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.release(cx);
        window.remove_window();
    }

    /// Lets go of what Settings holds before its window closes, however it
    /// is closed: a pending shortcut capture and the microphone meter.
    fn release(&mut self, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.cancel_hotkey_capture(cx));
        self.monitor = None;
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(gpui_kit::rgba(0x101012ff))
            .border_r_1()
            .border_color(theme::HAIRLINE)
            // Room for the native traffic lights, which macOS draws over
            // the sidebar. Elsewhere the title bar sits above the window.
            .child(div().h(px(if cfg!(target_os = "macos") {
                52.0
            } else {
                12.0
            })))
            .child(
                div()
                    .px(px(10.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(self.nav_row(Page::General, cx))
                    .child(self.nav_row(Page::Audio, cx))
                    .child(self.nav_row(Page::Models, cx))
                    .child(
                        div()
                            .h(px(13.0))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .child(div().h(px(1.0)).w_full().bg(theme::HAIRLINE)),
                    )
                    .children(self.license_nav(cx))
                    .child(self.nav_row(Page::About, cx)),
            )
    }

    fn nav_row(&self, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.page == page;
        div()
            .id(SharedString::from(format!("settings-nav-{}", page.title())))
            .role(gpui_kit::Role::Button)
            .aria_label(tf("{} settings", &[&page.title()]))
            .h(px(34.0))
            .w_full()
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded(px(8.0))
            .when(selected, |row| {
                row.bg(gpui_kit::rgba(0xffb3401f))
                    .shadow(vec![theme::inner_ring(gpui_kit::rgba(0xffb34024))])
            })
            .when(!selected, |row| row.hover(|row| row.bg(theme::HOVER)))
            .cursor_pointer()
            .on_click(cx.listener(move |view, _, window, cx| view.select_page(page, window, cx)))
            .child(
                div()
                    .size(px(24.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(if selected {
                        theme::AMBER
                    } else {
                        theme::RAISED
                    })
                    .child(
                        Icon::new(page.icon())
                            .text_color(if selected {
                                theme::HUD
                            } else {
                                theme::SECONDARY
                            })
                            .with_size(px(13.0)),
                    ),
            )
            .child(
                div()
                    .text_size(px(13.5))
                    .font_weight(if selected {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(theme::LABEL)
                    .child(page.title()),
            )
    }

    #[cfg(feature = "licensing")]
    fn license_nav(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        Some(self.nav_row(Page::License, cx).into_any_element())
    }

    #[cfg(not(feature = "licensing"))]
    fn license_nav(&self, _cx: &mut Context<Self>) -> Option<AnyElement> {
        None
    }

    fn pane(&self, cx: &mut Context<Self>) -> Div {
        let body = match self.page {
            Page::General => self.general(cx).into_any_element(),
            Page::Audio => self.audio(cx).into_any_element(),
            Page::Models => self.models(cx).into_any_element(),
            #[cfg(feature = "licensing")]
            Page::License => self.license(cx).into_any_element(),
            Page::About => self.about(cx).into_any_element(),
        };
        let error = self
            .error
            .clone()
            .or_else(|| self.hud.read(cx).error.clone());
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(52.0))
                    .px(px(28.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(theme::HAIRLINE)
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::LABEL)
                    .child(self.page.title()),
            )
            .child(
                div()
                    .id("settings-pane-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px(px(28.0))
                    .pt(px(24.0))
                    .pb(px(28.0))
                    .child(div().child(body).with_animation(
                        format!("settings-page-{}", self.page.title()),
                        Animation::new(TRANSITION).with_easing(motion::ease_out),
                        |page, t| page.opacity(t).mt(px(8.0 * (1.0 - t))),
                    )),
            )
            .when_some(error, |pane, error| {
                pane.child(
                    div()
                        .px(px(28.0))
                        .py(px(7.0))
                        .bg(theme::INSET)
                        .text_size(px(12.0))
                        .text_color(theme::RED)
                        .child(error),
                )
            })
    }
}

impl Focusable for SettingsWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings-window")
            .key_context("WhispleSettings")
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(
                cx.listener(|view, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        if view.cloud_config.is_some() {
                            view.cloud_config = None;
                            view.key_input = None;
                            window.focus(&view.focus_handle, cx);
                            cx.notify();
                        } else {
                            view.close(window, cx);
                        }
                        cx.stop_propagation();
                    }
                }),
            )
            .relative()
            .flex()
            .overflow_hidden()
            .bg(theme::HUD)
            .font_family(theme::UI_FONT)
            .child(self.sidebar(cx))
            .child(self.pane(cx))
            .when(self.cloud_config.is_some(), |root| {
                root.child(self.cloud_modal(cx))
            })
    }
}
