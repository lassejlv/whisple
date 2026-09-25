//! Optional file transcription with the user's OpenAI, Groq, xAI or Vercel
//! AI Gateway account.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use base64::Engine;
use reqwest::blocking::{multipart, Client};
use serde::Deserialize;
use serde_json::{json, Value};

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
    Xai,
    /// Vercel AI Gateway: one key for the OpenAI and Grok models.
    Vercel,
}

impl Provider {
    pub const ALL: [Self; 4] = [Self::OpenAi, Self::Groq, Self::Xai, Self::Vercel];

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "cloud-openai" => Some(Self::OpenAi),
            "cloud-groq" => Some(Self::Groq),
            "cloud-xai" => Some(Self::Xai),
            "cloud-vercel" => Some(Self::Vercel),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "cloud-openai",
            Self::Groq => "cloud-groq",
            Self::Xai => "cloud-xai",
            Self::Vercel => "cloud-vercel",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::OpenAi => 0,
            Self::Groq => 1,
            Self::Xai => 2,
            Self::Vercel => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Groq => "Groq",
            Self::Xai => "xAI",
            Self::Vercel => "Vercel",
        }
    }

    pub fn model(self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-transcribe",
            Self::Groq => "whisper-large-v3-turbo",
            Self::Xai => "grok-voice-transcribe-2.0",
            Self::Vercel => gateway_model().transcription(),
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::OpenAi => "GPT Transcribe · clear, accurate",
            Self::Groq => "Whisper v3 Turbo · fast, low cost",
            Self::Xai => "Grok STT 2 · accurate, low cost",
            Self::Vercel => "AI Gateway · OpenAI or Grok, one key",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::OpenAi => "icons/whisp/openai.svg",
            Self::Groq => "icons/whisp/groq-mark.svg",
            Self::Xai => "icons/whisp/xai.svg",
            Self::Vercel => "icons/whisp/vercel.svg",
        }
    }

    fn endpoint(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1/audio/transcriptions",
            Self::Groq => "https://api.groq.com/openai/v1/audio/transcriptions",
            Self::Xai => "https://api.x.ai/v1/stt",
            Self::Vercel => "https://ai-gateway.vercel.sh/v4/ai/transcription-model",
        }
    }
}

/// The model Vercel AI Gateway transcribes with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayModel {
    Grok,
    OpenAi,
}

impl GatewayModel {
    pub const ALL: [Self; 2] = [Self::Grok, Self::OpenAi];

    pub fn id(self) -> &'static str {
        match self {
            Self::Grok => "grok",
            Self::OpenAi => "openai",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.id() == id)
    }

    pub fn name(self) -> &'static str {
        match (self, self.fell_back()) {
            (Self::Grok, false) => "Grok STT 2",
            (Self::Grok, true) => "Grok STT",
            (Self::OpenAi, false) => "GPT Transcribe",
            (Self::OpenAi, true) => "GPT-4o Transcribe",
        }
    }

    /// The gateway id Whisple transcribes with: the newest model, or the one
    /// the gateway lists when it did not know the newest.
    pub fn transcription(self) -> &'static str {
        if self.fell_back() {
            self.listed()
        } else {
            self.newest()
        }
    }

    fn newest(self) -> &'static str {
        match self {
            Self::Grok => "spacexai/grok-voice-transcribe-2.0",
            Self::OpenAi => "openai/gpt-transcribe",
        }
    }

    /// The model id the gateway publishes. `spacexai/grok-stt` names no
    /// version.
    fn listed(self) -> &'static str {
        match self {
            Self::Grok => "spacexai/grok-stt",
            Self::OpenAi => "openai/gpt-4o-transcribe",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Grok => 0,
            Self::OpenAi => 1,
        }
    }

    fn fell_back(self) -> bool {
        FELL_BACK[self.index()].load(Ordering::Relaxed)
    }

    /// The chat model the assistant and translation use through the gateway.
    pub fn chat(self) -> &'static str {
        match self {
            Self::Grok => "spacexai/grok-4.7",
            Self::OpenAi => "openai/gpt-5.6-luna",
        }
    }

    /// The provider options that carry the spoken language. The gateway hands
    /// these to the model's own provider.
    fn language_options(self, language: &str) -> Value {
        match self {
            Self::Grok => json!({
                "xai": {"language": language},
                "spacexai": {"language": language},
            }),
            Self::OpenAi => json!({"openai": {"language": language}}),
        }
    }
}

static GATEWAY_MODEL: AtomicU8 = AtomicU8::new(0);

/// Set once the gateway answers that it does not know a model's newest id,
/// so later recordings go straight to the listed one until Whisple restarts.
static FELL_BACK: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

pub fn gateway_model() -> GatewayModel {
    GatewayModel::ALL
        .get(usize::from(GATEWAY_MODEL.load(Ordering::Relaxed)))
        .copied()
        .unwrap_or(GatewayModel::Grok)
}

pub fn set_gateway_model(model: GatewayModel) {
    let index = GatewayModel::ALL
        .iter()
        .position(|candidate| *candidate == model)
        .unwrap_or(0);
    GATEWAY_MODEL.store(index as u8, Ordering::Relaxed);
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

pub(crate) fn load_key(provider: Provider) -> Result<String, TranscriptionError> {
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
    let language = language.filter(|language| *language != "auto");
    let client = Client::builder()
        .timeout(Duration::from_secs(90))
        .user_agent("Whisple/0.1")
        .build()
        .map_err(|err| {
            TranscriptionError::Other(format!("Could not start cloud transcription: {err}"))
        })?;
    let response = if provider == Provider::Vercel {
        let model = gateway_model();
        let body = gateway_body(model, &wav, language);
        let send = |id: &str| {
            client
                .post(endpoint)
                .bearer_auth(key)
                .header("ai-gateway-protocol-version", "0.0.1")
                .header("ai-gateway-auth-method", "api-key")
                .header("ai-transcription-model-specification-version", "4")
                .header("ai-model-id", id)
                .json(&body)
                .send()
        };
        match send(model.transcription()) {
            // The newest id is not on the gateway yet: use the listed one.
            Ok(first) if !model.fell_back() && matches!(first.status().as_u16(), 400 | 404) => {
                let status = first.status().as_u16();
                let detail = first.text().unwrap_or_default();
                if status == 404 || detail.contains("model_not_found") {
                    FELL_BACK[model.index()].store(true, Ordering::Relaxed);
                    send(model.listed())
                } else {
                    return Err(response_error(provider, status));
                }
            }
            response => response,
        }
    } else {
        let request = client.post(endpoint).bearer_auth(key);
        let file = multipart::Part::bytes(wav)
            .file_name("recording.wav")
            .mime_str("audio/wav")
            .map_err(|err| TranscriptionError::Other(err.to_string()))?;
        let mut form = multipart::Form::new().text("model", provider.model());
        if let Some(language) = language {
            form = form.text(
                if provider == Provider::OpenAi {
                    "languages[]"
                } else {
                    "language"
                },
                language.to_string(),
            );
        }
        // xAI reads the fields in order and needs the file after the others.
        request.multipart(form.part("file", file)).send()
    };
    let response = response.map_err(|err| {
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

/// The gateway's transcription request: the audio as base64 JSON, with the
/// model named in the `ai-model-id` header.
fn gateway_body(model: GatewayModel, wav: &[u8], language: Option<&str>) -> Value {
    let mut body = json!({
        "audio": base64::engine::general_purpose::STANDARD.encode(wav),
        "mediaType": "audio/wav",
    });
    if let Some(language) = language {
        body["providerOptions"] = model.language_options(language);
    }
    body
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
    fn gateway_models_round_trip_their_ids() {
        for model in GatewayModel::ALL {
            assert_eq!(GatewayModel::from_id(model.id()), Some(model));
        }
        assert_eq!(GatewayModel::from_id("whisper"), None);
    }

    #[test]
    fn the_gateway_gets_base64_audio_and_the_language_for_its_provider() {
        let body = gateway_body(GatewayModel::OpenAi, b"RIFF", Some("da"));
        assert_eq!(body["audio"], "UklGRg==");
        assert_eq!(body["mediaType"], "audio/wav");
        assert_eq!(body["providerOptions"]["openai"]["language"], "da");
        let body = gateway_body(GatewayModel::Grok, b"RIFF", Some("de"));
        assert_eq!(body["providerOptions"]["xai"]["language"], "de");
        let body = gateway_body(GatewayModel::Grok, b"RIFF", None);
        assert!(body.get("providerOptions").is_none());
    }

    /// Tests run in parallel and share the gateway model, so none change it
    /// from the default, Grok, and only this one lets it fall back.
    #[test]
    fn the_gateway_asks_for_grok_stt_2_then_the_listed_model() {
        assert_eq!(
            Provider::Vercel.model(),
            "spacexai/grok-voice-transcribe-2.0"
        );
        assert_eq!(GatewayModel::Grok.name(), "Grok STT 2");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/v4/ai/transcription-model",
            listener.local_addr().unwrap()
        );
        let server = thread::spawn(move || {
            let mut models = Vec::new();
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0u8; 8192];
                let body_start = loop {
                    let count = socket.read(&mut chunk).unwrap();
                    assert!(count > 0, "request ended early");
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&request[..end]).to_lowercase();
                        let size: usize = header
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .unwrap()
                            .trim()
                            .parse()
                            .unwrap();
                        if request.len() >= end + 4 + size {
                            break end + 4;
                        }
                    }
                };
                let head = String::from_utf8_lossy(&request[..body_start]).to_lowercase();
                assert!(head.contains("authorization: bearer test-key"));
                assert!(head.contains("ai-transcription-model-specification-version: 4"));
                models.push(
                    head.lines()
                        .find_map(|line| line.strip_prefix("ai-model-id: "))
                        .unwrap()
                        .trim()
                        .to_string(),
                );
                let body: Value = serde_json::from_slice(&request[body_start..]).unwrap();
                assert_eq!(body["mediaType"], "audio/wav");
                assert_eq!(body["providerOptions"]["xai"]["language"], "de");
                let wav = base64::engine::general_purpose::STANDARD
                    .decode(body["audio"].as_str().unwrap())
                    .unwrap();
                assert_eq!(&wav[..4], b"RIFF");
                let (status, response) = if attempt == 0 {
                    (
                        "404 Not Found",
                        r#"{"error":{"message":"Model not found","type":"model_not_found"}}"#,
                    )
                } else {
                    (
                        "200 OK",
                        r#"{"text":"Hallo Welt.","segments":[],"language":"de"}"#,
                    )
                };
                write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
            }
            models
        });
        let result = transcribe_with_key(
            Provider::Vercel,
            &url,
            "test-key",
            &vec![0.25; 8_000],
            16_000,
            Some("de"),
            false,
        )
        .unwrap();
        assert_eq!(result, "Hallo Welt.");
        assert_eq!(
            server.join().unwrap(),
            ["spacexai/grok-voice-transcribe-2.0", "spacexai/grok-stt"]
        );
        // Later recordings and the model's label use the listed model.
        assert_eq!(Provider::Vercel.model(), "spacexai/grok-stt");
        assert_eq!(GatewayModel::Grok.name(), "Grok STT");
    }

    #[test]
    fn upload_audio_is_mono_16_khz_pcm() {
        let bytes = encode_wav(&vec![0.25; 8_000], 16_000).unwrap();
        let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        let samples = reader
            .samples::<i16>()
            .map(|sample| f32::from(sample.unwrap()) / f32::from(i16::MAX))
            .collect::<Vec<_>>();
        assert_eq!(samples.len(), 8_000);
        assert!((samples[0] - 0.25).abs() < 0.001);
    }

    #[test]
    fn every_provider_sends_its_model_and_language_before_the_file() {
        for (provider, language_field) in [
            (Provider::OpenAi, "languages[]"),
            (Provider::Groq, "language"),
            (Provider::Xai, "language"),
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
                let file_at = body.find("name=\"file\"").unwrap();
                assert!(body.find("name=\"model\"").unwrap() < file_at);
                assert!(body.find(&format!("name=\"{language_field}\"")).unwrap() < file_at);
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
