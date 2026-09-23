use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{actions, App, ClipboardItem, Context, FocusHandle, Focusable, KeyBinding, WeakEntity};

use crate::audio::{self, Mic};
use crate::models::{self, ModelSpec};
use crate::motion::{Ease, Spring};
use crate::place;
use crate::stt;

pub(crate) const WINDOW_WIDTH: f32 = 400.0;
pub(crate) const COLLAPSED_HEIGHT: f32 = 64.0;
pub(crate) const PICKER_EXTRA: f32 = 360.0;
pub(crate) const RESULT_EXTRA: f32 = 132.0;
pub(crate) const ERROR_EXTRA: f32 = 22.0;

const BARS: usize = 22;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reveal {
    Picker,
    Result,
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
        let selected = models::load_selected()
            .filter(|id| models::spec(id).is_some())
            .unwrap_or_else(|| models::recommended_id().to_string());
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

        if self.picker_open {
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

    fn settled_height(&self) -> f32 {
        let mut height = COLLAPSED_HEIGHT;
        if self.picker_open {
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
        self.picker_opened_at
            .is_some_and(|opened| opened.elapsed() < Duration::from_millis(380))
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
        if self.picker_open {
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
                .update(cx, |view, _| {
                    place::dock(
                        WINDOW_WIDTH,
                        view.chrome.value,
                        screen_x,
                        screen_y,
                        screen_w,
                        screen_h,
                    );
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
        self.error = None;
        self.copied = false;
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
            Phase::Idle | Phase::Result(_) => match Mic::start() {
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
        self.picker_open = !self.picker_open;
        self.picker_opened_at = if self.picker_open {
            Some(Instant::now())
        } else {
            None
        };
        cx.notify();
    }

    pub(crate) fn close_overlay(&mut self, cx: &mut Context<Self>) {
        if self.picker_open {
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
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { stt::transcribe(&model_id, &path, &samples, rate) })
                .await;
            this.update(cx, |view, cx| {
                match outcome {
                    Ok(text) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        view.phase = Phase::Result(text);
                        view.copied = true;
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
