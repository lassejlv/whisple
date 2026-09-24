//! Free local speech models, in the ggml format whisper.cpp reads.
//!
//! Large v3 Turbo is the best quality and speed OpenAI published for local
//! dictation. The Q5 file is the default: nearly the same accuracy, about a
//! third of the full download. The smaller English models are there when a
//! person wants something that finishes downloading quickly.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const HF: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Clone, Copy)]
pub struct ModelSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub chip: &'static str,
    pub file_name: &'static str,
    pub bytes: u64,
    pub blurb: &'static str,
    pub recommended: bool,
}

const MIB: u64 = 1024 * 1024;

pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "preview",
        name: "Quick preview",
        chip: "Preview",
        file_name: "ggml-tiny.en-q5_1.bin",
        bytes: 31 * MIB,
        blurb: "English only, lower accuracy",
        recommended: false,
    },
    ModelSpec {
        id: "turbo-q5",
        name: "Turbo",
        chip: "Turbo",
        file_name: "ggml-large-v3-turbo-q5_0.bin",
        bytes: 547 * MIB,
        blurb: "Best balance of speed and accuracy",
        recommended: true,
    },
    ModelSpec {
        id: "turbo-q8",
        name: "Turbo precise",
        chip: "Precise",
        file_name: "ggml-large-v3-turbo-q8_0.bin",
        bytes: 834 * MIB,
        blurb: "Names and punctuation",
        recommended: false,
    },
    ModelSpec {
        id: "turbo",
        name: "Turbo full",
        chip: "Full",
        file_name: "ggml-large-v3-turbo.bin",
        bytes: 1536 * MIB,
        blurb: "Full precision, larger download",
        recommended: false,
    },
    ModelSpec {
        id: "small-en",
        name: "Small English",
        chip: "Small",
        file_name: "ggml-small.en-q5_1.bin",
        bytes: 181 * MIB,
        blurb: "Good accuracy on a laptop CPU",
        recommended: false,
    },
    ModelSpec {
        id: "base-en",
        name: "Base English",
        chip: "Base",
        file_name: "ggml-base.en.bin",
        bytes: 142 * MIB,
        blurb: "Light, for short notes",
        recommended: false,
    },
];

pub fn recommended_id() -> &'static str {
    CATALOG
        .iter()
        .find(|model| model.recommended)
        .map(|model| model.id)
        .unwrap_or(CATALOG[0].id)
}

pub fn spec(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|model| model.id == id)
}

pub fn models_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("whisp")
        .join("models")
}

pub fn model_path(spec: &ModelSpec) -> PathBuf {
    models_dir().join(spec.file_name)
}

pub fn is_downloaded(spec: &ModelSpec) -> bool {
    let path = model_path(spec);
    fs::metadata(&path)
        .map(|meta| meta.len() > 1_000_000)
        .unwrap_or(false)
}

pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * MIB {
        format!("{:.1} GB", bytes as f64 / (1024.0 * MIB as f64))
    } else {
        format!("{} MB", bytes.div_ceil(MIB))
    }
}

pub fn load_selected() -> Option<String> {
    let selected = crate::settings::load().selected;
    if selected.is_empty() {
        None
    } else {
        Some(selected)
    }
}

pub fn save_selected(id: &str) {
    let mut prefs = crate::settings::load();
    prefs.selected = id.to_string();
    crate::settings::save(&prefs);
}

pub fn download(
    spec: &ModelSpec,
    received: &AtomicU64,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let dir = models_dir();
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let dest = model_path(spec);
    let partial = dest.with_extension("bin.partial");
    let url = format!("{HF}/{}", spec.file_name);

    let client = reqwest::blocking::Client::builder()
        .user_agent("whisp")
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()
        .map_err(|err| err.to_string())?;
    let mut response = client.get(&url).send().map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Download failed ({})", response.status()));
    }

    let mut file = File::create(&partial).map_err(|err| err.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = fs::remove_file(&partial);
            return Err("Download cancelled".into());
        }
        let read = response.read(&mut buf).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        file.write_all(&buf[..read])
            .map_err(|err| err.to_string())?;
        received.fetch_add(read as u64, Ordering::Relaxed);
    }
    file.flush().map_err(|err| err.to_string())?;
    drop(file);

    let size = fs::metadata(&partial).map(|meta| meta.len()).unwrap_or(0);
    if size < 1_000_000 {
        let _ = fs::remove_file(&partial);
        return Err("The download was incomplete.".into());
    }
    fs::rename(&partial, &dest).map_err(|err| err.to_string())?;
    Ok(dest)
}
