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
    pub show_in_menu_bar: bool,
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
    #[serde(default = "yes")]
    show_in_menu_bar: bool,
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
            show_in_menu_bar: true,
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

pub fn decode(raw: &str) -> Preferences {
    let Ok(file) = serde_json::from_str::<File>(raw) else {
        return Preferences::default();
    };
    let mut prefs = Preferences {
        onboarding_complete: file.onboarding_complete,
        ..Preferences::default()
    };
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
    prefs.show_in_menu_bar = file.show_in_menu_bar;
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
            selected: prefs.selected.clone(),
            language: prefs.language.clone(),
            show_hotkey: prefs.show_hotkey.clone(),
            copy_notes: prefs.copy_notes,
            clean_fillers: prefs.clean_fillers,
            input_device: prefs.input_device.clone(),
            open_on_startup: prefs.open_on_startup,
            show_in_menu_bar: prefs.show_in_menu_bar,
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
    fn unknown_language_falls_back_to_english() {
        let prefs = decode(r#"{"language":"zz"}"#);
        assert_eq!(prefs.language, "en");
    }

    #[test]
    fn a_bare_key_is_not_kept_as_the_shortcut() {
        let prefs = decode(r#"{"show_hotkey":"a","language":"fr"}"#);
        assert_eq!(prefs.show_hotkey, DEFAULT_HOTKEY);
        assert_eq!(prefs.language, "fr");
    }
}
