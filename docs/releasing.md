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

To rebuild an existing release, run either workflow manually with its tag. The upload step replaces assets with the same names. A stable version following `0.2.0-rc.1` should use version and tag `0.2.0`; the updater compares semantic versions and offers that stable build to prerelease users.

## macOS

The workflow uses ad hoc code signing and does not notarize the app or DMG. It needs no Apple credentials. macOS may require users to explicitly allow the downloaded app to open. To distribute with normal Gatekeeper trust later, add Developer ID signing and Apple notarization to the workflow. The updater checks the public release asset digest, bundle signature, app identity, and architecture before installing.

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
