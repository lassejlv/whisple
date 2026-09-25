mod capture;
mod levels;

pub use capture::{input_names, known_input, Mic};
pub use levels::to_whisper_pcm;

#[cfg(test)]
mod tests {
    use super::capture::push;
    use super::{known_input, to_whisper_pcm};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    #[test]
    fn level_monitor_does_not_retain_audio() {
        let samples = Mutex::new(Vec::new());
        let level = AtomicU32::new(0);
        push(&samples, &level, &[0.5, 0.5, -0.5, -0.5], 2, 100, false);
        assert!(f32::from_bits(level.load(Ordering::Relaxed)) > 0.0);
        assert!(samples.lock().unwrap().is_empty());
    }

    #[test]
    fn a_missing_microphone_is_rejected() {
        let names = vec!["Built-in".to_string()];
        assert!(known_input("", &names).is_ok());
        assert!(known_input("Built-in", &names).is_ok());
        assert!(known_input("Studio Mic", &names).is_err());
    }

    #[test]
    fn resamples_to_sixteen_kilohertz() {
        let input = vec![0.0, 1.0, 0.0, -1.0];
        let output = to_whisper_pcm(&input, 8_000);
        assert_eq!(output.len(), 8);
        assert!(output.iter().all(|sample| sample.abs() <= 1.0));
    }
}
