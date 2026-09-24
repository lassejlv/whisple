//! Optional file transcription with the user's OpenAI or Groq account.

use std::io::Cursor;
use std::time::Duration;

use reqwest::blocking::{multipart, Client};
use serde::Deserialize;

use crate::audio::to_whisper_pcm;
use crate::text::cleanup;

#[derive(Debug)]
pub enum TranscriptionError {
    Offline(String),
    Unauthorized(String),
    RateLimited(String),
    NoSpeech(String),
    Other(String),
}

impl TranscriptionError {
    pub fn message(&self) -> &str {
        match self {
            Self::Offline(message)
            | Self::Unauthorized(message)
            | Self::RateLimited(message)
            | Self::NoSpeech(message)
            | Self::Other(message) => message,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    OpenAi,
    Groq,
}

impl Provider {
    pub const ALL: [Self; 2] = [Self::OpenAi, Self::Groq];

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "cloud-openai" => Some(Self::OpenAi),
            "cloud-groq" => Some(Self::Groq),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "cloud-openai",
            Self::Groq => "cloud-groq",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::OpenAi => 0,
            Self::Groq => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Groq => "Groq",
        }
    }

    pub fn model(self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-transcribe",
            Self::Groq => "whisper-large-v3-turbo",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::OpenAi => "GPT Transcribe · clear, accurate",
            Self::Groq => "Whisper v3 Turbo · fast, low cost",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::OpenAi => "icons/whisp/openai.svg",
            Self::Groq => "icons/whisp/groq-mark.svg",
        }
    }

    fn endpoint(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1/audio/transcriptions",
            Self::Groq => "https://api.groq.com/openai/v1/audio/transcriptions",
        }
    }
}

fn entry(provider: Provider) -> Result<keyring::Entry, String> {
    keyring::Entry::new("app.whisple.cloud", provider.id())
        .map_err(|err| format!("Could not open the system credential store: {err}"))
}

pub fn has_key(provider: Provider) -> Result<bool, String> {
    match entry(provider)?.get_password() {
        Ok(key) => Ok(!key.trim().is_empty()),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(err) => Err(format!("Could not read the {} key: {err}", provider.name())),
    }
}

pub fn save_key(provider: Provider, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        return Err("Enter an API key without spaces or line breaks.".into());
    }
    entry(provider)?
        .set_password(key)
        .map_err(|err| format!("Could not save the {} key: {err}", provider.name()))
}

pub fn delete_key(provider: Provider) -> Result<(), String> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(format!(
            "Could not remove the {} key: {err}",
            provider.name()
        )),
    }
}

fn load_key(provider: Provider) -> Result<String, TranscriptionError> {
    match entry(provider)
        .map_err(TranscriptionError::Other)?
        .get_password()
    {
        Ok(key) if !key.trim().is_empty() => Ok(key),
        Ok(_) | Err(keyring::Error::NoEntry) => Err(TranscriptionError::Unauthorized(format!(
            "Add your {} API key in Voice models before recording.",
            provider.name()
        ))),
        Err(err) => Err(TranscriptionError::Other(format!(
            "Could not read the {} key: {err}",
            provider.name()
        ))),
    }
}

pub fn transcribe(
    provider: Provider,
    samples: &[f32],
    rate: u32,
    language: Option<&str>,
    clean: bool,
) -> Result<String, TranscriptionError> {
    let key = load_key(provider)?;
    transcribe_with_key(
        provider,
        provider.endpoint(),
        &key,
        samples,
        rate,
        language,
        clean,
    )
}

fn transcribe_with_key(
    provider: Provider,
    endpoint: &str,
    key: &str,
    samples: &[f32],
    rate: u32,
    language: Option<&str>,
    clean: bool,
) -> Result<String, TranscriptionError> {
    let wav = encode_wav(samples, rate).map_err(|message| {
        if message == "That clip was too short to transcribe." {
            TranscriptionError::NoSpeech(message)
        } else {
            TranscriptionError::Other(message)
        }
    })?;
    let file = multipart::Part::bytes(wav)
        .file_name("recording.wav")
        .mime_str("audio/wav")
        .map_err(|err| TranscriptionError::Other(err.to_string()))?;
    let mut form = multipart::Form::new()
        .text("model", provider.model())
        .part("file", file);
    if let Some(language) = language.filter(|language| *language != "auto") {
        form = form.text(
            if provider == Provider::OpenAi {
                "languages[]"
            } else {
                "language"
            },
            language.to_string(),
        );
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(90))
        .user_agent("Whisple/0.1")
        .build()
        .map_err(|err| {
            TranscriptionError::Other(format!("Could not start cloud transcription: {err}"))
        })?;
    let response = client
        .post(endpoint)
        .bearer_auth(key)
        .multipart(form)
        .send()
        .map_err(|err| {
            TranscriptionError::Offline(format!("Could not reach {}: {err}", provider.name()))
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(response_error(provider, status.as_u16()));
    }
    let transcript: Transcript = response.json().map_err(|err| {
        TranscriptionError::Other(format!(
            "Could not read the {} transcript: {err}",
            provider.name()
        ))
    })?;
    let text = if clean {
        cleanup(&transcript.text)
    } else {
        transcript
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    if text.is_empty() {
        Err(TranscriptionError::NoSpeech(
            "No speech came through.".into(),
        ))
    } else {
        Ok(text)
    }
}

fn response_error(provider: Provider, status: u16) -> TranscriptionError {
    match status {
        401 | 403 => TranscriptionError::Unauthorized(format!(
            "{} rejected the API key. Edit it in Voice models.",
            provider.name()
        )),
        429 => TranscriptionError::RateLimited(format!(
            "{} is rate limited. Try again shortly.",
            provider.name()
        )),
        _ => TranscriptionError::Other(format!(
            "{} transcription failed (HTTP {status}).",
            provider.name()
        )),
    }
}

#[derive(Deserialize)]
struct Transcript {
    text: String,
}

fn encode_wav(samples: &[f32], rate: u32) -> Result<Vec<u8>, String> {
    let pcm = to_whisper_pcm(samples, rate);
    if pcm.len() < 16_000 / 4 {
        return Err("That clip was too short to transcribe.".into());
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut output = Cursor::new(Vec::with_capacity(44 + pcm.len() * 2));
    {
        let mut writer = hound::WavWriter::new(&mut output, spec)
            .map_err(|err| format!("Could not prepare the recording: {err}"))?;
        for sample in pcm {
            let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer
                .write_sample(value)
                .map_err(|err| format!("Could not prepare the recording: {err}"))?;
        }
        writer
            .finalize()
            .map_err(|err| format!("Could not prepare the recording: {err}"))?;
    }
    Ok(output.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn cloud_ids_are_distinct_from_local_models() {
        for provider in Provider::ALL {
            assert_eq!(Provider::from_id(provider.id()), Some(provider));
            assert!(crate::models::spec(provider.id()).is_none());
        }
        assert_eq!(Provider::from_id("turbo-q5"), None);
    }

    #[test]
    fn http_failures_keep_recovery_categories() {
        assert!(matches!(
            response_error(Provider::Groq, 401),
            TranscriptionError::Unauthorized(_)
        ));
        assert!(matches!(
            response_error(Provider::Groq, 403),
            TranscriptionError::Unauthorized(_)
        ));
        assert!(matches!(
            response_error(Provider::OpenAi, 429),
            TranscriptionError::RateLimited(_)
        ));
        assert!(matches!(
            response_error(Provider::OpenAi, 500),
            TranscriptionError::Other(_)
        ));
    }

    #[test]
    fn short_cloud_recording_uses_no_speech_recovery() {
        let result = transcribe_with_key(
            Provider::Groq,
            "http://127.0.0.1:0/audio/transcriptions",
            "test-key",
            &[0.25; 100],
            16_000,
            None,
            false,
        );
        assert!(matches!(result, Err(TranscriptionError::NoSpeech(_))));
    }

    #[test]
    fn upload_audio_is_mono_16_khz_pcm() {
        let bytes = encode_wav(&vec![0.25; 8_000], 16_000).unwrap();
        let (samples, rate) = crate::audio::decode_wav(&bytes).unwrap();
        assert_eq!(rate, 16_000);
        assert_eq!(samples.len(), 8_000);
        assert!((samples[0] - 0.25).abs() < 0.001);
    }

    #[test]
    fn both_providers_send_their_model_and_language_fields() {
        for (provider, language_field) in [
            (Provider::OpenAi, "languages[]"),
            (Provider::Groq, "language"),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!(
                "http://{}/audio/transcriptions",
                listener.local_addr().unwrap()
            );
            let server = thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let count = socket.read(&mut chunk).unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(header_end) =
                        request.windows(4).position(|part| part == b"\r\n\r\n")
                    {
                        let header = String::from_utf8_lossy(&request[..header_end]);
                        let size = header
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap();
                        if request.len() >= header_end + 4 + size {
                            break;
                        }
                    }
                }
                let body = String::from_utf8_lossy(&request);
                assert!(
                    body.contains("authorization: Bearer test-key")
                        || body.contains("Authorization: Bearer test-key")
                );
                assert!(body.contains(&format!("name=\"model\"\r\n\r\n{}", provider.model())));
                assert!(body.contains(&format!("name=\"{language_field}\"\r\n\r\nda")));
                assert!(request.windows(4).any(|part| part == b"RIFF"));
                let response = "{\"text\":\"Hej verden.\"}";
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
            });
            let result = transcribe_with_key(
                provider,
                &url,
                "test-key",
                &vec![0.25; 8_000],
                16_000,
                Some("da"),
                false,
            )
            .unwrap();
            assert_eq!(result, "Hej verden.");
            server.join().unwrap();
        }
    }
}
