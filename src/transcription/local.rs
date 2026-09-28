use std::path::Path;
use std::sync::Mutex;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::to_whisper_pcm;
use crate::transcription::cleanup::{cleanup, no_speech};

struct CachedModel {
    id: String,
    context: WhisperContext,
}

static CACHE: Mutex<Option<CachedModel>> = Mutex::new(None);

pub fn transcribe(
    model_id: &str,
    model_path: &Path,
    samples: &[f32],
    rate: u32,
    language: Option<&str>,
    clean: bool,
) -> Result<String, String> {
    let pcm = to_whisper_pcm(samples, rate);
    if pcm.len() < 16_000 / 4 {
        return Err("That clip was too short to transcribe.".into());
    }

    whisper_rs::install_logging_hooks();
    let mut cache = CACHE
        .lock()
        .map_err(|_| "The speech engine is busy.".to_string())?;
    let needs_load = cache.as_ref().map(|cached| cached.id.as_str()) != Some(model_id);
    if needs_load {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(false);
        let context = WhisperContext::new_with_params(
            model_path
                .to_str()
                .ok_or("The model path is not valid UTF-8")?,
            params,
        )
        .map_err(|err| format!("Could not load the model: {err}"))?;
        *cache = Some(CachedModel {
            id: model_id.to_string(),
            context,
        });
    }

    let context = &cache.as_ref().expect("model was just loaded").context;
    let mut state = context
        .create_state()
        .map_err(|err| format!("Could not start transcription: {err}"))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let threads = std::thread::available_parallelism()
        .map(|count| count.get().min(4) as i32)
        .unwrap_or(2);
    params.set_n_threads(threads);
    // No language already means auto-detect. whisper.cpp's detect_language
    // flag detects and then returns without transcribing anything.
    params.set_language(language);
    params.set_detect_language(false);
    params.set_translate(false);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_no_context(true);
    params.set_suppress_blank(true);

    state
        .full(params, &pcm)
        .map_err(|err| format!("Transcription failed: {err}"))?;

    let mut pieces = Vec::new();
    for index in 0..state.full_n_segments() {
        let Some(segment) = state.get_segment(index) else {
            continue;
        };
        pieces.push(
            segment
                .to_str()
                .map_err(|err| format!("Transcription failed: {err}"))?
                .to_string(),
        );
    }
    let raw = join_segments(pieces.iter().map(String::as_str));

    let text = if clean {
        cleanup(&raw)
    } else {
        raw.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    if no_speech(&text) {
        Err("No speech came through.".into())
    } else {
        Ok(text)
    }
}

/// Joins Whisper segments with one space, except next to Chinese or Japanese
/// text, which is written without spaces between words.
fn join_segments<'a>(pieces: impl IntoIterator<Item = &'a str>) -> String {
    let mut raw = String::new();
    for piece in pieces {
        let piece = piece.trim();
        let Some(first) = piece.chars().next() else {
            continue;
        };
        if let Some(last) = raw.chars().last() {
            if !unspaced_script(last) && !unspaced_script(first) {
                raw.push(' ');
            }
        }
        raw.push_str(piece);
    }
    raw
}

fn unspaced_script(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3000..=0x30FF // CJK punctuation, hiragana, katakana
            | 0x3400..=0x4DBF // CJK extension A
            | 0x4E00..=0x9FFF // CJK unified ideographs
            | 0xF900..=0xFAFF // CJK compatibility ideographs
            | 0xFF00..=0xFFEF // full-width forms
    )
}

#[cfg(test)]
mod tests {
    use super::join_segments;

    #[test]
    fn segments_are_separated_by_one_space() {
        let pieces = [" Hey, Maya, running ten minutes late.", " Grab us a table."];
        assert_eq!(
            join_segments(pieces),
            "Hey, Maya, running ten minutes late. Grab us a table."
        );
        assert_eq!(join_segments(["one", "two"]), "one two");
    }

    #[test]
    fn blank_segments_add_no_space() {
        assert_eq!(
            join_segments([" First.", "  ", "", " Second."]),
            "First. Second."
        );
    }

    #[test]
    fn chinese_and_japanese_segments_join_without_spaces() {
        assert_eq!(join_segments(["我们走吧。", "好的。"]), "我们走吧。好的。");
        assert_eq!(
            join_segments(["今日は。", "ありがとう。"]),
            "今日は。ありがとう。"
        );
    }
}
