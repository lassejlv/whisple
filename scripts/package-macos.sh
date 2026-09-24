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
command -v rsvg-convert >/dev/null || { echo "rsvg-convert is required." >&2; exit 1; }
command -v iconutil >/dev/null || { echo "iconutil is required." >&2; exit 1; }

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
iconset="$project_dir/target/Whisple.iconset"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" "$iconset"
cp "$binary" "$bundle/Contents/MacOS/whisple"

for size in 16 32 128 256 512; do
    rsvg-convert -w "$size" -h "$size" assets/whisple-icon.svg -o "$iconset/icon_${size}x${size}.png"
    double_size=$((size * 2))
    rsvg-convert -w "$double_size" -h "$double_size" assets/whisple-icon.svg -o "$iconset/icon_${size}x${size}@2x.png"
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
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSMicrophoneUsageDescription</key><string>Whisple uses the microphone for local dictation.</string>
</dict>
</plist>
PLIST

plutil -lint "$bundle/Contents/Info.plist"
echo "$bundle"
