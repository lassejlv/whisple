#!/usr/bin/env bash
# Import release credentials into an isolated, temporary CI keychain.
set +x
set -euo pipefail
umask 077

: "${GITHUB_ENV:?GITHUB_ENV is required}"
: "${RUNNER_TEMP:?RUNNER_TEMP is required}"
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID is required}"
: "${APPLE_CERTIFICATE_P12_BASE64:?APPLE_CERTIFICATE_P12_BASE64 is required}"
: "${APPLE_CERTIFICATE_PASSWORD:?APPLE_CERTIFICATE_PASSWORD is required}"
: "${APPLE_NOTARY_KEY_P8_BASE64:?APPLE_NOTARY_KEY_P8_BASE64 is required}"
: "${APPLE_NOTARY_KEY_ID:?APPLE_NOTARY_KEY_ID is required}"
: "${APPLE_NOTARY_ISSUER_ID:?APPLE_NOTARY_ISSUER_ID is required}"
[[ "$APPLE_TEAM_ID" =~ ^[A-Z0-9]{10}$ ]] || { echo 'Invalid Apple team ID.' >&2; exit 1; }

signing_dir="$(mktemp -d "$RUNNER_TEMP/whisple-signing.XXXXXX")"
keychain="$signing_dir/signing.keychain-db"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cleanup_on_failure() {
    WHISPLE_SIGNING_DIR="$signing_dir" WHISPLE_SIGNING_KEYCHAIN="$keychain" \
        "$script_dir/cleanup-macos-signing.sh"
}
trap cleanup_on_failure EXIT

printf '%s' "$APPLE_CERTIFICATE_P12_BASE64" | base64 --decode > "$signing_dir/certificate.p12"
printf '%s' "$APPLE_NOTARY_KEY_P8_BASE64" | base64 --decode > "$signing_dir/notary.p8"
openssl pkey -in "$signing_dir/notary.p8" -check -noout >/dev/null
keychain_password="$(openssl rand -hex 32)"
# Save the exact original search list before creating or registering a keychain.
python3 - "$signing_dir/keychain-search-list.json" <<'PY'
import json
import pathlib
import shlex
import subprocess
import sys

existing = shlex.split(subprocess.check_output(
    ["security", "list-keychains", "-d", "user"], text=True))
pathlib.Path(sys.argv[1]).write_text(json.dumps(existing))
PY
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
# codesign consults the user search list even with an explicit --keychain.
# Keep the existing keychains available for certificate chain construction.
python3 - "$keychain" <<'PY'
import shlex
import subprocess
import sys

keychain = sys.argv[1]
existing = shlex.split(subprocess.check_output(
    ["security", "list-keychains", "-d", "user"], text=True))
subprocess.run(["security", "list-keychains", "-d", "user", "-s", keychain,
                *(path for path in existing if path != keychain)], check=True)
PY
security import "$signing_dir/certificate.p12" -k "$keychain" \
    -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s \
    -k "$keychain_password" "$keychain" >/dev/null

identities="$(security find-identity -v -p codesigning "$keychain")"
identity="$(printf '%s\n' "$identities" | awk -v team="$APPLE_TEAM_ID" \
    '/Developer ID Application:/ && index($0, "(" team ")\"") { print $2 }')"
if [[ ! "$identity" =~ ^[A-F0-9]{40}$ ]]; then
    echo 'Expected exactly one valid Developer ID Application identity for APPLE_TEAM_ID.' >&2
    exit 1
fi

# Exercise private-key access before spending time compiling the application.
cp /usr/bin/true "$signing_dir/signing-probe"
codesign --force --sign "$identity" --keychain "$keychain" --options runtime \
    --timestamp "$signing_dir/signing-probe"
codesign --verify --strict "$signing_dir/signing-probe"
rm -f "$signing_dir/signing-probe"

# store-credentials validates the key against Apple before any build starts.
xcrun notarytool store-credentials whisple-ci --keychain "$keychain" \
    --key "$signing_dir/notary.p8" --key-id "$APPLE_NOTARY_KEY_ID" \
    --issuer "$APPLE_NOTARY_ISSUER_ID" >/dev/null
rm -f "$signing_dir/certificate.p12" "$signing_dir/notary.p8"
{
    printf 'WHISPLE_CODESIGN_IDENTITY=%s\n' "$identity"
    printf 'WHISPLE_SIGNING_KEYCHAIN=%s\n' "$keychain"
    printf 'WHISPLE_SIGNING_DIR=%s\n' "$signing_dir"
    printf 'WHISPLE_NOTARY_PROFILE=whisple-ci\n'
} >> "$GITHUB_ENV"
trap - EXIT
echo "Developer ID and notarization credentials validated for team $APPLE_TEAM_ID."
