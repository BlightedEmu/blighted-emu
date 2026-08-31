#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Tests for no-game-data.sh. Each case uses a new temporary git repository.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly GUARD="$SCRIPT_DIR/no-game-data.sh"
readonly EXIT_OK=0
readonly EXIT_FOUND=1

failures=0

# Runs the guard in a fresh repository that tracks the given file (path, content).
check_case() {
  local name="$1" expected="$2" path="$3" content="$4"
  local repo
  repo="$(mktemp -d)"
  git -C "$repo" init -q
  mkdir -p "$(dirname "$repo/$path")"
  printf '%s' "$content" > "$repo/$path"
  git -C "$repo" add -- "$path"
  local actual=0
  "$GUARD" "$repo" >/dev/null 2>&1 || actual=$?
  rm -rf "$repo"
  if [[ "$actual" -eq "$expected" ]]; then
    echo "PASS: $name"
  else
    echo "FAIL: $name (expected exit $expected, got $actual)"
    failures=$((failures + 1))
  fi
}

check_case "plain source file is allowed" "$EXIT_OK" "src/main.rs" "fn main() {}"
check_case "file under local-data is refused" "$EXIT_FOUND" "local-data/notes.txt" "x"
check_case "pak file is refused" "$EXIT_FOUND" "assets/x.pak" "x"
check_case "datasheet file is refused" "$EXIT_FOUND" "data/x.datasheet" "x"
check_case "localization file is refused" "$EXIT_FOUND" "data/x.loc.xml" "<resources/>"
check_case "content bundle is refused" "$EXIT_FOUND" "out/any.bin" "NWCB....body"

if [[ "$failures" -gt 0 ]]; then
  echo "$failures test(s) failed"
  exit 1
fi
echo "all no-game-data tests passed"
