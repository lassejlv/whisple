use std::path::Path;
use std::sync::Mutex;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::to_whisper_pcm;
use crate::text::cleanup;

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
    params.set_language(language);
    params.set_detect_language(language.is_none());
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

    let segments = state
        .full_n_segments()
        .map_err(|err| format!("Transcription failed: {err}"))?;
    let mut raw = String::new();
    for index in 0..segments {
        let piece = state
            .full_get_segment_text(index)
            .map_err(|err| format!("Transcription failed: {err}"))?;
        if !raw.is_empty() && !piece.starts_with(' ') {
            raw.push(' ');
        }
        raw.push_str(piece.trim());
    }

    let text = if clean {
        cleanup(&raw)
    } else {
        raw.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    if text.is_empty() {
        Err("No speech came through.".into())
    } else {
        Ok(text)
    }
}
