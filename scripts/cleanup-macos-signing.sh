#!/usr/bin/env bash
set -euo pipefail

[[ -n "${WHISPLE_SIGNING_DIR:-}" ]] || exit 0
: "${WHISPLE_SIGNING_KEYCHAIN:?WHISPLE_SIGNING_KEYCHAIN is required}"
[[ "$(basename "$WHISPLE_SIGNING_DIR")" == whisple-signing.* && \
   "$WHISPLE_SIGNING_KEYCHAIN" == "$WHISPLE_SIGNING_DIR/signing.keychain-db" ]] || {
    echo 'Refusing to clean an unexpected signing directory.' >&2
    exit 1
}

search_list="$WHISPLE_SIGNING_DIR/keychain-search-list.json"
if [[ -f "$search_list" ]]; then
    python3 - "$search_list" <<'PY'
import json
import pathlib
import subprocess
import sys

original = json.loads(pathlib.Path(sys.argv[1]).read_text())
subprocess.run(["security", "list-keychains", "-d", "user", "-s", *original], check=True)
PY
fi
if [[ -f "$WHISPLE_SIGNING_KEYCHAIN" ]]; then
    security delete-keychain "$WHISPLE_SIGNING_KEYCHAIN"
fi
rm -rf "$WHISPLE_SIGNING_DIR"
