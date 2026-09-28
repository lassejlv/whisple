# Releases and updates

Whisple publishes macOS builds for Apple silicon and Intel, and Windows builds for x86_64. Packaged builds check GitHub's **latest stable release** on startup and every six hours. Drafts and prereleases are never offered as updates. Once the platform's ZIP is downloaded and verified, a **New update available** window shows the new and installed versions, the release's Markdown notes, **Later**, and **Install and restart**. It appears even when the voice bar is hidden, but waits for recording and transcription to finish. Dismissing it keeps the prepared update available through the tray and Settings → About. Installing waits for the app to exit, replaces it, and relaunches it.

Cargo builds and local packages omit licensing by default. Release jobs test and build with `--features licensing`, retaining the paid trial and license flow on both platforms.

### Preview the update window

In a debug build, set `WHISPLE_DEV_UPDATE_PREVIEW=0.0.3` to simulate a prepared
update. Use isolated preferences with `WHISPLE_DEV_DATA_DIR` and completed
onboarding. `WHISPLE_DEV_UPDATE_PREVIEW_DELAY_MS=15000` delays the offer to test
arrival during recording. Preview updates cannot launch an installer; the install
action exercises error feedback. These variables have no effect in release builds.

## Publish a release

1. Set a stable `X.Y.Z` version in `Cargo.toml` and update `Cargo.lock`. Use the GitHub **prerelease checkbox** to stage the build; do not add `-rc` to the version for this promotion flow.
2. Push the source and these workflows to the default branch, then push `v<version>` at that commit.
3. Create and **publish a prerelease** for that tag. Saving a draft alone does not trigger builds.
4. **Publish release** runs automatically:
   - It validates the tag against Cargo, pins the source commit, and keeps the release marked prerelease.
   - Both signed/notarized macOS builds and the Windows build run in parallel, with licensed tests.
   - Windows verifies both installers, and all three builds unpack their actual ZIP through `src/updater.rs` and validate the payload.
   - After every build succeeds, the final job verifies all ten files and checksums, attaches them to the same release, and checks GitHub's uploaded sizes and SHA-256 digests.
   - Only then does it clear prerelease and explicitly mark the release **Latest**. A newer existing latest version cannot be replaced by an older build.

If a build, test, upload, or digest check fails, the release stays a prerelease. Fix the failure and rerun **Publish release** with the existing tag and **publish enabled**. Release tags must include these scripts. Publishing a regular release is also handled by moving it back to prerelease at the start, but create it as a prerelease to avoid exposing it before the workflow starts.

The complete release contains:

- Each macOS architecture: `.dmg`, `.zip`, `.sha256` (six files total).
- Windows x86_64: `-setup.exe`, `.msi`, `.zip`, `.sha256` (four files).

To verify everything without publishing, run **Publish release** manually with `tag: main` (or a full commit SHA) and **publish disabled**. It runs the same builds, updater/archive checks, and final completeness gate; the three artifact bundles remain available for 14 days. Individual platform workflows remain available for build verification only; release uploads and promotion are centralized in **Publish release**.

The updater uses `/repos/lassejlv/whisple/releases/latest`, accepts only a newer stable semantic version, and requires the exact platform/architecture archive URL, an uploaded nonempty asset, and a valid GitHub SHA-256 digest. It verifies download size and hash before unpacking. A repository with no stable release returns no update normally. Existing older app versions that accepted prereleases retain that behavior until they install a build containing this updater change.

## macOS

The release workflow requires Developer ID Application signing and Apple notarization for both architectures. It signs the app with hardened runtime and a secure timestamp, notarizes and staples the app before creating the updater ZIP and DMG, then signs, notarizes, and staples the DMG. Gatekeeper must accept both artifacts before they are uploaded. Checksums are generated after stapling. The updater checks the public release asset digest, bundle signature, app identity, architecture, and the signing team when the installed app has one.

### GitHub configuration

Set the repository Actions variable `APPLE_TEAM_ID` to the team that owns both the Developer ID certificate and App Store Connect Team API key. Configure these repository Actions secrets:

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE_P12_BASE64` | Base64 of the Developer ID Application certificate and private key exported as `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Password protecting that `.p12` |
| `APPLE_NOTARY_KEY_P8_BASE64` | Base64 of the App Store Connect Team API private key (`.p8`) |
| `APPLE_NOTARY_KEY_ID` | Key ID from App Store Connect |
| `APPLE_NOTARY_ISSUER_ID` | Issuer ID from App Store Connect |

Use a Team API key with Developer access. `setup-macos-signing.sh` validates the certificate's team, performs a signing preflight, authenticates with Apple, imports credentials into a temporary keychain, and removes the decoded source files. Cleanup restores the original keychain search list and deletes the temporary keychain after success or failure. Credentials and recovery copies must remain outside the repository; never add them to workflow artifacts.

### Verify signing without publishing a release

Run **Build macOS release assets** manually with `tag: main` (or a commit/ref containing the signing scripts) to verify without publishing. Both architectures run the licensed Rust tests, build, sign, notarize, and pass Gatekeeper assessment. Download the DMG, updater ZIP, and checksums from the run's artifacts, retained for 14 days. This does not add a release to the updater feed.

The combined **Publish release** workflow calls this same verified flow and attaches assets after all platforms pass. Missing credentials or rejected notarization fail the job; release CI never falls back to ad hoc signing. Local packaging still uses ad hoc signing unless `WHISPLE_CODESIGN_IDENTITY` is provided.

The app icon and Finder DMG layout come from the [Release page in Paper](https://app.paper.design/file/01M391FYN8XXTW9JHFATBK4Q06/p-8-0). The DMG background includes light patches behind the item names because Finder renders those names in black over custom backgrounds. The builder combines the 1× and 2× artwork into one Retina-aware TIFF.

For local test images:

```sh
python3 -m venv target/release-tools
target/release-tools/bin/python -m pip install dmgbuild==1.6.7
PATH="$PWD/target/release-tools/bin:$PATH" ./scripts/build-macos-dmgs.sh --with-licensing
```

## Windows

Run **Build Windows release assets** manually with `tag: main` (or a full commit SHA) to verify a build without adding a release to the updater feed. The workflow runs licensed Rust tests, builds both installers and the updater ZIP, checks hashes, version and architecture, then silently installs and uninstalls each installer on the clean Windows runner. Each installed executable must match the updater payload, and its Start menu shortcut must be created and removed. Verified assets are retained for 14 days; installer failure logs are retained for 7 days.

Windows gets two installers with the same result, so users can pick either:

- **`Whisple-<version>-windows-x86_64-setup.exe`** (Inno Setup), translated into Whisple's interface languages.
- **`Whisple-<version>-windows-x86_64.msi`** (WiX), for tools that deploy MSI packages.

Both install per user into `%LOCALAPPDATA%\Programs\Whisple` without administrator rights and add a Start menu shortcut. The updater only runs from that folder, so it can replace `whisple.exe` without elevation. It downloads `Whisple-<version>-windows-x86_64.zip`, checks the GitHub asset digest, and confirms the executable's embedded `WhispleSemanticVersion` and processor before installing. `scripts/windows/apply-update.ps1` then waits for Whisple to quit, swaps the executable, keeps the previous one until the new one is in place, and relaunches it.

The builds are not code signed, so Windows SmartScreen warns on first launch until the download gains reputation; users choose **More info › Run anyway**. The GitHub digest is what ties an update to its release. To remove the warning, add Authenticode signing to `package-windows.ps1` before the installers are built.

whisper.cpp is built for an x86-64 baseline with AVX2, FMA and F16C rather than the runner's CPU, so the release runs on PCs from about 2013 onwards. Windows on Arm runs the x86_64 build under emulation.

`build.rs` embeds the icon from `assets/whisple.ico` and the version details into `whisple.exe`. Regenerate the icon from `assets/whisple-icon.png` with `scripts/windows/make-icon.ps1`.

To build the assets locally, install [Inno Setup 6](https://jrsoftware.org/isinfo.php) and the WiX v5 tool (`dotnet tool install --global wix --version 5.0.2`), then run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/windows/package-windows.ps1 -WithLicensing
```

The MSI does not remove the "Open at login" entry when uninstalled; Windows then lists a startup item for a missing program until the user removes it. The Inno Setup uninstaller removes it.
