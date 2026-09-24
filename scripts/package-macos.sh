#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile="release"
bundle_name="Whisple"
bundle_id="app.whisp"
target_triple=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug)
            profile="debug"
            bundle_name="Whisple-Debug"
            bundle_id="app.whisp.debug"
            shift
            ;;
        --target)
            if [[ $# -lt 2 ]]; then
                echo "--target requires a Rust target triple." >&2
                exit 2
            fi
            target_triple="$2"
            shift 2
            ;;
        *)
            echo "Usage: $0 [--debug] [--target aarch64-apple-darwin|x86_64-apple-darwin]" >&2
            exit 2
            ;;
    esac
done

case "$target_triple" in
    ""|aarch64-apple-darwin|x86_64-apple-darwin) ;;
    *) echo "Unsupported macOS target: $target_triple" >&2; exit 2 ;;
esac

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "macOS is required to package Whisple." >&2
    exit 1
fi
command -v iconutil >/dev/null || { echo "iconutil is required." >&2; exit 1; }
command -v sips >/dev/null || { echo "sips is required." >&2; exit 1; }

cd "$project_dir"
build_args=(build)
if [[ "$profile" == "release" ]]; then build_args+=(--release); fi
if [[ -n "$target_triple" ]]; then build_args+=(--target "$target_triple"); fi
cargo "${build_args[@]}"

if [[ -n "$target_triple" ]]; then
    binary="$project_dir/target/$target_triple/$profile/whisple"
    bundle="$project_dir/target/macos/$target_triple/$bundle_name.app"
else
    binary="$project_dir/target/$profile/whisple"
    bundle="$project_dir/target/$bundle_name.app"
fi
version="$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)"
bundle_version="${version%%[-+]*}"
build_number="${WHISPLE_BUILD_NUMBER:-${GITHUB_RUN_NUMBER:-1}}"
if [[ ! "$build_number" =~ ^[0-9]+$ ]]; then
    echo "WHISPLE_BUILD_NUMBER must contain only digits." >&2
    exit 2
fi
iconset="$project_dir/target/Whisple.iconset"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" "$iconset"
cp "$binary" "$bundle/Contents/MacOS/whisple"

for size in 16 32 128 256 512; do
    sips -z "$size" "$size" assets/whisple-icon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double_size=$((size * 2))
    sips -z "$double_size" "$double_size" assets/whisple-icon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/Whisple.icns"
rm -rf "$iconset"

cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>whisple</string>
    <key>CFBundleIdentifier</key><string>$bundle_id</string>
    <key>CFBundleName</key><string>Whisple</string>
    <key>CFBundleDisplayName</key><string>Whisple</string>
    <key>CFBundleIconFile</key><string>Whisple.icns</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$bundle_version</string>
    <key>CFBundleVersion</key><string>$build_number</string>
    <key>WhispleSemanticVersion</key><string>$version</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSMicrophoneUsageDescription</key><string>Whisple uses the microphone for dictation. Audio goes to OpenAI or Groq only when you select a cloud model.</string>
</dict>
</plist>
PLIST

plutil -lint "$bundle/Contents/Info.plist"
signing_identity="${WHISPLE_CODESIGN_IDENTITY:--}"
sign_args=(--force --sign "$signing_identity" --entitlements assets/whisple.entitlements)
if [[ "$signing_identity" != "-" ]]; then
    sign_args+=(--options runtime --timestamp)
fi
codesign "${sign_args[@]}" "$bundle"
codesign --verify --deep --strict "$bundle"
echo "$bundle"
