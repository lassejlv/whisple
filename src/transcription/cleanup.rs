/// Light cleanup for dictated text. Whisper already punctuates; this drops
/// filler words and stuttered repeats and makes sure the note reads like a
/// sentence.
pub fn cleanup(raw: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut capitalize_next = false;
    for word in raw.split_whitespace() {
        if is_filler(word) {
            // "…ten minutes late, um." keeps its full stop on the last kept word.
            if let (Some(end), Some(last)) = (sentence_end(word), words.last_mut()) {
                let kept = last.trim_end_matches([',', ';', ':']).to_string();
                *last = format!("{kept}{end}");
            }
            capitalize_next |= word.chars().next().is_some_and(char::is_uppercase);
            continue;
        }
        if words.last().is_some_and(|last| is_stutter(last, word)) {
            continue;
        }
        let mut word = word.to_string();
        if std::mem::take(&mut capitalize_next) {
            word = capitalize(&word);
        }
        words.push(word);
    }

    let mut text = words.join(" ");
    text = text
        .replace(" ,", ",")
        .replace(" .", ".")
        .replace(" ?", "?");
    if text.is_empty() {
        return text;
    }

    text = capitalize(&text);
    if !text.ends_with(['.', '!', '?']) {
        text.push('.');
    }
    text
}

pub fn no_speech(text: &str) -> bool {
    let text = text.trim().trim_end_matches(['.', '!', '?']).trim();
    text.is_empty() || text.eq_ignore_ascii_case("[BLANK_AUDIO]")
}

fn core(word: &str) -> String {
    word.trim_matches(|ch: char| !ch.is_alphanumeric())
        .to_lowercase()
}

fn is_filler(word: &str) -> bool {
    matches!(
        core(word).as_str(),
        "um" | "uh"
            | "erm"
            | "uhh"
            | "hmm"
            | "ah"
            | "eh"
            | "ehm"
            | "øh"
            | "øhm"
            | "äh"
            | "ähm"
            | "öh"
            | "öhm"
            | "euh"
    )
}

/// "I I think" or "the the car": the same word twice with nothing between.
/// Doubles that are often grammatical, like "had had" or "that that", stay.
fn is_stutter(previous: &str, word: &str) -> bool {
    let repeated = core(previous);
    previous.ends_with(|ch: char| ch.is_alphanumeric())
        && repeated.chars().any(char::is_alphabetic)
        && repeated == core(word)
        && !matches!(repeated.as_str(), "had" | "that" | "is" | "do" | "det")
}

fn sentence_end(word: &str) -> Option<char> {
    word.chars()
        .last()
        .filter(|ch| matches!(ch, '.' | '!' | '?'))
}

fn capitalize(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    if let Some(first) = chars.iter_mut().find(|ch| ch.is_alphabetic()) {
        *first = first.to_uppercase().next().unwrap_or(*first);
    }
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::{cleanup, no_speech};

    #[test]
    fn blank_audio_marker_is_not_dictation() {
        assert!(no_speech("[BLANK_AUDIO]."));
        assert!(no_speech(" [blank_audio] "));
        assert!(no_speech("  "));
        assert!(!no_speech("I said blank audio."));
    }

    #[test]
    fn drops_leading_fillers_and_finishes_the_sentence() {
        assert_eq!(cleanup("  um uh hello from whisp  "), "Hello from whisp.");
    }

    #[test]
    fn keeps_punctuation_whisper_already_added() {
        assert_eq!(cleanup("Hello there."), "Hello there.");
    }

    #[test]
    fn drops_fillers_in_the_middle_of_a_sentence() {
        assert_eq!(
            cleanup("Hey, Maya, um, running about ten minutes late."),
            "Hey, Maya, running about ten minutes late."
        );
        assert_eq!(cleanup("I think uh we should go"), "I think we should go.");
    }

    #[test]
    fn a_trailing_filler_hands_its_full_stop_back() {
        assert_eq!(cleanup("Running late, um."), "Running late.");
        assert_eq!(cleanup("Are you coming, uh?"), "Are you coming?");
    }

    #[test]
    fn the_sentence_after_a_filler_still_starts_with_a_capital() {
        assert_eq!(
            cleanup("That works. Um, see you soon."),
            "That works. See you soon."
        );
    }

    #[test]
    fn drops_fillers_in_other_languages() {
        assert_eq!(
            cleanup("Jeg kommer, øh, lidt senere."),
            "Jeg kommer, lidt senere."
        );
        assert_eq!(cleanup("Ähm, ich bin gleich da."), "Ich bin gleich da.");
        assert_eq!(cleanup("Je suis, euh, en retard."), "Je suis, en retard.");
    }

    #[test]
    fn collapses_stuttered_repeats() {
        assert_eq!(
            cleanup("I I think the the car is ready."),
            "I think the car is ready."
        );
        assert_eq!(cleanup("We we'll see."), "We we'll see.");
    }

    #[test]
    fn keeps_deliberate_and_grammatical_doubles() {
        assert_eq!(cleanup("She had had enough."), "She had had enough.");
        assert_eq!(
            cleanup("I know that that is true."),
            "I know that that is true."
        );
        assert_eq!(cleanup("No, no, not yet."), "No, no, not yet.");
        assert_eq!(cleanup("Dial 1 1 2."), "Dial 1 1 2.");
    }
}
