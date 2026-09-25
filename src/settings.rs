//! Preferences stored in `~/.config/whisp/settings.json`.
//!
//! Older files only recorded the selected model. Missing fields keep their
//! defaults so that file still loads.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY: &str = "ctrl-shift-space";
pub const DEFAULT_RECORD_HOTKEY: &str = "ctrl-alt-space";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub onboarding_complete: bool,
    /// The interface language's code. Empty follows the system language.
    pub app_language: String,
    pub selected: String,
    pub language: String,
    /// The language notes come out in. Empty means the spoken language.
    pub output_language: String,
    pub show_hotkey: String,
    /// Shows the bar and starts recording. Empty when turned off.
    pub record_hotkey: String,
    pub copy_notes: bool,
    pub clean_fillers: bool,
    /// Empty means the system default input.
    pub input_device: String,
    pub open_on_startup: bool,
    pub show_in_menu_bar: bool,
    /// "Open Spotify" launches the app instead of typing the words.
    pub voice_commands: bool,
    /// The assistant sees the front app, selection and a screenshot.
    pub screen_context: bool,
    /// The model Vercel AI Gateway transcribes with: "grok" or "openai".
    pub gateway_model: String,
}

pub struct Language {
    pub id: &'static str,
    pub name: &'static str,
}

const LANGUAGES: &[Language] = &[
    Language {
        id: "auto",
        name: "Detect automatically",
    },
    Language {
        id: "en",
        name: "English",
    },
    Language {
        id: "es",
        name: "Spanish",
    },
    Language {
        id: "fr",
        name: "French",
    },
    Language {
        id: "de",
        name: "German",
    },
    Language {
        id: "it",
        name: "Italian",
    },
    Language {
        id: "pt",
        name: "Portuguese",
    },
    Language {
        id: "nl",
        name: "Dutch",
    },
    Language {
        id: "sv",
        name: "Swedish",
    },
    Language {
        id: "da",
        name: "Danish",
    },
    Language {
        id: "no",
        name: "Norwegian",
    },
    Language {
        id: "fi",
        name: "Finnish",
    },
    Language {
        id: "pl",
        name: "Polish",
    },
    Language {
        id: "ru",
        name: "Russian",
    },
    Language {
        id: "uk",
        name: "Ukrainian",
    },
    Language {
        id: "ja",
        name: "Japanese",
    },
    Language {
        id: "zh",
        name: "Chinese",
    },
    Language {
        id: "ko",
        name: "Korean",
    },
    Language {
        id: "ar",
        name: "Arabic",
    },
    Language {
        id: "hi",
        name: "Hindi",
    },
    Language {
        id: "tr",
        name: "Turkish",
    },
];

#[derive(Serialize, Deserialize)]
struct File {
    // Files written before onboarding existed belong to existing users.
    #[serde(default = "yes")]
    onboarding_complete: bool,
    #[serde(default)]
    app_language: String,
    #[serde(default)]
    selected: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    output_language: String,
    #[serde(default)]
    show_hotkey: String,
    #[serde(default = "default_record_hotkey")]
    record_hotkey: String,
    #[serde(default = "yes")]
    copy_notes: bool,
    #[serde(default = "yes")]
    clean_fillers: bool,
    #[serde(default)]
    input_device: String,
    #[serde(default)]
    open_on_startup: bool,
    #[serde(default = "yes")]
    show_in_menu_bar: bool,
    #[serde(default = "yes")]
    voice_commands: bool,
    #[serde(default = "yes")]
    screen_context: bool,
    #[serde(default)]
    gateway_model: String,
}

fn yes() -> bool {
    true
}

fn default_record_hotkey() -> String {
    DEFAULT_RECORD_HOTKEY.into()
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            onboarding_complete: false,
            app_language: String::new(),
            selected: String::new(),
            language: "en".into(),
            output_language: String::new(),
            show_hotkey: DEFAULT_HOTKEY.into(),
            record_hotkey: DEFAULT_RECORD_HOTKEY.into(),
            copy_notes: true,
            clean_fillers: true,
            input_device: String::new(),
            open_on_startup: false,
            show_in_menu_bar: true,
            voice_commands: true,
            screen_context: true,
            gateway_model: crate::cloud::GatewayModel::Grok.id().into(),
        }
    }
}

impl Preferences {
    pub fn languages() -> &'static [Language] {
        LANGUAGES
    }
}

/// A language's English name, such as "Danish" for `da`. Not for "auto".
pub fn language_name(id: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|language| language.id == id && id != "auto")
        .map(|language| language.name)
}

pub fn load() -> Preferences {
    let Ok(raw) = fs::read_to_string(path()) else {
        return Preferences::default();
    };
    decode(&raw)
}

pub fn save(prefs: &Preferences) {
    let path = path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(&File::from(prefs)) {
        let _ = fs::write(path, raw);
    }
}

pub fn needs_onboarding() -> bool {
    !load().onboarding_complete
}

pub fn whisper_language(id: &str) -> Option<&str> {
    if id == "auto" {
        None
    } else {
        Some(id)
    }
}

pub fn decode(raw: &str) -> Preferences {
    let Ok(file) = serde_json::from_str::<File>(raw) else {
        return Preferences::default();
    };
    let mut prefs = Preferences {
        onboarding_complete: file.onboarding_complete,
        ..Preferences::default()
    };
    if let Some(lang) = crate::i18n::Lang::from_code(&file.app_language) {
        prefs.app_language = lang.code().to_string();
    }
    if !file.selected.is_empty() {
        prefs.selected = file.selected;
    }
    if Preferences::languages()
        .iter()
        .any(|language| language.id == file.language)
    {
        prefs.language = file.language;
    }
    if language_name(&file.output_language).is_some() {
        prefs.output_language = file.output_language;
    }
    if let Some(chord) = crate::hotkey::parse(&file.show_hotkey) {
        if chord.has_modifier() {
            prefs.show_hotkey = chord.canonical();
        }
    }
    prefs.record_hotkey = match crate::hotkey::parse(&file.record_hotkey) {
        Some(chord) if chord.has_modifier() => chord.canonical(),
        // A saved empty value means the user turned the shortcut off.
        _ if file.record_hotkey.is_empty() => String::new(),
        _ => DEFAULT_RECORD_HOTKEY.into(),
    };
    // One chord cannot do both jobs; the show shortcut came first.
    if prefs.record_hotkey == prefs.show_hotkey {
        prefs.record_hotkey.clear();
    }
    prefs.copy_notes = file.copy_notes;
    prefs.clean_fillers = file.clean_fillers;
    prefs.input_device = clean_device(&file.input_device);
    prefs.open_on_startup = file.open_on_startup;
    prefs.show_in_menu_bar = file.show_in_menu_bar;
    prefs.voice_commands = file.voice_commands;
    prefs.screen_context = file.screen_context;
    if let Some(model) = crate::cloud::GatewayModel::from_id(&file.gateway_model) {
        prefs.gateway_model = model.id().into();
    }
    prefs
}

fn clean_device(name: &str) -> String {
    name.trim().chars().filter(|ch| !ch.is_control()).collect()
}

fn path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("whisp")
        .join("settings.json")
}

impl From<&Preferences> for File {
    fn from(prefs: &Preferences) -> Self {
        Self {
            onboarding_complete: prefs.onboarding_complete,
            app_language: prefs.app_language.clone(),
            selected: prefs.selected.clone(),
            language: prefs.language.clone(),
            output_language: prefs.output_language.clone(),
            show_hotkey: prefs.show_hotkey.clone(),
            record_hotkey: prefs.record_hotkey.clone(),
            copy_notes: prefs.copy_notes,
            clean_fillers: prefs.clean_fillers,
            input_device: prefs.input_device.clone(),
            open_on_startup: prefs.open_on_startup,
            show_in_menu_bar: prefs.show_in_menu_bar,
            voice_commands: prefs.voice_commands,
            screen_context: prefs.screen_context,
            gateway_model: prefs.gateway_model.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gateway_model_defaults_to_grok_and_keeps_a_known_choice() {
        assert_eq!(
            decode(r#"{"selected":"cloud-vercel"}"#).gateway_model,
            "grok"
        );
        assert_eq!(
            decode(r#"{"gateway_model":"openai"}"#).gateway_model,
            "openai"
        );
        assert_eq!(
            decode(r#"{"gateway_model":"whisper"}"#).gateway_model,
            "grok"
        );
        let prefs = Preferences {
            gateway_model: "openai".into(),
            ..Preferences::default()
        };
        let raw = serde_json::to_string(&File::from(&prefs)).unwrap();
        assert_eq!(decode(&raw).gateway_model, "openai");
    }

    #[test]
    fn old_model_file_keeps_the_new_defaults() {
        let prefs = decode(r#"{"selected":"small-en"}"#);
        assert_eq!(prefs.selected, "small-en");
        assert_eq!(prefs.language, "en");
        assert_eq!(prefs.show_hotkey, DEFAULT_HOTKEY);
        assert_eq!(prefs.record_hotkey, DEFAULT_RECORD_HOTKEY);
        assert!(prefs.copy_notes);
        assert!(prefs.clean_fillers);
        assert!(prefs.input_device.is_empty());
        assert!(!prefs.open_on_startup);
        assert!(prefs.onboarding_complete);
        assert!(prefs.voice_commands);
        assert!(prefs.screen_context);
    }

    #[test]
    fn assistant_switches_survive_a_round_trip() {
        let prefs = decode(r#"{"voice_commands":false,"screen_context":false}"#);
        assert!(!prefs.voice_commands);
        assert!(!prefs.screen_context);
        let raw = serde_json::to_string(&File::from(&prefs)).unwrap();
        let again = decode(&raw);
        assert!(!again.voice_commands);
        assert!(!again.screen_context);
    }

    #[test]
    fn startup_and_microphone_survive_a_round_trip() {
        let prefs = decode(
            r#"{"open_on_startup":true,"input_device":"  Studio Mic\n","selected":"small-en"}"#,
        );
        assert!(prefs.open_on_startup);
        assert_eq!(prefs.input_device, "Studio Mic");
        let raw = serde_json::to_string(&File::from(&prefs)).unwrap();
        let again = decode(&raw);
        assert!(again.open_on_startup);
        assert_eq!(again.input_device, "Studio Mic");
        assert_eq!(again.selected, "small-en");
        assert!(again.onboarding_complete);
    }

    #[test]
    fn unfinished_onboarding_survives_a_round_trip() {
        let prefs = Preferences::default();
        assert!(!prefs.onboarding_complete);
        let raw = serde_json::to_string(&File::from(&prefs)).unwrap();
        assert!(!decode(&raw).onboarding_complete);
    }

    #[test]
    fn the_output_language_survives_a_round_trip() {
        let prefs = decode(r#"{"language":"da","output_language":"en"}"#);
        assert_eq!(prefs.language, "da");
        assert_eq!(prefs.output_language, "en");
        let raw = serde_json::to_string(&File::from(&prefs)).unwrap();
        assert_eq!(decode(&raw).output_language, "en");
        assert!(Preferences::default().output_language.is_empty());
    }

    #[test]
    fn an_unknown_output_language_means_the_spoken_one() {
        assert!(decode(r#"{"output_language":"zz"}"#)
            .output_language
            .is_empty());
        assert!(decode(r#"{"output_language":"auto"}"#)
            .output_language
            .is_empty());
        assert_eq!(language_name("da"), Some("Danish"));
        assert_eq!(language_name("auto"), None);
    }

    #[test]
    fn the_app_language_survives_a_round_trip() {
        assert!(Preferences::default().app_language.is_empty());
        let prefs = decode(r#"{"app_language":"sv"}"#);
        assert_eq!(prefs.app_language, "sv");
        let raw = serde_json::to_string(&File::from(&prefs)).unwrap();
        assert_eq!(decode(&raw).app_language, "sv");
        assert!(decode(r#"{"app_language":"xx"}"#).app_language.is_empty());
    }

    #[test]
    fn unknown_language_falls_back_to_english() {
        let prefs = decode(r#"{"language":"zz"}"#);
        assert_eq!(prefs.language, "en");
    }

    #[test]
    fn the_record_shortcut_survives_a_round_trip() {
        let prefs = decode(r#"{"record_hotkey":"super-shift-r"}"#);
        assert_eq!(prefs.record_hotkey, "shift-super-r");
        let raw = serde_json::to_string(&File::from(&prefs)).unwrap();
        assert_eq!(decode(&raw).record_hotkey, "shift-super-r");

        let off = decode(r#"{"record_hotkey":""}"#);
        assert!(off.record_hotkey.is_empty());
        let raw = serde_json::to_string(&File::from(&off)).unwrap();
        assert!(decode(&raw).record_hotkey.is_empty());
    }

    #[test]
    fn the_record_shortcut_never_repeats_the_show_shortcut() {
        let prefs = decode(r#"{"show_hotkey":"ctrl-alt-space"}"#);
        assert_eq!(prefs.show_hotkey, "ctrl-alt-space");
        assert!(prefs.record_hotkey.is_empty());
    }

    #[test]
    fn a_bare_key_is_not_kept_as_the_shortcut() {
        let prefs = decode(r#"{"show_hotkey":"a","language":"fr"}"#);
        assert_eq!(prefs.show_hotkey, DEFAULT_HOTKEY);
        assert_eq!(prefs.language, "fr");
    }
}
