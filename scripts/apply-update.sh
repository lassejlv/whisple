#!/usr/bin/env bash
# Runs outside Whisple after the app quits. Never overwrite signed code in place.
set -euo pipefail

target="$1"
staged="$2"
old_pid="$3"
work_dir="$4"
parent="$(dirname "$target")"
replacement="$parent/.Whisple-update-$$.app"
backup="$parent/.Whisple-backup-$$.app"

restore_previous_app() {
    status=$?
    trap - ERR
    if [[ -d "$backup" ]]; then
        if [[ -d "$target" ]]; then mv "$target" "$replacement" || true; fi
        mv "$backup" "$target" || true
    fi
    if [[ -d "$target" ]]; then /usr/bin/open -a "$target" || true; fi
    exit "$status"
}
trap restore_previous_app ERR

for _ in {1..200}; do
    if ! kill -0 "$old_pid" 2>/dev/null; then break; fi
    sleep 0.1
done
if kill -0 "$old_pid" 2>/dev/null; then
    echo "Whisple did not quit; the existing app was left untouched." >&2
    exit 1
fi

/usr/bin/ditto "$staged" "$replacement"
/usr/bin/codesign --verify --deep --strict "$replacement"
mv "$target" "$backup"
mv "$replacement" "$target"
/usr/bin/open -a "$target"

trap - ERR
rm -rf "$backup" "$work_dir"
