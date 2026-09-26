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

/// What the bar's update line says. Separate from the prepared app, so
/// dismissing the line keeps the update ready in the tray menu and About.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdatePrompt {
    Ready,
    JustUpdated,
}

pub(crate) struct PreparedUpdate {
    pub version: Version,
    temp: TempDir,
}

fn update_marker() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("whisple/installed-version")
}

fn write_update_marker(marker: &Path, version: &Version) -> Result<(), String> {
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Could not create the update status directory: {err}"))?;
    }
    fs::write(marker, version.to_string())
        .map_err(|err| format!("Could not record the update version: {err}"))
}

/// Only an actual version change after an install request displays this notice.
pub(crate) fn just_updated() -> bool {
    let marker = update_marker();
    let Ok(version) = fs::read_to_string(&marker) else {
        return false;
    };
    if version.trim() != env!("CARGO_PKG_VERSION") {
        return false;
    }
    let _ = fs::remove_file(marker);
    true
}

pub(crate) fn is_packaged() -> bool {
    current_install().is_some()
}

/// The installed app this process runs from: the `Whisple.app` bundle on
/// macOS, or `whisple.exe` in the per-user folder both Windows installers use.
/// Development builds and apps run from a disk image are never updated.
#[cfg(target_os = "macos")]
fn current_install() -> Option<PathBuf> {
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

#[cfg(target_os = "windows")]
fn current_install() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
    let expected = windows_install_dir()?.canonicalize().ok()?;
    let folder = executable.parent()?;
    let same_folder = folder
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.to_string_lossy());
    let named = executable
        .file_name()?
        .to_string_lossy()
        .eq_ignore_ascii_case("whisple.exe");
    (same_folder && named).then_some(executable)
}

/// `%LOCALAPPDATA%\Programs\Whisple`, where the installers put Whisple so it
/// can replace itself without administrator rights.
#[cfg(target_os = "windows")]
fn windows_install_dir() -> Option<PathBuf> {
    Some(dirs::data_local_dir()?.join("Programs").join("Whisple"))
}

fn asset_name(version: &Version, arch: &str) -> String {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else {
        "macos"
    };
    format!("Whisple-{version}-{os}-{arch}.zip")
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
    let Some(current) = current_install() else {
        return Ok(None);
    };
    let mut current_version =
        Version::parse(env!("CARGO_PKG_VERSION")).map_err(|err| err.to_string())?;
    if let Some(prepared_version) = prepared_version {
        current_version = current_version.max(prepared_version);
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
    let Some(candidate) = select_latest(&releases, &current_version, current_arch()) else {
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
    unpack(&archive, temp.path())?;
    validate(&staged(temp.path()), &current, &candidate.version)?;
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

/// Where the unpacked update sits inside its temporary folder.
fn staged(temp: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        temp.join("whisple.exe")
    } else {
        temp.join("Whisple.app")
    }
}

#[cfg(target_os = "macos")]
fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    let status = Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(archive)
        .arg(into)
        .status()
        .map_err(|err| format!("Could not unpack the update: {err}"))?;
    if !status.success() {
        return Err("Could not unpack the update archive.".into());
    }
    Ok(())
}

/// Windows 10 and later include bsdtar, which reads ZIP archives.
#[cfg(target_os = "windows")]
fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    let tar = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("tar.exe"))
        .ok_or("Could not find the Windows folder.")?;
    let mut command = Command::new(tar);
    command.arg("-xf").arg(archive).arg("-C").arg(into);
    let status = hidden(&mut command)
        .status()
        .map_err(|err| format!("Could not unpack the update: {err}"))?;
    if !status.success() {
        return Err("Could not unpack the update archive.".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn plist_value(path: &Path, key: &str) -> Result<String, String> {
    command_output(
        "/usr/libexec/PlistBuddy",
        &["-c", &format!("Print :{key}")],
        path,
    )
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn validate(app: &Path, current_app: &Path, version: &Version) -> Result<(), String> {
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

/// Windows releases are unsigned, so the GitHub digest checked during the
/// download is what ties the file to the release. This confirms the file is
/// Whisple at that version, built for this processor.
#[cfg(target_os = "windows")]
fn validate(exe: &Path, _current: &Path, version: &Version) -> Result<(), String> {
    if !exe.is_file() {
        return Err("The update archive has no whisple.exe.".into());
    }
    let product = crate::platform::windows::version_string(exe, "ProductName");
    let semantic = crate::platform::windows::version_string(exe, "WhispleSemanticVersion");
    if product.as_deref() != Some("Whisple") || semantic != Some(version.to_string()) {
        return Err("The updated app identity or version does not match the release.".into());
    }
    let machine = pe_machine(exe).ok_or("The update is not a Windows program.")?;
    let expected = if current_arch() == "arm64" {
        0xAA64
    } else {
        0x8664
    };
    if machine != expected {
        return Err("The update is not built for this PC's processor.".into());
    }
    Ok(())
}

/// The processor a Windows executable targets, from its PE header.
#[cfg(target_os = "windows")]
fn pe_machine(exe: &Path) -> Option<u16> {
    let mut header = [0u8; 1024];
    let mut file = File::open(exe).ok()?;
    file.read_exact(&mut header).ok()?;
    if &header[..2] != b"MZ" {
        return None;
    }
    let offset = u32::from_le_bytes(header[0x3C..0x40].try_into().ok()?) as usize;
    let signature = header.get(offset..offset + 4)?;
    if signature != b"PE\0\0" {
        return None;
    }
    let machine = header.get(offset + 4..offset + 6)?;
    Some(u16::from_le_bytes([machine[0], machine[1]]))
}

/// Keeps helper programs from flashing a console window.
#[cfg(target_os = "windows")]
fn hidden(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW)
}

impl PreparedUpdate {
    pub(crate) fn install(&mut self) -> Result<(), String> {
        let current = current_install().ok_or("Whisple must be installed to update.")?;
        let log =
            File::create(self.temp.path().join("update.log")).map_err(|err| err.to_string())?;
        let marker = update_marker();
        write_update_marker(&marker, &self.version)?;
        if let Err(err) = self
            .installer(&current)?
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().map_err(|err| err.to_string())?))
            .stderr(Stdio::from(log))
            .spawn()
        {
            let _ = fs::remove_file(marker);
            return Err(format!("Could not start the update installer: {err}"));
        }
        self.temp.disable_cleanup(true);
        Ok(())
    }

    /// The helper that waits for Whisple to quit, swaps in the update and
    /// starts it again.
    #[cfg(target_os = "macos")]
    fn installer(&self, current_app: &Path) -> Result<Command, String> {
        let helper = self.temp.path().join("apply-update.sh");
        fs::write(&helper, include_str!("../scripts/apply-update.sh"))
            .map_err(|err| err.to_string())?;
        let mut command = Command::new("/bin/bash");
        command
            .arg(&helper)
            .arg(current_app)
            .arg(staged(self.temp.path()))
            .arg(std::process::id().to_string())
            .arg(self.temp.path());
        Ok(command)
    }

    #[cfg(target_os = "windows")]
    fn installer(&self, current_exe: &Path) -> Result<Command, String> {
        let helper = self.temp.path().join("apply-update.ps1");
        fs::write(&helper, include_str!("../scripts/windows/apply-update.ps1"))
            .map_err(|err| err.to_string())?;
        let mut command = Command::new("powershell.exe");
        hidden(&mut command)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
            ])
            .arg("-File")
            .arg(&helper)
            .arg(current_exe)
            .arg(staged(self.temp.path()))
            .arg(std::process::id().to_string())
            .arg(self.temp.path())
            .arg(self.version.to_string());
        Ok(command)
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

    #[test]
    fn asset_names_match_the_release_workflows() {
        let version = Version::parse("0.3.0-rc.1").unwrap();
        let expected = if cfg!(target_os = "windows") {
            "Whisple-0.3.0-rc.1-windows-x86_64.zip"
        } else {
            "Whisple-0.3.0-rc.1-macos-x86_64.zip"
        };
        assert_eq!(asset_name(&version, "x86_64"), expected);
    }

    #[test]
    fn first_update_creates_status_directory_before_writing_marker() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("whisple/installed-version");
        let version = Version::parse("0.2.0").unwrap();
        write_update_marker(&marker, &version).unwrap();
        assert_eq!(fs::read_to_string(marker).unwrap(), "0.2.0");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn this_build_reports_its_processor() {
        let exe = std::env::current_exe().unwrap();
        let expected = if cfg!(target_arch = "aarch64") {
            0xAA64
        } else {
            0x8664
        };
        assert_eq!(pe_machine(&exe), Some(expected));
    }
}
