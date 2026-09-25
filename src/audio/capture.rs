use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::levels::{downmix, store_level};

const MAX_SECONDS: usize = 60;

pub struct Mic {
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
    rate: u32,
    _stream: cpal::Stream,
}

impl Mic {
    /// `preferred` is a device name from [`input_names`]. An empty name uses the
    /// system default.
    pub fn start(preferred: &str) -> Result<Self, String> {
        Self::open(preferred, true)
    }

    pub fn monitor(preferred: &str) -> Result<Self, String> {
        Self::open(preferred, false)
    }

    fn open(preferred: &str, capture_samples: bool) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = choose_input(&host, preferred)?;
        let supported = device
            .default_input_config()
            .map_err(|err| err.to_string())?;
        let rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
        let level = Arc::new(AtomicU32::new(0));
        let max_samples = rate as usize * MAX_SECONDS;

        let err_fn = |err| eprintln!("microphone: {err}");
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => {
                let samples = Arc::clone(&samples);
                let level = Arc::clone(&level);
                device
                    .build_input_stream(
                        &supported.into(),
                        move |data: &[f32], _| {
                            push(
                                &samples,
                                &level,
                                data,
                                channels,
                                max_samples,
                                capture_samples,
                            )
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|err| err.to_string())?
            }
            cpal::SampleFormat::I16 => {
                let samples = Arc::clone(&samples);
                let level = Arc::clone(&level);
                device
                    .build_input_stream(
                        &supported.into(),
                        move |data: &[i16], _| {
                            let converted: Vec<f32> = data
                                .iter()
                                .map(|sample| *sample as f32 / i16::MAX as f32)
                                .collect();
                            push(
                                &samples,
                                &level,
                                &converted,
                                channels,
                                max_samples,
                                capture_samples,
                            );
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|err| err.to_string())?
            }
            cpal::SampleFormat::U16 => {
                let samples = Arc::clone(&samples);
                let level = Arc::clone(&level);
                device
                    .build_input_stream(
                        &supported.into(),
                        move |data: &[u16], _| {
                            let converted: Vec<f32> = data
                                .iter()
                                .map(|sample| (*sample as f32 / u16::MAX as f32) * 2.0 - 1.0)
                                .collect();
                            push(
                                &samples,
                                &level,
                                &converted,
                                channels,
                                max_samples,
                                capture_samples,
                            );
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|err| err.to_string())?
            }
            other => return Err(format!("Unsupported microphone format: {other}")),
        };
        stream.play().map_err(|err| err.to_string())?;

        Ok(Self {
            samples,
            level,
            rate,
            _stream: stream,
        })
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    pub fn take(self) -> (Vec<f32>, u32) {
        let samples = self
            .samples
            .lock()
            .map(|mut guard| std::mem::take(&mut *guard))
            .unwrap_or_default();
        (samples, self.rate)
    }
}

pub fn input_names() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(|err| err.to_string())?;
    let mut names = Vec::new();
    for device in devices {
        let Ok(name) = device.name() else {
            continue;
        };
        let name = name.trim().to_string();
        if name.is_empty() || names.iter().any(|existing| existing == &name) {
            continue;
        }
        names.push(name);
    }
    names.sort_by_key(|name| name.to_lowercase());
    Ok(names)
}

pub fn known_input(preferred: &str, names: &[String]) -> Result<(), String> {
    if preferred.is_empty() || names.iter().any(|name| name == preferred) {
        Ok(())
    } else {
        Err(format!("Microphone \"{preferred}\" is not connected."))
    }
}

fn choose_input(host: &cpal::Host, preferred: &str) -> Result<cpal::Device, String> {
    if preferred.is_empty() {
        return host
            .default_input_device()
            .ok_or_else(|| "No microphone found. Try the sample in Models.".to_string());
    }
    let names = input_names()?;
    known_input(preferred, &names)?;
    let devices = host.input_devices().map_err(|err| err.to_string())?;
    for device in devices {
        if device.name().ok().as_deref().map(str::trim) == Some(preferred) {
            return Ok(device);
        }
    }
    Err(format!("Microphone \"{preferred}\" is not connected."))
}

pub(super) fn push(
    samples: &Mutex<Vec<f32>>,
    level: &AtomicU32,
    data: &[f32],
    channels: usize,
    max: usize,
    capture_samples: bool,
) {
    let mono = downmix(data, channels);
    store_level(level, &mono);
    if capture_samples {
        if let Ok(mut guard) = samples.lock() {
            let room = max.saturating_sub(guard.len());
            guard.extend(mono.into_iter().take(room));
        }
    }
}
