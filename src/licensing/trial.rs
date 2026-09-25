use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SavedTrial {
    pub(super) started_at: i64,
    pub(super) last_seen: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) token: Option<String>,
}

/// How long a trial runs before the trial server has confirmed it. Blocking
/// the server and deleting the local copies then gains an hour, not three
/// days.
pub(super) const UNCONFIRMED_TRIAL: i64 = 60 * 60;

pub(super) fn trial_state(record: &SavedTrial, checked_at: i64) -> Access {
    let expires_at = record.started_at.saturating_add(TRIAL_LENGTH);
    if record.started_at <= 0
        || record.last_seen < record.started_at
        || checked_at.saturating_add(CLOCK_TOLERANCE) < record.last_seen
        || checked_at.max(record.last_seen) >= expires_at
    {
        Access::TrialExpired
    } else {
        Access::Trial {
            expires_at,
            last_seen: record.last_seen.max(checked_at),
            confirmation_required: false,
            license_issue: None,
        }
    }
}

pub(super) fn use_trial_if_available(
    trial: &Result<Access, String>,
    paid_failure: Access,
) -> Access {
    if let Ok(Access::Trial {
        expires_at,
        last_seen,
        confirmation_required,
        ..
    }) = trial
    {
        let trial_access = Access::Trial {
            expires_at: *expires_at,
            last_seen: *last_seen,
            confirmation_required: *confirmation_required,
            license_issue: None,
        };
        if trial_access.allowed() {
            let reason = match &paid_failure {
                Access::Blocked { reason, .. } | Access::Unavailable { reason, .. } => {
                    reason.clone()
                }
                _ => return paid_failure,
            };
            return Access::Trial {
                expires_at: *expires_at,
                last_seen: *last_seen,
                confirmation_required: *confirmation_required,
                license_issue: Some(reason),
            };
        }
    }
    paid_failure
}

pub(super) fn merge_trials(
    keychain: Option<SavedTrial>,
    file: Option<SavedTrial>,
    checked_at: i64,
) -> SavedTrial {
    let records = [keychain, file];
    let mut known = records.iter().flatten();
    let Some(first) = known.next() else {
        return SavedTrial {
            started_at: checked_at,
            last_seen: checked_at,
            token: None,
        };
    };
    known.fold(first.clone(), |merged, record| SavedTrial {
        started_at: merged.started_at.min(record.started_at),
        last_seen: merged.last_seen.max(record.last_seen),
        token: merged.token.or_else(|| record.token.clone()),
    })
}

pub(super) fn confirmed_trial(
    device: &str,
    saved: [Option<&str>; 2],
) -> Option<(String, ServerTrial)> {
    saved
        .into_iter()
        .flatten()
        .find_map(|token| {
            trial_server::verify(token, device).map(|trial| (token.to_string(), trial))
        })
        .or_else(|| {
            let token = trial_server::fetch(device).ok()?;
            let trial = trial_server::verify(&token, device)?;
            Some((token, trial))
        })
}

pub(super) fn apply_server_trial(record: &mut SavedTrial, server: ServerTrial) {
    record.started_at = record
        .started_at
        .min(server.started_at)
        .min(server.expires_at.saturating_sub(TRIAL_LENGTH));
}

pub(super) fn trial_access(record: &SavedTrial, confirmed: bool, checked_at: i64) -> Access {
    let mut access = trial_state(record, checked_at);
    if !confirmed {
        if let Access::Trial {
            expires_at,
            confirmation_required,
            ..
        } = &mut access
        {
            *expires_at = (*expires_at).min(record.started_at.saturating_add(UNCONFIRMED_TRIAL));
            *confirmation_required = true;
            if checked_at.max(record.last_seen) >= *expires_at {
                return Access::Unavailable {
                    display_key: String::new(),
                    reason: t("Connect to the internet to continue your trial.").into(),
                };
            }
        }
    }
    access
}

pub(super) fn trial_file() -> Option<std::path::PathBuf> {
    Some(dirs::config_dir()?.join("whisp").join(".first-run"))
}

pub(super) fn read_trial_file() -> Result<Option<SavedTrial>, String> {
    let path = trial_file().ok_or("Could not locate the saved trial.")?;
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|_| "The trial backup could not be read.".into()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("Could not read the trial backup: {err}")),
    }
}

pub(super) fn write_trial_file(record: &SavedTrial) -> Result<(), String> {
    let path = trial_file().ok_or("Could not locate the saved trial.")?;
    let parent = path
        .parent()
        .ok_or("Could not locate the trial backup folder.")?;
    std::fs::create_dir_all(parent)
        .map_err(|err| format!("Could not save the trial backup: {err}"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|err| format!("Could not save the trial backup: {err}"))?;
    serde_json::to_writer(&mut temporary, record).map_err(|err| err.to_string())?;
    temporary
        .persist(path)
        .map_err(|err| format!("Could not save the trial backup: {}", err.error))?;
    Ok(())
}

pub(super) fn recover_trial_copies(
    keychain: Result<Option<SavedTrial>, String>,
    file: Result<Option<SavedTrial>, String>,
) -> Result<(Option<SavedTrial>, Option<SavedTrial>), String> {
    let keychain_error = keychain.as_ref().err().cloned();
    let file_error = file.as_ref().err().cloned();
    let keychain = keychain.ok().flatten();
    let file = file.ok().flatten();
    if keychain.is_none() && file.is_none() {
        if let Some(reason) = keychain_error.or(file_error) {
            return Err(reason);
        }
    }
    Ok((keychain, file))
}

/// Starts on first launch, independently of any Polar activation. Never delete
/// this credential when deactivating a paid key.
pub fn start_trial() -> Result<Access, String> {
    let _lock = TRIAL_LOCK
        .lock()
        .map_err(|_| "Could not read the trial state.")?;
    let entry = keyring::Entry::new(KEYRING_SERVICE, TRIAL_USER)
        .map_err(|err| format!("Could not open the system credential store: {err}"))?;
    let checked_at = now();
    let keychain = match entry.get_password() {
        Ok(raw) => serde_json::from_str::<SavedTrial>(&raw)
            .map(Some)
            .map_err(|_| "The saved trial could not be read.".to_string()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(format!("Could not read the saved trial: {err}")),
    };
    let (keychain, file) = recover_trial_copies(keychain, read_trial_file())?;
    let mut record = merge_trials(keychain.clone(), file.clone(), checked_at);
    let saved_tokens = [
        keychain.as_ref().and_then(|saved| saved.token.as_deref()),
        file.as_ref().and_then(|saved| saved.token.as_deref()),
    ];
    let server =
        trial_server::device_id().and_then(|device| confirmed_trial(&device, saved_tokens));
    record.token = server.as_ref().map(|(token, _)| token.clone());
    if let Some((_, trial)) = server {
        apply_server_trial(&mut record, trial);
    }
    let access = trial_access(&record, server.is_some(), checked_at);
    // Persist the highest observed time, including expiration. A restart or a
    // small clock adjustment cannot create a fresh 72-hour window.
    record.last_seen = checked_at.max(record.last_seen);
    let file_saved = file.as_ref() == Some(&record) || write_trial_file(&record).is_ok();
    let keychain_saved = keychain.as_ref() == Some(&record)
        || entry
            .set_password(&serde_json::to_string(&record).map_err(|err| err.to_string())?)
            .is_ok();
    if !file_saved && !keychain_saved {
        return Err("Could not save the trial in the credential store or its backup.".into());
    }
    Ok(access)
}
