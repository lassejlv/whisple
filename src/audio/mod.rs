mod capture;
mod levels;

pub use capture::{input_names, known_input, Mic};
pub use levels::to_whisper_pcm;

#[cfg(test)]
mod tests {
    use super::capture::{push, Recording};
    use super::{known_input, to_whisper_pcm};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    #[test]
    fn level_monitor_does_not_retain_audio() {
        let samples = Mutex::new(Recording::new(48_000, false));
        let level = AtomicU32::new(0);
        push(&samples, &level, &[0.5, 0.5, -0.5, -0.5], 2, 100, false);
        assert!(f32::from_bits(level.load(Ordering::Relaxed)) > 0.0);
        assert!(samples.lock().unwrap().samples.is_empty());
    }

    #[test]
    fn capture_converts_integer_stereo_and_stops_at_the_sample_limit() {
        let recording = Recording::buffer(16_000, true);
        let level = AtomicU32::new(0);
        let capacity = recording.lock().unwrap().samples.capacity();
        push(
            &recording,
            &level,
            &[i16::MAX, i16::MAX, 0, 0, -i16::MAX, -i16::MAX],
            2,
            2,
            true,
        );
        push(&recording, &level, &[i16::MAX; 8], 2, 2, true);
        let captured = recording.lock().unwrap();
        assert_eq!(captured.samples, [1.0, 0.0]);
        assert_eq!(captured.samples.capacity(), capacity);
    }

    #[test]
    fn unsigned_monitor_does_not_allocate_recording_storage() {
        let recording = Recording::buffer(48_000, false);
        let level = AtomicU32::new(0);
        push(&recording, &level, &[u16::MAX; 480], 1, 960_000, false);
        assert_eq!(recording.lock().unwrap().samples.capacity(), 0);
        assert_eq!(f32::from_bits(level.load(Ordering::Relaxed)), 1.0);
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
