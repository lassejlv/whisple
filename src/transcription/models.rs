use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
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
    #[cfg(debug_assertions)]
    if let Some(root) = crate::settings::dev_data_dir() {
        return root.join("models");
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("whisp")
        .join("models")
}

pub fn model_path(spec: &ModelSpec) -> PathBuf {
    models_dir().join(spec.file_name)
}

/// Remove only the files belonging to a catalog model. Callers must first
/// stop any download for this model, since its partial file is also removed.
pub fn uninstall(spec: &ModelSpec) -> Result<(), String> {
    uninstall_in(&models_dir(), spec)
}

fn uninstall_in(dir: &Path, spec: &ModelSpec) -> Result<(), String> {
    let model = dir.join(spec.file_name);
    remove_if_present(&model)?;
    remove_if_present(&model.with_extension("bin.partial"))
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("Could not remove {}: {err}", path.display())),
    }
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
    let url = format!("{HF}/{}", spec.file_name);

    let client = reqwest::blocking::Client::builder()
        .user_agent("whisp")
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()
        .map_err(|err| err.to_string())?;
    let response = client.get(&url).send().map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Download failed ({})", response.status()));
    }

    save_download(response, &dest, received, cancel)
}

fn save_download(
    mut response: impl Read,
    dest: &Path,
    received: &AtomicU64,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    // A cancelled worker may still be blocked in read while its replacement starts.
    // Unique sibling files keep its cleanup and writes isolated from that worker.
    let mut file = tempfile::Builder::new()
        .prefix(".whisple-download-")
        .tempfile_in(dest.parent().ok_or("Missing model directory")?)
        .map_err(|err| err.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
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
    if cancel.load(Ordering::Relaxed) {
        return Err("Download cancelled".into());
    }
    file.flush().map_err(|err| err.to_string())?;
    let size = file
        .as_file()
        .metadata()
        .map_err(|err| err.to_string())?
        .len();
    if size < 1_000_000 {
        return Err("The download was incomplete.".into());
    }
    file.persist(dest).map_err(|err| err.to_string())?;
    Ok(dest.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PausedRead {
        data: std::io::Cursor<Vec<u8>>,
        entered: Option<std::sync::mpsc::Sender<()>>,
        resume: std::sync::mpsc::Receiver<()>,
    }

    impl Read for PausedRead {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
                self.resume
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
            }
            self.data.read(buf)
        }
    }

    #[test]
    fn cancelled_download_cannot_delete_a_restarted_downloads_file() {
        use std::sync::{mpsc::channel, Arc};
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.bin");
        let old_cancel = Arc::new(AtomicBool::new(false));
        let (old_entered_tx, old_entered) = channel();
        let (old_resume, old_resume_rx) = channel();
        let old_dest = dest.clone();
        let worker_cancel = old_cancel.clone();
        let old = std::thread::spawn(move || {
            save_download(
                PausedRead {
                    data: std::io::Cursor::new(vec![1; 1_000_001]),
                    entered: Some(old_entered_tx),
                    resume: old_resume_rx,
                },
                &old_dest,
                &AtomicU64::new(0),
                &worker_cancel,
            )
        });
        old_entered
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        old_cancel.store(true, Ordering::Relaxed);
        let (new_entered_tx, new_entered) = channel();
        let (new_resume, new_resume_rx) = channel();
        let new_dest = dest.clone();
        let new = std::thread::spawn(move || {
            save_download(
                PausedRead {
                    data: std::io::Cursor::new(vec![2; 1_000_001]),
                    entered: Some(new_entered_tx),
                    resume: new_resume_rx,
                },
                &new_dest,
                &AtomicU64::new(0),
                &AtomicBool::new(false),
            )
        });
        new_entered
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        old_resume.send(()).unwrap();
        assert_eq!(old.join().unwrap().unwrap_err(), "Download cancelled");
        new_resume.send(()).unwrap();
        assert_eq!(new.join().unwrap().unwrap(), dest);
        assert_eq!(fs::read(&dest).unwrap(), vec![2; 1_000_001]);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn cancellation_at_eof_does_not_publish_the_model() {
        struct CancelAtEof<'a> {
            data: std::io::Cursor<Vec<u8>>,
            cancel: &'a AtomicBool,
        }
        impl Read for CancelAtEof<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let count = self.data.read(buf)?;
                if count == 0 {
                    self.cancel.store(true, Ordering::Relaxed);
                }
                Ok(count)
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.bin");
        let cancel = AtomicBool::new(false);
        let result = save_download(
            CancelAtEof {
                data: std::io::Cursor::new(vec![1; 1_000_001]),
                cancel: &cancel,
            },
            &dest,
            &AtomicU64::new(0),
            &cancel,
        );
        assert_eq!(result.unwrap_err(), "Download cancelled");
        assert!(!dest.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn uninstall_removes_only_the_catalog_models_files() {
        let dir = std::env::temp_dir().join(format!("whisp-uninstall-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let spec = &CATALOG[0];
        let model = dir.join(spec.file_name);
        let partial = model.with_extension("bin.partial");
        let other = dir.join(CATALOG[1].file_name);
        fs::write(&model, b"model").unwrap();
        fs::write(&partial, b"partial").unwrap();
        fs::write(&other, b"keep").unwrap();

        uninstall_in(&dir, spec).unwrap();
        assert!(!model.exists());
        assert!(!partial.exists());
        assert_eq!(fs::read(&other).unwrap(), b"keep");
        uninstall_in(&dir, spec).unwrap();

        fs::remove_dir_all(dir).unwrap();
    }
}
