# macOS releases and updates

Whisple publishes separate Apple silicon and Intel DMGs and ZIP update archives. The app checks the public GitHub releases feed on startup and every six hours. It follows the newest published release with a complete asset for its architecture, including prereleases. Once an update is downloaded and verified, the menu bar offers **Install Whisple v…**. Installing waits for the app to exit, replaces the app bundle, and relaunches it.

Cargo builds and local packages omit licensing by default. The release workflow
passes `--features licensing` for tests and `--with-licensing` to the package
script so published binaries retain the paid trial and license flow.

## Publish a release

1. Set the `Cargo.toml` version and update `Cargo.lock`. Use a semantic prerelease version such as `0.2.0-rc.1` for prereleases.
2. Merge the source and workflow into the default branch, then push the tag `v<version>` at that commit.
3. Publish a GitHub release for that tag. **Build macOS release assets** runs on `published` for both regular releases and prereleases.
4. Wait for both architecture jobs to finish and confirm the release has a DMG, ZIP, and `.sha256` for each architecture.

To rebuild an existing release, run the workflow manually with its tag. The upload step replaces assets with the same names. A stable version following `0.2.0-rc.1` should use version and tag `0.2.0`; the updater compares semantic versions and offers that stable build to prerelease users.

The current workflow uses ad hoc code signing and does not notarize the app or DMG. It needs no Apple credentials. macOS may require users to explicitly allow the downloaded app to open. To distribute with normal Gatekeeper trust later, add Developer ID signing and Apple notarization to the workflow. The updater checks the public release asset digest, bundle signature, app identity, and architecture before installing.

The app icon and Finder DMG layout come from the [Release page in Paper](https://app.paper.design/file/01M391FYN8XXTW9JHFATBK4Q06/p-8-0). The DMG background includes light patches behind the item names because Finder renders those names in black over custom backgrounds. The builder combines the 1× and 2× artwork into one Retina-aware TIFF.

For local test images:

```sh
python3 -m venv target/release-tools
target/release-tools/bin/python -m pip install dmgbuild==1.6.7
PATH="$PWD/target/release-tools/bin:$PATH" ./scripts/build-macos-dmgs.sh --with-licensing
```
