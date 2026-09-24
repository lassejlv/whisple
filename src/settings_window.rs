//! The full settings window from Paper's Settings page. The minibar remains
//! available while this window owns the longer lists and account controls.

use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputContentType, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::IndexPath;
use gpui_kit::component::{Icon, Root, Sizable};
use gpui_kit::{
    div, prelude::*, px, size, AnyElement, AnyWindowHandle, App, AppContext, Bounds, Context, Div,
    Entity, FocusHandle, Focusable, FontWeight, IntoElement, Render, SharedString, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowKind, WindowOptions,
};

use crate::app::Whisp;
use crate::audio::{self, Mic};
use crate::cloud::{self, Provider};
use crate::hotkey;
use crate::license::{self, Access};
use crate::models;
use crate::settings::{self, Preferences};
use crate::theme;
use crate::tray;

const WIDTH: f32 = 880.0;
const SIDEBAR_WIDTH: f32 = 232.0;
const RELEASES_URL: &str = "https://github.com/lassejlv/whisple/releases";

/// Where the bar sends people: a page, or a provider's API key dialog.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsTarget {
    General,
    Audio,
    Models,
    CloudKey(Provider),
    License,
}

impl SettingsTarget {
    fn page(self) -> Page {
        match self {
            Self::General => Page::General,
            Self::Audio => Page::Audio,
            Self::Models | Self::CloudKey(_) => Page::Models,
            Self::License => Page::License,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Audio,
    Models,
    License,
    About,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Audio => "Audio",
            Self::Models => "Models",
            Self::License => "License",
            Self::About => "About",
        }
    }

    fn height(self) -> f32 {
        match self {
            Self::Models => 690.0,
            Self::About => 590.0,
            _ => 548.0,
        }
    }

    fn icon(self) -> Lucide {
        match self {
            Self::General => Lucide::SlidersHorizontal,
            Self::Audio => Lucide::Mic,
            Self::Models => Lucide::Cpu,
            Self::License => Lucide::KeyRound,
            Self::About => Lucide::Info,
        }
    }
}

#[derive(Clone)]
struct Choice {
    id: String,
    label: SharedString,
}

impl SelectItem for Choice {
    type Value = String;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.id
    }
}

fn microphone_choices(names: &[String]) -> SearchableVec<Choice> {
    let mut choices = vec![Choice {
        id: String::new(),
        label: "System default".into(),
    }];
    choices.extend(names.iter().map(|name| Choice {
        id: name.clone(),
        label: name.clone().into(),
    }));
    SearchableVec::new(choices)
}

fn language_choices() -> SearchableVec<Choice> {
    SearchableVec::new(
        Preferences::languages()
            .iter()
            .map(|language| Choice {
                id: language.id.into(),
                label: language.name.into(),
            })
            .collect::<Vec<_>>(),
    )
}

pub(crate) struct SettingsWindow {
    focus_handle: FocusHandle,
    hud: Entity<Whisp>,
    page: Page,
    microphone_select: Entity<SelectState<SearchableVec<Choice>>>,
    language_select: Entity<SelectState<SearchableVec<Choice>>>,
    cloud_config: Option<Provider>,
    key_input: Option<Entity<InputState>>,
    key_visible: bool,
    license_input: Entity<InputState>,
    license_busy: bool,
    just_activated: bool,
    microphone_names: Vec<String>,
    monitor: Option<Mic>,
    monitor_level: f32,
    error: Option<String>,
}

#[derive(Clone)]
pub(crate) struct SettingsHandle {
    window: AnyWindowHandle,
    view: Entity<SettingsWindow>,
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
                // Opening from the gear keeps whatever page was last shown.
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
    let window_size = size(px(WIDTH), px(target.page().height()));
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
                titlebar: None,
                focus: true,
                show: true,
                kind: WindowKind::Normal,
                is_movable: true,
                app_owns_titlebar_drag: false,
                inactive_frame_interval: None,
                is_resizable: false,
                is_minimizable: true,
                display_id: None,
                window_background: WindowBackgroundAppearance::Transparent,
                icon: None,
                app_id: Some("whisple-settings".into()),
                window_min_size: Some(size(px(WIDTH), px(Page::General.height()))),
                window_decorations: Some(WindowDecorations::Client),
                tabbing_identifier: None,
            },
            |window, cx| {
                let view = cx.new(|cx| SettingsWindow::new(window, hud, Page::General, cx));
                view.update(cx, |view, cx| view.show_target(target, window, cx));
                settings_view = Some(view.clone());
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

impl SettingsWindow {
    fn new(
        window: &mut Window,
        hud: Entity<Whisp>,
        initial_page: Page,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&hud, |_, _, cx| cx.notify()).detach();
        let (input_device, language) = {
            let hud = hud.read(cx);
            (hud.input_device.clone(), hud.language.clone())
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
        let license_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste your Polar license key")
                .masked(true)
        });
        cx.subscribe(&license_input, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            let alive = handle
                .update(cx, |_, _, cx| {
                    this.update(cx, |view: &mut Self, cx| {
                        if view.page == Page::Audio {
                            view.monitor_level =
                                view.monitor.as_ref().map(|mic| mic.level()).unwrap_or(0.0);
                            cx.notify();
                        } else if view.page == Page::Models {
                            cx.notify();
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
            hud,
            page: initial_page,
            microphone_select,
            language_select,
            cloud_config: None,
            key_input: None,
            key_visible: false,
            license_input,
            license_busy: false,
            just_activated: false,
            microphone_names,
            monitor: None,
            monitor_level: 0.0,
            error: None,
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
        self.just_activated = false;
        self.page = page;
        window.resize(size(px(WIDTH), px(page.height())));
        if page == Page::Audio {
            self.microphone_names = audio::input_names().unwrap_or_default();
            let input_device = self.hud.read(cx).input_device.clone();
            let language = self.hud.read(cx).language.clone();
            self.microphone_select.update(cx, |state, cx| {
                state.set_items(microphone_choices(&self.microphone_names), window, cx);
                state.set_selected_value(&input_device, window, cx);
            });
            self.language_select.update(cx, |state, cx| {
                state.set_selected_value(&language, window, cx);
            });
            match Mic::monitor(&input_device) {
                Ok(monitor) => self.monitor = Some(monitor),
                Err(err) => self.error = Some(format!("Input level unavailable: {err}")),
            }
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn show_target(&mut self, target: SettingsTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.select_page(target.page(), window, cx);
        if let SettingsTarget::CloudKey(provider) = target {
            self.open_cloud(provider, window, cx);
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.cancel_hotkey_capture(cx));
        self.monitor = None;
        window.remove_window();
    }

    fn start_talking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hud = self.hud.clone();
        let hud_window = hud.read(cx).hud_window;
        self.close(window, cx);
        cx.defer(move |cx| {
            let _ = hud_window.update(cx, |_, window, cx| {
                hud.update(cx, |view, cx| view.set_visible(true, window, cx));
            });
        });
    }

    fn choose_microphone(&mut self, name: &str, cx: &mut Context<Self>) {
        if !name.is_empty()
            && !self
                .microphone_names
                .iter()
                .any(|candidate| candidate == name)
        {
            return;
        }
        self.monitor = None;
        self.hud.update(cx, |hud, cx| {
            hud.input_device = name.to_string();
            hud.persist_settings();
            cx.notify();
        });
        self.monitor = Mic::monitor(name).ok();
        cx.notify();
    }

    fn choose_language(&mut self, language: &str, cx: &mut Context<Self>) {
        self.hud
            .update(cx, |hud, cx| hud.choose_language(language, cx));
        cx.notify();
    }

    fn toggle_menu_bar(&mut self, cx: &mut Context<Self>) {
        let mut prefs = settings::load();
        let visible = !prefs.show_in_menu_bar;
        match tray::set_icon_visible(visible, cx) {
            Ok(()) => {
                prefs.show_in_menu_bar = visible;
                settings::save(&prefs);
                self.error = None;
            }
            Err(err) => self.error = Some(err),
        }
        cx.notify();
    }

    fn toggle_startup(&mut self, cx: &mut Context<Self>) {
        self.hud
            .update(cx, |hud, cx| hud.toggle_open_on_startup(cx));
        cx.notify();
    }

    fn toggle_copy(&mut self, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.toggle_copy_notes(cx));
        cx.notify();
    }

    fn toggle_clean(&mut self, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.toggle_clean_fillers(cx));
        cx.notify();
    }

    fn begin_shortcut(&mut self, cx: &mut Context<Self>) {
        self.hud.update(cx, |hud, cx| hud.begin_hotkey_capture(cx));
        cx.notify();
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
            .child(
                div()
                    .h(px(52.0))
                    .px(px(18.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(traffic_light(
                        0xff5f57ff,
                        "Close settings",
                        cx,
                        |view, window, cx| view.close(window, cx),
                    ))
                    .child(traffic_light(
                        0xfebc2eff,
                        "Minimize settings",
                        cx,
                        |_, window, _| window.minimize_window(),
                    ))
                    .child(traffic_light(
                        0x28c840ff,
                        "Zoom settings",
                        cx,
                        |_, window, _| window.zoom_window(),
                    )),
            )
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
                    .child(self.nav_row(Page::License, cx))
                    .child(self.nav_row(Page::About, cx)),
            )
    }

    fn nav_row(&self, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.page == page;
        div()
            .id(SharedString::from(format!("settings-nav-{}", page.title())))
            .role(gpui_kit::Role::Button)
            .aria_label(format!("{} settings", page.title()))
            .h(px(34.0))
            .w_full()
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded(px(8.0))
            .bg(if selected {
                gpui_kit::rgba(0xffb3401f)
            } else {
                gpui_kit::rgba(0x101012ff)
            })
            .when(selected, |row| {
                row.shadow(vec![theme::inner_ring(gpui_kit::rgba(0xffb34024))])
            })
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

    fn pane(&self, cx: &mut Context<Self>) -> Div {
        let body = match self.page {
            Page::General => self.general(cx).into_any_element(),
            Page::Audio => self.audio(cx).into_any_element(),
            Page::Models => self.models(cx).into_any_element(),
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
                    .child(body),
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

    fn general(&self, cx: &mut Context<Self>) -> Div {
        let (startup, shortcut, copy, clean, capturing) = {
            let hud = self.hud.read(cx);
            (
                hud.open_on_startup,
                hud.show_hotkey.clone(),
                hud.copy_notes,
                hud.clean_fillers,
                hud.recording_hotkey,
            )
        };
        let menu_bar = settings::load().show_in_menu_bar;
        div()
            .flex()
            .flex_col()
            .gap(px(26.0))
            .child(section(
                "Startup",
                vec![
                    setting_row(
                        "open-at-login",
                        "Open at login",
                        None,
                        toggle(startup).into_any_element(),
                        false,
                        cx,
                        |view, cx| view.toggle_startup(cx),
                    ),
                    setting_row(
                        "show-in-menu-bar",
                        "Show in menu bar",
                        Some("Keep the Whisple icon next to the clock."),
                        toggle(menu_bar).into_any_element(),
                        true,
                        cx,
                        |view, cx| view.toggle_menu_bar(cx),
                    ),
                ],
            ))
            .child(section(
                "Shortcut",
                vec![setting_row(
                    "show-whisple-hotkey",
                    "Show Whisple",
                    Some("Opens the bar from any app. Press again to stop."),
                    shortcut_control(&shortcut, capturing),
                    false,
                    cx,
                    |view, cx| view.begin_shortcut(cx),
                )],
            ))
            .child(section(
                "Output",
                vec![
                    setting_row(
                        "copy-to-clipboard",
                        "Copy to clipboard when done",
                        None,
                        toggle(copy).into_any_element(),
                        false,
                        cx,
                        |view, cx| view.toggle_copy(cx),
                    ),
                    setting_row(
                        "clean-up-notes",
                        "Clean up notes",
                        Some("Removes “um”, “uh” and repeated words."),
                        toggle(clean).into_any_element(),
                        true,
                        cx,
                        |view, cx| view.toggle_clean(cx),
                    ),
                ],
            ))
    }

    fn audio(&self, _cx: &mut Context<Self>) -> Div {
        let bars =
            (0..18).map(|index| {
                let threshold = (index + 1) as f32 / 18.0;
                div().w(px(4.0)).h(px(12.0)).rounded(px(2.0)).bg(
                    if self.monitor_level >= threshold {
                        theme::AMBER
                    } else {
                        theme::TRACK
                    },
                )
            });
        div()
            .flex()
            .flex_col()
            .gap(px(26.0))
            .child(section(
                "Input",
                vec![
                    selector_row(
                        "Microphone",
                        None,
                        selector_control(&self.microphone_select, "Microphone").into_any_element(),
                        false,
                    ),
                    div()
                        .h(px(58.0))
                        .px(px(16.0))
                        .border_t_1()
                        .border_color(theme::HAIRLINE)
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(label_stack(
                            "Input level",
                            Some("Say something to test your microphone."),
                        ))
                        .child(div().flex().gap(px(3.0)).items_center().children(bars))
                        .into_any_element(),
                ],
            ))
            .child(section(
                "Language",
                vec![selector_row(
                    "Spoken language",
                    Some("English-only models always transcribe English."),
                    selector_control(&self.language_select, "Spoken language").into_any_element(),
                    false,
                )],
            ))
    }

    fn models(&self, cx: &mut Context<Self>) -> Div {
        let (selected, cloud_keys, used) = {
            let hud = self.hud.read(cx);
            (hud.selected.clone(), hud.cloud_keys, hud.storage_used())
        };
        div()
            .flex()
            .flex_col()
            .gap(px(22.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(div().pl(px(4.0)).flex().items_center().justify_between()
                        .child(section_label("On this Mac"))
                        .child(div().text_size(px(12.0)).text_color(theme::TERTIARY).child(format!("{} used", models::format_size(used)))))
                    .child(group().children(models::CATALOG.iter().enumerate().map(|(index, spec)| {
                        self.model_row(spec, index > 0, selected == spec.id, cx)
                    }))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(div().pl(px(4.0)).flex().items_center().justify_between()
                        .child(section_label("Cloud"))
                        .child(div().text_size(px(12.0)).text_color(theme::TERTIARY).child("Uses your own API key")))
                    .child(group().children(Provider::ALL.map(|provider| {
                        self.cloud_row(provider, cloud_keys[provider.index()], selected == provider.id(), cx)
                    })))
                    .child(div().pl(px(4.0)).text_size(px(12.0)).text_color(theme::TERTIARY)
                        .child("Cloud recordings are sent to the selected provider. Keys are stored in your Mac’s Keychain.")),
            )
    }

    fn model_row(
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

    fn cloud_row(
        &self,
        provider: Provider,
        connected: bool,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let icon_color = if provider == Provider::OpenAi {
            theme::LABEL
        } else {
            gpui_kit::rgba(0xf54f35ff)
        };
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
            .cursor_pointer()
            .on_click(cx.listener(move |view, _, window, cx| {
                if connected {
                    view.hud
                        .update(cx, |hud, cx| hud.choose_cloud(provider, cx));
                } else {
                    view.open_cloud(provider, window, cx);
                }
            }))
            .child(
                div()
                    .size(px(30.0))
                    .flex_shrink_0()
                    .rounded(px(8.0))
                    .bg(icon_color)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::empty()
                            .path(provider.icon())
                            .text_color(if provider == Provider::OpenAi {
                                theme::HUD
                            } else {
                                theme::LABEL
                            })
                            .with_size(px(28.0)),
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
                                        .text_color(gpui_kit::rgba(0x49c38aff))
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
}

impl SettingsWindow {
    fn open_cloud(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
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

    fn save_cloud(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        self.hud.update(cx, |hud, cx| {
            hud.cloud_keys[provider.index()] = true;
            hud.choose_cloud(provider, cx);
        });
        self.close_modal(window, cx);
    }

    fn remove_cloud(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(provider) = self.cloud_config else {
            return;
        };
        match cloud::delete_key(provider) {
            Ok(()) => {
                self.hud.update(cx, |hud, cx| {
                    hud.remove_cloud_key_from_settings(provider, cx)
                });
                self.close_modal(window, cx);
            }
            Err(err) => {
                self.error = Some(err);
                cx.notify();
            }
        }
    }

    fn close_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cloud_config = None;
        self.key_input = None;
        self.key_visible = false;
        self.error = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn cloud_modal(&self, cx: &mut Context<Self>) -> Div {
        let provider = self.cloud_config.unwrap();
        let connected = self.hud.read(cx).cloud_keys[provider.index()];
        let input = self.key_input.clone();
        let url = match provider {
            Provider::OpenAi => "https://platform.openai.com/api-keys",
            Provider::Groq => "https://console.groq.com/keys",
        };
        modal_backdrop().child(
            div().w(px(480.0)).flex().flex_col().rounded(px(14.0)).overflow_hidden()
                .bg(theme::INSET).border_1().border_color(theme::EDGE)
                .child(div().p(px(22.0)).flex().flex_col().gap(px(18.0))
                    .child(div().flex().items_center().gap(px(14.0))
                        .child(div().size(px(44.0)).rounded(px(10.0))
                            .bg(if provider == Provider::OpenAi { theme::LABEL } else { gpui_kit::rgba(0xf54f35ff) })
                            .flex().items_center().justify_center()
                            .child(Icon::empty().path(provider.icon()).text_color(if provider == Provider::OpenAi { theme::HUD } else { theme::LABEL }).with_size(px(39.0))))
                        .child(div().flex_1().flex().flex_col().gap(px(3.0))
                            .child(div().text_size(px(16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme::LABEL)
                                .child(if connected { provider.name().to_string() } else { format!("Connect {}", provider.name()) }))
                            .child(div().text_size(px(12.0)).text_color(theme::SECONDARY).child(provider.model())))
                        .when(connected, |header| header.child(div().px(px(8.0)).py(px(5.0)).rounded_full().bg(gpui_kit::rgba(0x49c38a22)).text_size(px(11.0)).text_color(gpui_kit::rgba(0x49c38aff)).child("● Key saved"))))
                    .child(div().flex().flex_col().gap(px(8.0))
                        .child(div().flex().items_center().justify_between()
                            .child(div().text_size(px(12.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme::SECONDARY).child("API KEY"))
                            .child(div().id("settings-provider-key-url").role(gpui_kit::Role::Button).aria_label(format!("Open {} API keys", provider.name()))
                                .text_size(px(12.0)).text_color(theme::AMBER).cursor_pointer().on_click(move |_, _, cx| cx.open_url(url))
                                .child(if provider == Provider::OpenAi { "Manage keys at platform.openai.com ↗" } else { "Get a key at console.groq.com ↗" })))
                        .child(div().h(px(40.0)).px(px(12.0)).flex().items_center().gap(px(8.0)).rounded(px(8.0)).bg(theme::HUD).shadow(vec![theme::inner_ring(theme::HAIRLINE)])
                            .children(input.map(|state| div().flex_1().min_w_0().child(Input::new(&state).content_type(InputContentType::Password).appearance(false).px_0().py_0().h(px(20.0)).text_size(px(13.0)))))
                            .child(div().id("settings-show-key").role(gpui_kit::Role::Button).aria_label(if self.key_visible { "Hide API key" } else { "Show API key" })
                                .text_size(px(12.0)).text_color(theme::SECONDARY).cursor_pointer().on_click(cx.listener(|view, _, window, cx| {
                                    view.key_visible = !view.key_visible;
                                    if let Some(input) = &view.key_input { input.update(cx, |input, cx| input.set_masked(!view.key_visible, window, cx)); }
                                    cx.notify();
                                })).child(if self.key_visible { "Hide" } else { "Show" })))
                        .child(div().text_size(px(12.0)).line_height(px(17.0)).text_color(theme::SECONDARY).child(if connected {
                            format!("A key is saved on this device. Paste a new one to replace it. Audio goes to {} only when selected.", provider.name())
                        } else {
                            format!("Your key stays on this device, in your Keychain. Audio goes to {} only when this model is selected.", provider.name())
                        })))
                    .when_some(self.error.as_ref(), |body, error| body.child(div().text_size(px(12.0)).text_color(theme::RED).child(error.clone()))))
                .child(div().h(px(60.0)).px(px(22.0)).border_t_1().border_color(theme::HAIRLINE)
                    .flex().items_center().justify_between()
                    .child(if connected {
                        div().id("settings-remove-key").role(gpui_kit::Role::Button).aria_label(format!("Remove {} API key", provider.name()))
                            .text_size(px(12.0)).text_color(theme::RED).cursor_pointer().on_click(cx.listener(|view, _, window, cx| view.remove_cloud(window, cx))).child("Remove key").into_any_element()
                    } else {
                        div().text_size(px(12.0)).text_color(theme::TERTIARY).child(format!("Billed by {}", provider.name())).into_any_element()
                    })
                    .child(div().flex().items_center().gap(px(8.0))
                        .child(div().id("settings-cloud-cancel").role(gpui_kit::Role::Button).aria_label("Cancel").h(px(32.0)).px(px(14.0)).rounded(px(8.0)).bg(theme::RAISED).flex().items_center().text_size(px(12.0)).text_color(theme::LABEL).cursor_pointer().on_click(cx.listener(|view, _, window, cx| view.close_modal(window, cx))).child("Cancel"))
                        .child(div().id("settings-cloud-save").role(gpui_kit::Role::Button).aria_label(if connected { "Use model" } else { "Save and use" }).h(px(32.0)).px(px(14.0)).rounded(px(8.0)).bg(theme::AMBER).flex().items_center().text_size(px(12.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme::HUD).cursor_pointer().on_click(cx.listener(|view, _, window, cx| view.save_cloud(window, cx))).child(if connected { "Use model" } else { "Save & use" }))),
                ),
        )
    }

    fn activate_license(&mut self, cx: &mut Context<Self>) {
        if self.license_busy {
            return;
        }
        let key = self.license_input.read(cx).value().to_string();
        if key.trim().is_empty() {
            self.error = Some("Paste your license key first.".into());
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

    fn refresh_license(&mut self, cx: &mut Context<Self>) {
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

    fn deactivate_license(&mut self, cx: &mut Context<Self>) {
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

    fn license(&self, cx: &mut Context<Self>) -> Div {
        let status = self.hud.read(cx).license_access.clone();
        let trial_detail = status.trial_remaining().map(|remaining| {
            format!(
                "{} of your 3-day free trial. Then {} once to keep Whisple.",
                license::trial_left(remaining, false),
                license::PRICE
            )
        });
        let ended_detail = format!(
            "Your 3-day free trial has ended. Buy Whisple for {} once to keep dictating.",
            license::PRICE
        );
        let unlicensed_detail = format!(
            "Buy Whisple for {} once, then paste the key from your purchase email.",
            license::PRICE
        );
        let trial_license_issue = match &status {
            Access::Trial {
                license_issue: Some(reason),
                ..
            } => Some(format!("Saved license needs attention: {reason}")),
            _ => None,
        };
        let (title, detail) = match &status {
            Access::Unlicensed => ("No license on this device", unlicensed_detail.as_str()),
            Access::Checking => ("Checking license", "Contacting Polar to verify access."),
            Access::Trial { .. } if status.allowed() => {
                ("Free trial", trial_detail.as_deref().unwrap_or_default())
            }
            Access::Trial { .. } | Access::TrialExpired => {
                ("Free trial ended", ended_detail.as_str())
            }
            Access::Active(_) => ("License active", "Whisple is ready to use on this device."),
            Access::Offline(_) => (
                "License active offline",
                "A recent verification allows temporary offline use.",
            ),
            Access::Blocked { .. } => (
                "License needs attention",
                "Check the message below or enter another key.",
            ),
            Access::Unavailable { .. } => (
                "Could not verify access",
                "Connect to the internet and check your license again.",
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
                                    .child("Whisple is yours"),
                            )
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(theme::SECONDARY)
                                    .child("Your lifetime license is active. Thanks for supporting Whisple."),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme::SECONDARY)
                                    .child(format!(
                                        "Active on this Mac · {}",
                                        status.display_key().unwrap_or_default()
                                    )),
                            )
                            .child(
                                div()
                                    .id("license-start-talking")
                                    .role(gpui_kit::Role::Button)
                                    .aria_label("Start talking")
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
                                    .child("Start talking"),
                            ),
                    )
                },
            )
            .child(section(
                "Lifetime license",
                vec![link_row(
                    "license-buy",
                    "Buy Whisple",
                    format!("{} once · opens Polar checkout ↗", license::PRICE),
                    license::CHECKOUT_URL,
                    false,
                    cx,
                )],
            ))
            .child(section(
                "Activate on this device",
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
                                        "Paste the key from your purchase email or Polar account.",
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
                            .when(
                                !status.display_key().is_some_and(|key| !key.is_empty()),
                                |row| row.justify_end(),
                            )
                            .when(
                                status.display_key().is_some_and(|key| !key.is_empty()),
                                |row| {
                                    row.child(
                                        div()
                                            .id("license-deactivate")
                                            .role(gpui_kit::Role::Button)
                                            .aria_label("Deactivate license on this device")
                                            .text_size(px(12.0))
                                            .text_color(theme::SECONDARY)
                                            .cursor_pointer()
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.deactivate_license(cx)
                                            }))
                                            .child("Deactivate this device"),
                                    )
                                },
                            )
                            .child(
                                div()
                                    .id("license-activate")
                                    .role(gpui_kit::Role::Button)
                                    .aria_label("Activate license")
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
                                        "Working…"
                                    } else {
                                        "Activate key"
                                    }),
                            ),
                    )
                    .into_any_element()],
            ))
            .child(
                div()
                    .flex()
                    .gap(px(20.0))
                    .child(
                        div()
                            .id("license-refresh")
                            .role(gpui_kit::Role::Button)
                            .aria_label("Check license status")
                            .text_size(px(12.0))
                            .text_color(theme::AMBER)
                            .cursor_pointer()
                            .on_click(cx.listener(|view, _, _, cx| view.refresh_license(cx)))
                            .child("Check status"),
                    )
                    .child(
                        div()
                            .id("license-portal")
                            .role(gpui_kit::Role::Button)
                            .aria_label("Open Polar purchases")
                            .text_size(px(12.0))
                            .text_color(theme::SECONDARY)
                            .cursor_pointer()
                            .on_click(|_, _, cx| cx.open_url(license::CUSTOMER_PORTAL_URL))
                            .child("Find your key ↗"),
                    ),
            )
    }

    fn about(&self, cx: &mut Context<Self>) -> Div {
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
                    "Whisple restarts in a few seconds after installing."
                } else {
                    "Updates are checked against GitHub releases."
                }),
            ))
            .child(
                div()
                    .id("settings-update-action")
                    .role(gpui_kit::Role::Button)
                    .aria_label(if ready {
                        "Install and restart"
                    } else {
                        "Check for updates"
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
                                hud.check_updates_from_settings(cx)
                            }
                        })
                    }))
                    .child(if ready {
                        "Install & restart"
                    } else {
                        "Check now"
                    }),
            )
            .into_any_element()];
        if ready {
            update_rows.push(link_row(
                "settings-release-notes",
                "What’s new",
                "Release notes on GitHub ↗",
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
                            .child(format!(
                                "Version {} · Local-first voice dictation",
                                env!("CARGO_PKG_VERSION")
                            )),
                    ),
            )
            .child(section("Updates", update_rows))
            .child(section(
                "More",
                vec![
                    link_row(
                        "settings-website",
                        "Website",
                        "whisple.app ↗",
                        "https://whisple.app",
                        false,
                        cx,
                    ),
                    link_row(
                        "settings-source",
                        "Source code",
                        "GitHub ↗",
                        "https://github.com/lassejlv/whisple",
                        true,
                        cx,
                    ),
                    link_row(
                        "settings-acknowledgements",
                        "Acknowledgements",
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
                    .child("MIT licensed · Made with GPUI"),
            )
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
            .rounded(px(16.0))
            .overflow_hidden()
            .bg(theme::HUD)
            .border_1()
            .border_color(theme::HAIRLINE)
            .font_family(theme::UI_FONT)
            .child(self.sidebar(cx))
            .child(self.pane(cx))
            .when(self.cloud_config.is_some(), |root| {
                root.child(self.cloud_modal(cx))
            })
    }
}

fn traffic_light(
    color: u32,
    label: &'static str,
    cx: &mut Context<SettingsWindow>,
    action: impl Fn(&mut SettingsWindow, &mut Window, &mut Context<SettingsWindow>) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(label))
        .role(gpui_kit::Role::Button)
        .aria_label(label)
        .size(px(12.0))
        .rounded_full()
        .bg(gpui_kit::rgba(color))
        .cursor_pointer()
        .on_click(cx.listener(move |view, _, window, cx| action(view, window, cx)))
}

fn section(title: &'static str, rows: Vec<AnyElement>) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .pl(px(4.0))
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::SECONDARY)
                .child(title),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .rounded(px(12.0))
                .overflow_hidden()
                .bg(theme::INSET)
                .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
                .children(rows),
        )
}

fn selector_control(
    state: &Entity<SelectState<SearchableVec<Choice>>>,
    label: &'static str,
) -> Div {
    div()
        .w(px(220.0))
        .h(px(30.0))
        .flex_shrink_0()
        .rounded(px(8.0))
        .bg(theme::RAISED)
        .child(
            Select::new(state)
                .accessibility_label(label)
                .appearance(false)
                .small()
                .w_full()
                .h_full(),
        )
}

fn selector_row(
    title: &'static str,
    subtitle: Option<&'static str>,
    trailing: AnyElement,
    divider: bool,
) -> AnyElement {
    div()
        .min_h(px(if subtitle.is_some() { 58.0 } else { 50.0 }))
        .pl(px(16.0))
        .pr(px(14.0))
        .py(px(9.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .when(divider, |row| {
            row.border_t_1().border_color(theme::HAIRLINE)
        })
        .child(label_stack(title, subtitle))
        .child(trailing)
        .into_any_element()
}

fn setting_row(
    id: &'static str,
    title: &'static str,
    subtitle: Option<&'static str>,
    trailing: AnyElement,
    divider: bool,
    cx: &mut Context<SettingsWindow>,
    action: impl Fn(&mut SettingsWindow, &mut Context<SettingsWindow>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(title)
        .min_h(px(if subtitle.is_some() { 58.0 } else { 50.0 }))
        .pl(px(16.0))
        .pr(px(14.0))
        .py(px(9.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .when(divider, |row| {
            row.border_t_1().border_color(theme::HAIRLINE)
        })
        .cursor_pointer()
        .on_click(cx.listener(move |view, _, window, cx| {
            action(view, cx);
            window.refresh();
        }))
        .child(label_stack(title, subtitle))
        .child(trailing)
        .into_any_element()
}

fn label_stack(title: &str, subtitle: Option<&str>) -> Div {
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(13.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::LABEL)
                .child(title.to_string()),
        )
        .children(subtitle.map(|subtitle| {
            div()
                .text_size(px(12.0))
                .text_color(theme::SECONDARY)
                .child(subtitle.to_string())
        }))
}

fn toggle(on: bool) -> Div {
    div()
        .w(px(38.0))
        .h(px(22.0))
        .flex_shrink_0()
        .p(px(2.0))
        .flex()
        .when(on, |track| track.justify_end())
        .rounded_full()
        .bg(if on { theme::AMBER } else { theme::TRACK })
        .child(div().size(px(18.0)).rounded_full().bg(theme::KNOB))
}

fn shortcut_control(shortcut: &str, capturing: bool) -> AnyElement {
    let content = if capturing {
        div()
            .text_size(px(12.0))
            .text_color(theme::AMBER)
            .child("Press shortcut…")
            .into_any_element()
    } else {
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .children(hotkey::keycaps(shortcut).into_iter().map(|cap| {
                div()
                    .h(px(22.0))
                    .min_w(px(22.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.0))
                    .bg(theme::RAISED)
                    .text_size(px(12.0))
                    .text_color(theme::LABEL)
                    .child(cap)
            }))
            .into_any_element()
    };
    div()
        .h(px(30.0))
        .px(px(4.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.0))
        .rounded(px(8.0))
        .bg(theme::HUD)
        .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
        .child(content)
        .child(div().w(px(1.0)).h(px(16.0)).bg(theme::HAIRLINE))
        .child(
            div()
                .px(px(8.0))
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::AMBER)
                .child("Change"),
        )
        .into_any_element()
}

fn section_label(label: &'static str) -> Div {
    div()
        .text_size(px(12.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::SECONDARY)
        .child(label)
}

fn group() -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(12.0))
        .overflow_hidden()
        .bg(theme::INSET)
        .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
}

fn modal_backdrop() -> Div {
    div()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui_kit::rgba(0x000000b3))
}

fn link_row(
    id: &'static str,
    title: &'static str,
    detail: impl Into<SharedString>,
    url: &'static str,
    divider: bool,
    _cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(title)
        .h(px(42.0))
        .px(px(16.0))
        .flex()
        .items_center()
        .justify_between()
        .when(divider, |row| {
            row.border_t_1().border_color(theme::HAIRLINE)
        })
        .cursor_pointer()
        .on_click(move |_, _, cx| cx.open_url(url))
        .child(
            div()
                .text_size(px(13.0))
                .text_color(theme::LABEL)
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(theme::SECONDARY)
                .child(detail.into()),
        )
        .into_any_element()
}
