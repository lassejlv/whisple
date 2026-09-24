//! Preferences stored in `~/.config/whisp/settings.json`.
//!
//! Older files only recorded the selected model. Missing fields keep their
//! defaults so that file still loads.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY: &str = "ctrl-shift-space";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub onboarding_complete: bool,
    pub selected: String,
    pub language: String,
    pub show_hotkey: String,
    pub copy_notes: bool,
    pub clean_fillers: bool,
    /// Empty means the system default input.
    pub input_device: String,
    pub open_on_startup: bool,
}

pub struct Language {
    pub id: &'static str,
    pub name: &'static str,
    /// The language's own name, empty where it matches `name`.
    pub native: &'static str,
}

const LANGUAGES: &[Language] = &[
    Language {
        id: "auto",
        name: "Detect automatically",
        native: "",
    },
    Language {
        id: "en",
        name: "English",
        native: "",
    },
    Language {
        id: "es",
        name: "Spanish",
        native: "Español",
    },
    Language {
        id: "fr",
        name: "French",
        native: "Français",
    },
    Language {
        id: "de",
        name: "German",
        native: "Deutsch",
    },
    Language {
        id: "it",
        name: "Italian",
        native: "Italiano",
    },
    Language {
        id: "pt",
        name: "Portuguese",
        native: "Português",
    },
    Language {
        id: "nl",
        name: "Dutch",
        native: "Nederlands",
    },
    Language {
        id: "sv",
        name: "Swedish",
        native: "Svenska",
    },
    Language {
        id: "da",
        name: "Danish",
        native: "Dansk",
    },
    Language {
        id: "no",
        name: "Norwegian",
        native: "Norsk",
    },
    Language {
        id: "fi",
        name: "Finnish",
        native: "Suomi",
    },
    Language {
        id: "pl",
        name: "Polish",
        native: "Polski",
    },
    Language {
        id: "ru",
        name: "Russian",
        native: "Русский",
    },
    Language {
        id: "uk",
        name: "Ukrainian",
        native: "Українська",
    },
    Language {
        id: "ja",
        name: "Japanese",
        native: "日本語",
    },
    Language {
        id: "zh",
        name: "Chinese",
        native: "中文",
    },
    Language {
        id: "ko",
        name: "Korean",
        native: "한국어",
    },
    Language {
        id: "ar",
        name: "Arabic",
        native: "العربية",
    },
    Language {
        id: "hi",
        name: "Hindi",
        native: "हिन्दी",
    },
    Language {
        id: "tr",
        name: "Turkish",
        native: "Türkçe",
    },
];

#[derive(Serialize, Deserialize)]
struct File {
    // Files written before onboarding existed belong to existing users.
    #[serde(default = "yes")]
    onboarding_complete: bool,
    #[serde(default)]
    selected: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    show_hotkey: String,
    #[serde(default = "yes")]
    copy_notes: bool,
    #[serde(default = "yes")]
    clean_fillers: bool,
    #[serde(default)]
    input_device: String,
    #[serde(default)]
    open_on_startup: bool,
}

fn yes() -> bool {
    true
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            onboarding_complete: false,
            selected: String::new(),
            language: "en".into(),
            show_hotkey: DEFAULT_HOTKEY.into(),
            copy_notes: true,
            clean_fillers: true,
            input_device: String::new(),
            open_on_startup: false,
        }
    }
}

impl Preferences {
    pub fn languages() -> &'static [Language] {
        LANGUAGES
    }
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

pub fn language_name(id: &str) -> &'static str {
    LANGUAGES
        .iter()
        .find(|language| language.id == id)
        .map(|language| language.name)
        .unwrap_or("English")
}

pub fn decode(raw: &str) -> Preferences {
    let Ok(file) = serde_json::from_str::<File>(raw) else {
        return Preferences::default();
    };
    let mut prefs = Preferences::default();
    prefs.onboarding_complete = file.onboarding_complete;
    if !file.selected.is_empty() {
        prefs.selected = file.selected;
    }
    if Preferences::languages()
        .iter()
        .any(|language| language.id == file.language)
    {
        prefs.language = file.language;
    }
    if let Some(chord) = crate::hotkey::parse(&file.show_hotkey) {
        if chord.has_modifier() {
            prefs.show_hotkey = chord.canonical();
        }
    }
    prefs.copy_notes = file.copy_notes;
    prefs.clean_fillers = file.clean_fillers;
    prefs.input_device = clean_device(&file.input_device);
    prefs.open_on_startup = file.open_on_startup;
    prefs
}

pub fn microphone_label(name: &str) -> &str {
    if name.is_empty() {
        "System default"
    } else {
        name
    }
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
            selected: prefs.selected.clone(),
            language: prefs.language.clone(),
            show_hotkey: prefs.show_hotkey.clone(),
            copy_notes: prefs.copy_notes,
            clean_fillers: prefs.clean_fillers,
            input_device: prefs.input_device.clone(),
            open_on_startup: prefs.open_on_startup,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_model_file_keeps_the_new_defaults() {
        let prefs = decode(r#"{"selected":"small-en"}"#);
        assert_eq!(prefs.selected, "small-en");
        assert_eq!(prefs.language, "en");
        assert_eq!(prefs.show_hotkey, DEFAULT_HOTKEY);
        assert!(prefs.copy_notes);
        assert!(prefs.clean_fillers);
        assert!(prefs.input_device.is_empty());
        assert!(!prefs.open_on_startup);
        assert!(prefs.onboarding_complete);
        assert_eq!(microphone_label(""), "System default");
    }

    #[test]
    fn startup_and_microphone_survive_a_round_trip() {
        let prefs = decode(
            r#"{"open_on_startup":true,"input_device":"  Studio Mic\n","selected":"small-en"}"#,
        );
        assert!(prefs.open_on_startup);
        assert_eq!(prefs.input_device, "Studio Mic");
        assert_eq!(microphone_label(&prefs.input_device), "Studio Mic");
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
    fn unknown_language_falls_back_to_english() {
        let prefs = decode(r#"{"language":"zz"}"#);
        assert_eq!(prefs.language, "en");
    }

    #[test]
    fn a_bare_key_is_not_kept_as_the_shortcut() {
        let prefs = decode(r#"{"show_hotkey":"a","language":"fr"}"#);
        assert_eq!(prefs.show_hotkey, DEFAULT_HOTKEY);
        assert_eq!(prefs.language, "fr");
        assert_eq!(language_name("fr"), "French");
    }
}
