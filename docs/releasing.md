# macOS releases and updates

Whisple publishes separate Apple silicon and Intel DMGs and ZIP update archives. The app checks the public GitHub releases feed on startup and every six hours. It follows the newest published release with a complete asset for its architecture, including prereleases. Once an update is downloaded and verified, the menu bar offers **Install Whisple v…**. Installing waits for the app to exit, replaces the app bundle, and relaunches it.

## Set up Developer ID signing

Whisple is distributed outside the Mac App Store. Create a **Developer ID Application** certificate for the Whisple developer team (`V62Z6SMR4K`) in [Apple Developer Certificates](https://developer.apple.com/account/resources/certificates/list), download it, and install it in the login keychain. The certificate must have its private key in Keychain Access. An Apple Development certificate cannot sign a public release. Check the installed identities with `security find-identity -v -p codesigning`.

Export the Developer ID certificate **with its private key** as a password-protected `.p12` file. For notarization, use an Apple Account app-specific password or an App Store Connect API key. A team API key needs its Issuer ID; an individual key does not. Keep private files outside the repository. Apple documents [Developer ID certificates](https://developer.apple.com/help/account/certificates/create-developer-id-certificates) and [App Store Connect API keys](https://developer.apple.com/help/app-store-connect/get-started/app-store-connect-api).

Set these GitHub Actions secrets for `lassejlv/whisple`:

| Secret | Value |
| --- | --- |
| `WHISPLE_DEVELOPER_ID_P12_BASE64` | Base64 of the exported `.p12` file (`base64 < certificate.p12 \| tr -d '\n'`) |
| `WHISPLE_DEVELOPER_ID_PASSWORD` | Password used when exporting the `.p12` file |
| `WHISPLE_NOTARY_APPLE_ID` | Apple Account email used for notarization (Apple ID method) |
| `WHISPLE_NOTARY_APP_PASSWORD` | App-specific password for that account (Apple ID method) |
| `WHISPLE_NOTARY_KEY_P8_BASE64` | Base64 of the downloaded `.p8` file (`base64 < AuthKey_XXXX.p8 \| tr -d '\n'`) |
| `WHISPLE_NOTARY_KEY_ID` | App Store Connect API Key ID |
| `WHISPLE_NOTARY_ISSUER_ID` | Issuer ID for a team API key; omit for an individual API key |

Set either the two Apple ID notarization secrets or the API key secrets. The Apple ID method avoids waiting for App Store Connect API access approval.

The release workflow imports the certificate into a temporary runner keychain, requires a Developer ID identity for the Whisple team, signs the app with hardened runtime and a secure timestamp, notarizes and staples the app, then signs, notarizes, and staples the DMG. It creates the updater ZIP from the stapled app and writes checksums after stapling. The keychain and private key files stay on the ephemeral GitHub runner.

## Publish a release

1. Set the `Cargo.toml` version and update `Cargo.lock`. Use a semantic prerelease version such as `0.2.0-rc.1` for prereleases.
2. Merge the source and workflow into the default branch, then push the tag `v<version>` at that commit.
3. Publish a GitHub release for that tag. **Build macOS release assets** runs on `published` for both regular releases and prereleases.
4. Wait for both architecture jobs to finish and confirm the release has a DMG, ZIP, and `.sha256` for each architecture.

To rebuild an existing release, run the workflow manually with its tag. The upload step replaces assets with the same names. A stable version following `0.2.0-rc.1` should use version and tag `0.2.0`; the updater compares semantic versions and offers that stable build to prerelease users.

Release jobs fail if signing credentials are missing. The updater checks the public release asset digest, bundle signature, app identity, architecture, and, for an already Developer ID signed installation, the Apple team before installing. Publish a new tag containing this workflow; rebuilding an older tag checks out its older release scripts.

The app icon and Finder DMG layout come from the [Release page in Paper](https://app.paper.design/file/01M391FYN8XXTW9JHFATBK4Q06/p-8-0). The DMG background includes light patches behind the item names because Finder renders those names in black over custom backgrounds. The builder combines the 1× and 2× artwork into one Retina-aware TIFF.

For local test images:

```sh
python3 -m venv target/release-tools
target/release-tools/bin/python -m pip install dmgbuild==1.6.7
PATH="$PWD/target/release-tools/bin:$PATH" ./scripts/build-macos-dmgs.sh
```

For a signed local build, set `WHISPLE_CODESIGN_IDENTITY` to the full Developer ID Application identity or its 40-character SHA-1 fingerprint. Run `./scripts/package-macos.sh`, then `./scripts/notarize-macos.sh app target/Whisple.app` with either the `WHISPLE_NOTARY_APPLE_ID` / `WHISPLE_NOTARY_APP_PASSWORD` / `WHISPLE_APPLE_TEAM_ID` environment variables or the API key variables `WHISPLE_NOTARY_KEY_PATH` / `WHISPLE_NOTARY_KEY_ID` (and `WHISPLE_NOTARY_ISSUER_ID` for a team key). Build final images with `./scripts/build-macos-dmgs.sh --use-packaged-app`, then notarize each DMG with `./scripts/notarize-macos.sh dmg path/to/image.dmg` and regenerate its `.sha256` file. Verify the app with `spctl --assess --type execute --verbose=2 target/Whisple.app` and each DMG with `xcrun stapler validate path/to/image.dmg`.
