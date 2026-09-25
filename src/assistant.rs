//! What happens to a transcript that is more than a note: voice commands
//! like "Open Spotify", and requests to the assistant like "Hey Whisple,
//! reply to this email", answered with what is on screen.
//!
//! The assistant uses the user's own OpenAI or Groq key and returns exactly
//! one action: an answer to show, text to type, an app or a site to open.

use std::time::Duration;

use base64::Engine as _;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::apps;
use crate::cloud::{self, Provider, TranscriptionError};
use crate::commands::{self, Command};
use crate::screen::{self, Snapshot};

/// App names beyond this are left out of the request.
const MAX_APPS: usize = 400;

/// What a transcript asks for, decided from the words alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Dictation,
    Command(Command),
    Ask(String),
}

pub fn route(transcript: &str, commands_on: bool) -> Route {
    if let Some(request) = commands::addressed(transcript) {
        return Route::Ask(request);
    }
    if commands_on {
        if let Some(command) = commands::parse(transcript) {
            return Route::Command(command);
        }
    }
    Route::Dictation
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A note to type: not a command after all, or plain dictation that
    /// needed translating.
    Dictation {
        text: String,
        /// The language the note was translated into.
        translated: Option<&'static str>,
        /// Why a translation was skipped; the original words are kept.
        warning: Option<String>,
    },
    /// A short confirmation, like "Opened Spotify".
    Opened(String),
    Answer {
        text: String,
        provider: Provider,
    },
    /// Text the assistant wrote to insert at the cursor.
    Typed {
        text: String,
        provider: Provider,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Launch,
    NoKey,
    Key(Provider),
    Offline,
    RateLimited,
    Other,
}

#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

impl Error {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn launch(message: String) -> Self {
        Self::new(ErrorKind::Launch, message)
    }
}

/// Everything a request needs, gathered on the UI thread.
pub struct Request {
    /// The whole transcript, typed as a note if the command finds no app.
    pub transcript: String,
    pub route: Route,
    pub provider: Option<Provider>,
    /// `None` when the user turned screen context off.
    pub screen: Option<Snapshot>,
    /// The language to write notes and answers in, when it differs from
    /// the spoken one.
    pub translate_to: Option<&'static str>,
}

/// Carries out a command or asks the assistant. Blocking; run it off the UI
/// thread.
pub fn perform(request: Request) -> Result<Outcome, Error> {
    match request.route {
        Route::Dictation => Ok(dictate(
            request.transcript,
            request.provider,
            request.translate_to,
        )),
        Route::Command(command) => match run_command(&command)? {
            Some(outcome) => Ok(outcome),
            None => Ok(dictate(
                request.transcript,
                request.provider,
                request.translate_to,
            )),
        },
        Route::Ask(question) => {
            // "Hey Whisple, open Spotify" needs no model when Spotify is
            // installed; it works offline and at once.
            if let Some(command) = commands::parse(&question) {
                if let Some(outcome) = run_command(&command)? {
                    return Ok(outcome);
                }
            }
            let provider = request.provider.ok_or_else(|| {
                Error::new(
                    ErrorKind::NoKey,
                    "Add an OpenAI or Groq key in Settings › Models so Whisple can answer.",
                )
            })?;
            let mut screen = request.screen;
            let screenshot = screen.as_mut().and_then(|snapshot| {
                screen::complete(snapshot);
                screen::capture_png()
                    .map_err(|err| eprintln!("Whisple could not capture the screen: {err}"))
                    .ok()
            });
            let apps = apps::installed();
            let names: Vec<String> = apps
                .iter()
                .take(MAX_APPS)
                .map(|app| app.name.clone())
                .collect();
            let action = ask(
                provider,
                &Question {
                    text: &question,
                    screen: screen.as_ref(),
                    screenshot: screenshot.as_deref(),
                    apps: &names,
                    reply_language: request.translate_to,
                },
            )?;
            carry_out(action, provider, &apps)
        }
    }
}

/// A note, translated when an output language is set. A failed translation
/// keeps the spoken words, so nothing the user said is lost.
fn dictate(text: String, provider: Option<Provider>, target: Option<&'static str>) -> Outcome {
    let Some(language) = target else {
        return Outcome::Dictation {
            text,
            translated: None,
            warning: None,
        };
    };
    let translation = match provider {
        Some(provider) => translate(provider, &text, language).map_err(|err| err.message),
        None => Err(format!(
            "Add an OpenAI or Groq key in Settings › Models to translate into {language}."
        )),
    };
    match translation {
        Ok(translated) => Outcome::Dictation {
            text: translated,
            translated: Some(language),
            warning: None,
        },
        Err(message) => Outcome::Dictation {
            text,
            translated: None,
            warning: Some(message),
        },
    }
}

/// Opens what a command names. `None` when no installed app matches, so the
/// words are typed instead.
fn run_command(command: &Command) -> Result<Option<Outcome>, Error> {
    match command {
        Command::OpenUrl(url) => {
            apps::open_url(url).map_err(Error::launch)?;
            Ok(Some(Outcome::Opened(format!("Opened {}", site(url)))))
        }
        Command::OpenApp(name) => {
            let installed = apps::installed();
            let Some(app) = apps::find(&installed, name) else {
                return Ok(None);
            };
            apps::launch(app).map_err(Error::launch)?;
            Ok(Some(Outcome::Opened(format!("Opened {}", app.name))))
        }
    }
}

fn carry_out(
    action: Action,
    provider: Provider,
    installed: &[apps::App],
) -> Result<Outcome, Error> {
    match action {
        Action::Answer(text) => Ok(Outcome::Answer { text, provider }),
        Action::Type(text) => Ok(Outcome::Typed { text, provider }),
        Action::OpenUrl(url) => {
            apps::open_url(&url).map_err(Error::launch)?;
            Ok(Outcome::Opened(format!("Opened {}", site(&url))))
        }
        Action::OpenApp(name) => {
            let app = apps::find(installed, &name).ok_or_else(|| {
                Error::launch(format!("{name} is not installed on this computer."))
            })?;
            apps::launch(app).map_err(Error::launch)?;
            Ok(Outcome::Opened(format!("Opened {}", app.name)))
        }
    }
}

/// "https://www.github.com/x" reads as "github.com".
fn site(url: &str) -> String {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Answer(String),
    Type(String),
    OpenApp(String),
    OpenUrl(String),
}

pub struct Question<'a> {
    pub text: &'a str,
    pub screen: Option<&'a Snapshot>,
    pub screenshot: Option<&'a [u8]>,
    pub apps: &'a [String],
    /// The language `text` should be written in, when the user set one.
    pub reply_language: Option<&'a str>,
}

fn endpoint(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "https://api.openai.com/v1/chat/completions",
        Provider::Groq => "https://api.groq.com/openai/v1/chat/completions",
        Provider::Xai => "https://api.x.ai/v1/chat/completions",
    }
}

/// Chat models that read screenshots.
fn model(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "gpt-5.6",
        Provider::Groq => "qwen/qwen3.6-27b",
        Provider::Xai => "grok-4.7",
    }
}

pub fn ask(provider: Provider, question: &Question) -> Result<Action, Error> {
    let key = cloud::load_key(provider).map_err(|err| match err {
        TranscriptionError::Unauthorized(message) => Error::new(ErrorKind::Key(provider), message),
        other => Error::new(ErrorKind::Other, other.message()),
    })?;
    ask_with_key(provider, endpoint(provider), &key, question)
}

fn ask_with_key(
    provider: Provider,
    endpoint: &str,
    key: &str,
    question: &Question,
) -> Result<Action, Error> {
    let content = complete(provider, endpoint, key, &body(provider, question))?;
    parse_action(&content)
        .map_err(|message| Error::new(ErrorKind::Other, format!("{}: {message}", provider.name())))
}

/// Translates a note into `language` with the provider's chat model.
pub fn translate(provider: Provider, text: &str, language: &str) -> Result<String, Error> {
    let key = cloud::load_key(provider).map_err(|err| match err {
        TranscriptionError::Unauthorized(message) => Error::new(ErrorKind::Key(provider), message),
        other => Error::new(ErrorKind::Other, other.message()),
    })?;
    translate_with_key(provider, endpoint(provider), &key, text, language)
}

fn translate_with_key(
    provider: Provider,
    endpoint: &str,
    key: &str,
    text: &str,
    language: &str,
) -> Result<String, Error> {
    let body = translation_body(provider, text, language);
    let content = complete(provider, endpoint, key, &body)?;
    let content = match content.rfind("</think>") {
        Some(end) => &content[end + "</think>".len()..],
        None => &content,
    };
    let translated = content.trim();
    if translated.is_empty() {
        return Err(Error::new(
            ErrorKind::Other,
            format!("{} sent an empty translation.", provider.name()),
        ));
    }
    Ok(translated.to_string())
}

fn translation_body(provider: Provider, text: &str, language: &str) -> Value {
    let messages = json!([
        {
            "role": "system",
            "content": format!("Translate the user's dictated note into {language}. Keep its meaning, tone, names and punctuation. If it is already in {language}, return it unchanged. Reply with only the translation, no quotes or notes.")
        },
        {"role": "user", "content": text},
    ]);
    match provider {
        Provider::OpenAi => json!({
            "model": model(provider),
            "messages": messages,
            "reasoning_effort": "low",
            "max_completion_tokens": 4000,
        }),
        Provider::Groq => json!({
            "model": model(provider),
            "messages": messages,
            "reasoning_effort": "none",
            "max_completion_tokens": 2048,
        }),
        Provider::Xai => json!({
            "model": model(provider),
            "messages": messages,
            "max_completion_tokens": 2048,
        }),
    }
}

/// Sends one chat request and returns the reply's text.
fn complete(provider: Provider, endpoint: &str, key: &str, body: &Value) -> Result<String, Error> {
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("Whisple/0.1")
        .build()
        .map_err(|err| {
            Error::new(
                ErrorKind::Other,
                format!("Could not start the request: {err}"),
            )
        })?;
    let response = client
        .post(endpoint)
        .bearer_auth(key)
        .json(body)
        .send()
        .map_err(|err| {
            Error::new(
                ErrorKind::Offline,
                format!("Could not reach {}: {err}", provider.name()),
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        let detail = response
            .json::<Value>()
            .ok()
            .and_then(|body| body["error"]["message"].as_str().map(str::to_string));
        return Err(response_error(provider, status.as_u16(), detail));
    }
    let reply: Value = response.json().map_err(|err| {
        Error::new(
            ErrorKind::Other,
            format!("Could not read {}'s answer: {err}", provider.name()),
        )
    })?;
    reply["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Other,
                format!("{} sent an empty answer.", provider.name()),
            )
        })
}

fn response_error(provider: Provider, status: u16, detail: Option<String>) -> Error {
    match status {
        401 | 403 => Error::new(
            ErrorKind::Key(provider),
            format!(
                "{} rejected the API key. Edit it in Voice models.",
                provider.name()
            ),
        ),
        429 => Error::new(
            ErrorKind::RateLimited,
            format!("{} is rate limited. Try again shortly.", provider.name()),
        ),
        _ => Error::new(
            ErrorKind::Other,
            match detail {
                Some(detail) => format!("{} could not answer: {detail}", provider.name()),
                None => format!("{} could not answer (HTTP {status}).", provider.name()),
            },
        ),
    }
}

const INSTRUCTIONS: &str = "You are Whisple, a voice assistant that lives in a small bar at the bottom of the user's screen on {platform}. The user just spoke to you. You can see what is on their screen: the app in front, its window title, any selected text and a screenshot.

Choose exactly one action and reply with only a JSON object {\"action\": ..., \"text\": ..., \"target\": ...}:
- \"answer\": answer a question in `text`. When the user says \"this\", \"here\" or refers to what they see, use the screen. Plain text only, no Markdown, at most three short sentences.
- \"type\": write text for the user to insert at their cursor, such as a reply, a message, a summary or a rewrite of the selected text. Put only the text to insert in `text`, ready to send, in the language of what is on screen unless asked otherwise.
- \"open_app\": open an installed app. Put its exact name from the installed apps in `target`.
- \"open_url\": open a website. Put a full https:// address in `target`.
If you cannot help, use \"answer\" and say why in one sentence. Leave `target` empty for \"answer\" and \"type\".";

fn body(provider: Provider, question: &Question) -> Value {
    let platform = if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    };
    let mut content = vec![json!({"type": "text", "text": prompt(question)})];
    if let Some(png) = question.screenshot {
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        content.push(json!({
            "type": "image_url",
            "image_url": {"url": format!("data:image/png;base64,{data}")}
        }));
    }
    let messages = json!([
        {"role": "system", "content": INSTRUCTIONS.replace("{platform}", platform)},
        {"role": "user", "content": content},
    ]);
    match provider {
        Provider::OpenAi => json!({
            "model": model(provider),
            "messages": messages,
            "reasoning_effort": "low",
            "max_completion_tokens": 4000,
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "whisple_action",
                    "strict": true,
                    "schema": {
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "enum": ["answer", "type", "open_app", "open_url"]
                            },
                            "text": {"type": "string"},
                            "target": {"type": "string"}
                        },
                        "required": ["action", "text", "target"],
                        "additionalProperties": false
                    }
                }
            }
        }),
        Provider::Groq => json!({
            "model": model(provider),
            "messages": messages,
            // Qwen can think first; a voice bar needs the answer now.
            "reasoning_effort": "none",
            "max_completion_tokens": 1024,
            "response_format": {"type": "json_object"}
        }),
        Provider::Xai => json!({
            "model": model(provider),
            "messages": messages,
            "max_completion_tokens": 1024,
            "response_format": {"type": "json_object"}
        }),
    }
}

/// The user's words with the screen context written out.
fn prompt(question: &Question) -> String {
    let mut prompt = format!("Request: {}\n", question.text);
    match question.screen {
        Some(screen) if !screen.is_empty() || question.screenshot.is_some() => {
            prompt.push_str("\nOn screen:\n");
            if let Some(app) = &screen.app {
                prompt.push_str(&format!("- App: {app}\n"));
            }
            if let Some(window) = &screen.window {
                prompt.push_str(&format!("- Window: {window}\n"));
            }
            if let Some(selection) = &screen.selection {
                prompt.push_str(&format!("- Selected text:\n\"\"\"\n{selection}\n\"\"\"\n"));
            }
            if question.screenshot.is_some() {
                prompt.push_str("- A screenshot of the display is attached.\n");
            }
        }
        Some(_) => prompt.push_str("\nNothing could be read from the screen.\n"),
        None => prompt.push_str("\nThe user has turned off sharing their screen.\n"),
    }
    if !question.apps.is_empty() {
        prompt.push_str(&format!("\nInstalled apps: {}\n", question.apps.join(", ")));
    }
    if let Some(language) = question.reply_language {
        prompt.push_str(&format!("\nWrite `text` in {language}.\n"));
    }
    prompt
}

#[derive(Deserialize)]
struct Reply {
    action: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    target: String,
}

/// Reads the model's JSON, tolerating a code fence or a thinking block
/// around it.
fn parse_action(content: &str) -> Result<Action, String> {
    let content = match content.rfind("</think>") {
        Some(end) => &content[end + "</think>".len()..],
        None => content,
    };
    let start = content.find('{').ok_or("the answer was not JSON")?;
    let end = content.rfind('}').ok_or("the answer was not JSON")?;
    let reply: Reply = serde_json::from_str(&content[start..=end])
        .map_err(|err| format!("the answer was not understood ({err})"))?;
    let text = reply.text.trim().to_string();
    let target = reply.target.trim().to_string();
    match reply.action.as_str() {
        "type" if !text.is_empty() => Ok(Action::Type(text)),
        "open_app" if !target.is_empty() => Ok(Action::OpenApp(target)),
        "open_url" if !target.is_empty() => {
            let url = if target.starts_with("https://") || target.starts_with("http://") {
                target
            } else {
                format!("https://{target}")
            };
            Ok(Action::OpenUrl(url))
        }
        _ if !text.is_empty() => Ok(Action::Answer(text)),
        _ => Err("the answer was empty".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn transcripts_route_by_their_words() {
        assert_eq!(route("Buy milk.", true), Route::Dictation);
        assert_eq!(
            route("Open Spotify.", true),
            Route::Command(Command::OpenApp("Spotify".into()))
        );
        assert_eq!(route("Open Spotify.", false), Route::Dictation);
        assert_eq!(
            route("Hey Whisple, what is this?", false),
            Route::Ask("What is this?".into())
        );
    }

    #[test]
    fn a_command_for_a_missing_app_is_typed() {
        let outcome = perform(Request {
            transcript: "Open Zzyzx Qqq.".into(),
            route: Route::Command(Command::OpenApp("Zzyzx Qqq".into())),
            provider: None,
            screen: None,
            translate_to: None,
        })
        .unwrap();
        assert_eq!(
            outcome,
            Outcome::Dictation {
                text: "Open Zzyzx Qqq.".into(),
                translated: None,
                warning: None,
            }
        );
    }

    #[test]
    fn a_question_without_a_key_asks_for_one() {
        let err = perform(Request {
            transcript: "Hey Whisple, what is this?".into(),
            route: Route::Ask("What is this?".into()),
            provider: None,
            screen: None,
            translate_to: None,
        })
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::NoKey);
    }

    #[test]
    fn replies_become_actions() {
        assert_eq!(
            parse_action(r#"{"action":"answer","text":"It is a stack trace.","target":""}"#),
            Ok(Action::Answer("It is a stack trace.".into()))
        );
        assert_eq!(
            parse_action("<think>hmm</think>\n```json\n{\"action\":\"type\",\"text\":\"Thanks, see you at 5.\"}\n```"),
            Ok(Action::Type("Thanks, see you at 5.".into()))
        );
        assert_eq!(
            parse_action(r#"{"action":"open_url","text":"","target":"github.com"}"#),
            Ok(Action::OpenUrl("https://github.com".into()))
        );
        assert_eq!(
            parse_action(r#"{"action":"open_app","text":"Opening","target":"Spotify"}"#),
            Ok(Action::OpenApp("Spotify".into()))
        );
        assert!(parse_action(r#"{"action":"answer","text":"","target":""}"#).is_err());
        assert!(parse_action("Sure! Here you go.").is_err());
    }

    #[test]
    fn the_prompt_carries_the_screen() {
        let screen = Snapshot {
            app: Some("Mail".into()),
            window: Some("Re: Friday".into()),
            selection: Some("Can you make it at 5?".into()),
        };
        let apps = vec!["Mail".to_string(), "Spotify".to_string()];
        let text = prompt(&Question {
            text: "Reply that I can",
            screen: Some(&screen),
            screenshot: Some(b"png"),
            apps: &apps,
            reply_language: Some("Danish"),
        });
        assert!(text.contains("Request: Reply that I can"));
        assert!(text.contains("- App: Mail"));
        assert!(text.contains("- Window: Re: Friday"));
        assert!(text.contains("Can you make it at 5?"));
        assert!(text.contains("screenshot"));
        assert!(text.contains("Installed apps: Mail, Spotify"));
        assert!(text.contains("Write `text` in Danish."));

        let private = prompt(&Question {
            text: "What time is it in Tokyo?",
            screen: None,
            screenshot: None,
            apps: &[],
            reply_language: None,
        });
        assert!(private.contains("turned off sharing"));
    }

    #[test]
    fn sites_read_without_their_scheme() {
        assert_eq!(site("https://www.github.com/lassejlv"), "github.com");
        assert_eq!(site("https://news.ycombinator.com"), "news.ycombinator.com");
    }

    #[test]
    fn every_provider_sends_the_screenshot_and_reads_the_action() {
        for provider in Provider::ALL {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/chat/completions", listener.local_addr().unwrap());
            let server = thread::spawn(move || {
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
                let body: Value = serde_json::from_slice(&request[body_start..]).unwrap();
                assert_eq!(body["model"], model(provider));
                let parts = body["messages"][1]["content"].as_array().unwrap();
                assert!(parts[0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("Request: Summarize"));
                assert_eq!(
                    parts[1]["image_url"]["url"],
                    "data:image/png;base64,iVBORw=="
                );
                let content = r#"{"action":"answer","text":"A weather report.","target":""}"#;
                let reply =
                    json!({"choices": [{"message": {"role": "assistant", "content": content}}]})
                        .to_string();
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                )
                .unwrap();
            });
            let action = ask_with_key(
                provider,
                &url,
                "test-key",
                &Question {
                    text: "Summarize this",
                    screen: Some(&Snapshot::default()),
                    screenshot: Some(&[0x89, b'P', b'N', b'G']),
                    apps: &[],
                    reply_language: None,
                },
            )
            .unwrap();
            assert_eq!(action, Action::Answer("A weather report.".into()));
            server.join().unwrap();
        }
    }

    #[test]
    fn a_note_that_cannot_be_translated_keeps_the_spoken_words() {
        let outcome = dictate("Hej med dig.".into(), None, Some("English"));
        let Outcome::Dictation {
            text,
            translated,
            warning,
        } = outcome
        else {
            panic!("expected a note");
        };
        assert_eq!(text, "Hej med dig.");
        assert_eq!(translated, None);
        assert!(warning.unwrap().contains("to translate into English"));
    }

    #[test]
    fn every_provider_translates_a_note() {
        for provider in Provider::ALL {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/chat/completions", listener.local_addr().unwrap());
            let server = thread::spawn(move || {
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
                let body: Value = serde_json::from_slice(&request[body_start..]).unwrap();
                assert_eq!(body["model"], model(provider));
                assert!(body["messages"][0]["content"]
                    .as_str()
                    .unwrap()
                    .contains("into English"));
                assert_eq!(body["messages"][1]["content"], "Hej med dig.");
                assert!(body.get("response_format").is_none());
                let reply = json!({"choices": [{"message": {"role": "assistant", "content": "  Hi there.\n"}}]})
                    .to_string();
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                )
                .unwrap();
            });
            let translated =
                translate_with_key(provider, &url, "test-key", "Hej med dig.", "English").unwrap();
            assert_eq!(translated, "Hi there.");
            server.join().unwrap();
        }
    }

    #[test]
    fn a_rejected_key_points_to_the_provider() {
        let err = response_error(Provider::Groq, 401, None);
        assert_eq!(err.kind, ErrorKind::Key(Provider::Groq));
        let err = response_error(Provider::OpenAi, 400, Some("Unknown model".into()));
        assert_eq!(err.kind, ErrorKind::Other);
        assert!(err.message.contains("Unknown model"));
    }
}
