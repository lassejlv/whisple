use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{
    AnyWindowHandle, App, ClipboardItem, Context, FocusHandle, Focusable, KeyBinding, Keystroke,
    Menu, MenuItem, WeakEntity,
};

use crate::assistant::{self, ErrorKind, Outcome, Route};
use crate::audio::{self, Mic};
use crate::cloud::{self, Provider};
#[cfg(target_os = "macos")]
use crate::dictation;
use crate::hotkey::{self, Shortcut};
use crate::i18n::{t, tf};
use crate::license::{self, Access};
use crate::models::{self, ModelSpec};
use crate::motion::{Ease, Spring};
use crate::place;
use crate::screen::{self, Snapshot};
use crate::settings::{self, Preferences};
use crate::settings_window::SettingsTarget;
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
/// The one-line notice under the bar: an error or a ready update.
pub(crate) const NOTICE_EXTRA: f32 = 52.0;
/// The model dropdown: rows, the divider above "Manage models…", and padding.
pub(crate) const MENU_ROW: f32 = 36.0;
pub(crate) const MENU_PAD: f32 = 6.0;
pub(crate) const MENU_DIVIDER: f32 = 9.0;

/// Waveform bars across the bar while recording, oldest on the left.
pub(crate) const BARS: usize = 19;
/// One line of text in the result panel.
pub(crate) const RESULT_LINE: f32 = 22.0;
/// Assistant answers grow the result panel up to this many lines.
const MAX_ANSWER_LINES: usize = 6;
/// Roughly how many characters fit on a result line.
const LINE_CHARS: usize = 46;

/// Where an in-flight transcript goes after dictation: typed, or handed to
/// a command or the assistant. The macOS editor to type into travels along.
#[cfg(target_os = "macos")]
type TypingTarget = Option<dictation::Target>;
#[cfg(not(target_os = "macos"))]
type TypingTarget = ();

/// What the result panel is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResultKind {
    Dictation,
    /// A note translated into this language.
    Translated(&'static str),
    /// A voice command's confirmation, like "Opened Spotify".
    Command,
    Answer(Provider),
    /// Text the assistant wrote for the cursor.
    Typed(Provider),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reveal {
    Menu,
    Result,
}

/// A model the dropdown can switch to right away.
pub(crate) struct MenuChoice {
    pub id: &'static str,
    pub name: &'static str,
    pub detail: &'static str,
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
    Command,
    AssistantKey,
    Assistant,
}

impl Recovery {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Microphone => t("Microphone unavailable"),
            Self::MicrophoneDisconnected => t("Microphone disconnected"),
            Self::MicrophonePermission => t("Microphone access blocked"),
            Self::Model => t("No ready model"),
            Self::NoSpeech => t("No speech detected"),
            Self::CloudOffline => t("Cloud unavailable"),
            Self::CloudKey(_) => t("API key rejected"),
            Self::CloudRateLimited => t("Too many requests"),
            Self::CloudOther => t("Cloud transcription failed"),
            Self::LocalFallback => t("Local transcription failed"),
            Self::Command => t("Could not open that"),
            Self::AssistantKey => t("The assistant needs a cloud key"),
            Self::Assistant => t("Whisple could not answer"),
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
    pub menu_open: bool,
    pub cloud_keys: [bool; Provider::ALL.len()],
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
    pub result_kind: ResultKind,
    /// The bar's label while a command or the assistant works, such as
    /// "Thinking…". `None` while transcribing.
    pub working: Option<&'static str>,
    pub chrome: Ease,
    pub press_id: Option<String>,
    pub press: Ease,
    pub bars: Vec<Spring>,
    pub reveal: Option<Reveal>,
    settings_window: Option<crate::settings_window::SettingsHandle>,
    pub license_access: Access,
    license_checking: bool,
    license_generation: u64,
    last_license_check: Instant,
    transcription_id: u64,
    pub language: String,
    /// The language notes come out in. Empty means the spoken language.
    pub output_language: String,
    pub show_hotkey: String,
    /// Shows the bar and starts recording. Empty when turned off.
    pub record_hotkey: String,
    pub copy_notes: bool,
    #[cfg(target_os = "macos")]
    dictation_target: Option<dictation::Target>,
    pub clean_fillers: bool,
    pub voice_commands: bool,
    pub screen_context: bool,
    /// The app and window the user was in when the bar opened.
    screen: Snapshot,
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
    /// The shortcut Settings is waiting for a new chord for.
    pub recording_hotkey: Option<Shortcut>,
    /// When the current recording started, for the bar's timer.
    pub listen_started: Option<Instant>,
    /// Length of the audio behind the last transcript.
    pub recorded: Duration,
    /// The last transcript, kept so the result panel can slide shut showing
    /// it after a new recording has already replaced the phase.
    pub last_text: String,
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
            license_access: Access::Checking,
            license_checking: false,
            license_generation: 0,
            last_license_check: Instant::now(),
            transcription_id: 0,
            language: prefs.language,
            output_language: prefs.output_language,
            show_hotkey: prefs.show_hotkey,
            record_hotkey: prefs.record_hotkey,
            copy_notes: prefs.copy_notes,
            #[cfg(target_os = "macos")]
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
            update_prompt: updater::just_updated().then_some(UpdatePrompt::JustUpdated),
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
        // A hidden window need not render, so start listening at creation.
        view.listen_for_commands(window.window_handle(), cx);
        view.refresh_license(cx);
        #[cfg(target_os = "macos")]
        view.check_for_updates(cx);
        view
    }

    pub(crate) fn tick(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
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

    /// The settled height of a panel above the bar, hairline included.
    pub(crate) fn panel_height(&self, reveal: Option<Reveal>) -> f32 {
        match reveal {
            Some(Reveal::Result) => RESULT_EXTRA + (self.result_lines() - 2) as f32 * RESULT_LINE,
            Some(Reveal::Menu) => menu_height(self.menu_choices().len()),
            None => 0.0,
        }
    }

    /// Lines of text the result panel shows: two for a note, more for an
    /// assistant's answer.
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

    /// Time on the running recording, if one is running.
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
                        let license_interval = if matches!(
                            view.license_access,
                            Access::Trial { .. } | Access::Unavailable { .. }
                        ) {
                            Duration::from_secs(5 * 60)
                        } else {
                            Duration::from_secs(6 * 60 * 60)
                        };
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
                #[cfg(target_os = "macos")]
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

    /// The update line shows only while the bar is free; an error wins.
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

    /// Cloud providers with a saved key, then ready local models.
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

    /// The record shortcut: opens the bar if needed and starts recording, or
    /// stops the running recording and transcribes it.
    fn record_from_shortcut(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if self.recording_hotkey.is_some() || matches!(self.phase, Phase::Transcribing) {
            return;
        }
        if !self.bar_visible {
            self.set_visible(true, window, cx);
        }
        self.toggle_listen(cx);
    }

    pub(crate) fn toggle_listen(&mut self, cx: &mut Context<Self>) {
        if self.recording_hotkey.is_some() || self.actions_suppressed() {
            return;
        }
        if matches!(self.phase, Phase::Idle | Phase::Result(_)) && !self.require_license(cx) {
            return;
        }
        self.error = None;
        self.recovery = None;
        self.copied = false;
        self.stop_recording();
        if !self.selected_ready() {
            self.show_model_choices(cx);
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
                    self.menu_open = false;
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
                    self.snap_chrome();
                }
            },
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
            self.menu_open = false;
            self.snap_chrome();
            cx.notify();
            return;
        }
        self.start_download(spec, cx);
    }

    pub(crate) fn choose_cloud(&mut self, provider: Provider, cx: &mut Context<Self>) {
        if !self.cloud_keys[provider.index()] {
            self.open_settings_at(SettingsTarget::CloudKey(provider), cx);
            return;
        }
        self.selected = provider.id().to_string();
        self.pending_uninstall = None;
        self.menu_open = false;
        // A saved key after a rejected one: offer the kept recording again.
        if matches!(self.recovery, Some(Recovery::CloudKey(_))) && self.failed_audio.is_some() {
            self.error = Some(tf(
                "{} key saved. Retry your recording.",
                &[&provider.name()],
            ));
            self.recovery = Some(Recovery::CloudOther);
        } else {
            self.error = None;
            self.recovery = None;
        }
        self.persist();
        self.snap_chrome();
        cx.notify();
    }

    /// Picks a ready model from the dropdown.
    pub(crate) fn choose_from_menu(&mut self, id: &str, cx: &mut Context<Self>) {
        match Provider::from_id(id) {
            Some(provider) => self.choose_cloud(provider, cx),
            None => self.choose_model(id, cx),
        }
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
            self.error =
                Some(t("Finish recording or downloading before removing this model.").into());
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

    /// The model capsule: a dropdown of ready models, or Settings when none
    /// is ready yet.
    pub(crate) fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        self.stop_recording();
        if self.menu_open {
            self.menu_open = false;
        } else {
            self.show_model_choices(cx);
            return;
        }
        cx.notify();
    }

    fn show_model_choices(&mut self, cx: &mut Context<Self>) {
        if self.menu_choices().is_empty() {
            self.menu_open = false;
            self.open_settings_at(SettingsTarget::Models, cx);
            return;
        }
        self.menu_open = true;
        cx.notify();
    }

    /// Closes the bar and Settings and walks through onboarding again. It
    /// opens a fresh bar when it finishes.
    pub(crate) fn restart_onboarding(&mut self, cx: &mut Context<Self>) {
        if self.visibility_locked() {
            self.error = Some(t("Finish recording before setting up again.").into());
            self.snap_chrome();
            cx.notify();
            return;
        }
        self.stop_recording();
        self.cancel_download(cx);
        // Drop any result still in flight; this bar is about to close.
        self.transcription_id = self.transcription_id.wrapping_add(1);
        let hud = self.hud_window;
        let settings = self.settings_window.take();
        cx.defer(move |cx| {
            // Open onboarding first so the app always has a window.
            crate::onboarding::open(cx);
            if let Some(settings) = settings {
                settings.close(cx);
            }
            hud.update(cx, |_, window, _| window.remove_window()).ok();
        });
    }

    pub(crate) fn open_settings_window(&mut self, cx: &mut Context<Self>) {
        self.open_settings_at(SettingsTarget::General, cx);
    }

    pub(crate) fn open_license_window(&mut self, cx: &mut Context<Self>) {
        self.open_settings_at(SettingsTarget::License, cx);
    }

    /// Ends the trial the moment its time is up. Nothing opens by itself: the
    /// bar shows the ended trial and Unlock leads to the License page.
    fn expire_trial_if_needed(&mut self, cx: &mut Context<Self>) -> bool {
        if matches!(&self.license_access, Access::Trial { .. }) && !self.license_access.allowed() {
            self.set_license_access(Access::TrialExpired, cx);
            return true;
        }
        false
    }

    /// Dictation is locked: the trial ended or the license needs attention.
    pub(crate) fn locked(&self) -> bool {
        !matches!(self.license_access, Access::Checking) && !self.license_access.allowed()
    }

    /// Trial time left, but only on the last day, when the bar mentions it.
    pub(crate) fn trial_ending(&self) -> Option<Duration> {
        self.license_access
            .trial_remaining()
            .filter(|left| !left.is_zero() && *left < Duration::from_secs(24 * 60 * 60))
    }

    pub(crate) fn open_settings_at(&mut self, target: SettingsTarget, cx: &mut Context<Self>) {
        self.stop_recording();
        self.menu_open = false;
        self.snap_chrome();
        let hud = cx.entity();
        let existing = self.settings_window.clone();
        cx.defer(move |cx| {
            let handle = crate::settings_window::open(cx, hud.clone(), existing, target);
            hud.update(cx, |view, cx| {
                view.settings_window = Some(handle);
                cx.notify();
            });
        });
        cx.notify();
    }

    fn require_license(&mut self, cx: &mut Context<Self>) -> bool {
        self.expire_trial_if_needed(cx);
        if self.license_access.allowed() {
            return true;
        }
        if matches!(self.license_access, Access::Checking) {
            self.error = Some(t("Checking your license. Try again in a moment.").into());
            self.snap_chrome();
            cx.notify();
            return false;
        }
        // Pressing record on a locked bar is the one place that leads to
        // buying or fixing the license.
        self.open_license_window(cx);
        false
    }

    pub(crate) fn set_license_access(&mut self, access: Access, cx: &mut Context<Self>) {
        if !access.allowed() && matches!(self.phase, Phase::Listening(_) | Phase::Transcribing) {
            self.transcription_id = self.transcription_id.wrapping_add(1);
            self.phase = Phase::Idle;
            self.failed_audio = None;
            self.transcribing_provider = None;
            self.working = None;
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
            _ => self.error.as_deref() == Some(t("Checking your license. Try again in a moment.")),
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
        self.snap_chrome();
        cx.notify();
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

    pub(crate) fn set_input_device(&mut self, name: &str, cx: &mut Context<Self>) {
        self.input_device = name.to_string();
        self.persist();
        cx.notify();
    }

    /// A key was just saved for `provider`: switch to it.
    pub(crate) fn use_saved_cloud_key(&mut self, provider: Provider, cx: &mut Context<Self>) {
        self.cloud_keys[provider.index()] = true;
        self.choose_cloud(provider, cx);
    }

    /// The key for `provider` was removed: fall back to the recommended model.
    pub(crate) fn forget_cloud_key(&mut self, provider: Provider, cx: &mut Context<Self>) {
        self.cloud_keys[provider.index()] = false;
        if self.selected == provider.id() {
            self.selected = models::recommended_id().to_string();
            self.persist();
        }
        cx.notify();
    }

    pub(crate) fn check_for_updates_now(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        self.check_for_updates(cx);
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
    }

    pub(crate) fn update_summary(&self) -> String {
        #[cfg(target_os = "macos")]
        {
            if self.update_checking {
                return t("Checking for updates…").into();
            }
            if let Some(update) = &self.update {
                return tf("Whisple v{} is ready to install", &[&update.version]);
            }
        }
        tf("Version {}", &[&env!("CARGO_PKG_VERSION")])
    }

    /// The version of a downloaded, verified update waiting to install.
    pub(crate) fn ready_update(&self) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            self.update
                .as_ref()
                .map(|update| update.version.to_string())
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    pub(crate) fn dismiss_update(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            self.update_prompt = None;
        }
        self.snap_chrome();
        cx.notify();
    }

    /// Installs and restarts. Never while a recording is running.
    pub(crate) fn install_update(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        if !self.visibility_locked() {
            if let Some(update) = self.update.as_mut() {
                match update.install() {
                    Ok(()) => cx.quit(),
                    Err(err) => {
                        self.error = Some(tf("Could not install the update: {}", &[&err]));
                        self.snap_chrome();
                    }
                }
            }
        }
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
        cx.notify();
    }

    /// Sets the language notes come out in; empty keeps the spoken one.
    pub(crate) fn choose_output_language(&mut self, id: &str, cx: &mut Context<Self>) {
        if !id.is_empty() && settings::language_name(id).is_none() {
            return;
        }
        self.output_language = id.to_string();
        self.persist();
        cx.notify();
    }

    /// The language to translate notes into, unless it is the one spoken.
    fn translation_target(&self) -> Option<&'static str> {
        (self.output_language != self.language)
            .then(|| settings::language_name(&self.output_language))
            .flatten()
    }

    pub(crate) fn toggle_voice_commands(&mut self, cx: &mut Context<Self>) {
        self.voice_commands = !self.voice_commands;
        self.persist();
        cx.notify();
    }

    pub(crate) fn toggle_screen_context(&mut self, cx: &mut Context<Self>) {
        self.screen_context = !self.screen_context;
        if !self.screen_context {
            self.screen = Snapshot::default();
        }
        self.persist();
        cx.notify();
    }

    /// The cloud provider the assistant asks: the selected one when it is a
    /// cloud model, otherwise any with a saved key.
    fn assistant_provider(&self) -> Option<Provider> {
        Provider::from_id(&self.selected)
            .filter(|provider| self.cloud_keys[provider.index()])
            .or_else(|| {
                Provider::ALL
                    .into_iter()
                    .find(|provider| self.cloud_keys[provider.index()])
            })
    }

    pub(crate) fn begin_hotkey_capture(&mut self, slot: Shortcut, cx: &mut Context<Self>) {
        if self.recording_hotkey == Some(slot) {
            self.stop_recording();
        } else {
            self.recording_hotkey = Some(slot);
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
        let Some(slot) = self.recording_hotkey else {
            return;
        };
        if keystroke.key == "escape" {
            self.recording_hotkey = None;
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
            self.error = Some(t("Use Ctrl, Alt, or Super as well.").into());
            cx.notify();
            return;
        }
        if let Err(err) = hotkey::install(slot, chord.clone()) {
            self.error = Some(tf("Could not use that shortcut: {}", &[&err]));
            cx.notify();
            return;
        }
        match slot {
            Shortcut::Show => self.show_hotkey = chord.canonical(),
            Shortcut::Record => self.record_hotkey = chord.canonical(),
        }
        self.recording_hotkey = None;
        self.error = None;
        self.suppress_actions_until = Some(Instant::now() + Duration::from_millis(280));
        self.persist();
        hotkey::set_paused(false);
        cx.notify();
    }

    pub(crate) fn close_overlay(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if self.recording_hotkey.is_some() {
            self.recording_hotkey = None;
            hotkey::set_paused(false);
            self.error = None;
            self.suppress_actions_until = Some(Instant::now() + Duration::from_millis(280));
            cx.notify();
            return;
        }
        if self.actions_suppressed() {
            return;
        }
        if self.menu_open {
            self.menu_open = false;
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

    pub(crate) fn copy_result(&mut self, cx: &mut Context<Self>) {
        let Phase::Result(text) = &self.phase else {
            return;
        };
        // "Opened Spotify" is a confirmation, not something to paste.
        if self.result_kind == ResultKind::Command {
            return;
        }
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
        #[cfg(not(target_os = "macos"))]
        let target = ();
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
                        match assistant::route(&text, view.voice_commands) {
                            Route::Dictation if view.translation_target().is_none() => {
                                view.finish_dictation(text, target, copy, cx)
                            }
                            route => view.act(route, text, target, copy, cx),
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

    /// Types a note into the original editor and shows it.
    fn finish_dictation(
        &mut self,
        text: String,
        target: TypingTarget,
        copy: bool,
        cx: &mut Context<Self>,
    ) {
        let insert_error = match type_into(target, &text) {
            Typing::Failed(err) => Some(err),
            Typing::Inserted | Typing::NoTarget => None,
        };
        if copy {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
            self.copied = true;
        } else {
            self.copied = false;
        }
        self.show_result(text, ResultKind::Dictation);
        self.error = insert_error;
    }

    fn show_result(&mut self, text: String, kind: ResultKind) {
        self.working = None;
        self.result_kind = kind;
        self.last_text.clone_from(&text);
        self.phase = Phase::Result(text);
        self.error = None;
    }

    /// Runs a voice command, asks the assistant, or translates a note,
    /// keeping the bar busy until it is done.
    fn act(
        &mut self,
        route: Route,
        transcript: String,
        target: TypingTarget,
        copy: bool,
        cx: &mut Context<Self>,
    ) {
        let transcription_id = self.transcription_id;
        let asking = matches!(route, Route::Ask(_));
        let translate_to = self.translation_target();
        let provider = (asking || translate_to.is_some())
            .then(|| self.assistant_provider())
            .flatten();
        self.phase = Phase::Transcribing;
        self.transcribing_provider = provider;
        self.working = Some(match route {
            Route::Ask(_) => t("Thinking…"),
            Route::Command(_) => t("Opening…"),
            Route::Dictation => t("Translating…"),
        });
        let request = assistant::Request {
            transcript,
            route,
            provider,
            screen: self.screen_context.then(|| self.screen.clone()),
            translate_to,
        };
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { assistant::perform(request) })
                .await;
            this.update(cx, |view, cx| {
                if view.transcription_id != transcription_id {
                    return;
                }
                view.transcribing_provider = None;
                view.working = None;
                match outcome {
                    Ok(Outcome::Dictation {
                        text,
                        translated,
                        warning,
                    }) => {
                        view.finish_dictation(text, target, copy, cx);
                        if let Some(language) = translated {
                            view.result_kind = ResultKind::Translated(language);
                        }
                        if warning.is_some() {
                            view.error = warning;
                        }
                    }
                    Ok(Outcome::Opened(message)) => {
                        view.copied = false;
                        view.show_result(message, ResultKind::Command);
                    }
                    Ok(Outcome::Answer { text, provider }) => {
                        view.copied = copy;
                        if copy {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        }
                        view.show_result(text, ResultKind::Answer(provider));
                    }
                    Ok(Outcome::Typed { text, provider }) => {
                        let typing = type_into(target, &text);
                        // Text the user asked for must land somewhere: when
                        // it could not be typed, it waits on the clipboard.
                        let copied = copy || !matches!(typing, Typing::Inserted);
                        if copied {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        }
                        view.copied = copied;
                        view.show_result(text, ResultKind::Typed(provider));
                        if let Typing::Failed(err) = typing {
                            view.error = Some(err);
                        }
                    }
                    Err(err) => {
                        view.phase = Phase::Idle;
                        view.recovery = Some(match err.kind {
                            ErrorKind::Launch => Recovery::Command,
                            ErrorKind::NoKey => Recovery::AssistantKey,
                            ErrorKind::Key(provider) => Recovery::CloudKey(provider),
                            ErrorKind::Offline => Recovery::CloudOffline,
                            ErrorKind::RateLimited => Recovery::CloudRateLimited,
                            ErrorKind::Other => Recovery::Assistant,
                        });
                        view.error = Some(err.message);
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

    fn stop_recording(&mut self) {
        if self.recording_hotkey.is_some() {
            self.recording_hotkey = None;
            hotkey::set_paused(false);
        }
    }

    pub(crate) fn cancel_hotkey_capture(&mut self, cx: &mut Context<Self>) {
        if self.recording_hotkey.is_some() {
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
            app_language: settings::load().app_language,
            selected: self.selected.clone(),
            language: self.language.clone(),
            output_language: self.output_language.clone(),
            show_hotkey: self.show_hotkey.clone(),
            record_hotkey: self.record_hotkey.clone(),
            copy_notes: self.copy_notes,
            clean_fillers: self.clean_fillers,
            input_device: self.input_device.clone(),
            open_on_startup: self.open_on_startup,
            show_in_menu_bar: settings::load().show_in_menu_bar,
            voice_commands: self.voice_commands,
            screen_context: self.screen_context,
            gateway_model: cloud::gateway_model().id().into(),
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
                        false
                    }
                    Ok(None) => false,
                    // The menu bar and About page offer a retry; a failed
                    // background check never interrupts the bar.
                    Err(err) => {
                        eprintln!("Whisple update check failed: {err}");
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
                view.snap_chrome();
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
        self.update_prompt = Some(UpdatePrompt::Ready);
        self.snap_chrome();
        cx.notify();
    }

    fn refresh_models(&mut self) {
        self.models = load_models();
    }
}

/// Only macOS types into other apps so far.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum Typing {
    Inserted,
    NoTarget,
    Failed(String),
}

/// Types `text` into the editor that had focus before the bar opened.
fn type_into(target: TypingTarget, text: &str) -> Typing {
    #[cfg(target_os = "macos")]
    {
        match target {
            Some(target) => match target.insert(text) {
                Ok(()) => Typing::Inserted,
                Err(err) => Typing::Failed(err),
            },
            None => Typing::NoTarget,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let () = target;
        let _ = text;
        Typing::NoTarget
    }
}

/// The dropdown's height above the bar, hairline included.
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
