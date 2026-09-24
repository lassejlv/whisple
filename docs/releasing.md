# macOS releases and updates

Whisple publishes separate Apple silicon and Intel DMGs and ZIP update archives. The app checks the public GitHub releases feed on startup and every six hours. It follows the newest published release with a complete asset for its architecture, including prereleases. Once an update is downloaded and verified, the menu bar offers **Install Whisple v…**. Installing waits for the app to exit, replaces the app bundle, and relaunches it.

## Publish a release

1. Set the `Cargo.toml` version and update `Cargo.lock`. Use a semantic prerelease version such as `0.2.0-rc.1` for prereleases.
2. Merge the source and workflow into the default branch, then push the tag `v<version>` at that commit.
3. Publish a GitHub release for that tag. **Build macOS release assets** runs on `published` for both regular releases and prereleases.
4. Wait for both architecture jobs to finish and confirm the release has a DMG, ZIP, and `.sha256` for each architecture.

To rebuild an existing release, run the workflow manually with its tag. The upload step replaces assets with the same names. A stable version following `0.2.0-rc.1` should use version and tag `0.2.0`; the updater compares semantic versions and offers that stable build to prerelease users.

The current workflow uses ad hoc code signing and does not notarize the app or DMG. It needs no Apple credentials. macOS may require users to explicitly allow the downloaded app to open. To distribute with normal Gatekeeper trust later, add Developer ID signing and Apple notarization to the workflow. The updater checks the public release asset digest, bundle signature, app identity, and architecture before installing.

For local test images, run `./scripts/build-macos-dmgs.sh`.
