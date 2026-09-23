/// Light cleanup for dictated text. Whisper already punctuates; this only
/// drops leading filler and makes sure the note reads like a sentence.
pub fn cleanup(raw: &str) -> String {
    let mut words: Vec<&str> = raw.split_whitespace().collect();
    while words.first().is_some_and(|word| is_filler(word)) {
        words.remove(0);
    }

    let mut text = words.join(" ");
    text = text
        .replace(" ,", ",")
        .replace(" .", ".")
        .replace(" ?", "?");
    if text.is_empty() {
        return text;
    }

    let mut chars: Vec<char> = text.chars().collect();
    if let Some(first) = chars.iter_mut().find(|ch| ch.is_alphabetic()) {
        *first = first.to_ascii_uppercase();
    }
    text = chars.into_iter().collect();

    if !text.ends_with(['.', '!', '?']) {
        text.push('.');
    }
    text
}

fn is_filler(word: &str) -> bool {
    matches!(
        word.trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
            .to_ascii_lowercase()
            .as_str(),
        "um" | "uh" | "erm" | "uhh" | "hmm" | "ah"
    )
}

#[cfg(test)]
mod tests {
    use super::cleanup;

    #[test]
    fn drops_leading_fillers_and_finishes_the_sentence() {
        assert_eq!(cleanup("  um uh hello from whisp  "), "Hello from whisp.");
    }

    #[test]
    fn keeps_punctuation_whisper_already_added() {
        assert_eq!(cleanup("Hello there."), "Hello there.");
    }
}
