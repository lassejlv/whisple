use super::*;

pub(crate) const WINDOW_WIDTH: f32 = 400.0;
pub(crate) const WINDOW_RADIUS: f32 = 20.0;
pub(crate) const BAR_HEIGHT: f32 = 56.0;
pub(crate) const COLLAPSED_HEIGHT: f32 = BAR_HEIGHT + 2.0;
pub(crate) const RESULT_EXTRA: f32 = 117.0;
pub(crate) const NOTICE_EXTRA: f32 = 52.0;
pub(crate) const MENU_ROW: f32 = 36.0;
pub(crate) const MENU_PAD: f32 = 6.0;
pub(crate) const MENU_DIVIDER: f32 = 9.0;

pub(crate) const BARS: usize = 19;
pub(crate) const RESULT_LINE: f32 = 22.0;
pub(super) const MAX_ANSWER_LINES: usize = 6;
pub(super) const LINE_CHARS: usize = 46;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) type TypingTarget = Option<dictation::Target>;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) type TypingTarget = ();

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResultKind {
    Dictation,
    Translated(&'static str),
    Command,
    Answer(Provider),
    Typed(Provider),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reveal {
    Menu,
    Result,
}

pub(crate) struct MenuChoice {
    pub id: &'static str,
    pub name: &'static str,
    pub detail: &'static str,
}

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
    pub(super) cancel: Arc<AtomicBool>,
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
    pub(super) failed_audio: Option<(Arc<Vec<f32>>, u32)>,
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
    pub(super) settings_window: Option<crate::ui::settings::SettingsHandle>,
    #[cfg(feature = "licensing")]
    pub license_access: Access,
    #[cfg(feature = "licensing")]
    pub(super) license_checking: bool,
    #[cfg(feature = "licensing")]
    pub(super) license_generation: u64,
    #[cfg(feature = "licensing")]
    pub(super) last_license_check: Instant,
    pub(super) transcription_id: u64,
    pub language: String,
    /// The language notes come out in. Empty means the spoken language.
    pub output_language: String,
    pub show_hotkey: String,
    /// Shows the bar and starts recording. Empty when turned off.
    pub record_hotkey: String,
    pub copy_notes: bool,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) dictation_target: Option<dictation::Target>,
    pub clean_fillers: bool,
    pub voice_commands: bool,
    pub screen_context: bool,
    pub(super) screen: Snapshot,
    pub input_device: String,
    pub open_on_startup: bool,
    #[cfg(target_os = "macos")]
    pub(super) update: Option<PreparedUpdate>,
    #[cfg(target_os = "macos")]
    pub(crate) update_prompt: Option<UpdatePrompt>,
    #[cfg(target_os = "macos")]
    pub(super) update_checking: bool,
    #[cfg(target_os = "macos")]
    pub(super) last_update_check: Instant,
    pub recording_hotkey: Option<Shortcut>,
    pub listen_started: Option<Instant>,
    pub recorded: Duration,
    /// The last transcript, kept so the result panel can slide shut showing
    /// it after a new recording has already replaced the phase.
    pub last_text: String,
    pub bar_visible: bool,
    #[cfg(target_os = "macos")]
    pub(super) was_window_active: bool,
    pub(super) suppress_actions_until: Option<Instant>,
    pub(super) last_tick: Instant,
    pub(super) screen_x: f32,
    pub(super) screen_y: f32,
    pub(super) screen_w: f32,
    pub(super) screen_h: f32,
    pub(super) placed_height: f32,
}
