//! Polar license-key access for the local desktop app.
//!
//! These customer-portal endpoints are public. No organization access token is
//! shipped with Whisple; the key and this device's activation live in Keychain.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::DateTime;
use reqwest::blocking::{Client, Response};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CHECKOUT_URL: &str =
    "https://buy.polar.sh/polar_cl_jvWJVAZBAHpctw43ZsWUNlIfBCYx0f6jizX8x4Hoqud";
pub const CUSTOMER_PORTAL_URL: &str = "https://polar.sh/whisple/portal";
/// The one-time price shown in the app. Keep it in step with the Polar product.
pub const PRICE: &str = "$19";

const ORGANIZATION_ID: &str = "9c7736ba-52be-460e-ae28-a9c5bc2e5b26";
const BENEFIT_ID: &str = "41af04ce-d991-4575-af1d-b216fe69ce2d";
const API_BASE: &str = "https://api.polar.sh/v1/customer-portal/license-keys";
const KEYRING_SERVICE: &str = "app.whisple.license";
const KEYRING_USER: &str = "polar-license";
const OFFLINE_GRACE: i64 = 72 * 60 * 60;
const TRIAL_USER: &str = "trial-v1";
const TRIAL_LENGTH: i64 = 72 * 60 * 60;
const CLOCK_TOLERANCE: i64 = 5 * 60;
static TRIAL_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Access {
    Unlicensed,
    Checking,
    Trial {
        expires_at: i64,
        last_seen: i64,
        license_issue: Option<String>,
    },
    TrialExpired,
    Active(String),
    Offline(String),
    Blocked {
        display_key: String,
        reason: String,
    },
    Unavailable {
        display_key: String,
        reason: String,
    },
}

impl Access {
    pub fn allowed(&self) -> bool {
        matches!(self, Self::Active(_) | Self::Offline(_))
            || self
                .trial_remaining()
                .is_some_and(|remaining| !remaining.is_zero())
    }

    pub fn trial_remaining(&self) -> Option<Duration> {
        match self {
            Self::Trial {
                expires_at,
                last_seen,
                ..
            } => {
                let current = now();
                if current.saturating_add(CLOCK_TOLERANCE) < *last_seen {
                    return None;
                }
                Some(Duration::from_secs(
                    expires_at.saturating_sub(current.max(*last_seen)).max(0) as u64,
                ))
            }
            _ => None,
        }
    }

    pub fn display_key(&self) -> Option<&str> {
        match self {
            Self::Active(key) | Self::Offline(key) => Some(key),
            Self::Blocked { display_key, .. } | Self::Unavailable { display_key, .. } => {
                Some(display_key)
            }
            Self::Unlicensed | Self::Checking | Self::Trial { .. } | Self::TrialExpired => None,
        }
    }
}

/// Trial time left in words: "2 days 4 hours left", or "5h" when `compact`.
/// Minutes round up, so the last minute never reads as zero.
pub fn trial_left(remaining: Duration, compact: bool) -> String {
    let total_minutes = remaining.as_secs().div_ceil(60).max(1);
    let days = total_minutes / (24 * 60);
    let hours = total_minutes % (24 * 60) / 60;
    let minutes = total_minutes % 60;
    let plural =
        |count: u64, unit: &str| format!("{count} {unit}{}", if count == 1 { "" } else { "s" });
    match (compact, days, hours) {
        (true, 1.., _) => format!("{days}d {hours}h"),
        (true, 0, 1..) => format!("{hours}h"),
        (true, 0, 0) => format!("{minutes}m"),
        (false, 1.., 0) => format!("{} left", plural(days, "day")),
        (false, 1.., _) => format!("{} {} left", plural(days, "day"), plural(hours, "hour")),
        (false, 0, 1..) => format!("{hours}h {minutes}m left"),
        (false, 0, 0) => format!("{} left", plural(minutes, "minute")),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedTrial {
    started_at: i64,
    last_seen: i64,
}

fn trial_state(record: &SavedTrial, checked_at: i64) -> Access {
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
            license_issue: None,
        }
    }
}

fn use_trial_if_available(trial: &Result<Access, String>, paid_failure: Access) -> Access {
    if let Ok(Access::Trial {
        expires_at,
        last_seen,
        ..
    }) = trial
    {
        let trial_access = Access::Trial {
            expires_at: *expires_at,
            last_seen: *last_seen,
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
                license_issue: Some(reason),
            };
        }
    }
    paid_failure
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
    let (mut record, created) = match entry.get_password() {
        Ok(raw) => serde_json::from_str::<SavedTrial>(&raw)
            .map(|record| (record, false))
            .map_err(|_| "The saved trial could not be read.".to_string())?,
        Err(keyring::Error::NoEntry) => (
            SavedTrial {
                started_at: checked_at,
                last_seen: checked_at,
            },
            true,
        ),
        Err(err) => return Err(format!("Could not read the saved trial: {err}")),
    };
    let access = trial_state(&record, checked_at);
    // Persist the highest observed time, including expiration. A restart or a
    // small clock adjustment cannot create a fresh 72-hour window.
    let observed = checked_at.max(record.last_seen);
    if observed != record.last_seen || created {
        record.last_seen = observed;
        entry
            .set_password(&serde_json::to_string(&record).map_err(|err| err.to_string())?)
            .map_err(|err| format!("Could not save the trial in the credential store: {err}"))?;
    }
    Ok(access)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedLicense {
    key: String,
    activation_id: String,
    display_key: String,
    /// Last successful online check. Zero disables offline access.
    verified_at: i64,
    expires_at: Option<i64>,
}

#[derive(Deserialize)]
struct LicenseResponse {
    organization_id: String,
    benefit_id: String,
    status: String,
    display_key: String,
    expires_at: Option<String>,
    activation: Option<ActivationId>,
}

#[derive(Deserialize)]
struct ActivationId {
    id: String,
}

#[derive(Deserialize)]
struct ActivateResponse {
    id: String,
    license_key: LicenseResponse,
}

#[derive(Debug)]
enum ApiError {
    Rejected(StatusCode, String),
    Unavailable(String),
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|err| format!("Could not open the system credential store: {err}"))
}

fn load() -> Result<Option<SavedLicense>, String> {
    match entry()?.get_password() {
        Ok(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|_| "The saved license could not be read. Enter the key again.".into()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(format!("Could not read the saved license: {err}")),
    }
}

fn save(record: &SavedLicense) -> Result<(), String> {
    let raw = serde_json::to_string(record).map_err(|err| err.to_string())?;
    entry()?
        .set_password(&raw)
        .map_err(|err| format!("Could not save the license in the credential store: {err}"))
}

fn remove() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(format!("Could not remove the saved license: {err}")),
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64
}

fn expiry(raw: Option<&str>) -> Result<Option<i64>, String> {
    raw.map(|value| {
        DateTime::parse_from_rfc3339(value)
            .map(|date| date.timestamp())
            .map_err(|_| "Polar returned an unreadable license expiration date.".to_string())
    })
    .transpose()
}

fn check_response(
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

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(concat!("Whisple/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| format!("Could not start license validation: {err}"))
}

fn post(client: &Client, endpoint: &str, body: serde_json::Value) -> Result<Response, ApiError> {
    post_at(client, API_BASE, endpoint, body)
}

fn post_at(
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

fn offline_access(record: &SavedLicense, checked_at: i64) -> bool {
    record.verified_at > 0
        && checked_at >= record.verified_at
        && checked_at - record.verified_at <= OFFLINE_GRACE
        && record.expires_at.is_none_or(|expires| expires > checked_at)
}

/// Refreshes the saved key. A recent successful check permits short offline use.
pub fn check_saved() -> Access {
    let trial = start_trial();
    let record = match load() {
        Ok(Some(record)) => record,
        Ok(None) => {
            return trial.unwrap_or_else(|reason| Access::Unavailable {
                display_key: String::new(),
                reason,
            })
        }
        Err(reason) => {
            return use_trial_if_available(
                &trial,
                Access::Unavailable {
                    display_key: String::new(),
                    reason,
                },
            )
        }
    };
    let checked_at = now();
    let client = match client() {
        Ok(client) => client,
        Err(reason) => {
            return use_trial_if_available(
                &trial,
                Access::Unavailable {
                    display_key: record.display_key,
                    reason,
                },
            )
        }
    };
    let response = post(
        &client,
        "validate",
        json!({
            "key": record.key,
            "organization_id": ORGANIZATION_ID,
            "activation_id": record.activation_id,
            "benefit_id": BENEFIT_ID,
        }),
    );
    let result = match response {
        Ok(response) => response
            .json::<LicenseResponse>()
            .map_err(|_| ApiError::Unavailable("Polar returned an unreadable response.".into()))
            .and_then(|body| {
                check_response(&body, Some(&record.activation_id), checked_at)
                    .map(|expires_at| (body.display_key, expires_at))
                    .map_err(|reason| ApiError::Rejected(StatusCode::OK, reason))
            }),
        Err(err) => Err(err),
    };
    let paid = match result {
        Ok((display_key, expires_at)) => {
            let updated = SavedLicense {
                display_key: display_key.clone(),
                verified_at: checked_at,
                expires_at,
                ..record
            };
            match save(&updated) {
                Ok(()) => Access::Active(display_key),
                Err(reason) => Access::Unavailable {
                    display_key,
                    reason,
                },
            }
        }
        Err(ApiError::Rejected(_, reason)) => {
            if save(&SavedLicense {
                verified_at: 0,
                ..record.clone()
            })
            .is_err()
            {
                let _ = remove();
            }
            Access::Blocked {
                display_key: record.display_key,
                reason,
            }
        }
        Err(ApiError::Unavailable(_reason)) if offline_access(&record, checked_at) => {
            Access::Offline(record.display_key)
        }
        Err(ApiError::Unavailable(reason)) => Access::Unavailable {
            display_key: record.display_key,
            reason,
        },
    };
    use_trial_if_available(&trial, paid)
}

/// Activates a new key for this device and saves it after checking the grant.
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
            };
            let _ = deactivate_record(&client, &failed);
            return Err(reason);
        }
    };
    let saved = SavedLicense {
        key: key.to_string(),
        activation_id: activation.id,
        display_key: activation.license_key.display_key,
        verified_at: now(),
        expires_at,
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

fn api_message(error: ApiError) -> String {
    match error {
        ApiError::Rejected(_, message) | ApiError::Unavailable(message) => message,
    }
}

fn deactivate_record(client: &Client, record: &SavedLicense) -> Result<(), String> {
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

/// Releases the device slot in Polar before forgetting the local credential.
pub fn deactivate() -> Result<(), String> {
    if let Some(record) = load()? {
        deactivate_record(&client()?, &record)?;
    }
    remove()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn response(status: &str, benefit_id: &str, activation_id: Option<&str>) -> LicenseResponse {
        LicenseResponse {
            organization_id: ORGANIZATION_ID.into(),
            benefit_id: benefit_id.into(),
            status: status.into(),
            display_key: "****-ABCD".into(),
            expires_at: None,
            activation: activation_id.map(|id| ActivationId { id: id.into() }),
        }
    }

    #[test]
    fn only_the_expected_grant_and_device_allow_access() {
        assert!(check_response(
            &response("granted", BENEFIT_ID, Some("device")),
            Some("device"),
            100
        )
        .is_ok());
        assert!(check_response(
            &response("revoked", BENEFIT_ID, Some("device")),
            Some("device"),
            100
        )
        .is_err());
        assert!(check_response(
            &response("granted", "another-benefit", Some("device")),
            Some("device"),
            100
        )
        .is_err());
        assert!(check_response(
            &response("granted", BENEFIT_ID, Some("other")),
            Some("device"),
            100
        )
        .is_err());
    }

    #[test]
    fn offline_grace_ends_after_72_hours_or_expiration() {
        let mut saved = SavedLicense {
            key: "key".into(),
            activation_id: "device".into(),
            display_key: "****-ABCD".into(),
            verified_at: 100,
            expires_at: None,
        };
        assert!(offline_access(&saved, 100 + OFFLINE_GRACE));
        assert!(!offline_access(&saved, 100 + OFFLINE_GRACE + 1));
        assert!(!offline_access(&saved, 99));
        saved.expires_at = Some(200);
        assert!(!offline_access(&saved, 200));
    }

    #[test]
    fn trial_time_left_reads_in_days_then_hours_then_minutes() {
        let hours = |h: u64| Duration::from_secs(h * 3600);
        assert_eq!(trial_left(hours(72), false), "3 days left");
        assert_eq!(trial_left(hours(52), false), "2 days 4 hours left");
        assert_eq!(trial_left(hours(25), false), "1 day 1 hour left");
        assert_eq!(
            trial_left(hours(5) + Duration::from_secs(50 * 60), false),
            "5h 50m left"
        );
        assert_eq!(trial_left(Duration::from_secs(61), false), "2 minutes left");
        assert_eq!(trial_left(Duration::ZERO, false), "1 minute left");
        assert_eq!(trial_left(hours(52), true), "2d 4h");
        assert_eq!(
            trial_left(hours(5) + Duration::from_secs(50 * 60), true),
            "5h"
        );
        assert_eq!(trial_left(Duration::from_secs(42 * 60), true), "42m");
    }

    #[test]
    fn trial_ends_at_exactly_72_hours_and_never_restarts_from_last_seen() {
        let mut trial = SavedTrial {
            started_at: 1_000,
            last_seen: 1_000,
        };
        assert!(matches!(trial_state(&trial, 1_000), Access::Trial { .. }));
        trial.last_seen = 1_000 + TRIAL_LENGTH - 1;
        assert!(matches!(
            trial_state(&trial, 1_000 + TRIAL_LENGTH - 1),
            Access::Trial { .. }
        ));
        assert_eq!(
            trial_state(&trial, 1_000 + TRIAL_LENGTH),
            Access::TrialExpired
        );
        trial.last_seen = 1_000 + TRIAL_LENGTH;
        assert_eq!(trial_state(&trial, 1_000), Access::TrialExpired);
    }

    #[test]
    fn trial_denies_large_clock_rollback_and_invalid_records() {
        let mut trial = SavedTrial {
            started_at: 1_000,
            last_seen: 2_000,
        };
        assert!(matches!(
            trial_state(&trial, 2_000 - CLOCK_TOLERANCE),
            Access::Trial { .. }
        ));
        assert_eq!(
            trial_state(&trial, 2_000 - CLOCK_TOLERANCE - 1),
            Access::TrialExpired
        );
        trial.last_seen = 999;
        assert_eq!(trial_state(&trial, 2_000), Access::TrialExpired);
    }

    #[test]
    fn live_trial_access_checks_the_clock_instead_of_caching_permission() {
        let current = now();
        let valid = Access::Trial {
            expires_at: current + 10,
            last_seen: current,
            license_issue: None,
        };
        assert!(valid.allowed());
        assert!(!Access::Trial {
            expires_at: current,
            last_seen: current,
            license_issue: None,
        }
        .allowed());
        assert_eq!(Access::TrialExpired.trial_remaining(), None);
    }

    #[test]
    fn invalid_paid_key_cannot_block_a_valid_trial_or_look_activated() {
        let current = now();
        let trial = Ok(Access::Trial {
            expires_at: current + 60,
            last_seen: current,
            license_issue: None,
        });
        let failure = Access::Blocked {
            display_key: "••••-1234".into(),
            reason: "License revoked".into(),
        };
        assert!(matches!(
            use_trial_if_available(&trial, failure.clone()),
            Access::Trial {
                license_issue: Some(reason),
                ..
            } if reason == "License revoked"
        ));
        let expired = Ok(Access::TrialExpired);
        assert_eq!(use_trial_if_available(&expired, failure.clone()), failure);
        assert_eq!(
            use_trial_if_available(&trial, Access::Active("paid".into())),
            Access::Active("paid".into())
        );
    }

    #[test]
    fn expired_keys_are_rejected_even_when_polar_returns_granted() {
        let mut license = response("granted", BENEFIT_ID, None);
        license.expires_at = Some("2026-09-24T12:00:00Z".into());
        assert!(check_response(&license, None, 1_790_251_200).is_err());
    }

    #[test]
    fn polar_activation_payload_contains_the_grant_we_check() {
        let raw = json!({
            "id": "device-activation",
            "license_key": {
                "organization_id": ORGANIZATION_ID,
                "benefit_id": BENEFIT_ID,
                "status": "granted",
                "display_key": "****-ABCD",
                "expires_at": null,
            }
        });
        let activation: ActivateResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(activation.id, "device-activation");
        assert!(check_response(&activation.license_key, None, 100).is_ok());
    }

    #[test]
    fn public_activation_request_needs_no_embedded_api_token() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            assert_eq!(request_line.trim_end(), "POST /activate HTTP/1.1");
            let mut content_length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" {
                    break;
                }
                assert!(!header.to_ascii_lowercase().starts_with("authorization:"));
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = value.trim().parse().unwrap();
                }
            }
            let mut request_body = vec![0; content_length];
            reader.read_exact(&mut request_body).unwrap();
            let request: serde_json::Value = serde_json::from_slice(&request_body).unwrap();
            assert_eq!(request["key"], "TEST-KEY");
            assert_eq!(request["organization_id"], ORGANIZATION_ID);

            let body = json!({
                "id": "device-activation",
                "license_key": {
                    "organization_id": ORGANIZATION_ID,
                    "benefit_id": BENEFIT_ID,
                    "status": "granted",
                    "display_key": "****-ABCD",
                    "expires_at": null,
                }
            })
            .to_string();
            write!(
                reader.get_mut(),
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let response = post_at(
            &client().unwrap(),
            &format!("http://{address}"),
            "activate",
            json!({ "key": "TEST-KEY", "organization_id": ORGANIZATION_ID, "label": "Whisple desktop" }),
        )
        .unwrap();
        let activation: ActivateResponse = response.json().unwrap();
        assert_eq!(activation.id, "device-activation");
        assert!(check_response(&activation.license_key, None, 100).is_ok());
        server.join().unwrap();
    }
}
