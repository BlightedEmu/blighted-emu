#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Tests for launcher-release-manifest.py, with stand-in update files and signatures.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly MANIFEST_SCRIPT="$SCRIPT_DIR/launcher-release-manifest.py"
readonly VERSION="0.1.0"
readonly BASE_URL="https://github.com/BlightedEmu/blighted-emu/releases/download/launcher-v$VERSION"
readonly WINDOWS_FILE="BlightedLauncher_${VERSION}_x64-setup.exe"
readonly LINUX_FILE="BlightedLauncher_${VERSION}_amd64.AppImage"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

printf 'setup' > "$work/$WINDOWS_FILE"
printf 'windows-signature\n' > "$work/$WINDOWS_FILE.sig"
printf 'appimage' > "$work/$LINUX_FILE"
printf 'linux-signature\n' > "$work/$LINUX_FILE.sig"

run_manifest() {
  python3 -I "$MANIFEST_SCRIPT" --version "$VERSION" --notes "- First release" \
    --pub-date "2026-11-12T12:00:00Z" --base-url "$BASE_URL" \
    --windows "$work/$WINDOWS_FILE" --linux "$work/$LINUX_FILE" --out "$work/latest.json"
}

run_manifest
python3 -I - "$work/latest.json" "$BASE_URL" "$WINDOWS_FILE" "$LINUX_FILE" "$VERSION" <<'PY'
import json
import sys

path, base, windows, linux, version = sys.argv[1:]
data = json.load(open(path, encoding="utf-8"))
assert data["version"] == version, data
assert data["notes"] == "- First release", data
assert data["platforms"]["windows-x86_64"] == {"signature": "windows-signature", "url": f"{base}/{windows}"}, data
assert data["platforms"]["linux-x86_64"] == {"signature": "linux-signature", "url": f"{base}/{linux}"}, data
PY
echo "ok: both systems have their address and signature"

rm "$work/$LINUX_FILE.sig"
if run_manifest 2> "$work/error.txt"; then
  echo "FAIL: a missing signature was accepted" >&2
  exit 1
fi
grep -q "signature missing" "$work/error.txt"
echo "ok: a missing signature stops the script"
