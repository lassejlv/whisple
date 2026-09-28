use std::borrow::Cow;
use std::sync::atomic::{AtomicU32, Ordering};

pub(super) const WHISPER_RATE: u32 = 16_000;
const TAPS: usize = 64;
const PHASES: usize = 256;
const HALF: usize = TAPS / 2;

pub(super) fn store_energy(level: &AtomicU32, energy: f32, frames: usize) {
    if frames != 0 {
        let boosted = ((energy / frames as f32).sqrt() * 8.0).clamp(0.0, 1.0);
        level.store(boosted.to_bits(), Ordering::Relaxed);
    }
}

/// Streaming low-pass resampling with fixed storage. Coefficients are prepared
/// before opening the microphone. No allocations, trigonometry or buffer moves
/// occur while processing audio. The small lookahead is flushed on stop.
pub(super) struct Resampler {
    rate: u32,
    coefficients: Vec<[f32; TAPS]>,
    history: [f32; TAPS],
    input_frames: usize,
    output_frames: usize,
}

impl Resampler {
    pub(super) fn new(rate: u32) -> Self {
        let rate = rate.max(1);
        let coefficients = if rate == WHISPER_RATE {
            Vec::new()
        } else {
            // Leave a transition band below the lower Nyquist frequency.
            let cutoff = (WHISPER_RATE as f64 / rate as f64).min(1.0) * 0.90;
            (0..PHASES)
                .map(|phase| {
                    let fraction = phase as f64 / PHASES as f64;
                    let mut weights = [0.0; TAPS];
                    let mut sum = 0.0;
                    for (index, weight) in weights.iter_mut().enumerate() {
                        let distance = index as f64 - (HALF - 1) as f64 - fraction;
                        let x = std::f64::consts::PI * distance * cutoff;
                        let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin() / x };
                        let window =
                            0.5 + 0.5 * (std::f64::consts::PI * distance / HALF as f64).cos();
                        *weight = (cutoff * sinc * window) as f32;
                        sum += *weight;
                    }
                    for weight in &mut weights {
                        *weight /= sum;
                    }
                    weights
                })
                .collect()
        };
        Self {
            rate,
            coefficients,
            history: [0.0; TAPS],
            input_frames: 0,
            output_frames: 0,
        }
    }

    pub(super) fn push(&mut self, sample: f32, output: &mut Vec<f32>, limit: usize) {
        if self.rate == WHISPER_RATE {
            self.input_frames += 1;
            self.output_frames += 1;
            if output.len() < limit {
                output.push(sample);
            }
            return;
        }
        self.history[self.input_frames % TAPS] = sample;
        self.input_frames += 1;
        loop {
            let numerator = self.output_frames as u64 * u64::from(self.rate);
            let center = (numerator / u64::from(WHISPER_RATE)) as usize;
            if center + HALF >= self.input_frames || output.len() >= limit {
                break;
            }
            let phase = ((numerator % u64::from(WHISPER_RATE)) * PHASES as u64
                / u64::from(WHISPER_RATE)) as usize;
            let mut value = 0.0;
            for (tap, weight) in self.coefficients[phase].iter().enumerate() {
                if let Some(index) = (center + tap).checked_sub(HALF - 1) {
                    value += self.history[index % TAPS] * weight;
                }
            }
            output.push(value.clamp(-1.0, 1.0));
            self.output_frames += 1;
        }
    }

    pub(super) fn finish(&mut self, output: &mut Vec<f32>, limit: usize) {
        if self.rate == WHISPER_RATE {
            return;
        }
        let count = ((self.input_frames as u64 * u64::from(WHISPER_RATE)
            + u64::from(self.rate) / 2)
            / u64::from(self.rate)) as usize;
        let count = count.min(limit);
        while output.len() < count {
            self.push(0.0, output, count);
        }
    }
}

pub fn to_whisper_pcm(samples: &[f32], rate: u32) -> Cow<'_, [f32]> {
    if rate == WHISPER_RATE || samples.is_empty() {
        return Cow::Borrowed(samples);
    }
    let rate = rate.max(1);
    let count = ((samples.len() as u64 * u64::from(WHISPER_RATE) + u64::from(rate) / 2)
        / u64::from(rate)) as usize;
    let mut output = Vec::with_capacity(count);
    let mut resampler = Resampler::new(rate);
    for &sample in samples {
        resampler.push(sample, &mut output, count);
    }
    resampler.finish(&mut output, count);
    Cow::Owned(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_whisper_audio_is_borrowed() {
        assert!(matches!(
            to_whisper_pcm(&[0.1; 20], 16_000),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn resampling_is_independent_of_callback_boundaries_and_flushes_the_tail() {
        for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
            let input: Vec<_> = (0..rate / 10).map(|n| (n as f32 * 0.13).sin()).collect();
            let whole = to_whisper_pcm(&input, rate);
            assert_eq!(whole.len(), 1600);
            let mut streaming = Resampler::new(rate);
            let mut output = Vec::with_capacity(1600);
            for chunk in input.chunks(137) {
                for &sample in chunk {
                    streaming.push(sample, &mut output, 1600);
                }
            }
            streaming.finish(&mut output, 1600);
            assert_eq!(output.as_slice(), whole.as_ref());
            assert!(output.iter().all(|x| x.is_finite()));
        }
    }

    #[test]
    fn downsampling_rejects_frequencies_above_output_nyquist() {
        let rms = |frequency: f32| {
            let input: Vec<_> = (0..48_000)
                .map(|n| (std::f32::consts::TAU * frequency * n as f32 / 48_000.0).sin())
                .collect();
            let output = to_whisper_pcm(&input, 48_000);
            let steady = &output[100..output.len() - 100];
            (steady.iter().map(|x| x * x).sum::<f32>() / steady.len() as f32).sqrt()
        };
        assert!(rms(1_000.0) > 0.65);
        assert!(rms(12_000.0) < 0.02);
    }
}
