pub(crate) mod boot_clock;
mod polar;
mod trial;
pub(crate) mod trial_server;
pub(crate) use polar::*;
pub(crate) use trial::*;

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::DateTime;
use reqwest::blocking::{Client, Response};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;

use self::boot_clock::Snapshot;
use self::trial_server::ServerTrial;
use crate::i18n::{t, tf};

pub const CHECKOUT_URL: &str =
    "https://buy.polar.sh/polar_cl_jvWJVAZBAHpctw43ZsWUNlIfBCYx0f6jizX8x4Hoqud";
pub const CUSTOMER_PORTAL_URL: &str = "https://polar.sh/whisple/portal";
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
    Checking,
    Trial {
        expires_at: i64,
        last_seen: i64,
        confirmation_required: bool,
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
            Self::Checking | Self::Trial { .. } | Self::TrialExpired => None,
        }
    }
}

pub fn trial_left(remaining: Duration, compact: bool) -> String {
    let total_minutes = remaining.as_secs().div_ceil(60).max(1);
    let days = total_minutes / (24 * 60);
    let hours = total_minutes % (24 * 60) / 60;
    let minutes = total_minutes % 60;
    let day_count = || {
        if days == 1 {
            tf("{} day", &[&days])
        } else {
            tf("{} days", &[&days])
        }
    };
    let hour_count = || {
        if hours == 1 {
            tf("{} hour", &[&hours])
        } else {
            tf("{} hours", &[&hours])
        }
    };
    let minute_count = || {
        if minutes == 1 {
            tf("{} minute", &[&minutes])
        } else {
            tf("{} minutes", &[&minutes])
        }
    };
    match (compact, days, hours) {
        (true, 1.., _) => tf("{}d {}h", &[&days, &hours]),
        (true, 0, 1..) => tf("{}h", &[&hours]),
        (true, 0, 0) => tf("{}m", &[&minutes]),
        (false, 1.., 0) => tf("{} left", &[&day_count()]),
        (false, 1.., _) => tf("{} {} left", &[&day_count(), &hour_count()]),
        (false, 0, 1..) => tf("{}h {}m left", &[&hours, &minutes]),
        (false, 0, 0) => tf("{} left", &[&minute_count()]),
    }
}

/// Refreshes the saved key. A recent successful check permits short offline use.
pub fn check_saved() -> Access {
    // Keep the trial's last-seen time current even for paying users. Its
    // server request can be slow, so paid validation must not wait for it.
    let trial_check = std::thread::spawn(start_trial);
    let record = match load() {
        Ok(Some(record)) => record,
        Ok(None) => {
            return finish_trial_check(trial_check).unwrap_or_else(|reason| Access::Unavailable {
                display_key: String::new(),
                reason,
            })
        }
        Err(reason) => {
            return use_trial_if_available(
                &finish_trial_check(trial_check),
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
                &finish_trial_check(trial_check),
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
                last_seen: checked_at.max(record.last_seen),
                offline_clock: boot_clock::snapshot()
                    .as_ref()
                    .map(|snapshot| OfflineClock::from_snapshot(snapshot, OFFLINE_GRACE as u64)),
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
        Err(ApiError::Unavailable(reason)) => {
            let anchor = boot_clock::snapshot()
                .as_ref()
                .and_then(|snapshot| offline_access(&record, checked_at, snapshot));
            let mut updated = record.clone();
            updated.last_seen = checked_at.max(updated.last_seen);
            if let Some(anchor) = &anchor {
                updated.offline_clock = Some(anchor.clone());
            }
            let persisted = updated == record || save(&updated).is_ok();
            if anchor.is_some() && persisted {
                Access::Offline(record.display_key)
            } else {
                Access::Unavailable {
                    display_key: record.display_key,
                    reason,
                }
            }
        }
    };
    if matches!(&paid, Access::Active(_) | Access::Offline(_)) {
        paid
    } else {
        use_trial_if_available(&finish_trial_check(trial_check), paid)
    }
}

fn finish_trial_check(
    check: std::thread::JoinHandle<Result<Access, String>>,
) -> Result<Access, String> {
    check
        .join()
        .map_err(|_| "Could not check the saved trial.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn snapshot(seconds: u64) -> Snapshot {
        Snapshot {
            boot_id: "boot-1".into(),
            seconds,
        }
    }

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
            last_seen: 0,
            offline_clock: None,
        };
        assert!(offline_access(&saved, 100 + OFFLINE_GRACE, &snapshot(10)).is_some());
        assert!(offline_access(&saved, 100 + OFFLINE_GRACE + 1, &snapshot(10)).is_none());
        assert!(offline_access(&saved, 99, &snapshot(10)).is_none());
        saved.expires_at = Some(200);
        assert!(offline_access(&saved, 200, &snapshot(10)).is_none());
    }

    #[test]
    fn offline_use_cannot_be_stretched_by_setting_the_clock_back() {
        let mut saved = SavedLicense {
            key: "key".into(),
            activation_id: "device".into(),
            display_key: "****-ABCD".into(),
            verified_at: 1_000,
            expires_at: None,
            last_seen: 1_000 + OFFLINE_GRACE - 10,
            offline_clock: None,
        };
        assert!(offline_access(&saved, 1_000 + OFFLINE_GRACE - 5, &snapshot(10)).is_some());
        assert!(offline_access(&saved, 1_010, &snapshot(10)).is_none());
        saved.last_seen = 1_000 + OFFLINE_GRACE + 60;
        assert!(offline_access(&saved, 1_000 + OFFLINE_GRACE + 30, &snapshot(10)).is_none());
        saved.last_seen = 0;
        assert!(offline_access(&saved, 1_010, &snapshot(10)).is_some());
    }

    #[test]
    fn a_frozen_wall_clock_cannot_extend_offline_use() {
        let saved = SavedLicense {
            key: "key".into(),
            activation_id: "device".into(),
            display_key: "****-ABCD".into(),
            verified_at: 1_000,
            expires_at: None,
            last_seen: 1_000,
            offline_clock: Some(OfflineClock::from_snapshot(
                &snapshot(100),
                OFFLINE_GRACE as u64,
            )),
        };
        let frozen_wall_time = 1_060;
        assert!(offline_access(
            &saved,
            frozen_wall_time,
            &snapshot(100 + OFFLINE_GRACE as u64)
        )
        .is_some());
        assert!(offline_access(
            &saved,
            frozen_wall_time,
            &snapshot(101 + OFFLINE_GRACE as u64)
        )
        .is_none());
        let rebooted = Snapshot {
            boot_id: "boot-2".into(),
            seconds: 50,
        };
        assert!(offline_access(&saved, frozen_wall_time, &rebooted).is_none());
    }

    #[test]
    fn an_unconfirmed_trial_locks_after_an_hour_until_the_server_answers() {
        let record = SavedTrial {
            started_at: 1_000,
            last_seen: 1_000,
            token: None,
        };
        assert!(matches!(
            trial_access(&record, false, 1_000),
            Access::Trial {
                expires_at,
                confirmation_required: true,
                ..
            } if expires_at == 1_000 + UNCONFIRMED_TRIAL
        ));
        assert!(matches!(
            trial_access(&record, false, 1_000 + UNCONFIRMED_TRIAL - 1),
            Access::Trial { .. }
        ));
        assert!(matches!(
            trial_access(&record, false, 1_000 + UNCONFIRMED_TRIAL),
            Access::Unavailable { .. }
        ));
        assert!(matches!(
            trial_access(&record, true, 1_000 + TRIAL_LENGTH - 1),
            Access::Trial { .. }
        ));
        assert_eq!(
            trial_access(&record, false, 1_000 + TRIAL_LENGTH),
            Access::TrialExpired
        );
    }

    #[test]
    fn the_server_start_wins_when_it_is_earlier() {
        let mut record = SavedTrial {
            started_at: 9_000,
            last_seen: 9_000,
            token: None,
        };
        apply_server_trial(
            &mut record,
            ServerTrial {
                started_at: 1_000,
                expires_at: 1_000 + TRIAL_LENGTH,
            },
        );
        assert_eq!(record.started_at, 1_000);
        apply_server_trial(
            &mut record,
            ServerTrial {
                started_at: 5_000,
                expires_at: 5_000 + TRIAL_LENGTH,
            },
        );
        assert_eq!(record.started_at, 1_000);
        let mut short = SavedTrial {
            started_at: 100_000,
            last_seen: 100_000,
            token: None,
        };
        apply_server_trial(
            &mut short,
            ServerTrial {
                started_at: 100_000,
                expires_at: 100_000 + 3_600,
            },
        );
        assert_eq!(trial_state(&short, 100_000 + 3_600), Access::TrialExpired);
    }

    #[test]
    fn a_saved_token_travels_with_the_merged_trial() {
        let with_token = SavedTrial {
            started_at: 1_000,
            last_seen: 2_000,
            token: Some("payload.signature".into()),
        };
        let without = SavedTrial {
            started_at: 1_500,
            last_seen: 3_000,
            token: None,
        };
        let merged = merge_trials(Some(without), Some(with_token), 4_000);
        assert_eq!(merged.token.as_deref(), Some("payload.signature"));
        assert_eq!(merged.started_at, 1_000);
        assert_eq!(merged.last_seen, 3_000);
        let old: SavedTrial = serde_json::from_str(r#"{"started_at":1,"last_seen":2}"#).unwrap();
        assert_eq!(old.token, None);
    }

    #[test]
    fn a_trial_survives_losing_one_of_its_two_copies() {
        let started = SavedTrial {
            started_at: 1_000,
            last_seen: 5_000,
            token: None,
        };
        let fresh = SavedTrial {
            started_at: 9_000,
            last_seen: 9_000,
            token: None,
        };
        assert_eq!(merge_trials(None, Some(started.clone()), 9_000), started);
        assert_eq!(merge_trials(Some(started.clone()), None, 9_000), started);
        assert_eq!(
            merge_trials(Some(fresh.clone()), Some(started.clone()), 9_000),
            SavedTrial {
                started_at: 1_000,
                last_seen: 9_000,
                token: None,
            }
        );
        assert_eq!(merge_trials(None, None, 9_000), fresh);
    }

    #[test]
    fn a_valid_trial_copy_recovers_a_damaged_keychain_item() {
        let saved = SavedTrial {
            started_at: 1_000,
            last_seen: 2_000,
            token: None,
        };
        let (keychain, file) = recover_trial_copies(
            Err("The saved trial could not be read.".into()),
            Ok(Some(saved.clone())),
        )
        .unwrap();
        assert_eq!(keychain, None);
        assert_eq!(file, Some(saved));
        assert!(
            recover_trial_copies(Err("The saved trial could not be read.".into()), Ok(None))
                .is_err()
        );
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
            token: None,
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
            token: None,
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
            confirmation_required: false,
            license_issue: None,
        };
        assert!(valid.allowed());
        assert!(!Access::Trial {
            expires_at: current,
            last_seen: current,
            confirmation_required: false,
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
            confirmation_required: false,
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
