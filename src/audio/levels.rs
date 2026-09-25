use std::sync::atomic::{AtomicU32, Ordering};

const WHISPER_RATE: u32 = 16_000;

pub(super) fn downmix(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }
    data.chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

pub(super) fn store_level(level: &AtomicU32, samples: &[f32]) {
    if samples.is_empty() {
        return;
    }
    let energy = samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32;
    let boosted = (energy.sqrt() * 8.0).clamp(0.0, 1.0);
    level.store(boosted.to_bits(), Ordering::Relaxed);
}

pub fn to_whisper_pcm(samples: &[f32], rate: u32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    if rate == WHISPER_RATE {
        return samples.to_vec();
    }
    let ratio = WHISPER_RATE as f32 / rate as f32;
    let out_len = ((samples.len() as f32) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    let last = samples.len() - 1;
    for index in 0..out_len {
        let position = index as f32 / ratio;
        let left = (position.floor() as usize).min(last);
        let right = (left + 1).min(last);
        let mix = position - left as f32;
        out.push(samples[left] * (1.0 - mix) + samples[right] * mix);
    }
    out
}
