#!/usr/bin/env bash
# Notarize and staple a signed macOS app or DMG.
set -euo pipefail

if [[ $# -ne 2 || ( "$1" != "app" && "$1" != "dmg" ) ]]; then
    echo "Usage: $0 app|dmg /path/to/Whisple.app|/path/to/Whisple.dmg" >&2
    exit 2
fi
kind="$1"
artifact="$2"
if [[ ! -e "$artifact" ]]; then
    echo "Missing notarization artifact: $artifact" >&2
    exit 1
fi
command -v jq >/dev/null || { echo "jq is required." >&2; exit 1; }
if [[ -n "${WHISPLE_NOTARY_KEY_PATH:-}" && -n "${WHISPLE_NOTARY_KEY_ID:-}" ]]; then
    notary_auth=(--key "$WHISPLE_NOTARY_KEY_PATH" --key-id "$WHISPLE_NOTARY_KEY_ID")
    if [[ -n "${WHISPLE_NOTARY_ISSUER_ID:-}" ]]; then
        notary_auth+=(--issuer "$WHISPLE_NOTARY_ISSUER_ID")
    fi
elif [[ -n "${WHISPLE_NOTARY_APPLE_ID:-}" && -n "${WHISPLE_NOTARY_APP_PASSWORD:-}" && -n "${WHISPLE_APPLE_TEAM_ID:-}" ]]; then
    notary_auth=(--apple-id "$WHISPLE_NOTARY_APPLE_ID" --password "$WHISPLE_NOTARY_APP_PASSWORD" --team-id "$WHISPLE_APPLE_TEAM_ID")
else
    echo "Set a notarization API key, or Apple ID, app-specific password, and team ID." >&2
    exit 2
fi

submission="$artifact"
if [[ "$kind" == "app" ]]; then
    if [[ ! -d "$artifact" ]]; then
        echo "Expected an app bundle: $artifact" >&2
        exit 2
    fi
    temporary_directory="$(mktemp -d "${TMPDIR:-/tmp}/whisple-notary.XXXXXX")"
    trap 'rm -rf "$temporary_directory"' EXIT
    temporary_archive="$temporary_directory/Whisple.zip"
    ditto -c -k --sequesterRsrc --keepParent "$artifact" "$temporary_archive"
    submission="$temporary_archive"
elif [[ ! -f "$artifact" ]]; then
    echo "Expected a disk image: $artifact" >&2
    exit 2
fi

result="$(xcrun notarytool submit "$submission" \
    "${notary_auth[@]}" \
    --wait --output-format json)"
if ! jq -e '.status == "Accepted"' <<< "$result" >/dev/null; then
    echo "Apple did not accept the notarization submission:" >&2
    jq '{id, status, message}' <<< "$result" >&2
    submission_id="$(jq -r '.id // empty' <<< "$result")"
    if [[ -n "$submission_id" ]]; then
        xcrun notarytool log "$submission_id" "${notary_auth[@]}" >&2 || true
    fi
    exit 1
fi

xcrun stapler staple "$artifact"
xcrun stapler validate "$artifact"
if [[ "$kind" == "app" ]]; then
    codesign --verify --deep --strict "$artifact"
    spctl --assess --type execute --verbose=2 "$artifact"
else
    codesign --verify --strict "$artifact"
    hdiutil verify -quiet "$artifact"
fi
