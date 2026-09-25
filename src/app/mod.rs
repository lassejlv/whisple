use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{
    AnyWindowHandle, App, ClipboardItem, Context, FocusHandle, Focusable, KeyBinding, Keystroke,
    Menu, MenuItem, WeakEntity,
};

use crate::assistant::context::{self as screen, Snapshot};
use crate::assistant::{self, ErrorKind, Outcome, Route};
use crate::audio::{self, Mic};
use crate::i18n::{t, tf};
#[cfg(feature = "licensing")]
use crate::licensing::{self as license, Access};
use crate::platform::hotkey::{self, Shortcut};
#[cfg(target_os = "macos")]
use crate::platform::macos::dictation;
use crate::platform::placement as place;
use crate::platform::tray;
#[cfg(target_os = "windows")]
use crate::platform::windows::dictation;
use crate::settings::{self, Preferences};
use crate::startup;
use crate::transcription::cloud::{self, Provider};
use crate::transcription::local as stt;
use crate::transcription::models::{self, ModelSpec};
use crate::ui::motion::{Ease, Spring};
use crate::ui::settings::SettingsTarget;
#[cfg(target_os = "macos")]
use crate::updater::{self, PreparedUpdate, UpdatePrompt};

mod actions;
mod recording;
mod state;
pub(crate) use state::*;

gpui_kit::actions!(whisp, [ToggleListen, CloseOverlay, CopyResult, QuitWhisp]);

impl Whisp {
    pub(crate) fn new(
        window: &mut gpui_kit::Window,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let models = load_models();
        let prefs = settings::load();
        let selected = models::load_selected()
            .filter(|id| models::spec(id).is_some() || Provider::from_id(id).is_some())
            .unwrap_or_else(|| models::recommended_id().to_string());
        let cloud_keys = Provider::ALL.map(|provider| provider.id() == selected);
        let hotkey_error = [
            (Shortcut::Show, &prefs.show_hotkey),
            (Shortcut::Record, &prefs.record_hotkey),
        ]
        .into_iter()
        .find_map(|(slot, source)| {
            hotkey::parse(source).and_then(|chord| hotkey::install(slot, chord).err())
        })
        .map(|err| tf("Could not register the shortcut: {}", &[&err]));
        if prefs.open_on_startup {
            if let Err(err) = startup::apply(true) {
                eprintln!("could not refresh the login item: {err}");
            }
        }
        let view = cx.weak_entity();
        cx.intercept_keystrokes(move |event, _, cx| {
            if hotkey::is_modifier_only(&event.keystroke.key) {
                return;
            }
            let keystroke = event.keystroke.clone();
            view.update(cx, |this, cx| {
                if this.recording_hotkey.is_none() {
                    return;
                }
                cx.stop_propagation();
                this.capture_hotkey(&keystroke, cx);
            })
            .ok();
        })
        .detach();
        let (screen_x, screen_y, screen_w, screen_h) = cx
            .primary_display()
            .map(|display| {
                let bounds = display.bounds();
                (
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height),
                )
            })
            .unwrap_or((0.0, 0.0, 1920.0, 1200.0));
        #[cfg(target_os = "macos")]
        cx.observe_window_activation(window, |view, window, cx| {
            #[cfg(debug_assertions)]
            if crate::dev_ui_test() {
                return;
            }
            if !view.bar_visible {
                return;
            }
            if window.is_window_active() {
                view.was_window_active = true;
            } else if view.was_window_active && !view.visibility_locked() {
                view.set_visible(false, window, cx);
            }
        })
        .detach();
        let mut view = Self {
            focus_handle: cx.focus_handle(),
            hud_window: window.window_handle(),
            phase: Phase::Idle,
            levels: VecDeque::new(),
            menu_open: false,
            cloud_keys,
            models,
            selected,
            download: None,
            pending_uninstall: None,
            error: hotkey_error,
            recovery: None,
            failed_audio: None,
            transcribing_provider: None,
            copied: false,
            result_kind: ResultKind::Dictation,
            working: None,
            chrome: Ease::chrome(COLLAPSED_HEIGHT),
            press_id: None,
            press: Ease::press(0.0),
            bars: vec![Spring::level(0.08); BARS],
            reveal: None,
            settings_window: None,
            #[cfg(feature = "licensing")]
            license_access: Access::Checking,
            #[cfg(feature = "licensing")]
            license_checking: false,
            #[cfg(feature = "licensing")]
            license_generation: 0,
            #[cfg(feature = "licensing")]
            last_license_check: Instant::now(),
            transcription_id: 0,
            language: prefs.language,
            output_language: prefs.output_language,
            show_hotkey: prefs.show_hotkey,
            record_hotkey: prefs.record_hotkey,
            copy_notes: prefs.copy_notes,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            dictation_target: None,
            clean_fillers: prefs.clean_fillers,
            voice_commands: prefs.voice_commands,
            screen_context: prefs.screen_context,
            screen: Snapshot::default(),
            input_device: prefs.input_device,
            open_on_startup: prefs.open_on_startup,
            #[cfg(target_os = "macos")]
            update: None,
            #[cfg(target_os = "macos")]
            update_prompt: (cfg!(feature = "licensing") && updater::just_updated())
                .then_some(UpdatePrompt::JustUpdated),
            #[cfg(target_os = "macos")]
            update_checking: false,
            #[cfg(target_os = "macos")]
            last_update_check: Instant::now(),
            recording_hotkey: None,
            listen_started: None,
            recorded: Duration::ZERO,
            last_text: String::new(),
            bar_visible: visible,
            #[cfg(target_os = "macos")]
            was_window_active: false,
            suppress_actions_until: None,
            last_tick: Instant::now(),
            screen_x,
            screen_y,
            screen_w,
            screen_h,
            placed_height: COLLAPSED_HEIGHT,
        };
        view.listen_for_commands(window.window_handle(), cx);
        view.refresh_cloud_keys(cx);
        #[cfg(feature = "licensing")]
        view.refresh_license(cx);
        #[cfg(target_os = "macos")]
        view.check_for_updates(cx);
        view
    }

    fn refresh_cloud_keys(&self, cx: &mut Context<Self>) {
        let initial = self.cloud_keys;
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let keys = cx
                .background_executor()
                .spawn(async { Provider::ALL.map(cloud::has_key) })
                .await;
            this.update(cx, |view, cx| {
                for (provider, has_key) in Provider::ALL.into_iter().zip(keys) {
                    if let Ok(has_key) = has_key {
                        let index = provider.index();
                        if view.cloud_keys[index] == initial[index] {
                            view.cloud_keys[index] = has_key;
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn tick(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        #[cfg(feature = "licensing")]
        self.expire_trial_if_needed(cx);
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        let dt = if dt > 0.1 { 1.0 / 60.0 } else { dt };

        self.chrome.set(self.settled_height());
        let mut moving = self.chrome.step();
        moving |= self.press.step();
        if self.press.target() == 0.0 && !self.press.busy() {
            self.press_id = None;
        }

        if matches!(self.phase, Phase::Listening(_)) {
            self.note_level();
        } else {
            for bar in &mut self.bars {
                bar.target = 0.08;
            }
        }
        for bar in &mut self.bars {
            moving |= bar.step(dt);
        }

        if let Some(reveal) = self.desired_reveal() {
            self.reveal = Some(reveal);
        } else if !self.chrome.busy() {
            self.reveal = None;
        }

        if moving
            || self.download.is_some()
            || matches!(self.phase, Phase::Listening(_) | Phase::Transcribing)
        {
            window.request_animation_frame();
        }

        if self.bar_visible {
            let height = self.window_height();
            let actual = window.viewport_size().height.as_f32();
            // Windows' resize keeps the top-left fixed, so a taller panel grows
            // off the bottom of the screen and only the header stays visible.
            // AppKit's content-size change is async and needs the resizable
            // style bit, which a borderless window does not get. `place::dock`
            // holds the bottom edge on every platform.
            if (self.placed_height - height).abs() >= 0.5 || (actual - height).abs() >= 1.0 {
                let (screen_x, screen_y, screen_w, screen_h) =
                    (self.screen_x, self.screen_y, self.screen_w, self.screen_h);
                // Run before the next draw, outside this render pass. Resizing
                // the Metal surface during rendering can leave AppKit waiting
                // a second for a drawable, even with only one resize.
                #[cfg(target_os = "macos")]
                cx.on_next_frame(window, move |view, window, cx| {
                    view.placed_height = height;
                    place::dock(WINDOW_WIDTH, height, screen_x, screen_y, screen_w, screen_h);
                    apply_window_height(window, height, cx);
                });
                #[cfg(not(target_os = "macos"))]
                {
                    self.placed_height = height;
                    place::dock(WINDOW_WIDTH, height, screen_x, screen_y, screen_w, screen_h);
                    apply_window_height(window, height, cx);
                }
            }
        }
    }

    fn settled_height(&self) -> f32 {
        let mut height = COLLAPSED_HEIGHT + self.panel_height(self.desired_reveal());
        if self.notice_visible() {
            height += NOTICE_EXTRA;
        }
        height
    }

    /// The window's height for this frame, in whole points.
    ///
    /// On macOS every resize makes AppKit ask GPUI for a synchronous frame
    /// that holds a Metal drawable until the run loop turns. Resizing on each
    /// animation frame could exhaust the layer's drawables, and the next frame
    /// then blocked for a full second. So the window jumps to its final
    /// height when a panel grows, shrinks only once a panel has closed, and
    /// the HUD animates inside it.
    fn window_height(&self) -> f32 {
        #[cfg(target_os = "macos")]
        let height = self.chrome.window_height(self.placed_height);
        #[cfg(not(target_os = "macos"))]
        let height = self.chrome.value;
        height.round()
    }

    pub(crate) fn panel_height(&self, reveal: Option<Reveal>) -> f32 {
        match reveal {
            Some(Reveal::Result) => RESULT_EXTRA + (self.result_lines() - 2) as f32 * RESULT_LINE,
            Some(Reveal::Menu) => menu_height(self.menu_choices().len()),
            None => 0.0,
        }
    }

    pub(crate) fn result_lines(&self) -> usize {
        match self.result_kind {
            ResultKind::Dictation | ResultKind::Translated(_) | ResultKind::Command => 2,
            ResultKind::Answer(_) | ResultKind::Typed(_) => self
                .last_text
                .chars()
                .count()
                .div_ceil(LINE_CHARS)
                .clamp(2, MAX_ANSWER_LINES),
        }
    }

    pub(crate) fn listening_for(&self) -> Option<Duration> {
        self.listen_started.map(|started| started.elapsed())
    }

    pub(crate) fn press_down(&mut self, id: &str) {
        if self.press_id.as_deref() != Some(id) {
            self.press.snap(0.0);
            self.press_id = Some(id.to_string());
        }
        self.press.set(1.0);
    }

    pub(crate) fn press_up(&mut self, id: &str) {
        if self.press_id.as_deref() == Some(id) {
            self.press.set(0.0);
        }
    }

    pub(crate) fn press_scale(&self, id: &str) -> f32 {
        if self.press_id.as_deref() == Some(id) {
            1.0 - 0.03 * self.press.value
        } else {
            1.0
        }
    }

    fn snap_chrome(&mut self) {
        self.chrome.snap(self.settled_height());
        self.reveal = self.desired_reveal();
    }

    fn listen_for_commands(
        &self,
        window_handle: gpui_kit::AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        let screen_x = self.screen_x;
        let screen_y = self.screen_y;
        let screen_w = self.screen_w;
        let screen_h = self.screen_h;
        cx.spawn(async move |this: WeakEntity<Self>, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;
            let alive = window_handle
                .update(cx, |_, window, cx| {
                    this.update(cx, |view, cx| {
                        let presses = hotkey::take_presses();
                        if presses.show {
                            view.set_visible(!view.bar_visible, window, cx);
                        }
                        if presses.record {
                            view.record_from_shortcut(window, cx);
                        }
                        while let Some(command) = tray::take_command() {
                            match command {
                                tray::Command::Show => view.set_visible(true, window, cx),
                                tray::Command::Hide => view.set_visible(false, window, cx),
                                tray::Command::Settings => {
                                    view.open_settings_window(cx);
                                }
                                #[cfg(target_os = "macos")]
                                tray::Command::Update => {
                                    view.show_or_check_for_updates(cx);
                                    if view.update.is_some() {
                                        view.set_visible(true, window, cx);
                                    }
                                }
                                #[cfg(not(target_os = "macos"))]
                                tray::Command::Update => {}
                                tray::Command::Quit => cx.quit(),
                            }
                        }
                        #[cfg(target_os = "macos")]
                        if view.last_update_check.elapsed() >= Duration::from_secs(6 * 60 * 60) {
                            view.check_for_updates(cx);
                        }
                        // A trial records the time often, so setting the clock
                        // back between sessions costs the time used, not
                        // just the time since launch. A locked trial waiting
                        // for the server tries again as often.
                        #[cfg(feature = "licensing")]
                        let license_interval = if matches!(
                            view.license_access,
                            Access::Trial { .. } | Access::Offline(_) | Access::Unavailable { .. }
                        ) {
                            Duration::from_secs(5 * 60)
                        } else {
                            Duration::from_secs(6 * 60 * 60)
                        };
                        #[cfg(feature = "licensing")]
                        if view.last_license_check.elapsed() >= license_interval {
                            view.refresh_license(cx);
                        }
                        if view.bar_visible {
                            // Re-anchor the native size already chosen by tick;
                            // the animated HUD height would undo its resize and
                            // clip an opening panel.
                            place::dock(
                                WINDOW_WIDTH,
                                view.placed_height,
                                screen_x,
                                screen_y,
                                screen_w,
                                screen_h,
                            );
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
    }

    pub(crate) fn set_visible(
        &mut self,
        visible: bool,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        if !visible && self.visibility_locked() {
            return;
        }
        let was_visible = self.bar_visible;
        self.bar_visible = visible;
        if !visible {
            self.pending_uninstall = None;
            self.menu_open = false;
        }
        #[cfg(target_os = "macos")]
        {
            self.was_window_active = false;
        }
        tray::set_visible(visible, cx);
        place::set_mapped(visible);
        if visible {
            // Capture the editor and screen before the HUD takes keyboard
            // focus.
            if !was_visible {
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                {
                    self.dictation_target = dictation::Target::focused();
                }
                self.screen = if self.screen_context {
                    screen::focused()
                } else {
                    Snapshot::default()
                };
            }
            window.activate_window();
            cx.activate(true);
            window.focus(&self.focus_handle, cx);
        } else {
            self.stop_recording();
            self.failed_audio = None;
        }
        cx.notify();
    }

    fn visibility_locked(&self) -> bool {
        matches!(self.phase, Phase::Listening(_) | Phase::Transcribing)
    }

    pub(crate) fn update_line_visible(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.error.is_none() && self.update_prompt.is_some() && !self.visibility_locked()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    fn notice_visible(&self) -> bool {
        self.error.is_some() || self.update_line_visible()
    }

    fn desired_reveal(&self) -> Option<Reveal> {
        if self.menu_open {
            Some(Reveal::Menu)
        } else if matches!(self.phase, Phase::Result(_)) {
            Some(Reveal::Result)
        } else {
            None
        }
    }

    pub(crate) fn menu_choices(&self) -> Vec<MenuChoice> {
        let local = self
            .models
            .iter()
            .filter(|model| model.ready)
            .map(|model| MenuChoice {
                id: model.spec.id,
                name: t(model.spec.name),
                detail: t("On device"),
            });
        let cloud = Provider::ALL
            .into_iter()
            .filter(|provider| self.cloud_keys[provider.index()])
            .map(|provider| MenuChoice {
                id: provider.id(),
                name: provider.name(),
                detail: t("Cloud"),
            });
        cloud.chain(local).collect()
    }

    fn rest_bars(&mut self) {
        for bar in &mut self.bars {
            bar.snap(0.08);
        }
    }

    pub(crate) fn selected_spec(&self) -> &'static ModelSpec {
        models::spec(&self.selected)
            .or_else(|| models::spec(models::recommended_id()))
            .expect("the catalog has a recommended model")
    }

    pub(crate) fn selected_ready(&self) -> bool {
        if let Some(provider) = Provider::from_id(&self.selected) {
            return self.cloud_keys[provider.index()];
        }
        self.models
            .iter()
            .any(|model| model.spec.id == self.selected && model.ready)
    }

    fn refresh_models(&mut self) {
        self.models = load_models();
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum Typing {
    Inserted,
    NoTarget,
    Failed(String),
}

fn type_into(target: TypingTarget, text: &str) -> Typing {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        match target {
            Some(target) => match target.insert(text) {
                Ok(()) => Typing::Inserted,
                Err(err) => Typing::Failed(err),
            },
            None => Typing::NoTarget,
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let () = target;
        let _ = text;
        Typing::NoTarget
    }
}

fn menu_height(choices: usize) -> f32 {
    let divider = if choices > 0 { MENU_DIVIDER } else { 0.0 };
    MENU_PAD * 2.0 + (choices + 1) as f32 * MENU_ROW + divider + 1.0
}

fn load_models() -> Vec<InstalledModel> {
    models::CATALOG
        .iter()
        .map(|spec| InstalledModel {
            spec,
            ready: models::is_downloaded(spec),
        })
        .collect()
}

impl Focusable for Whisp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(target_os = "macos")]
const COPY_KEYS: &str = "cmd-c";
#[cfg(not(target_os = "macos"))]
const COPY_KEYS: &str = "ctrl-c";

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", ToggleListen, Some("Whisp")),
        KeyBinding::new("escape", CloseOverlay, Some("Whisp")),
        KeyBinding::new(COPY_KEYS, CopyResult, Some("Whisp")),
        KeyBinding::new("cmd-q", QuitWhisp, None),
    ]);
    cx.on_action::<QuitWhisp>(|_, cx| cx.quit());
    cx.set_menus([Menu::new("Whisple").items([MenuItem::action(t("Quit Whisple"), QuitWhisp)])]);
}

fn apply_window_height(window: &mut gpui_kit::Window, height: f32, cx: &mut App) {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let _ = cx;
        window.resize(gpui_kit::size(
            gpui_kit::px(WINDOW_WIDTH),
            gpui_kit::px(height),
        ));
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        let _ = height;
        // The frame was already moved in `place::dock`. Pull GPUI's viewport
        // up to the content size before this frame lays out.
        window.bounds_changed(cx);
    }
}
