use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::input::InputState;
use gpui_kit::{
    App, ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyBinding, Keystroke, Menu,
    MenuItem, WeakEntity,
};

use crate::audio::{self, Mic};
use crate::hotkey;
use crate::models::{self, ModelSpec};
use crate::motion::{Ease, Spring};
use crate::place;
use crate::settings::{self, Preferences};
use crate::startup;
use crate::stt;
use crate::tray;
#[cfg(target_os = "macos")]
use crate::updater::{self, PreparedUpdate};

pub(crate) const WINDOW_WIDTH: f32 = 400.0;
pub(crate) const WINDOW_RADIUS: f32 = 20.0;
/// The bar row. The window adds its 1px border above and below.
pub(crate) const BAR_HEIGHT: f32 = 56.0;
pub(crate) const COLLAPSED_HEIGHT: f32 = BAR_HEIGHT + 2.0;
/// Panel heights above the bar, each including its 1px hairline to the bar.
/// With the bar they give the HUD heights measured in the design: result 175,
/// model picker 503, settings 431, language 451.
pub(crate) const RESULT_EXTRA: f32 = 117.0;
pub(crate) const PICKER_EXTRA: f32 = 445.0;
pub(crate) const SETTINGS_EXTRA: f32 = 373.0;
pub(crate) const LANGUAGE_EXTRA: f32 = 393.0;
pub(crate) const MICROPHONE_EXTRA: f32 = LANGUAGE_EXTRA;
pub(crate) const ERROR_EXTRA: f32 = 22.0;

/// Waveform bars across the bar while recording, oldest on the left.
pub(crate) const BARS: usize = 19;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reveal {
    Picker,
    Result,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsPage {
    Main,
    Language,
    Microphone,
}

gpui_kit::actions!(whisp, [ToggleListen, CloseOverlay, CopyResult, QuitWhisp]);

pub(crate) enum Phase {
    Idle,
    Listening(Mic),
    Transcribing,
    Result(String),
}

pub(crate) struct InstalledModel {
    pub spec: &'static ModelSpec,
    pub ready: bool,
}

pub(crate) struct Download {
    pub id: String,
    pub received: Arc<AtomicU64>,
    pub total: u64,
    cancel: Arc<AtomicBool>,
}

pub(crate) struct Whisp {
    pub focus_handle: FocusHandle,
    pub phase: Phase,
    pub levels: VecDeque<f32>,
    pub picker_open: bool,
    pub models: Vec<InstalledModel>,
    pub selected: String,
    pub download: Option<Download>,
    pub pending_uninstall: Option<String>,
    pub error: Option<String>,
    pub copied: bool,
    pub chrome: Ease,
    pub press_id: Option<String>,
    pub press: Ease,
    pub bars: Vec<Spring>,
    pub reveal: Option<Reveal>,
    pub picker_opened_at: Option<Instant>,
    pub settings_open: bool,
    pub settings_page: SettingsPage,
    pub settings_opened_at: Option<Instant>,
    pub language: String,
    pub show_hotkey: String,
    pub copy_notes: bool,
    pub clean_fillers: bool,
    pub input_device: String,
    pub open_on_startup: bool,
    #[cfg(target_os = "macos")]
    update: Option<PreparedUpdate>,
    #[cfg(target_os = "macos")]
    update_checking: bool,
    #[cfg(target_os = "macos")]
    last_update_check: Instant,
    pub microphones: Vec<String>,
    pub recording_hotkey: bool,
    /// When the current recording started, for the bar's timer.
    pub listen_started: Option<Instant>,
    /// The language page's search field, alive only while that page shows.
    pub language_search: Option<Entity<InputState>>,
    /// Length of the audio behind the last transcript.
    pub recorded: Duration,
    /// The last transcript, kept so the result panel can slide shut showing
    /// it after a new recording has already replaced the phase.
    pub last_text: String,
    pub page_fade: Ease,
    pub bar_visible: bool,
    #[cfg(target_os = "macos")]
    was_window_active: bool,
    suppress_actions_until: Option<Instant>,
    last_tick: Instant,
    screen_x: f32,
    screen_y: f32,
    screen_w: f32,
    screen_h: f32,
    placed_height: f32,
}

impl Whisp {
    pub(crate) fn new(
        window: &mut gpui_kit::Window,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let models = load_models();
        let prefs = settings::load();
        let selected = models::load_selected()
            .filter(|id| models::spec(id).is_some())
            .unwrap_or_else(|| models::recommended_id().to_string());
        let hotkey_error = hotkey::parse(&prefs.show_hotkey)
            .and_then(|chord| hotkey::install(chord).err())
            .map(|err| format!("Could not register the shortcut: {err}"));
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
                if !this.recording_hotkey {
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
            phase: Phase::Idle,
            levels: VecDeque::new(),
            picker_open: false,
            models,
            selected,
            download: None,
            pending_uninstall: None,
            error: hotkey_error,
            copied: false,
            chrome: Ease::chrome(COLLAPSED_HEIGHT),
            press_id: None,
            press: Ease::press(0.0),
            bars: vec![Spring::level(0.08); BARS],
            reveal: None,
            picker_opened_at: None,
            settings_open: false,
            settings_page: SettingsPage::Main,
            settings_opened_at: None,
            language: prefs.language,
            show_hotkey: prefs.show_hotkey,
            copy_notes: prefs.copy_notes,
            clean_fillers: prefs.clean_fillers,
            input_device: prefs.input_device,
            open_on_startup: prefs.open_on_startup,
            #[cfg(target_os = "macos")]
            update: None,
            #[cfg(target_os = "macos")]
            update_checking: false,
            #[cfg(target_os = "macos")]
            last_update_check: Instant::now(),
            microphones: Vec::new(),
            recording_hotkey: false,
            listen_started: None,
            language_search: None,
            recorded: Duration::ZERO,
            last_text: String::new(),
            page_fade: Ease::at(1.0, Duration::from_millis(180), 0.02),
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
        // A hidden window need not render, so start listening at creation.
        view.listen_for_commands(window.window_handle(), cx);
        #[cfg(target_os = "macos")]
        view.check_for_updates(cx);
        view
    }

    pub(crate) fn tick(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        let dt = if dt > 0.1 { 1.0 / 60.0 } else { dt };

        self.chrome.set(self.settled_height());
        let mut moving = self.chrome.step();
        moving |= self.press.step();
        moving |= self.page_fade.step();
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

        if self.settings_open {
            self.reveal = Some(Reveal::Settings);
        } else if self.picker_open {
            self.reveal = Some(Reveal::Picker);
        } else if matches!(self.phase, Phase::Result(_)) {
            self.reveal = Some(Reveal::Result);
        } else if !self.chrome.busy() {
            self.reveal = None;
        }

        if moving
            || self.staggering()
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
        let reveal = if self.settings_open {
            Some(Reveal::Settings)
        } else if self.picker_open {
            Some(Reveal::Picker)
        } else if matches!(self.phase, Phase::Result(_)) {
            Some(Reveal::Result)
        } else {
            None
        };
        let mut height = COLLAPSED_HEIGHT + self.panel_height(reveal);
        if self.error.is_some() {
            height += ERROR_EXTRA;
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

    /// The settled height of a panel above the bar, hairline included.
    pub(crate) fn panel_height(&self, reveal: Option<Reveal>) -> f32 {
        match reveal {
            Some(Reveal::Result) => RESULT_EXTRA,
            Some(Reveal::Picker) => PICKER_EXTRA,
            Some(Reveal::Settings) => match self.settings_page {
                SettingsPage::Main => SETTINGS_EXTRA,
                SettingsPage::Language => LANGUAGE_EXTRA,
                SettingsPage::Microphone => MICROPHONE_EXTRA,
            },
            None => 0.0,
        }
    }

    /// Time on the running recording, if one is running.
    pub(crate) fn listening_for(&self) -> Option<Duration> {
        self.listen_started.map(|started| started.elapsed())
    }

    fn staggering(&self) -> bool {
        let window = Duration::from_millis(380);
        self.picker_opened_at
            .is_some_and(|opened| opened.elapsed() < window)
            || (self.settings_page == SettingsPage::Main
                && self
                    .settings_opened_at
                    .is_some_and(|opened| opened.elapsed() < window))
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
        if self.settings_open {
            self.reveal = Some(Reveal::Settings);
        } else if self.picker_open {
            self.reveal = Some(Reveal::Picker);
        } else if matches!(self.phase, Phase::Result(_)) {
            self.reveal = Some(Reveal::Result);
        } else {
            self.reveal = None;
        }
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
                        let pressed = hotkey::take_press();
                        if pressed {
                            view.set_visible(!view.bar_visible, window, cx);
                        }
                        while let Some(command) = tray::take_command() {
                            match command {
                                tray::Command::Show => view.set_visible(true, window, cx),
                                tray::Command::Hide => view.set_visible(false, window, cx),
                                tray::Command::Settings => {
                                    if !view.settings_open {
                                        view.toggle_settings(cx);
                                    }
                                    view.set_visible(true, window, cx);
                                }
                                #[cfg(target_os = "macos")]
                                tray::Command::Update => view.install_or_check_for_updates(cx),
                                #[cfg(not(target_os = "macos"))]
                                tray::Command::Update => {}
                                tray::Command::Quit => cx.quit(),
                            }
                        }
                        #[cfg(target_os = "macos")]
                        if view.last_update_check.elapsed() >= Duration::from_secs(6 * 60 * 60) {
                            view.check_for_updates(cx);
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
        self.bar_visible = visible;
        if !visible {
            self.pending_uninstall = None;
        }
        #[cfg(target_os = "macos")]
        {
            self.was_window_active = false;
        }
        tray::set_visible(visible, cx);
        place::set_mapped(visible);
        if visible {
            window.activate_window();
            cx.activate(true);
            window.focus(&self.focus_handle, cx);
        } else {
            self.stop_recording();
            #[cfg(target_os = "macos")]
            cx.hide();
        }
        cx.notify();
    }

    fn visibility_locked(&self) -> bool {
        matches!(self.phase, Phase::Listening(_) | Phase::Transcribing)
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
        self.models
            .iter()
            .any(|model| model.spec.id == self.selected && model.ready)
    }

    pub(crate) fn toggle_listen(&mut self, cx: &mut Context<Self>) {
        if self.recording_hotkey || self.actions_suppressed() {
            return;
        }
        self.error = None;
        self.copied = false;
        self.close_settings();
        if !self.selected_ready() {
            self.picker_open = true;
            self.picker_opened_at = None;
            self.error = Some("Download a model before recording.".into());
            self.snap_chrome();
            cx.notify();
            return;
        }

        match &self.phase {
            Phase::Listening(_) => {
                let Phase::Listening(mic) = std::mem::replace(&mut self.phase, Phase::Transcribing)
                else {
                    return;
                };
                let (samples, rate) = mic.take();
                self.recorded = self
                    .listen_started
                    .take()
                    .map_or(Duration::ZERO, |started| started.elapsed());
                self.levels.clear();
                self.rest_bars();
                self.transcribe(samples, rate, cx);
                self.snap_chrome();
            }
            Phase::Transcribing => {}
            Phase::Idle | Phase::Result(_) => match Mic::start(&self.input_device) {
                Ok(mic) => {
                    self.levels.clear();
                    self.rest_bars();
                    self.phase = Phase::Listening(mic);
                    self.listen_started = Some(Instant::now());
                    self.picker_open = false;
                    self.picker_opened_at = None;
                    self.snap_chrome();
                }
                Err(err) => {
                    self.error = Some(err);
                    self.picker_open = true;
                    self.picker_opened_at = None;
                    self.snap_chrome();
                }
            },
        }
        cx.notify();
    }

    pub(crate) fn transcribe_sample(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.copied = false;
        if !self.selected_ready() {
            self.error = Some("Choose a downloaded model first.".into());
            cx.notify();
            return;
        }
        if matches!(self.phase, Phase::Listening(_) | Phase::Transcribing) {
            cx.notify();
            return;
        }
        match audio::decode_wav(include_bytes!("../assets/sample.wav")) {
            Ok((samples, rate)) => {
                self.recorded =
                    Duration::from_secs_f64(samples.len() as f64 / f64::from(rate.max(1)));
                self.phase = Phase::Transcribing;
                self.close_settings();
                self.picker_open = false;
                self.picker_opened_at = None;
                self.snap_chrome();
                self.transcribe(samples, rate, cx);
            }
            Err(err) => self.error = Some(err),
        }
        cx.notify();
    }

    pub(crate) fn choose_model(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(spec) = models::spec(id) else {
            return;
        };
        self.pending_uninstall = None;
        self.error = None;
        if models::is_downloaded(spec) {
            self.selected = spec.id.to_string();
            models::save_selected(spec.id);
            self.refresh_models();
            self.picker_open = false;
            cx.notify();
            return;
        }
        self.start_download(spec, cx);
    }

    pub(crate) fn cancel_download(&mut self, cx: &mut Context<Self>) {
        if let Some(download) = self.download.take() {
            download.cancel.store(true, Ordering::Relaxed);
            cx.notify();
        }
    }

    pub(crate) fn uninstall_model(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(spec) = models::spec(id) else {
            return;
        };
        if !models::is_downloaded(spec) {
            self.pending_uninstall = None;
            self.refresh_models();
            cx.notify();
            return;
        }
        if self.pending_uninstall.as_deref() != Some(id) {
            self.pending_uninstall = Some(id.to_string());
            cx.notify();
            return;
        }
        self.pending_uninstall = None;
        if matches!(self.phase, Phase::Listening(_) | Phase::Transcribing)
            || self
                .download
                .as_ref()
                .is_some_and(|download| download.id == id)
        {
            self.error = Some("Finish recording or downloading before removing this model.".into());
            cx.notify();
            return;
        }
        match models::uninstall(spec) {
            Ok(()) => {
                self.refresh_models();
                if self.selected == id {
                    let replacement = self
                        .models
                        .iter()
                        .find(|model| model.ready && model.spec.recommended)
                        .or_else(|| self.models.iter().find(|model| model.ready))
                        .map(|model| model.spec.id)
                        .unwrap_or_else(models::recommended_id);
                    self.selected = replacement.to_string();
                    self.persist();
                }
                self.error = None;
            }
            Err(err) => self.error = Some(err),
        }
        cx.notify();
    }

    /// Disk used by the downloaded models.
    pub(crate) fn storage_used(&self) -> u64 {
        self.models
            .iter()
            .filter(|model| model.ready)
            .map(|model| model.spec.bytes)
            .sum()
    }

    pub(crate) fn toggle_picker(&mut self, cx: &mut Context<Self>) {
        self.stop_recording();
        self.pending_uninstall = None;
        if self.settings_open {
            self.settings_open = false;
            self.settings_opened_at = None;
            self.settings_page = SettingsPage::Main;
            self.picker_open = true;
            self.picker_opened_at = Some(Instant::now());
            cx.notify();
            return;
        }
        self.picker_open = !self.picker_open;
        self.picker_opened_at = if self.picker_open {
            Some(Instant::now())
        } else {
            None
        };
        cx.notify();
    }

    pub(crate) fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        self.stop_recording();
        self.pending_uninstall = None;
        self.picker_open = false;
        self.picker_opened_at = None;
        self.settings_open = !self.settings_open;
        if self.settings_open {
            self.settings_page = SettingsPage::Main;
            self.settings_opened_at = Some(Instant::now());
            self.page_fade.snap(1.0);
        } else {
            self.settings_opened_at = None;
            self.settings_page = SettingsPage::Main;
        }
        self.error = None;
        cx.notify();
    }

    pub(crate) fn open_languages(&mut self, cx: &mut Context<Self>) {
        self.stop_recording();
        self.settings_page = SettingsPage::Language;
        self.page_fade.snap(0.0);
        self.page_fade.set(1.0);
        cx.notify();
    }

    pub(crate) fn open_microphones(&mut self, cx: &mut Context<Self>) {
        self.stop_recording();
        self.settings_page = SettingsPage::Microphone;
        self.page_fade.snap(0.0);
        self.page_fade.set(1.0);
        cx.notify();
        // Listing CoreAudio devices takes tens of milliseconds, so the page
        // opens with the last known list and refreshes once the scan is done.
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let listed = cx
                .background_executor()
                .spawn(async { audio::input_names() })
                .await;
            this.update(cx, |view, cx| {
                match listed {
                    Ok(names) => {
                        view.microphones = names;
                        view.error = None;
                    }
                    Err(err) => {
                        view.microphones.clear();
                        view.error = Some(err);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn choose_microphone(&mut self, name: &str, cx: &mut Context<Self>) {
        if audio::known_input(name, &self.microphones).is_err() {
            return;
        }
        self.input_device = name.to_string();
        self.persist();
        self.show_main_page();
        cx.notify();
    }

    pub(crate) fn toggle_open_on_startup(&mut self, cx: &mut Context<Self>) {
        self.set_open_on_startup(!self.open_on_startup, cx);
    }

    pub(crate) fn set_open_on_startup(&mut self, on: bool, cx: &mut Context<Self>) {
        if on == self.open_on_startup {
            return;
        }
        if let Err(err) = startup::apply(on) {
            self.error = Some(err);
            cx.notify();
            return;
        }
        self.open_on_startup = on;
        self.error = None;
        self.persist();
        cx.notify();
    }

    pub(crate) fn choose_language(&mut self, id: &str, cx: &mut Context<Self>) {
        if !Preferences::languages()
            .iter()
            .any(|language| language.id == id)
        {
            return;
        }
        self.language = id.to_string();
        self.persist();
        self.show_main_page();
        cx.notify();
    }

    pub(crate) fn show_main_page(&mut self) {
        self.settings_page = SettingsPage::Main;
        self.page_fade.snap(0.0);
        self.page_fade.set(1.0);
    }

    pub(crate) fn begin_hotkey_capture(&mut self, cx: &mut Context<Self>) {
        if self.recording_hotkey {
            self.stop_recording();
        } else {
            self.recording_hotkey = true;
            self.error = None;
            hotkey::set_paused(true);
        }
        cx.notify();
    }

    pub(crate) fn toggle_copy_notes(&mut self, cx: &mut Context<Self>) {
        self.set_copy_notes(!self.copy_notes, cx);
    }

    pub(crate) fn set_copy_notes(&mut self, on: bool, cx: &mut Context<Self>) {
        if on == self.copy_notes {
            return;
        }
        self.copy_notes = on;
        self.persist();
        cx.notify();
    }

    pub(crate) fn toggle_clean_fillers(&mut self, cx: &mut Context<Self>) {
        self.set_clean_fillers(!self.clean_fillers, cx);
    }

    pub(crate) fn set_clean_fillers(&mut self, on: bool, cx: &mut Context<Self>) {
        if on == self.clean_fillers {
            return;
        }
        self.clean_fillers = on;
        self.persist();
        cx.notify();
    }

    pub(crate) fn capture_hotkey(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        if hotkey::is_modifier_only(&keystroke.key) {
            return;
        }
        if keystroke.key == "escape" {
            self.recording_hotkey = false;
            hotkey::set_paused(false);
            self.error = None;
            self.suppress_actions_until = Some(Instant::now() + Duration::from_millis(280));
            cx.notify();
            return;
        }
        let Some(chord) = hotkey::from_keystroke(keystroke) else {
            return;
        };
        if !chord.has_modifier() {
            self.error = Some("Use Ctrl, Alt, or Super as well.".into());
            cx.notify();
            return;
        }
        if let Err(err) = hotkey::install(chord.clone()) {
            self.error = Some(format!("Could not use that shortcut: {err}"));
            cx.notify();
            return;
        }
        self.show_hotkey = chord.canonical();
        self.recording_hotkey = false;
        self.error = None;
        self.suppress_actions_until = Some(Instant::now() + Duration::from_millis(280));
        self.persist();
        hotkey::set_paused(false);
        cx.notify();
    }

    pub(crate) fn close_overlay(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if self.recording_hotkey {
            self.recording_hotkey = false;
            hotkey::set_paused(false);
            self.error = None;
            self.suppress_actions_until = Some(Instant::now() + Duration::from_millis(280));
            cx.notify();
            return;
        }
        if self.actions_suppressed() {
            return;
        }
        if self.settings_open && self.settings_page != SettingsPage::Main {
            self.settings_page = SettingsPage::Main;
            self.page_fade.snap(1.0);
            cx.notify();
            return;
        }
        if self.settings_open {
            self.settings_open = false;
            self.settings_opened_at = None;
            self.settings_page = SettingsPage::Main;
        } else if self.picker_open {
            self.picker_open = false;
            self.picker_opened_at = None;
            self.pending_uninstall = None;
        } else if matches!(self.phase, Phase::Result(_)) {
            self.phase = Phase::Idle;
            self.copied = false;
        } else if matches!(self.phase, Phase::Idle) {
            self.set_visible(false, window, cx);
        }
        self.error = None;
        self.snap_chrome();
        cx.notify();
    }

    pub(crate) fn quit(&mut self, cx: &mut Context<Self>) {
        cx.quit();
    }

    pub(crate) fn copy_result(&mut self, cx: &mut Context<Self>) {
        let Phase::Result(text) = &self.phase else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        self.copied = true;
        cx.notify();
    }

    pub(crate) fn note_level(&mut self) {
        let Phase::Listening(mic) = &self.phase else {
            return;
        };
        let level = mic.level();
        self.levels.push_back(level);
        while self.levels.len() > BARS {
            self.levels.pop_front();
        }
        for (index, bar) in self.bars.iter_mut().enumerate() {
            let from_end = BARS - 1 - index;
            bar.target = self
                .levels
                .iter()
                .rev()
                .nth(from_end)
                .copied()
                .unwrap_or(0.08);
        }
    }

    fn transcribe(&mut self, samples: Vec<f32>, rate: u32, cx: &mut Context<Self>) {
        let spec = self.selected_spec();
        let model_id = spec.id.to_string();
        let path = models::model_path(spec);
        let language = settings::whisper_language(&self.language).map(str::to_string);
        let clean = self.clean_fillers;
        let copy = self.copy_notes;
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    stt::transcribe(&model_id, &path, &samples, rate, language.as_deref(), clean)
                })
                .await;
            this.update(cx, |view, cx| {
                match outcome {
                    Ok(text) => {
                        if copy {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                            view.copied = true;
                        } else {
                            view.copied = false;
                        }
                        view.last_text.clone_from(&text);
                        view.phase = Phase::Result(text);
                        view.error = None;
                    }
                    Err(err) => {
                        view.phase = Phase::Idle;
                        view.error = Some(err);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn close_settings(&mut self) {
        self.settings_open = false;
        self.settings_opened_at = None;
        self.settings_page = SettingsPage::Main;
        self.stop_recording();
    }

    fn stop_recording(&mut self) {
        if self.recording_hotkey {
            self.recording_hotkey = false;
            hotkey::set_paused(false);
        }
    }

    fn actions_suppressed(&self) -> bool {
        self.suppress_actions_until
            .is_some_and(|until| Instant::now() < until)
    }

    fn persist(&self) {
        settings::save(&Preferences {
            selected: self.selected.clone(),
            language: self.language.clone(),
            show_hotkey: self.show_hotkey.clone(),
            copy_notes: self.copy_notes,
            clean_fillers: self.clean_fillers,
            input_device: self.input_device.clone(),
            open_on_startup: self.open_on_startup,
        });
    }

    fn start_download(&mut self, spec: &'static ModelSpec, cx: &mut Context<Self>) {
        if self
            .download
            .as_ref()
            .is_some_and(|download| download.id == spec.id)
        {
            return;
        }
        if let Some(current) = &self.download {
            current.cancel.store(true, Ordering::Relaxed);
        }

        let received = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let id = spec.id.to_string();
        self.download = Some(Download {
            id: id.clone(),
            received: Arc::clone(&received),
            total: spec.bytes,
            cancel: Arc::clone(&cancel),
        });
        cx.notify();

        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { models::download(spec, &received, &cancel) })
                .await;
            this.update(cx, |view, cx| {
                let current = view.download.as_ref().map(|download| download.id.clone());
                if current.as_deref() == Some(spec.id) {
                    view.download = None;
                }
                match outcome {
                    Ok(_) => {
                        view.refresh_models();
                        view.selected = spec.id.to_string();
                        models::save_selected(spec.id);
                        view.error = None;
                    }
                    Err(err) if err == "Download cancelled" => {}
                    Err(err) => view.error = Some(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    #[cfg(target_os = "macos")]
    fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        self.last_update_check = Instant::now();
        if !updater::is_packaged() || self.update_checking {
            return;
        }
        let prepared_version = self.update.as_ref().map(|update| update.version.clone());
        self.update_checking = true;
        tray::set_update(tray::UpdateStatus::Checking, cx);
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { updater::check_and_prepare(prepared_version) })
                .await;
            this.update(cx, |view, cx| {
                view.update_checking = false;
                let failed = match outcome {
                    Ok(Some(update)) => {
                        view.update = Some(update);
                        false
                    }
                    Ok(None) => false,
                    Err(err) => {
                        eprintln!("Whisple update check failed: {err}");
                        view.error = Some(format!("Update check failed: {err}"));
                        true
                    }
                };
                let version = view
                    .update
                    .as_ref()
                    .map(|update| update.version.to_string());
                let menu_status = match (version.as_deref(), failed) {
                    (Some(version), _) => tray::UpdateStatus::Available(version),
                    (None, true) => tray::UpdateStatus::Error,
                    (None, false) => tray::UpdateStatus::UpToDate,
                };
                tray::set_update(menu_status, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    #[cfg(target_os = "macos")]
    fn install_or_check_for_updates(&mut self, cx: &mut Context<Self>) {
        if self.update.is_none() {
            self.check_for_updates(cx);
            return;
        }
        if self.visibility_locked() {
            self.error = Some("Finish recording before installing the update.".into());
            cx.notify();
            return;
        }
        match self.update.as_mut().unwrap().install() {
            Ok(()) => cx.quit(),
            Err(err) => {
                self.error = Some(format!("Could not install the update: {err}"));
                cx.notify();
            }
        }
    }

    fn refresh_models(&mut self) {
        self.models = load_models();
    }
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
    cx.set_menus([Menu::new("Whisple").items([MenuItem::action("Quit Whisple", QuitWhisp)])]);
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
