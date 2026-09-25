//! The language of Whisple's own interface: buttons, headings, onboarding
//! and settings. Dictation, translation and the assistant are separate.
//!
//! English text is the key. `t("Start recording")` returns the current
//! language's version, falling back to English, so a missing entry never
//! hides a label. A test checks every `t(…)` and `tf(…)` in the source has
//! an entry in each table.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

mod da;
mod de;
mod es;
mod fr;
mod nb;
mod sv;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Da,
    De,
    Nb,
    Sv,
    Es,
    Fr,
}

impl Lang {
    pub const ALL: [Self; 7] = [
        Self::En,
        Self::Da,
        Self::De,
        Self::Nb,
        Self::Sv,
        Self::Es,
        Self::Fr,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Da => "da",
            Self::De => "de",
            Self::Nb => "nb",
            Self::Sv => "sv",
            Self::Es => "es",
            Self::Fr => "fr",
        }
    }

    /// The language's name in itself, as a language picker shows it.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Da => "Dansk",
            Self::De => "Deutsch",
            Self::Nb => "Norsk",
            Self::Sv => "Svenska",
            Self::Es => "Español",
            Self::Fr => "Français",
        }
    }

    /// A supported language from a code or locale such as `da`, `da-DK` or
    /// `de_AT.UTF-8`.
    pub fn from_code(code: &str) -> Option<Self> {
        let primary = code
            .split(['-', '_', '.', '@'])
            .next()?
            .to_ascii_lowercase();
        // Norwegian systems report Bokmål, Nynorsk or plain Norwegian.
        let primary = match primary.as_str() {
            "no" | "nn" => "nb".to_string(),
            _ => primary,
        };
        Self::ALL.into_iter().find(|lang| lang.code() == primary)
    }

    fn index(self) -> u8 {
        match self {
            Self::En => 0,
            Self::Da => 1,
            Self::De => 2,
            Self::Nb => 3,
            Self::Sv => 4,
            Self::Es => 5,
            Self::Fr => 6,
        }
    }

    fn strings(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::Da => da::STRINGS,
            Self::De => de::STRINGS,
            Self::Nb => nb::STRINGS,
            Self::Sv => sv::STRINGS,
            Self::Es => es::STRINGS,
            Self::Fr => fr::STRINGS,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn current() -> Lang {
    let index = CURRENT.load(Ordering::Relaxed);
    Lang::ALL
        .into_iter()
        .find(|lang| lang.index() == index)
        .unwrap_or(Lang::En)
}

pub fn set(lang: Lang) {
    CURRENT.store(lang.index(), Ordering::Relaxed);
}

/// The first of the system's preferred languages that Whisple speaks.
pub fn detect() -> Lang {
    first_supported(sys_locale::get_locales())
}

fn first_supported(locales: impl IntoIterator<Item = String>) -> Lang {
    locales
        .into_iter()
        .find_map(|locale| Lang::from_code(&locale))
        .unwrap_or(Lang::En)
}

/// The saved choice, or the system language when none was saved.
pub fn resolve(saved: &str) -> Lang {
    Lang::from_code(saved).unwrap_or_else(detect)
}

/// `english` in the current interface language.
pub fn t(english: &'static str) -> &'static str {
    lookup(current(), english)
}

/// A translated template with each `{}` filled in order.
pub fn tf(template: &'static str, args: &[&dyn Display]) -> String {
    fill(t(template), args)
}

fn lookup(lang: Lang, english: &'static str) -> &'static str {
    if lang == Lang::En {
        return english;
    }
    table(lang).get(english).copied().unwrap_or(english)
}

fn table(lang: Lang) -> &'static HashMap<&'static str, &'static str> {
    static TABLES: [OnceLock<HashMap<&'static str, &'static str>>; 7] =
        [const { OnceLock::new() }; 7];
    TABLES[lang.index() as usize].get_or_init(|| lang.strings().iter().copied().collect())
}

fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut rest = template;
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        match args.next() {
            Some(arg) => out.push_str(&arg.to_string()),
            None => out.push_str("{}"),
        }
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    #[test]
    fn locales_map_onto_supported_languages() {
        assert_eq!(Lang::from_code("da-DK"), Some(Lang::Da));
        assert_eq!(Lang::from_code("de_AT.UTF-8"), Some(Lang::De));
        assert_eq!(Lang::from_code("FR"), Some(Lang::Fr));
        assert_eq!(Lang::from_code("sv-SE"), Some(Lang::Sv));
        assert_eq!(Lang::from_code("nn-NO"), Some(Lang::Nb));
        assert_eq!(Lang::from_code("no"), Some(Lang::Nb));
        assert_eq!(Lang::from_code("fi-FI"), None);
        assert_eq!(Lang::from_code(""), None);
        assert_eq!(
            first_supported(["fi-FI".to_string(), "es-MX".to_string()]),
            Lang::Es
        );
        assert_eq!(first_supported(Vec::<String>::new()), Lang::En);
        assert_eq!(resolve("da"), Lang::Da);
    }

    #[test]
    fn templates_fill_in_order() {
        assert_eq!(fill("{} of {}", &[&1, &"two"]), "1 of two");
        assert_eq!(fill("no args", &[]), "no args");
        assert_eq!(fill("{} and {}", &[&"one"]), "one and {}");
    }

    #[test]
    fn a_missing_entry_falls_back_to_english() {
        assert_eq!(
            lookup(Lang::Da, "No such label anywhere"),
            "No such label anywhere"
        );
        assert_eq!(lookup(Lang::En, "Start recording"), "Start recording");
        assert_ne!(lookup(Lang::Da, "Start recording"), "Start recording");
    }

    /// Every string the source passes to `t` or `tf`, plus the catalog text
    /// shown through `t` at runtime.
    fn keys() -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        collect(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path(),
            &mut keys,
        );
        for spec in crate::models::CATALOG {
            keys.insert(spec.name.to_string());
            keys.insert(spec.blurb.to_string());
        }
        for provider in crate::cloud::Provider::ALL {
            keys.insert(provider.description().to_string());
        }
        for language in crate::settings::Preferences::languages() {
            keys.insert(language.name.to_string());
        }
        keys
    }

    fn collect(dir: &Path, keys: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                // The tables themselves hold keys, not uses.
                if path.file_name().is_some_and(|name| name != "i18n") {
                    collect(&path, keys);
                }
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs")
                || path.file_name().is_some_and(|name| name == "i18n.rs")
            {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            for call in ["t(", "tf("] {
                let mut rest = source.as_str();
                while let Some(at) = rest.find(call) {
                    let before = rest[..at].chars().next_back();
                    rest = &rest[at + call.len()..];
                    if before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.') {
                        continue;
                    }
                    let trimmed = rest.trim_start();
                    let Some(literal) = trimmed.strip_prefix('"') else {
                        continue;
                    };
                    let end = literal.find('"').expect("closed string literal");
                    keys.insert(literal[..end].to_string());
                }
            }
        }
    }

    #[test]
    fn every_label_is_translated() {
        let keys = keys();
        assert!(keys.len() > 100, "found only {} keys", keys.len());
        for lang in Lang::ALL.into_iter().filter(|lang| *lang != Lang::En) {
            let table: BTreeSet<&str> = lang.strings().iter().map(|(key, _)| *key).collect();
            let missing: Vec<&String> = keys
                .iter()
                .filter(|key| !table.contains(key.as_str()))
                .collect();
            assert!(missing.is_empty(), "{lang:?} is missing {missing:#?}");
            let stale: Vec<&&str> = table.iter().filter(|key| !keys.contains(**key)).collect();
            assert!(stale.is_empty(), "{lang:?} has unused entries {stale:#?}");
        }
    }

    #[test]
    fn translations_keep_their_placeholders() {
        for lang in Lang::ALL {
            for (key, value) in lang.strings() {
                assert_eq!(
                    key.matches("{}").count(),
                    value.matches("{}").count(),
                    "{lang:?}: {key}"
                );
                assert!(!value.trim().is_empty(), "{lang:?}: {key}");
            }
            let unique: BTreeSet<&str> = lang.strings().iter().map(|(key, _)| *key).collect();
            assert_eq!(unique.len(), lang.strings().len(), "{lang:?} repeats a key");
        }
    }
}
