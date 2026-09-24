use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::input::InputState;
use gpui_kit::{
    AnyWindowHandle, App, ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyBinding,
    Keystroke, Menu, MenuItem, WeakEntity,
};

use crate::audio::{self, Mic};
use crate::cloud::{self, Provider};
#[cfg(target_os = "macos")]
use crate::dictation;
use crate::hotkey;
use crate::license::{self, Access};
use crate::models::{self, ModelSpec};
use crate::motion::{Ease, Spring};
use crate::place;
use crate::settings::{self, Preferences};
use crate::startup;
use crate::stt;
use crate::tray;
#[cfg(target_os = "macos")]
use crate::updater::{self, PreparedUpdate, UpdatePrompt};

pub(crate) const WINDOW_WIDTH: f32 = 400.0;
pub(crate) const WINDOW_RADIUS: f32 = 20.0;
/// The bar row. The window adds its 1px border above and below.
pub(crate) const BAR_HEIGHT: f32 = 56.0;
pub(crate) const COLLAPSED_HEIGHT: f32 = BAR_HEIGHT + 2.0;
/// Panel heights above the bar, each including its 1px hairline to the bar.
/// With the bar they give the HUD heights measured in the design.
pub(crate) const RESULT_EXTRA: f32 = 117.0;
pub(crate) const PICKER_EXTRA: f32 = 645.0;
pub(crate) const CLOUD_KEY_EXTRA: f32 = 278.0;
pub(crate) const SETTINGS_EXTRA: f32 = 373.0;
pub(crate) const LANGUAGE_EXTRA: f32 = 393.0;
pub(crate) const MICROPHONE_EXTRA: f32 = LANGUAGE_EXTRA;
pub(crate) const ERROR_EXTRA: f32 = 100.0;
pub(crate) const UPDATE_EXTRA: f32 = 224.0;

/// Waveform bars across the bar while recording, oldest on the left.
pub(crate) const BARS: usize = 19;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reveal {
    Picker,
    Result,
    Settings,
    Update,
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Recovery {
    Microphone,
    MicrophoneDisconnected,
    MicrophonePermission,
    Model,
    NoSpeech,
    CloudOffline,
    CloudKey(Provider),
    CloudRateLimited,
    CloudOther,
    LocalFallback,
    UpdateCheck,
}

impl Recovery {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Microphone => "Microphone unavailable",
            Self::MicrophoneDisconnected => "Microphone disconnected",
            Self::MicrophonePermission => "Microphone access blocked",
            Self::Model => "No ready model",
            Self::NoSpeech => "No speech detected",
            Self::CloudOffline => "Cloud unavailable",
            Self::CloudKey(_) => "API key rejected",
            Self::CloudRateLimited => "Too many requests",
            Self::CloudOther => "Cloud transcription failed",
            Self::LocalFallback => "Local transcription failed",
            Self::UpdateCheck => "Update check failed",
        }
    }
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
    pub hud_window: AnyWindowHandle,
    pub phase: Phase,
    pub levels: VecDeque<f32>,
    pub picker_open: bool,
    pub cloud_config: Option<Provider>,
    pub cloud_keys: [bool; 2],
    pub key_input: Option<Entity<InputState>>,
    pub key_visible: bool,
    pub models: Vec<InstalledModel>,
    pub selected: String,
    pub download: Option<Download>,
    pub pending_uninstall: Option<String>,
    pub error: Option<String>,
    pub recovery: Option<Recovery>,
    /// Retained only after a cloud failure; never written to disk.
    failed_audio: Option<(Arc<Vec<f32>>, u32)>,
    pub transcribing_provider: Option<Provider>,
    pub copied: bool,
    pub chrome: Ease,
    pub press_id: Option<String>,
    pub press: Ease,
    pub bars: Vec<Spring>,
    pub reveal: Option<Reveal>,
    pub picker_opened_at: Option<Instant>,
    pub settings_open: bool,
    settings_window: Option<crate::settings_window::SettingsHandle>,
    pub license_access: Access,
    license_checking: bool,
    license_generation: u64,
    license_initial_check_done: bool,
    last_license_check: Instant,
    transcription_id: u64,
    pub settings_page: SettingsPage,
    pub settings_opened_at: Option<Instant>,
    pub language: String,
    pub show_hotkey: String,
    pub copy_notes: bool,
    #[cfg(target_os = "macos")]
    dictation_target: Option<dictation::Target>,
    pub clean_fillers: bool,
    pub input_device: String,
    pub open_on_startup: bool,
    #[cfg(target_os = "macos")]
    update: Option<PreparedUpdate>,
    #[cfg(target_os = "macos")]
    pub(crate) update_prompt: Option<UpdatePrompt>,
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
            .filter(|id| models::spec(id).is_some() || Provider::from_id(id).is_some())
            .unwrap_or_else(|| models::recommended_id().to_string());
        let cloud_keys = Provider::ALL.map(|provider| cloud::has_key(provider).unwrap_or(false));
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
            hud_window: window.window_handle(),
            phase: Phase::Idle,
            levels: VecDeque::new(),
            picker_open: false,
            cloud_config: None,
            cloud_keys,
            key_input: None,
            key_visible: false,
            models,
            selected,
            download: None,
            pending_uninstall: None,
            error: hotkey_error,
            recovery: None,
            failed_audio: None,
            transcribing_provider: None,
            copied: false,
            chrome: Ease::chrome(COLLAPSED_HEIGHT),
            press_id: None,
            press: Ease::press(0.0),
            bars: vec![Spring::level(0.08); BARS],
            reveal: None,
            picker_opened_at: None,
            settings_open: false,
            settings_window: None,
            license_access: Access::Checking,
            license_checking: false,
            license_generation: 0,
            license_initial_check_done: false,
            last_license_check: Instant::now(),
            transcription_id: 0,
            settings_page: SettingsPage::Main,
            settings_opened_at: None,
            language: prefs.language,
            show_hotkey: prefs.show_hotkey,
            copy_notes: prefs.copy_notes,
            #[cfg(target_os = "macos")]
            dictation_target: None,
            clean_fillers: prefs.clean_fillers,
            input_device: prefs.input_device,
            open_on_startup: prefs.open_on_startup,
            #[cfg(target_os = "macos")]
            update: None,
            #[cfg(target_os = "macos")]
            update_prompt: updater::just_updated().then_some(UpdatePrompt::JustUpdated),
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
        view.refresh_license(cx);
        #[cfg(target_os = "macos")]
        view.check_for_updates(cx);
        view
    }

    pub(crate) fn tick(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        if let Some(prompt) = self.update_prompt {
            self.update_prompt = Some(prompt.after_recording(self.visibility_locked()));
        }
        self.expire_trial_if_needed(cx);
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

        if let Some(reveal) = self.desired_reveal() {
            self.reveal = Some(reveal);
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
        let mut height = COLLAPSED_HEIGHT + self.panel_height(self.desired_reveal());
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
            Some(Reveal::Update) => UPDATE_EXTRA,
            Some(Reveal::Picker) if self.cloud_config.is_some() => CLOUD_KEY_EXTRA,
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
                        let pressed = hotkey::take_press();
                        if pressed {
                            view.set_visible(!view.bar_visible, window, cx);
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
                                    if view.update.is_some() && !view.visibility_locked() {
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
                        if view.last_license_check.elapsed() >= Duration::from_secs(6 * 60 * 60) {
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
        #[cfg(target_os = "macos")]
        let was_visible = self.bar_visible;
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
            // Capture the editor before the HUD takes keyboard focus.
            #[cfg(target_os = "macos")]
            if !was_visible {
                self.dictation_target = dictation::Target::focused();
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

    fn update_panel_visible(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.update_prompt.is_some() && !self.visibility_locked()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    fn desired_reveal(&self) -> Option<Reveal> {
        #[cfg(target_os = "macos")]
        let update_requested = self.update_prompt == Some(UpdatePrompt::Confirm);
        #[cfg(not(target_os = "macos"))]
        let update_requested = false;
        if self.settings_open {
            Some(Reveal::Settings)
        } else if self.picker_open {
            Some(Reveal::Picker)
        } else if update_requested && self.update_panel_visible() {
            Some(Reveal::Update)
        } else if matches!(self.phase, Phase::Result(_)) {
            Some(Reveal::Result)
        } else if self.update_panel_visible() {
            Some(Reveal::Update)
        } else {
            None
        }
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

    pub(crate) fn toggle_listen(&mut self, cx: &mut Context<Self>) {
        if self.recording_hotkey || self.actions_suppressed() {
            return;
        }
        if matches!(self.phase, Phase::Idle | Phase::Result(_)) && !self.require_license(cx) {
            return;
        }
        self.error = None;
        self.recovery = None;
        self.copied = false;
        self.close_settings();
        if !self.selected_ready() {
            self.picker_open = true;
            self.picker_opened_at = None;
            self.error = Some("Choose a ready model before recording.".into());
            self.recovery = Some(Recovery::Model);
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
                    self.failed_audio = None;
                    #[cfg(target_os = "macos")]
                    if self.dictation_target.is_none() {
                        dictation::request_access();
                        self.dictation_target = dictation::Target::focused();
                    }
                    self.levels.clear();
                    self.rest_bars();
                    self.phase = Phase::Listening(mic);
                    self.listen_started = Some(Instant::now());
                    self.picker_open = false;
                    self.picker_opened_at = None;
                    self.snap_chrome();
                }
                Err(err) => {
                    self.recovery = Some(
                        if err.to_ascii_lowercase().contains("permission")
                            || err.to_ascii_lowercase().contains("denied")
                        {
                            Recovery::MicrophonePermission
                        } else if !self.input_device.is_empty()
                            && audio::input_names().is_ok_and(|names| {
                                audio::known_input(&self.input_device, &names).is_err()
                            })
                        {
                            Recovery::MicrophoneDisconnected
                        } else {
                            Recovery::Microphone
                        },
                    );
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
        if !self.require_license(cx) {
            return;
        }
        self.error = None;
        self.recovery = None;
        self.copied = false;
        if !self.selected_ready() {
            self.error = Some("Choose a ready model first.".into());
            self.recovery = Some(Recovery::Model);
            cx.notify();
            return;
        }
        if matches!(self.phase, Phase::Listening(_) | Phase::Transcribing) {
            cx.notify();
            return;
        }
        match audio::decode_wav(include_bytes!("../assets/sample.wav")) {
            Ok((samples, rate)) => {
                self.failed_audio = None;
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
        self.recovery = None;
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

    pub(crate) fn choose_cloud(&mut self, provider: Provider, cx: &mut Context<Self>) {
        if !self.cloud_keys[provider.index()] {
            self.open_cloud_config(provider, cx);
            return;
        }
        self.selected = provider.id().to_string();
        self.pending_uninstall = None;
        self.error = None;
        self.recovery = None;
        self.picker_open = false;
        self.cloud_config = None;
        self.persist();
        cx.notify();
    }

    pub(crate) fn open_cloud_config(&mut self, provider: Provider, cx: &mut Context<Self>) {
        self.stop_recording();
        self.cloud_config = Some(provider);
        self.key_input = None;
        self.key_visible = false;
        self.picker_open = true;
        self.error = None;
        self.recovery = None;
        cx.notify();
    }

    pub(crate) fn back_from_cloud_config(&mut self, cx: &mut Context<Self>) {
        self.cloud_config = None;
        self.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_key_visibility(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        self.key_visible = !self.key_visible;
        if let Some(input) = &self.key_input {
            input.update(cx, |input, cx| {
                input.set_masked(!self.key_visible, window, cx)
            });
        }
        cx.notify();
    }

    pub(crate) fn save_cloud_key(&mut self, cx: &mut Context<Self>) {
        let Some(provider) = self.cloud_config else {
            return;
        };
        let key = self
            .key_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        if key.trim().is_empty() && self.cloud_keys[provider.index()] {
            self.choose_cloud(provider, cx);
            return;
        }
        match cloud::save_key(provider, &key) {
            Ok(()) => {
                self.cloud_keys[provider.index()] = true;
                self.key_input = None;
                self.choose_cloud(provider, cx);
                if self.failed_audio.is_some() {
                    self.error =
                        Some("Key updated. Retry your recording or use local Turbo.".into());
                    self.recovery = Some(Recovery::CloudOther);
                    self.snap_chrome();
                }
            }
            Err(err) => {
                self.error = Some(err);
                cx.notify();
            }
        }
    }

    pub(crate) fn remove_cloud_key(&mut self, cx: &mut Context<Self>) {
        let Some(provider) = self.cloud_config else {
            return;
        };
        match cloud::delete_key(provider) {
            Ok(()) => {
                self.cloud_keys[provider.index()] = false;
                if self.selected == provider.id() {
                    self.selected = models::recommended_id().to_string();
                    self.persist();
                }
                self.cloud_config = None;
                self.error = None;
            }
            Err(err) => self.error = Some(err),
        }
        cx.notify();
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
        self.cloud_config = None;
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
        self.cloud_config = None;
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

    pub(crate) fn open_settings_window(&mut self, cx: &mut Context<Self>) {
        self.open_settings_window_at(false, cx);
    }

    pub(crate) fn open_license_window(&mut self, cx: &mut Context<Self>) {
        self.open_settings_window_at(true, cx);
    }

    fn expire_trial_if_needed(&mut self, cx: &mut Context<Self>) -> bool {
        if matches!(&self.license_access, Access::Trial { .. }) && !self.license_access.allowed() {
            self.set_license_access(Access::TrialExpired, cx);
            self.open_license_window(cx);
            return true;
        }
        false
    }

    fn open_settings_window_at(&mut self, license_page: bool, cx: &mut Context<Self>) {
        self.stop_recording();
        self.settings_open = false;
        self.settings_opened_at = None;
        self.settings_page = SettingsPage::Main;
        self.snap_chrome();
        let hud = cx.entity();
        let existing = self.settings_window.clone();
        cx.defer(move |cx| {
            let handle = crate::settings_window::open(cx, hud.clone(), existing, license_page);
            hud.update(cx, |view, cx| {
                view.settings_window = Some(handle);
                cx.notify();
            });
        });
        cx.notify();
    }

    fn require_license(&mut self, cx: &mut Context<Self>) -> bool {
        if self.expire_trial_if_needed(cx) {
            return false;
        }
        if self.license_access.allowed() {
            return true;
        }
        self.error = Some(match &self.license_access {
            Access::Checking => "Checking your license. Try again in a moment.".into(),
            Access::Blocked { reason, .. } | Access::Unavailable { reason, .. } => reason.clone(),
            _ => "Enter your license key to use Whisple.".into(),
        });
        self.open_settings_window_at(true, cx);
        false
    }

    pub(crate) fn set_license_access(&mut self, access: Access, cx: &mut Context<Self>) {
        let show_license = !self.license_initial_check_done && !access.allowed();
        self.license_initial_check_done = true;
        if !access.allowed() && matches!(self.phase, Phase::Listening(_) | Phase::Transcribing) {
            self.transcription_id = self.transcription_id.wrapping_add(1);
            self.phase = Phase::Idle;
            self.failed_audio = None;
            self.transcribing_provider = None;
            self.listen_started = None;
            self.levels.clear();
            self.rest_bars();
            #[cfg(target_os = "macos")]
            {
                self.dictation_target = None;
            }
            self.snap_chrome();
        }
        let previous_license_error = match &self.license_access {
            Access::Blocked { reason, .. } | Access::Unavailable { reason, .. } => {
                self.error.as_deref() == Some(reason.as_str())
            }
            _ => matches!(
                self.error.as_deref(),
                Some("Checking your license. Try again in a moment.")
                    | Some("Enter your license key to use Whisple.")
            ),
        };
        match &access {
            Access::Blocked { reason, .. } | Access::Unavailable { reason, .. } => {
                self.error = Some(reason.clone());
            }
            _ if previous_license_error => self.error = None,
            _ => {}
        }
        self.license_access = access;
        self.license_checking = false;
        self.license_generation = self.license_generation.wrapping_add(1);
        self.last_license_check = Instant::now();
        cx.notify();
        if show_license {
            self.open_settings_window_at(true, cx);
        }
    }

    pub(crate) fn refresh_license(&mut self, cx: &mut Context<Self>) {
        if self.license_checking {
            return;
        }
        self.license_checking = true;
        self.last_license_check = Instant::now();
        let generation = self.license_generation;
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let access = cx
                .background_executor()
                .spawn(async { license::check_saved() })
                .await;
            this.update(cx, |view, cx| {
                if view.license_generation == generation {
                    view.set_license_access(access, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn persist_settings(&self) {
        self.persist();
    }

    pub(crate) fn remove_cloud_key_from_settings(
        &mut self,
        provider: Provider,
        cx: &mut Context<Self>,
    ) {
        self.cloud_keys[provider.index()] = false;
        if self.selected == provider.id() {
            self.selected = models::recommended_id().to_string();
            self.persist();
        }
        cx.notify();
    }

    pub(crate) fn check_updates_from_settings(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        self.check_for_updates(cx);
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
    }

    pub(crate) fn update_summary(&self) -> String {
        #[cfg(target_os = "macos")]
        {
            if self.update_checking {
                return "Checking for updates…".into();
            }
            if let Some(update) = &self.update {
                return format!("Whisple v{} is ready to install", update.version);
            }
        }
        format!("Version {}", env!("CARGO_PKG_VERSION"))
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn update_details(&self) -> (String, String) {
        self.update
            .as_ref()
            .map(|update| (update.version.to_string(), update.notes.clone()))
            .unwrap_or_default()
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn dismiss_update(&mut self, cx: &mut Context<Self>) {
        self.update_prompt = None;
        cx.notify();
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn review_update(&mut self, cx: &mut Context<Self>) {
        if self.update.is_some() {
            self.update_prompt = Some(UpdatePrompt::Confirm);
            cx.notify();
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn install_update(&mut self, cx: &mut Context<Self>) {
        let busy = self.visibility_locked();
        if busy {
            self.update_prompt = Some(UpdatePrompt::AfterRecording);
            cx.notify();
            return;
        }
        if !self
            .update_prompt
            .is_some_and(|prompt| prompt.may_install(busy))
        {
            return;
        }
        if let Some(update) = self.update.as_mut() {
            match update.install() {
                Ok(()) => cx.quit(),
                Err(err) => {
                    self.error = Some(format!("Could not install the update: {err}"));
                    cx.notify();
                }
            }
        }
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
        self.error = None;
        self.recovery = None;
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
        if self.picker_open && self.cloud_config.is_some() {
            self.back_from_cloud_config(cx);
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
        self.recovery = None;
        self.failed_audio = None;
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
        self.transcribe_audio(Arc::new(samples), rate, None, false, cx);
    }

    fn transcribe_audio(
        &mut self,
        samples: Arc<Vec<f32>>,
        rate: u32,
        local_override: Option<&'static ModelSpec>,
        retry: bool,
        cx: &mut Context<Self>,
    ) {
        if self.expire_trial_if_needed(cx) {
            return;
        }
        self.transcription_id = self.transcription_id.wrapping_add(1);
        let transcription_id = self.transcription_id;
        let provider = local_override
            .is_none()
            .then(|| Provider::from_id(&self.selected))
            .flatten();
        self.transcribing_provider = provider;
        let local = provider.is_none().then(|| {
            let spec = local_override.unwrap_or_else(|| self.selected_spec());
            (spec.id.to_string(), models::model_path(spec))
        });
        let language = settings::whisper_language(&self.language).map(str::to_string);
        let clean = self.clean_fillers;
        let copy = self.copy_notes;
        #[cfg(target_os = "macos")]
        let target = self.dictation_target.take();
        let task_samples = Arc::clone(&samples);
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    if let Some(provider) = provider {
                        cloud::transcribe(provider, &task_samples, rate, language.as_deref(), clean)
                            .map_err(|err| (err.message().to_string(), Some(err)))
                    } else {
                        let (model_id, path) = local.expect("local model was selected");
                        stt::transcribe(
                            &model_id,
                            &path,
                            &task_samples,
                            rate,
                            language.as_deref(),
                            clean,
                        )
                        .map_err(|err| (err, None))
                    }
                })
                .await;
            this.update(cx, |view, cx| {
                if view.transcription_id != transcription_id {
                    return;
                }
                if !view.license_access.allowed() {
                    view.expire_trial_if_needed(cx);
                    return;
                }
                view.transcribing_provider = None;
                match outcome {
                    Ok(text) => {
                        view.failed_audio = None;
                        view.recovery = None;
                        #[cfg(target_os = "macos")]
                        let insert_error = target.and_then(|target| target.insert(&text).err());
                        if copy {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                            view.copied = true;
                        } else {
                            view.copied = false;
                        }
                        view.last_text.clone_from(&text);
                        view.phase = Phase::Result(text);
                        #[cfg(target_os = "macos")]
                        {
                            view.error = insert_error;
                        }
                        #[cfg(not(target_os = "macos"))]
                        {
                            view.error = None;
                        }
                    }
                    Err((err, cloud_error)) => {
                        view.phase = Phase::Idle;
                        view.recovery = Some(match cloud_error {
                            Some(cloud::TranscriptionError::Offline(_)) => Recovery::CloudOffline,
                            Some(cloud::TranscriptionError::Unauthorized(_)) => {
                                Recovery::CloudKey(provider.expect("cloud request"))
                            }
                            Some(cloud::TranscriptionError::RateLimited(_)) => {
                                Recovery::CloudRateLimited
                            }
                            Some(cloud::TranscriptionError::NoSpeech(_)) => Recovery::NoSpeech,
                            Some(cloud::TranscriptionError::Other(_)) => Recovery::CloudOther,
                            None if err == "No speech came through."
                                || err == "That clip was too short to transcribe." =>
                            {
                                Recovery::NoSpeech
                            }
                            None if retry && local_override.is_some() => Recovery::LocalFallback,
                            None => Recovery::Model,
                        });
                        view.failed_audio = ((provider.is_some() || retry)
                            && !matches!(view.recovery, Some(Recovery::NoSpeech)))
                        .then_some((samples, rate));
                        view.error = Some(err);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.recovery = None;
        self.snap_chrome();
        cx.notify();
    }

    pub(crate) fn failed_audio_available(&self) -> bool {
        self.failed_audio.is_some()
    }

    pub(crate) fn retry_audio(&mut self, local: bool, cx: &mut Context<Self>) {
        if !self.require_license(cx) || !matches!(self.phase, Phase::Idle) {
            return;
        }
        let Some((samples, rate)) = self.failed_audio.take() else {
            return;
        };
        let local_override = if local {
            let Some(spec) = self
                .models
                .iter()
                .find(|model| model.ready && model.spec.id.starts_with("turbo"))
                .map(|model| model.spec)
            else {
                self.failed_audio = Some((samples, rate));
                return;
            };
            Some(spec)
        } else {
            None
        };
        self.error = None;
        self.recovery = None;
        self.phase = Phase::Transcribing;
        self.transcribe_audio(samples, rate, local_override, true, cx);
        self.snap_chrome();
        cx.notify();
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

    pub(crate) fn cancel_hotkey_capture(&mut self, cx: &mut Context<Self>) {
        if self.recording_hotkey {
            self.stop_recording();
            self.error = None;
            cx.notify();
        }
    }

    fn actions_suppressed(&self) -> bool {
        self.suppress_actions_until
            .is_some_and(|until| Instant::now() < until)
    }

    fn persist(&self) {
        settings::save(&Preferences {
            onboarding_complete: settings::load().onboarding_complete,
            selected: self.selected.clone(),
            language: self.language.clone(),
            show_hotkey: self.show_hotkey.clone(),
            copy_notes: self.copy_notes,
            clean_fillers: self.clean_fillers,
            input_device: self.input_device.clone(),
            open_on_startup: self.open_on_startup,
            show_in_menu_bar: settings::load().show_in_menu_bar,
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
                        view.update_prompt = Some(UpdatePrompt::Ready);
                        if view.recovery == Some(Recovery::UpdateCheck) {
                            view.error = None;
                            view.recovery = None;
                        }
                        false
                    }
                    Ok(None) => {
                        if view.recovery == Some(Recovery::UpdateCheck) {
                            view.error = None;
                            view.recovery = None;
                        }
                        false
                    }
                    Err(err) => {
                        eprintln!("Whisple update check failed: {err}");
                        view.error = Some(format!("Update check failed: {err}"));
                        view.recovery = Some(Recovery::UpdateCheck);
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
    fn show_or_check_for_updates(&mut self, cx: &mut Context<Self>) {
        if self.update.is_none() {
            self.check_for_updates(cx);
            return;
        }
        let busy = self.visibility_locked();
        self.update_prompt = Some(if busy {
            UpdatePrompt::AfterRecording
        } else {
            UpdatePrompt::Confirm
        });
        if busy {
            self.error =
                Some("Update ready. Confirm installation after recording finishes.".into());
        }
        cx.notify();
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
