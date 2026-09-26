#!/usr/bin/env bash
# Notarize and staple an already signed app, or sign/notarize/staple a DMG.
set -euo pipefail

[[ $# == 1 ]] || { echo "Usage: $0 path/to/Whisple.app|image.dmg" >&2; exit 2; }
artifact="$1"
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID is required}"
: "${WHISPLE_CODESIGN_IDENTITY:?WHISPLE_CODESIGN_IDENTITY is required}"
: "${WHISPLE_SIGNING_KEYCHAIN:?WHISPLE_SIGNING_KEYCHAIN is required}"
: "${WHISPLE_NOTARY_PROFILE:?WHISPLE_NOTARY_PROFILE is required}"
[[ -e "$artifact" ]] || { echo "Missing artifact: $artifact" >&2; exit 1; }

work_dir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/whisple-notary.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
auth=(--keychain-profile "$WHISPLE_NOTARY_PROFILE" --keychain "$WHISPLE_SIGNING_KEYCHAIN")
case "$artifact" in
    *.app)
        codesign --verify --deep --strict "$artifact"
        details="$(codesign -dvvv "$artifact" 2>&1)"
        printf '%s\n' "$details" | grep -q 'flags=.*runtime' || { echo 'Hardened runtime is required.' >&2; exit 1; }
        submission="$work_dir/Whisple.zip"
        ditto -c -k --sequesterRsrc --keepParent "$artifact" "$submission"
        ;;
    *.dmg)
        codesign --force --sign "$WHISPLE_CODESIGN_IDENTITY" \
            --keychain "$WHISPLE_SIGNING_KEYCHAIN" --timestamp "$artifact"
        codesign --verify --strict "$artifact"
        details="$(codesign -dvvv "$artifact" 2>&1)"
        submission="$artifact"
        ;;
    *) echo 'Only .app and .dmg artifacts are supported.' >&2; exit 2 ;;
esac
printf '%s\n' "$details" | grep -Fxq "TeamIdentifier=$APPLE_TEAM_ID"
printf '%s\n' "$details" | grep -q '^Authority=Developer ID Application:'
printf '%s\n' "$details" | grep -q '^Timestamp='

result="$work_dir/result.json"
submit_status=0
xcrun notarytool submit "$submission" "${auth[@]}" --wait --timeout 30m \
    --output-format json > "$result" || submit_status=$?
cat "$result"
status="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("status", ""))' "$result")"
if [[ "$submit_status" != 0 || "$status" != Accepted ]]; then
    submission_id="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("id", ""))' "$result")"
    if [[ -n "$submission_id" ]]; then
        xcrun notarytool log "$submission_id" "${auth[@]}" || true
    fi
    echo "Apple notarization did not finish with Accepted status: $status" >&2
    exit 1
fi
xcrun stapler staple "$artifact"
xcrun stapler validate "$artifact"
case "$artifact" in
    *.app) spctl --assess --type execute --verbose=2 "$artifact" ;;
    *.dmg) spctl --assess --type open --context context:primary-signature --verbose=2 "$artifact" ;;
esac
