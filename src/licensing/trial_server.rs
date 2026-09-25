use std::time::Duration;

use base64::Engine;
use reqwest::blocking::Client;
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

const TRIAL_URL: &str = "https://cloud.whisple.app/v1/trial";

/// Keys the server signs with, base64. Keep the old key here for one release
/// when rotating.
const PUBLIC_KEYS: &[&str] = &["5V2WHQz4l6mnfI8xjEXDgIoGPyxQQyP691duq8jxeTw="];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerTrial {
    pub started_at: i64,
    pub expires_at: i64,
}

#[derive(Deserialize)]
struct Payload {
    v: u32,
    device: String,
    started_at: i64,
    expires_at: i64,
}

#[derive(Deserialize)]
struct Reply {
    token: String,
}

/// `sha256("whisple-trial-v1:" + hardware ID)` in lowercase hex. The raw
/// hardware ID never leaves the computer.
pub fn device_id() -> Option<String> {
    hardware_id().map(|id| device_hash(&id))
}

fn device_hash(hardware_id: &str) -> String {
    Sha256::digest(format!("whisple-trial-v1:{hardware_id}").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The Mac's IOPlatformUUID, which survives reinstalls, new macOS users and
/// erased disks.
#[cfg(target_os = "macos")]
fn hardware_id() -> Option<String> {
    let output = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    platform_uuid(&String::from_utf8_lossy(&output.stdout))
}

/// Windows uses the installation's MachineGuid.
#[cfg(target_os = "windows")]
fn hardware_id() -> Option<String> {
    crate::platform::windows::machine_guid()
}

/// Development builds on Linux use the machine ID instead.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn hardware_id() -> Option<String> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn platform_uuid(ioreg: &str) -> Option<String> {
    ioreg
        .lines()
        .find(|line| line.contains("\"IOPlatformUUID\""))
        .and_then(|line| line.rsplit('"').nth(1))
        .map(str::to_string)
        .filter(|id| !id.is_empty())
}

pub fn fetch(device: &str) -> Result<String, String> {
    fetch_from(TRIAL_URL, device)
}

fn fetch_from(url: &str, device: &str) -> Result<String, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(concat!("Whisple/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| format!("Could not start the trial check: {err}"))?;
    let response = client
        .post(url)
        .json(&json!({
            "device": device,
            "app_version": env!("CARGO_PKG_VERSION"),
        }))
        .send()
        .map_err(|_| "Could not reach the Whisple server.".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "The Whisple server could not check the trial (HTTP {}).",
            response.status().as_u16()
        ));
    }
    response
        .json::<Reply>()
        .map(|reply| reply.token)
        .map_err(|_| "The Whisple server sent an unreadable trial.".to_string())
}

/// The trial a token describes, when its signature is from the server and it
/// was issued for `device`.
pub fn verify(token: &str, device: &str) -> Option<ServerTrial> {
    verify_with(token, device, PUBLIC_KEYS)
}

fn verify_with(token: &str, device: &str, keys: &[&str]) -> Option<ServerTrial> {
    let (payload, signature) = token.split_once('.')?;
    let payload = decode_url(payload)?;
    let signature = decode_url(signature)?;
    let signed = keys.iter().any(|key| {
        base64::engine::general_purpose::STANDARD
            .decode(key)
            .is_ok_and(|key| {
                UnparsedPublicKey::new(&ED25519, key)
                    .verify(&payload, &signature)
                    .is_ok()
            })
    });
    if !signed {
        return None;
    }
    let payload: Payload = serde_json::from_slice(&payload).ok()?;
    (payload.v == 1
        && payload.device == device
        && payload.started_at > 0
        && payload.expires_at > payload.started_at)
        .then_some(ServerTrial {
            started_at: payload.started_at,
            expires_at: payload.expires_at,
        })
}

fn decode_url(part: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(part.trim_end_matches('='))
        .ok()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    const DEVICE: &str = "5f2b1c9a0d3e4f5a6b7c8d9e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c";

    fn key_pair() -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap()
    }

    fn public_key() -> String {
        base64::engine::general_purpose::STANDARD.encode(key_pair().public_key().as_ref())
    }

    pub(crate) fn sign(payload: &str) -> String {
        let url = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let signature = key_pair().sign(payload.as_bytes());
        format!("{}.{}", url.encode(payload), url.encode(signature.as_ref()))
    }

    fn payload(device: &str) -> String {
        format!(
            r#"{{"v":1,"device":"{device}","started_at":1000,"expires_at":260200,"issued_at":5000}}"#
        )
    }

    #[test]
    fn the_device_id_is_a_hash_of_the_hardware_id() {
        assert_eq!(
            device_hash("ABC-123"),
            "3cdf7c4959953d90d47c9ba90510a059046816598a7bd2a33afabaf42390e852"
        );
        let id = device_hash("ABC-123");
        assert_eq!(id.len(), 64);
        assert!(id
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()));
        assert_ne!(id, device_hash("ABC-124"));
    }

    #[test]
    fn ioreg_output_gives_the_platform_uuid() {
        let ioreg = r#"+-o J316sAP  <class IOPlatformExpertDevice, id 0x100000110>
    {
      "IOPlatformSerialNumber" = "C02XXXXX"
      "IOPlatformUUID" = "0A1B2C3D-4E5F-6789-ABCD-EF0123456789"
    }"#;
        assert_eq!(
            platform_uuid(ioreg).as_deref(),
            Some("0A1B2C3D-4E5F-6789-ABCD-EF0123456789")
        );
        assert_eq!(platform_uuid("nothing here"), None);
    }

    #[test]
    fn a_signed_token_for_this_mac_gives_its_trial() {
        let key = public_key();
        let token = sign(&payload(DEVICE));
        assert_eq!(
            verify_with(&token, DEVICE, &[&key]),
            Some(ServerTrial {
                started_at: 1000,
                expires_at: 260200,
            })
        );
        let (payload_part, signature_part) = token.split_once('.').unwrap();
        let padded = format!("{payload_part}==.{signature_part}==");
        assert!(verify_with(&padded, DEVICE, &[&key]).is_some());
        assert!(verify_with(&token, DEVICE, &[PUBLIC_KEYS[0], &key]).is_some());
    }

    #[test]
    fn a_forged_copied_or_damaged_token_is_refused() {
        let key = public_key();
        let token = sign(&payload(DEVICE));
        assert_eq!(verify_with(&token, DEVICE, PUBLIC_KEYS), None);
        let other = "0".repeat(64);
        assert_eq!(verify_with(&token, &other, &[&key]), None);
        let (_, signature) = token.split_once('.').unwrap();
        let edited = payload(DEVICE).replace("1000", "9000");
        let forged = format!(
            "{}.{signature}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(edited)
        );
        assert_eq!(verify_with(&forged, DEVICE, &[&key]), None);
        let future = sign(&payload(DEVICE).replace(r#""v":1"#, r#""v":2"#));
        assert_eq!(verify_with(&future, DEVICE, &[&key]), None);
        assert_eq!(verify_with("not-a-token", DEVICE, &[&key]), None);
        assert_eq!(verify_with("a.b", DEVICE, &[&key]), None);
    }

    #[test]
    fn the_built_in_public_key_is_an_ed25519_key() {
        for key in PUBLIC_KEYS {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(key)
                .unwrap();
            assert_eq!(bytes.len(), 32);
        }
    }

    #[test]
    fn the_request_names_the_device_and_version_and_returns_the_token() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/trial", listener.local_addr().unwrap());
        let token = sign(&payload(DEVICE));
        let reply_token = token.clone();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
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
            assert!(head.starts_with("post /v1/trial"));
            assert!(head.contains("content-type: application/json"));
            let body: serde_json::Value = serde_json::from_slice(&request[body_start..]).unwrap();
            assert_eq!(body["device"], DEVICE);
            assert_eq!(body["app_version"], env!("CARGO_PKG_VERSION"));
            let reply = json!({ "token": reply_token }).to_string();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            )
            .unwrap();
        });
        assert_eq!(fetch_from(&url, DEVICE).unwrap(), token);
        server.join().unwrap();
    }

    #[test]
    fn an_unreachable_or_refusing_server_is_an_error() {
        assert!(fetch_from("http://127.0.0.1:9/v1/trial", DEVICE).is_err());
    }
}
