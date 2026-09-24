#!/usr/bin/env bash
# Build Apple silicon and Intel DMGs in target/dist/ (or one with --arch).
# Requires Rust, Xcode command-line tools, and dmgbuild 1.6.7.
# Bundles use ad hoc signing by default. Set WHISPLE_CODESIGN_IDENTITY for
# Developer ID signing; notarize and staple the finished DMG before upload.
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"

selected_arch="both"
use_packaged_app=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --arch)
            [[ $# -ge 2 ]] || { echo "--arch needs arm64 or x86_64." >&2; exit 2; }
            selected_arch="$2"
            shift 2
            ;;
        --use-packaged-app)
            use_packaged_app=true
            shift
            ;;
        *)
            echo "Usage: $0 [--arch arm64|x86_64] [--use-packaged-app]" >&2
            exit 2
            ;;
    esac
done
case "$selected_arch" in
    both|arm64|x86_64) ;;
    *) echo "Unsupported architecture: $selected_arch" >&2; exit 2 ;;
esac
if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "macOS is required to build DMGs." >&2
    exit 1
fi
command -v hdiutil >/dev/null || { echo "hdiutil is required." >&2; exit 1; }
command -v ditto >/dev/null || { echo "ditto is required." >&2; exit 1; }
command -v lipo >/dev/null || { echo "lipo is required." >&2; exit 1; }
command -v rustup >/dev/null || { echo "rustup is required." >&2; exit 1; }
command -v tiffutil >/dev/null || { echo "tiffutil is required." >&2; exit 1; }
command -v dmgbuild >/dev/null || { echo "dmgbuild 1.6.7 is required (python3 -m pip install dmgbuild==1.6.7)." >&2; exit 1; }

if [[ "$selected_arch" == "both" ]]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
elif [[ "$selected_arch" == "arm64" ]]; then
    rustup target add aarch64-apple-darwin
else
    rustup target add x86_64-apple-darwin
fi

version="$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)"
host_triple="$(rustc -vV | awk '/^host: / { print $2 }')"
dist_dir="$project_dir/target/dist"
mkdir -p "$dist_dir"
background_tiff="$project_dir/target/whisple-dmg-background.tiff"
tiffutil -cathidpicheck \
    "$project_dir/assets/whisple-dmg-background.png" \
    "$project_dir/assets/whisple-dmg-background@2x.png" \
    -out "$background_tiff" >/dev/null
for target_triple in aarch64-apple-darwin x86_64-apple-darwin; do
    case "$target_triple" in
        aarch64-apple-darwin) arch="arm64" ;;
        x86_64-apple-darwin) arch="x86_64" ;;
    esac
    if [[ "$selected_arch" != "both" && "$selected_arch" != "$arch" ]]; then
        continue
    fi

    if [[ "$use_packaged_app" == true && "$target_triple" == "$host_triple" ]]; then
        app_bundle="$project_dir/target/Whisple.app"
    elif [[ "$use_packaged_app" == true ]]; then
        app_bundle="$project_dir/target/macos/$target_triple/Whisple.app"
    elif [[ "$target_triple" == "$host_triple" ]]; then
        app_bundle="$("$project_dir/scripts/package-macos.sh" | tail -n 1)"
    else
        app_bundle="$("$project_dir/scripts/package-macos.sh" --target "$target_triple" | tail -n 1)"
    fi
    codesign --verify --deep --strict "$app_bundle"
    binary_arch="$(lipo -archs "$app_bundle/Contents/MacOS/whisple")"
    if [[ "$binary_arch" != "$arch" ]]; then
        echo "Expected $arch binary in $app_bundle; got $binary_arch." >&2
        exit 1
    fi

    dmg="$dist_dir/Whisple-$version-macos-$arch.dmg"
    rm -f "$dmg"
    dmgbuild -s "$project_dir/scripts/dmg-settings.py" \
        -D "application=$app_bundle" \
        -D "background=$background_tiff" \
        "Whisple" "$dmg"
    hdiutil verify -quiet "$dmg"
    if [[ "${WHISPLE_CODESIGN_IDENTITY:--}" != "-" ]]; then
        codesign --sign "$WHISPLE_CODESIGN_IDENTITY" --timestamp "$dmg"
        codesign --verify --strict "$dmg"
    fi
    zip="$dist_dir/Whisple-$version-macos-$arch.zip"
    rm -f "$zip"
    ditto -c -k --sequesterRsrc --keepParent "$app_bundle" "$zip"
    (
        cd "$dist_dir"
        shasum -a 256 "$(basename "$dmg")" "$(basename "$zip")"
    ) > "$dist_dir/Whisple-$version-macos-$arch.sha256"
    echo "$dmg"
    echo "$zip"
done
