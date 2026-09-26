# Releases and updates

Whisple publishes macOS builds for Apple silicon and Intel, and Windows builds for x86_64. Every build checks the public GitHub releases feed on startup and every six hours. It follows the newest published release with a complete update archive for its platform and architecture, including prereleases. Once an update is downloaded and verified, the tray menu offers **Install Whisple v…**. Installing waits for the app to exit, replaces it, and relaunches it.

Cargo builds and local packages omit licensing by default. The release workflows
pass `--features licensing` for tests and build with licensing, so published
binaries retain the paid trial and license flow on both platforms.

## Publish a release

1. Set the `Cargo.toml` version and update `Cargo.lock`. Use a semantic prerelease version such as `0.2.0-rc.1` for prereleases.
2. Merge the source and workflows into the default branch, then push the tag `v<version>` at that commit.
3. Publish a GitHub release for that tag. **Build macOS release assets** and **Build Windows release assets** both run on `published`, for regular releases and prereleases.
4. Wait for all jobs to finish and confirm the release has these assets:
   - For each macOS architecture: a DMG, ZIP, and `.sha256`.
   - For Windows: `-setup.exe`, `.msi`, `.zip`, and `.sha256`.

To rebuild an existing release, run either workflow manually with its tag. For macOS, also enable **publish**. The upload step replaces assets with the same names. Tags must contain the release scripts used by the workflow. A stable version following `0.2.0-rc.1` should use version and tag `0.2.0`; the updater compares semantic versions and offers that stable build to prerelease users.

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

Use a Team API key with Developer access. `setup-macos-signing.sh` validates the certificate's team and authenticates with Apple, imports credentials into a temporary keychain, and removes the decoded source files. The workflow deletes the temporary keychain after success or failure. Credentials and recovery copies must remain outside the repository; never add them to workflow artifacts.

### Verify signing without publishing a release

Run **Build macOS release assets** manually with `tag: main` (or a commit/ref containing the signing scripts) and **publish disabled**. Both architectures run the licensed Rust tests, build, sign, notarize, and pass Gatekeeper assessment. Download the DMG, updater ZIP, and checksums from the run's artifacts, retained for 14 days. This does not add a release to the updater feed.

Publishing a GitHub release triggers the same verified flow and attaches the assets automatically. Missing credentials or rejected notarization fail the job; release CI never falls back to ad hoc signing. Local packaging still uses ad hoc signing unless `WHISPLE_CODESIGN_IDENTITY` is provided.

The app icon and Finder DMG layout come from the [Release page in Paper](https://app.paper.design/file/01M391FYN8XXTW9JHFATBK4Q06/p-8-0). The DMG background includes light patches behind the item names because Finder renders those names in black over custom backgrounds. The builder combines the 1× and 2× artwork into one Retina-aware TIFF.

For local test images:

```sh
python3 -m venv target/release-tools
target/release-tools/bin/python -m pip install dmgbuild==1.6.7
PATH="$PWD/target/release-tools/bin:$PATH" ./scripts/build-macos-dmgs.sh --with-licensing
```

## Windows

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
