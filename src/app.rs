use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    actions, App, ClipboardItem, Context, FocusHandle, Focusable, KeyBinding, Keystroke, WeakEntity,
};

use crate::audio::{self, Mic};
use crate::hotkey;
use crate::models::{self, ModelSpec};
use crate::motion::{Ease, Spring};
use crate::place;
use crate::settings::{self, Preferences};
use crate::startup;
use crate::stt;

pub(crate) const WINDOW_WIDTH: f32 = 400.0;
pub(crate) const COLLAPSED_HEIGHT: f32 = 64.0;
pub(crate) const PICKER_EXTRA: f32 = 360.0;
pub(crate) const RESULT_EXTRA: f32 = 132.0;
/// 14 top + 48 header + 8 gap + 342 list + 10 bottom + 1 separator.
/// The list is 6 rows of 52 with 5 gaps of 6.
pub(crate) const SETTINGS_LIST_H: f32 = 342.0;
pub(crate) const SETTINGS_BODY_H: f32 = 398.0;
pub(crate) const SETTINGS_EXTRA: f32 = 423.0;
pub(crate) const ERROR_EXTRA: f32 = 22.0;

const BARS: usize = 22;

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

actions!(whisp, [ToggleListen, CloseOverlay]);

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
    pub microphones: Vec<String>,
    pub recording_hotkey: bool,
    pub page_fade: Ease,
    pub bar_visible: bool,
    suppress_actions_until: Option<Instant>,
    last_tick: Instant,
    pin_started: bool,
    screen_x: f32,
    screen_y: f32,
    screen_w: f32,
    screen_h: f32,
    placed_height: f32,
}

impl Whisp {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let models = load_models();
        let prefs = settings::load();
        let selected = models::load_selected()
            .filter(|id| models::spec(id).is_some())
            .unwrap_or_else(|| models::recommended_id().to_string());
        if let Some(chord) = hotkey::parse(&prefs.show_hotkey) {
            hotkey::install(chord);
        }
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
        Self {
            focus_handle: cx.focus_handle(),
            phase: Phase::Idle,
            levels: VecDeque::new(),
            picker_open: false,
            models,
            selected,
            download: None,
            error: None,
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
            microphones: Vec::new(),
            recording_hotkey: false,
            page_fade: Ease::at(1.0, Duration::from_millis(180), 0.02),
            bar_visible: true,
            suppress_actions_until: None,
            last_tick: Instant::now(),
            pin_started: false,
            screen_x,
            screen_y,
            screen_w,
            screen_h,
            placed_height: 0.0,
        }
    }

    pub(crate) fn tick(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.ensure_pin(cx);
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
            let height = self.chrome.value;
            if (self.placed_height - height).abs() >= 0.5 {
                self.placed_height = height;
                window.resize(gpui::size(gpui::px(WINDOW_WIDTH), gpui::px(height)));
                place::dock(
                    WINDOW_WIDTH,
                    height,
                    self.screen_x,
                    self.screen_y,
                    self.screen_w,
                    self.screen_h,
                );
            }
        }
    }

    fn settled_height(&self) -> f32 {
        let mut height = COLLAPSED_HEIGHT;
        if self.settings_open {
            height += SETTINGS_EXTRA;
        } else if self.picker_open {
            height += PICKER_EXTRA;
        } else if matches!(self.phase, Phase::Result(_)) {
            height += RESULT_EXTRA;
        }
        if self.error.is_some() {
            height += ERROR_EXTRA;
        }
        height
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

    fn ensure_pin(&mut self, cx: &mut Context<Self>) {
        if self.pin_started {
            return;
        }
        self.pin_started = true;
        let screen_x = self.screen_x;
        let screen_y = self.screen_y;
        let screen_w = self.screen_w;
        let screen_h = self.screen_h;
        cx.spawn(async move |this: WeakEntity<Self>, cx| loop {
            gpui::Timer::after(Duration::from_millis(80)).await;
            let alive = this
                .update(cx, |view, cx| {
                    let pressed = hotkey::take_press();
                    if pressed {
                        view.bar_visible = !view.bar_visible;
                        cx.notify();
                    }
                    if view.bar_visible {
                        if pressed {
                            place::set_mapped(true);
                            cx.activate(true);
                        }
                        place::dock(
                            WINDOW_WIDTH,
                            view.chrome.value,
                            screen_x,
                            screen_y,
                            screen_w,
                            screen_h,
                        );
                    } else {
                        place::set_mapped(false);
                    }
                })
                .is_ok();
            if !alive {
                break;
            }
        })
        .detach();
    }

    fn rest_bars(&mut self) {
        for bar in &mut self.bars {
            bar.snap(0.08);
        }
    }

    pub(crate) fn selected_spec(&self) -> &'static ModelSpec {
        models::spec(&self.selected).unwrap_or(&models::CATALOG[0])
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

    pub(crate) fn toggle_picker(&mut self, cx: &mut Context<Self>) {
        self.stop_recording();
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
        match audio::input_names() {
            Ok(names) => {
                self.microphones = names;
                self.error = None;
            }
            Err(err) => {
                self.microphones.clear();
                self.error = Some(err);
            }
        }
        self.settings_page = SettingsPage::Microphone;
        self.page_fade.snap(0.0);
        self.page_fade.set(1.0);
        cx.notify();
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
        let next = !self.open_on_startup;
        if let Err(err) = startup::apply(next) {
            self.error = Some(err);
            cx.notify();
            return;
        }
        self.open_on_startup = next;
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
        self.copy_notes = !self.copy_notes;
        self.persist();
        cx.notify();
    }

    pub(crate) fn toggle_clean_fillers(&mut self, cx: &mut Context<Self>) {
        self.clean_fillers = !self.clean_fillers;
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
        self.show_hotkey = chord.canonical();
        self.recording_hotkey = false;
        self.error = None;
        self.suppress_actions_until = Some(Instant::now() + Duration::from_millis(280));
        self.persist();
        hotkey::install(chord);
        hotkey::set_paused(false);
        cx.notify();
    }

    pub(crate) fn close_overlay(&mut self, cx: &mut Context<Self>) {
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
        } else if matches!(self.phase, Phase::Result(_)) {
            self.phase = Phase::Idle;
            self.copied = false;
        }
        self.error = None;
        self.snap_chrome();
        cx.notify();
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

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", ToggleListen, Some("Whisp")),
        KeyBinding::new("escape", CloseOverlay, Some("Whisp")),
    ]);
}
