use super::*;

const MAX_APPS: usize = 400;

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
    Dictation {
        text: String,
        translated: Option<&'static str>,
        /// Why a translation was skipped; the original words are kept.
        warning: Option<String>,
    },
    Opened(String),
    Answer {
        text: String,
        provider: Provider,
    },
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
    pub(super) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn launch(message: String) -> Self {
        Self::new(ErrorKind::Launch, message)
    }
}

pub struct Request {
    pub transcript: String,
    pub route: Route,
    pub provider: Option<Provider>,
    pub screen: Option<Snapshot>,
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
pub(super) fn dictate(
    text: String,
    provider: Option<Provider>,
    target: Option<&'static str>,
) -> Outcome {
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

pub(super) fn run_command(command: &Command) -> Result<Option<Outcome>, Error> {
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

pub(super) fn carry_out(
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

pub(super) fn site(url: &str) -> String {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}
