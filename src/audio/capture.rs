use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::levels::{store_energy, Resampler, WHISPER_RATE};

const MAX_SECONDS: usize = 60;

pub struct Mic {
    samples: Arc<Mutex<Recording>>,
    level: Arc<AtomicU32>,
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
        let samples = Arc::new(Recording::buffer(rate, capture_samples));
        let level = Arc::new(AtomicU32::new(0));
        let max_samples = WHISPER_RATE as usize * MAX_SECONDS;

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
                            push(
                                &samples,
                                &level,
                                data,
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
                            push(
                                &samples,
                                &level,
                                data,
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
            _stream: stream,
        })
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    pub fn take(self) -> (Vec<f32>, u32) {
        // Stop the callback before taking its buffer; recording never competes
        // with a reader for the mutex, and no samples arrive after the take.
        let Self {
            samples, _stream, ..
        } = self;
        drop(_stream);
        let audio = samples
            .lock()
            .map(|mut recording| {
                let Recording { samples, resampler } = &mut *recording;
                if let Some(resampler) = resampler {
                    resampler.finish(samples, WHISPER_RATE as usize * MAX_SECONDS);
                }
                std::mem::take(samples)
            })
            .unwrap_or_default();
        (audio, WHISPER_RATE)
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

pub(super) struct Recording {
    pub(super) samples: Vec<f32>,
    resampler: Option<Resampler>,
}

impl Recording {
    pub(super) fn buffer(rate: u32, capture: bool) -> Mutex<Self> {
        let buffer = Mutex::new(Self::new(rate, capture));
        // Some platforms allocate native mutex storage on first lock. Do that
        // during setup rather than on the audio callback's first invocation.
        drop(buffer.lock().unwrap());
        buffer
    }

    pub(super) fn new(rate: u32, capture: bool) -> Self {
        Self {
            samples: if capture {
                Vec::with_capacity(WHISPER_RATE as usize * MAX_SECONDS)
            } else {
                Vec::new()
            },
            resampler: capture.then(|| Resampler::new(rate)),
        }
    }
}

pub(super) trait Sample: Copy {
    fn normalized(self) -> f32;
}
impl Sample for f32 {
    fn normalized(self) -> f32 {
        self
    }
}
impl Sample for i16 {
    fn normalized(self) -> f32 {
        self as f32 / i16::MAX as f32
    }
}
impl Sample for u16 {
    fn normalized(self) -> f32 {
        (self as f32 / u16::MAX as f32) * 2.0 - 1.0
    }
}

pub(super) fn push<S: Sample>(
    recording: &Mutex<Recording>,
    level: &AtomicU32,
    data: &[S],
    channels: usize,
    max: usize,
    capture_samples: bool,
) {
    let channels = channels.max(1);
    let mut recording = capture_samples.then(|| recording.lock().ok()).flatten();
    let mut energy = 0.0;
    let mut frames = 0;
    for frame in data.chunks(channels) {
        let mono = frame.iter().map(|sample| sample.normalized()).sum::<f32>() / frame.len() as f32;
        energy += mono * mono;
        frames += 1;
        if let Some(recording) = recording.as_deref_mut() {
            let Recording { samples, resampler } = recording;
            if samples.len() < max {
                if let Some(resampler) = resampler {
                    resampler.push(mono, samples, max);
                }
            }
        }
    }
    store_energy(level, energy, frames);
}
