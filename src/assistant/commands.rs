/// Longest app name or site a command may carry. Longer phrases after "open"
/// are almost always dictation ("Open the file and check the totals.").
const MAX_TARGET_WORDS: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    OpenApp(String),
    OpenUrl(String),
}

/// Verbs that start a command, longest first so "switch to" wins over a
/// shorter prefix. Whisper keeps the language it heard, so the common
/// translations of "open" are here too.
const VERBS: &[&str] = &[
    "switch over to",
    "switch to",
    "bring up",
    "pull up",
    "go to",
    "open up",
    "open",
    "launch",
    "start",
    "åbn",
    "åben",
    "öffne",
    "starte",
    "abre",
    "abrir",
    "ouvre",
    "ouvrir",
    "apri",
    "öppna",
    "åpne",
];

/// Politeness in front of the verb: "Please open Slack", "Can you open…".
const LEADS: &[&str] = &[
    "um",
    "uh",
    "so",
    "please",
    "can you",
    "could you",
    "would you",
    "will you",
    "i want to",
    "i wanna",
    "i'd like to",
    "i would like to",
    "let's",
    "lets",
    "now",
    "okay",
    "ok",
    "and",
];

pub fn parse(transcript: &str) -> Option<Command> {
    let text = normalize(transcript);
    let mut rest = text.as_str();
    loop {
        let before = rest;
        for lead in LEADS {
            if let Some(after) = strip_word_prefix(rest, lead) {
                rest = after;
            }
        }
        if rest == before {
            break;
        }
    }
    let rest = VERBS
        .iter()
        .find_map(|verb| strip_word_prefix(rest, verb))?;
    let target = clean_target(rest);
    if target.is_empty() || target.split_whitespace().count() > MAX_TARGET_WORDS {
        return None;
    }
    if let Some(url) = as_url(&target) {
        return Some(Command::OpenUrl(url));
    }
    // "Open a new tab" and "start the meeting" read as commands but name no
    // app; leave them for `apps::find` to reject.
    Some(Command::OpenApp(original_case(transcript, &target)))
}

pub fn addressed(transcript: &str) -> Option<String> {
    let words: Vec<&str> = transcript.split_whitespace().collect();
    let bare = |word: &str| {
        word.trim_matches(|ch: char| !ch.is_alphanumeric())
            .to_lowercase()
    };
    let first = bare(words.first()?);
    let greeted = matches!(first.as_str(), "hey" | "hi" | "hej" | "okay" | "ok" | "yo");
    let name_index = usize::from(greeted);
    let name = bare(words.get(name_index)?);
    // "Whisper" and "whistle" are what speech models often hear for the name.
    // Only accept them after a greeting, so a note that starts "Whisper it"
    // is still typed.
    let known = if greeted {
        is_name(&name) || matches!(name.as_str(), "whisper" | "whistle" | "whisp")
    } else {
        is_name(&name)
    };
    if !known {
        return None;
    }
    let request = words[name_index + 1..].join(" ");
    let request = request
        .trim_start_matches(|ch: char| ch.is_ascii_punctuation() || ch.is_whitespace())
        .trim();
    if request.is_empty() {
        return None;
    }
    let mut chars = request.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

fn is_name(word: &str) -> bool {
    matches!(
        word,
        "whisple" | "whispel" | "whispple" | "wisple" | "wispel" | "whispl" | "whisble"
    )
}

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            word.trim_matches(|ch: char| {
                !ch.is_alphanumeric() && ch != '\'' && ch != '+' && ch != '#'
            })
            .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_word_prefix<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = text.strip_prefix(prefix)?;
    if rest.is_empty() {
        return Some(rest);
    }
    rest.strip_prefix(' ')
}

fn clean_target(rest: &str) -> String {
    let mut words: Vec<&str> = rest.split_whitespace().collect();
    while let Some(first) = words.first() {
        if matches!(
            *first,
            "the" | "my" | "a" | "an" | "up" | "app" | "application"
        ) {
            words.remove(0);
        } else {
            break;
        }
    }
    loop {
        let len = words.len();
        if len >= 2 && words[len - 2] == "for" && words[len - 1] == "me" {
            words.truncate(len - 2);
            continue;
        }
        match words.last() {
            Some(&"please" | &"app" | &"application" | &"now" | &"up") if len > 1 => {
                words.pop();
            }
            _ => break,
        }
    }
    words.join(" ")
}

fn as_url(target: &str) -> Option<String> {
    let joined = target.replace(" dot ", ".");
    let host = joined
        .strip_prefix("https://")
        .or_else(|| joined.strip_prefix("http://"))
        .unwrap_or(&joined);
    if host.contains(' ') || !host.contains('.') {
        return None;
    }
    let domain = host.split('/').next()?;
    let labels: Vec<&str> = domain.split('.').collect();
    let tld = labels.last()?;
    let valid = labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        })
        && tld.len() >= 2
        && tld.chars().all(|ch| ch.is_ascii_alphabetic());
    valid.then(|| format!("https://{host}"))
}

fn original_case(transcript: &str, target: &str) -> String {
    let wanted: Vec<&str> = target.split_whitespace().collect();
    let words: Vec<String> = transcript
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|ch: char| {
                !ch.is_alphanumeric() && ch != '\'' && ch != '+' && ch != '#'
            })
            .to_string()
        })
        .filter(|word| !word.is_empty())
        .collect();
    for start in 0..words.len() {
        let window = words.get(start..start + wanted.len());
        if window.is_some_and(|window| {
            window
                .iter()
                .zip(&wanted)
                .all(|(word, want)| word.to_lowercase() == *want)
        }) {
            return words[start..start + wanted.len()].join(" ");
        }
    }
    target.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> Option<Command> {
        Some(Command::OpenApp(name.to_string()))
    }

    #[test]
    fn open_and_its_synonyms_name_an_app() {
        assert_eq!(parse("Open Spotify."), app("Spotify"));
        assert_eq!(
            parse("launch Visual Studio Code"),
            app("Visual Studio Code")
        );
        assert_eq!(parse("Switch to Slack!"), app("Slack"));
        assert_eq!(parse("Start Firefox"), app("Firefox"));
        assert_eq!(parse("Bring up the calculator."), app("calculator"));
    }

    #[test]
    fn politeness_and_articles_are_ignored() {
        assert_eq!(parse("Please open the Notes app."), app("Notes"));
        assert_eq!(parse("Can you open Safari for me?"), app("Safari"));
        assert_eq!(parse("Um, I wanna open Spotify please."), app("Spotify"));
    }

    #[test]
    fn other_languages_open_apps_too() {
        assert_eq!(parse("Åbn Spotify."), app("Spotify"));
        assert_eq!(parse("Öffne Safari"), app("Safari"));
        assert_eq!(parse("Abre Chrome."), app("Chrome"));
    }

    #[test]
    fn a_spoken_site_opens_in_the_browser() {
        assert_eq!(
            parse("Open github.com."),
            Some(Command::OpenUrl("https://github.com".into()))
        );
        assert_eq!(
            parse("Go to news dot ycombinator dot com"),
            Some(Command::OpenUrl("https://news.ycombinator.com".into()))
        );
        assert_eq!(parse("Open version 2.5"), app("version 2.5"));
    }

    #[test]
    fn long_sentences_stay_dictation() {
        assert_eq!(
            parse("Open the file and check the totals before Friday."),
            None
        );
        assert_eq!(parse("We should open a new office."), None);
        assert_eq!(parse("Run the numbers again."), None);
        assert_eq!(parse("Open."), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn the_assistant_answers_to_its_name() {
        assert_eq!(
            addressed("Hey Whisple, what does this error mean?"),
            Some("What does this error mean?".into())
        );
        assert_eq!(
            addressed("Whisple, summarize this page."),
            Some("Summarize this page.".into())
        );
        assert_eq!(
            addressed("Hey whisper, open Spotify."),
            Some("Open Spotify.".into())
        );
        assert_eq!(
            addressed("OK Whistle. reply to her"),
            Some("Reply to her".into())
        );
    }

    #[test]
    fn notes_that_merely_mention_the_name_are_typed() {
        assert_eq!(addressed("Whisper it to me later."), None);
        assert_eq!(addressed("I love Whisple."), None);
        assert_eq!(addressed("Hey Whisple."), None);
        assert_eq!(addressed("Hey John, call me."), None);
    }
}
