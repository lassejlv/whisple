use std::time::Duration;

use base64::Engine as _;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{json, Value};

use self::context::{self as screen, Snapshot};
use crate::assistant::commands::Command;
use crate::transcription::cloud::{self, Models, Provider, TranscriptionError};

pub(crate) mod apps;
pub(crate) mod commands;
pub(crate) mod context;
mod routing;
pub(crate) use routing::*;

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
    pub reply_language: Option<&'a str>,
}

fn endpoint(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "https://api.openai.com/v1/chat/completions",
        Provider::Groq => "https://api.groq.com/openai/v1/chat/completions",
        Provider::Xai => "https://api.x.ai/v1/chat/completions",
        Provider::Vercel => "https://ai-gateway.vercel.sh/v1/chat/completions",
    }
}

fn chat_models(provider: Provider) -> Models {
    match provider {
        // Luna is GPT-5.6's fast tier.
        Provider::OpenAi => Models::new("gpt-5.6-luna", "gpt-5.5"),
        // Groq retired Qwen 3.6 for Qwen 3.8, and keeps no older Qwen.
        Provider::Groq => Models::only("qwen/qwen3.8-27b"),
        Provider::Xai => Models::new("grok-4.7", "grok-4.3"),
        Provider::Vercel => cloud::gateway_model().chat_models(),
    }
}

fn model(provider: Provider) -> &'static str {
    chat_models(provider).current()
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
        // Grok 4.7 thinks hard by default; a note needs no deep thought.
        Provider::Xai => json!({
            "model": model(provider),
            "messages": messages,
            "reasoning_effort": "low",
            "max_completion_tokens": 2048,
        }),
        Provider::Vercel => json!({
            "model": model(provider),
            "messages": messages,
            "max_tokens": 2048,
        }),
    }
}

fn complete(provider: Provider, endpoint: &str, key: &str, body: &Value) -> Result<String, Error> {
    complete_with(provider, chat_models(provider), endpoint, key, body)
}

/// Like `complete`, retrying once with the fallback model when the provider
/// does not know the newest one.
fn complete_with(
    provider: Provider,
    models: Models,
    endpoint: &str,
    key: &str,
    body: &Value,
) -> Result<String, Error> {
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
    let send = |body: &Value| {
        client
            .post(endpoint)
            .bearer_auth(key)
            .json(body)
            .send()
            .map_err(|err| {
                Error::new(
                    ErrorKind::Offline,
                    format!("Could not reach {}: {err}", provider.name()),
                )
            })
    };
    let mut response = send(body)?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let text = response.text().unwrap_or_default();
        let tried = body["model"].as_str().unwrap_or_default();
        match models.fall_back_from(tried) {
            Some(fallback) if cloud::is_unknown_model(status, &text) => {
                let mut retry = body.clone();
                retry["model"] = json!(fallback);
                response = send(&retry)?;
            }
            _ => return Err(response_error(provider, status, error_message(&text))),
        }
    }
    let status = response.status();
    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        return Err(response_error(
            provider,
            status.as_u16(),
            error_message(&text),
        ));
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

fn error_message(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|body| body["error"]["message"].as_str().map(str::to_string))
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
            "reasoning_effort": "low",
            "max_completion_tokens": 1024,
            "response_format": {"type": "json_object"}
        }),
        // The gateway's OpenAI-compatible API, whichever model it routes to.
        Provider::Vercel => json!({
            "model": model(provider),
            "messages": messages,
            "max_tokens": 1024,
            "response_format": {"type": "json_object"}
        }),
    }
}

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
    fn a_chat_model_the_provider_does_not_know_is_retried_with_the_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/chat/completions", listener.local_addr().unwrap());
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
                let body: Value = serde_json::from_slice(&request[body_start..]).unwrap();
                models.push(body["model"].as_str().unwrap().to_string());
                let (status, reply) = if attempt == 0 {
                    (
                        "404 Not Found",
                        json!({"error": {"message": "The model does not exist", "code": "model_not_found"}}),
                    )
                } else {
                    (
                        "200 OK",
                        json!({"choices": [{"message": {"role": "assistant", "content": "Hello."}}]}),
                    )
                };
                let reply = reply.to_string();
                write!(
                    socket,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                )
                .unwrap();
            }
            models
        });
        let models = Models::new("test-newest-chat", "test-older-chat");
        let body = json!({"model": models.current(), "messages": []});
        let reply = complete_with(Provider::OpenAi, models, &url, "test-key", &body).unwrap();
        assert_eq!(reply, "Hello.");
        assert_eq!(
            server.join().unwrap(),
            ["test-newest-chat", "test-older-chat"]
        );
        assert_eq!(models.current(), "test-older-chat");
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
