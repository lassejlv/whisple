//! macOS release updates. GitHub's public release API is the update feed;
//! the binary asset's SHA-256 digest and the app signature are checked before
//! a separate helper swaps bundles after Whisple exits.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const RELEASES_API: &str = "https://api.github.com/repos/lassejlv/whisple/releases?per_page=20";
const DOWNLOAD_PREFIX: &str = "https://github.com/lassejlv/whisple/releases/download/";
const MAX_UPDATE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    published_at: Option<String>,
    assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

struct Candidate<'a> {
    version: Version,
    asset: &'a ReleaseAsset,
}

pub(crate) struct PreparedUpdate {
    pub version: Version,
    temp: TempDir,
}

pub(crate) fn is_packaged() -> bool {
    current_app_bundle().is_some()
}

fn current_app_bundle() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
    let macos = executable.parent()?;
    if macos.file_name()? != "MacOS" || executable.file_name()? != "whisple" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    if app.file_name()? != "Whisple.app"
        || app.starts_with("/Volumes")
        || app.to_string_lossy().contains("/AppTranslocation/")
    {
        return None;
    }
    Some(app.to_path_buf())
}

fn asset_name(version: &Version, arch: &str) -> String {
    format!("Whisple-{version}-macos-{arch}.zip")
}

fn current_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    }
}

fn select_latest<'a>(
    releases: &'a [Release],
    current: &Version,
    arch: &str,
) -> Option<Candidate<'a>> {
    // Publication time, not GitHub's 'latest' endpoint, includes prereleases.
    let mut sorted: Vec<_> = releases
        .iter()
        .filter(|release| !release.draft && release.published_at.is_some())
        .collect();
    sorted.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    for release in sorted {
        let Some(version) = release
            .tag_name
            .strip_prefix('v')
            .and_then(|tag| Version::parse(tag).ok())
        else {
            continue;
        };
        let expected_name = asset_name(&version, arch);
        let Some(asset) = release.assets.iter().find(|asset| {
            asset.name == expected_name
                && asset.state == "uploaded"
                && asset.size > 0
                && asset.size <= MAX_UPDATE_BYTES
                && asset.browser_download_url.starts_with(DOWNLOAD_PREFIX)
                && valid_digest(asset.digest.as_deref()).is_some()
        }) else {
            // A just-published release is incomplete until CI attaches its assets.
            continue;
        };
        return (version > *current).then_some(Candidate { version, asset });
    }
    None
}

fn valid_digest(digest: Option<&str>) -> Option<&str> {
    let hex = digest?.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(hex)
}

pub(crate) fn check_and_prepare(
    prepared_version: Option<Version>,
) -> Result<Option<PreparedUpdate>, String> {
    let Some(current_app) = current_app_bundle() else {
        return Ok(None);
    };
    let mut current = Version::parse(env!("CARGO_PKG_VERSION")).map_err(|err| err.to_string())?;
    if let Some(prepared_version) = prepared_version {
        current = current.max(prepared_version);
    }
    let client = Client::builder()
        .user_agent(format!("Whisple/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(10 * 60))
        .build()
        .map_err(|err| err.to_string())?;
    let body = client
        .get(RELEASES_API)
        .timeout(Duration::from_secs(30))
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.text())
        .map_err(|err| format!("Could not check GitHub releases: {err}"))?;
    let releases: Vec<Release> = serde_json::from_str(&body)
        .map_err(|err| format!("Could not read GitHub releases: {err}"))?;
    let Some(candidate) = select_latest(&releases, &current, current_arch()) else {
        return Ok(None);
    };
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("whisple");
    fs::create_dir_all(&cache_dir).map_err(|err| err.to_string())?;
    let temp = tempfile::Builder::new()
        .prefix("update-")
        .tempdir_in(cache_dir)
        .map_err(|err| err.to_string())?;
    let archive = temp.path().join("Whisple.zip");
    download_and_verify(&client, candidate.asset, &archive)?;
    let status = Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&archive)
        .arg(temp.path())
        .status()
        .map_err(|err| format!("Could not unpack the update: {err}"))?;
    if !status.success() {
        return Err("Could not unpack the update archive.".into());
    }
    let updated_app = temp.path().join("Whisple.app");
    validate_app(&updated_app, &current_app, &candidate.version)?;
    fs::remove_file(archive).map_err(|err| err.to_string())?;
    Ok(Some(PreparedUpdate {
        version: candidate.version,
        temp,
    }))
}

fn download_and_verify(
    client: &Client,
    asset: &ReleaseAsset,
    archive: &Path,
) -> Result<(), String> {
    let mut response = client
        .get(&asset.browser_download_url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("Could not download the update: {err}"))?;
    let mut file = File::create(archive).map_err(|err| err.to_string())?;
    let mut hash = Sha256::new();
    let mut received = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = response.read(&mut buffer).map_err(|err| err.to_string())?;
        if count == 0 {
            break;
        }
        received += count as u64;
        if received > asset.size || received > MAX_UPDATE_BYTES {
            return Err("The update download exceeded its expected size.".into());
        }
        hash.update(&buffer[..count]);
        file.write_all(&buffer[..count])
            .map_err(|err| err.to_string())?;
    }
    if received != asset.size {
        return Err("The update download was incomplete.".into());
    }
    let actual: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let expected = valid_digest(asset.digest.as_deref()).ok_or("Missing update checksum.")?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err("The update checksum did not match GitHub's release asset.".into());
    }
    Ok(())
}

fn command_output(program: &str, args: &[&str], path: &Path) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .arg(path)
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(format!("Could not validate update with {program}."));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn plist_value(path: &Path, key: &str) -> Result<String, String> {
    command_output(
        "/usr/libexec/PlistBuddy",
        &["-c", &format!("Print :{key}")],
        path,
    )
}

fn team_id(app: &Path) -> Option<String> {
    let output = Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=4"])
        .arg(app)
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|team| *team != "not set")
        .map(str::to_string)
}

fn validate_app(app: &Path, current_app: &Path, version: &Version) -> Result<(), String> {
    if !app.is_dir() {
        return Err("The update archive has no Whisple.app.".into());
    }
    let status = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .status()
        .map_err(|err| err.to_string())?;
    if !status.success() {
        return Err("The updated app has an invalid code signature.".into());
    }
    let plist = app.join("Contents/Info.plist");
    if plist_value(&plist, "CFBundleIdentifier")? != "app.whisp"
        || plist_value(&plist, "CFBundleExecutable")? != "whisple"
        || plist_value(&plist, "WhispleSemanticVersion")? != version.to_string()
    {
        return Err("The updated app identity or version does not match the release.".into());
    }
    let archs = command_output(
        "/usr/bin/lipo",
        &["-archs"],
        &app.join("Contents/MacOS/whisple"),
    )?;
    if !archs.split_whitespace().any(|arch| arch == current_arch()) {
        return Err("The update does not contain this Mac's architecture.".into());
    }
    if let Some(current_team) = team_id(current_app) {
        if team_id(app).as_deref() != Some(current_team.as_str()) {
            return Err("The update was signed by a different Apple developer.".into());
        }
    }
    Ok(())
}

impl PreparedUpdate {
    pub(crate) fn install(&mut self) -> Result<(), String> {
        let current_app =
            current_app_bundle().ok_or("Whisple must run from an app bundle to update.")?;
        let helper = self.temp.path().join("apply-update.sh");
        fs::write(&helper, include_str!("../scripts/apply-update.sh"))
            .map_err(|err| err.to_string())?;
        let log =
            File::create(self.temp.path().join("update.log")).map_err(|err| err.to_string())?;
        Command::new("/bin/bash")
            .arg(&helper)
            .arg(&current_app)
            .arg(self.temp.path().join("Whisple.app"))
            .arg(std::process::id().to_string())
            .arg(self.temp.path())
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().map_err(|err| err.to_string())?))
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|err| format!("Could not start the update installer: {err}"))?;
        self.temp.disable_cleanup(true);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, published: &str, arch: &str, complete: bool) -> Release {
        let version = Version::parse(tag.strip_prefix('v').unwrap()).unwrap();
        Release {
            tag_name: tag.into(),
            draft: false,
            published_at: Some(published.into()),
            assets: if complete {
                vec![ReleaseAsset {
                    name: asset_name(&version, arch),
                    state: "uploaded".into(),
                    size: 1024,
                    digest: Some(format!("sha256:{}", "a".repeat(64))),
                    browser_download_url: format!("{DOWNLOAD_PREFIX}{tag}/update.zip"),
                }]
            } else {
                vec![]
            },
        }
    }

    #[test]
    fn follows_latest_complete_release_including_prereleases() {
        let releases = [
            release("v1.2.0-rc.2", "2026-09-24T10:00:00Z", "arm64", false),
            release("v1.2.0-rc.1", "2026-09-23T10:00:00Z", "arm64", true),
            release("v1.1.0", "2026-09-22T10:00:00Z", "arm64", true),
        ];
        let candidate =
            select_latest(&releases, &Version::parse("1.1.0").unwrap(), "arm64").unwrap();
        assert_eq!(candidate.version.to_string(), "1.2.0-rc.1");
        let stable = [
            release("v1.2.0", "2026-09-25T10:00:00Z", "arm64", true),
            releases[1].clone(),
        ];
        assert_eq!(
            select_latest(&stable, &Version::parse("1.2.0-rc.1").unwrap(), "arm64")
                .unwrap()
                .version
                .to_string(),
            "1.2.0"
        );
    }

    #[test]
    fn rejects_wrong_arch_and_downgrades() {
        let releases = [release("v1.0.1", "2026-09-24T10:00:00Z", "x86_64", true)];
        assert!(select_latest(&releases, &Version::parse("1.0.0").unwrap(), "arm64").is_none());
        assert!(select_latest(&releases, &Version::parse("1.0.2").unwrap(), "x86_64").is_none());
    }
}
