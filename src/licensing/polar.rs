use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SavedLicense {
    pub(super) key: String,
    pub(super) activation_id: String,
    pub(super) display_key: String,
    pub(super) verified_at: i64,
    pub(super) expires_at: Option<i64>,
    #[serde(default)]
    pub(super) last_seen: i64,
    #[serde(default)]
    pub(super) offline_clock: Option<OfflineClock>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct OfflineClock {
    pub(super) boot_id: String,
    pub(super) anchored_at: u64,
    pub(super) remaining_secs: u64,
}

impl OfflineClock {
    pub(super) fn from_snapshot(snapshot: &Snapshot, remaining_secs: u64) -> Self {
        Self {
            boot_id: snapshot.boot_id.clone(),
            anchored_at: snapshot.seconds,
            remaining_secs,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct LicenseResponse {
    pub(super) organization_id: String,
    pub(super) benefit_id: String,
    pub(super) status: String,
    pub(super) display_key: String,
    pub(super) expires_at: Option<String>,
    pub(super) activation: Option<ActivationId>,
}

#[derive(Deserialize)]
pub(super) struct ActivationId {
    pub(super) id: String,
}

#[derive(Deserialize)]
pub(super) struct ActivateResponse {
    pub(super) id: String,
    pub(super) license_key: LicenseResponse,
}

#[derive(Debug)]
pub(super) enum ApiError {
    Rejected(StatusCode, String),
    Unavailable(String),
}

pub(super) fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|err| format!("Could not open the system credential store: {err}"))
}

pub(super) fn load() -> Result<Option<SavedLicense>, String> {
    match entry()?.get_password() {
        Ok(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|_| "The saved license could not be read. Enter the key again.".into()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(format!("Could not read the saved license: {err}")),
    }
}

pub(super) fn save(record: &SavedLicense) -> Result<(), String> {
    let raw = serde_json::to_string(record).map_err(|err| err.to_string())?;
    entry()?
        .set_password(&raw)
        .map_err(|err| format!("Could not save the license in the credential store: {err}"))
}

pub(super) fn remove() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(format!("Could not remove the saved license: {err}")),
    }
}

pub(super) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64
}

pub(super) fn expiry(raw: Option<&str>) -> Result<Option<i64>, String> {
    raw.map(|value| {
        DateTime::parse_from_rfc3339(value)
            .map(|date| date.timestamp())
            .map_err(|_| "Polar returned an unreadable license expiration date.".to_string())
    })
    .transpose()
}

pub(super) fn check_response(
    response: &LicenseResponse,
    activation_id: Option<&str>,
    checked_at: i64,
) -> Result<Option<i64>, String> {
    if response.organization_id != ORGANIZATION_ID || response.benefit_id != BENEFIT_ID {
        return Err("This key is not for Whisple.".into());
    }
    if response.status != "granted" {
        return Err("This license no longer grants access.".into());
    }
    if let Some(expected) = activation_id {
        if response.activation.as_ref().map(|item| item.id.as_str()) != Some(expected) {
            return Err("This device activation is no longer valid.".into());
        }
    }
    let expires_at = expiry(response.expires_at.as_deref())?;
    if expires_at.is_some_and(|expires| expires <= checked_at) {
        return Err("This license has expired.".into());
    }
    Ok(expires_at)
}

pub(super) fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(concat!("Whisple/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| format!("Could not start license validation: {err}"))
}

pub(super) fn post(
    client: &Client,
    endpoint: &str,
    body: serde_json::Value,
) -> Result<Response, ApiError> {
    post_at(client, API_BASE, endpoint, body)
}

pub(super) fn post_at(
    client: &Client,
    api_base: &str,
    endpoint: &str,
    body: serde_json::Value,
) -> Result<Response, ApiError> {
    let response = client
        .post(format!("{api_base}/{endpoint}"))
        .json(&body)
        .send()
        .map_err(|_| {
            ApiError::Unavailable("Could not reach Polar. Check your connection.".into())
        })?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return Err(ApiError::Unavailable(
            "Polar is temporarily unavailable. Try again shortly.".into(),
        ));
    }
    let message = match status {
        StatusCode::FORBIDDEN => {
            "Polar could not activate this key. Check its status and available device slots."
        }
        StatusCode::NOT_FOUND => "This license key or device activation was not found.",
        StatusCode::UNPROCESSABLE_ENTITY | StatusCode::BAD_REQUEST => {
            "Polar rejected this license key. Check it and try again."
        }
        _ => "Polar rejected this license request.",
    };
    Err(ApiError::Rejected(status, message.into()))
}

/// Offline use must satisfy both wall time and elapsed time in this boot. The
/// latter keeps advancing if the user holds the system clock still.
pub(super) fn offline_access(
    record: &SavedLicense,
    checked_at: i64,
    snapshot: &Snapshot,
) -> Option<OfflineClock> {
    let observed = checked_at.max(record.last_seen);
    let wall_allowed = record.verified_at > 0
        && checked_at >= record.verified_at
        && checked_at.saturating_add(CLOCK_TOLERANCE) >= record.last_seen
        && observed - record.verified_at <= OFFLINE_GRACE
        && record.expires_at.is_none_or(|expires| expires > observed);
    if !wall_allowed {
        return None;
    }
    if let Some(anchor) = &record.offline_clock {
        if anchor.boot_id != snapshot.boot_id
            || snapshot.seconds.checked_sub(anchor.anchored_at)? > anchor.remaining_secs
        {
            return None;
        }
        return Some(anchor.clone());
    }
    // Older records have no boot anchor. Preserve their remaining wall-clock
    // grace once, then persist this anchor before granting offline access.
    let remaining_secs = OFFLINE_GRACE.saturating_sub(checked_at - record.verified_at) as u64;
    Some(OfflineClock::from_snapshot(snapshot, remaining_secs))
}

pub fn activate(key: &str) -> Result<Access, String> {
    let key = key.trim();
    if key.is_empty() || key.len() > 256 || key.chars().any(char::is_whitespace) {
        return Err("Enter a license key without spaces or line breaks.".into());
    }
    let previous = match load() {
        Ok(saved) => saved,
        Err(reason) if reason.starts_with("The saved license could not be read.") => None,
        Err(reason) => return Err(reason),
    };
    if previous.as_ref().is_some_and(|saved| saved.key == key) {
        return match check_saved() {
            status @ (Access::Active(_) | Access::Offline(_)) => Ok(status),
            Access::Trial {
                license_issue: Some(reason),
                ..
            } => Err(reason),
            Access::Blocked { reason, .. } | Access::Unavailable { reason, .. } => Err(reason),
            _ => Err("This license could not be validated.".into()),
        };
    }
    let client = client()?;
    let response = post(
        &client,
        "activate",
        json!({
            "key": key,
            "organization_id": ORGANIZATION_ID,
            "label": "Whisple desktop",
        }),
    )
    .map_err(api_message)?;
    let activation = response
        .json::<ActivateResponse>()
        .map_err(|_| "Polar returned an unreadable activation response.".to_string())?;
    let expires_at = match check_response(&activation.license_key, None, now()) {
        Ok(expires_at) => expires_at,
        Err(reason) => {
            let failed = SavedLicense {
                key: key.to_string(),
                activation_id: activation.id,
                display_key: activation.license_key.display_key,
                verified_at: 0,
                expires_at: None,
                last_seen: 0,
                offline_clock: None,
            };
            let _ = deactivate_record(&client, &failed);
            return Err(reason);
        }
    };
    let activated_at = now();
    let saved = SavedLicense {
        key: key.to_string(),
        activation_id: activation.id,
        display_key: activation.license_key.display_key,
        verified_at: activated_at,
        expires_at,
        last_seen: activated_at,
        offline_clock: boot_clock::snapshot()
            .as_ref()
            .map(|snapshot| OfflineClock::from_snapshot(snapshot, OFFLINE_GRACE as u64)),
    };
    if let Err(reason) = save(&saved) {
        let _ = deactivate_record(&client, &saved);
        return Err(reason);
    }
    if let Some(previous) = previous.filter(|old| old.key != saved.key) {
        let _ = deactivate_record(&client, &previous);
    }
    Ok(Access::Active(saved.display_key))
}

pub(super) fn api_message(error: ApiError) -> String {
    match error {
        ApiError::Rejected(_, message) | ApiError::Unavailable(message) => message,
    }
}

pub(super) fn deactivate_record(client: &Client, record: &SavedLicense) -> Result<(), String> {
    match post(
        client,
        "deactivate",
        json!({
            "key": record.key,
            "organization_id": ORGANIZATION_ID,
            "activation_id": record.activation_id,
        }),
    ) {
        Ok(_) => Ok(()),
        Err(ApiError::Rejected(StatusCode::NOT_FOUND, _)) => {
            // A rotated or revoked key may already have lost this activation.
            Ok(())
        }
        Err(err) => Err(api_message(err)),
    }
}

pub fn deactivate() -> Result<(), String> {
    if let Some(record) = load()? {
        deactivate_record(&client()?, &record)?;
    }
    remove()
}
