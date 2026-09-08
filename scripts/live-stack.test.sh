#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Tests for live-stack.sh: it starts nothing when a file, a program or NW_GAME_DIR is missing.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly STACK_SCRIPT="$SCRIPT_DIR/live-stack.sh"
readonly EXIT_USAGE=2
readonly EXIT_MISSING=3

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

failures=0

expect() {
  local name="$1" expected_exit="$2" expected_text="$3"
  shift 3
  local actual=0 output
  output="$("$@" 2>&1)" || actual=$?
  if [[ "$actual" -eq "$expected_exit" && "$output" == *"$expected_text"* ]]; then
    echo "PASS: $name"
  else
    echo "FAIL: $name (exit $actual, output: $output)"
    failures=$((failures + 1))
  fi
}

expect "an empty folder names the missing env file" "$EXIT_MISSING" "local-data/m3-test.env" \
  env LIVE_STACK_ROOT="$work_dir" NW_GAME_DIR="$work_dir" "$STACK_SCRIPT"

expect "an empty folder names a missing program" "$EXIT_MISSING" "target/release/sim" \
  env LIVE_STACK_ROOT="$work_dir" NW_GAME_DIR="$work_dir" "$STACK_SCRIPT"

expect "no game folder is named" "$EXIT_MISSING" "NW_GAME_DIR" \
  env -u NW_GAME_DIR LIVE_STACK_ROOT="$work_dir" "$STACK_SCRIPT"

expect "nine sims are refused" "$EXIT_USAGE" "sims must be 1 to 8" \
  env LIVE_STACK_ROOT="$work_dir" NW_GAME_DIR="$work_dir" "$STACK_SCRIPT" 9

expect "a sim count that is not a number is refused" "$EXIT_USAGE" "not 'five'" \
  env LIVE_STACK_ROOT="$work_dir" NW_GAME_DIR="$work_dir" "$STACK_SCRIPT" five

expect "a snapshot interval that is not a number is refused" "$EXIT_USAGE" "not 'often'" \
  env LIVE_STACK_ROOT="$work_dir" NW_GAME_DIR="$work_dir" LIVE_STACK_SNAPSHOT_MS=often \
  "$STACK_SCRIPT"

if [[ -d "$work_dir/local-data/logs" ]]; then
  echo "FAIL: a refused start wrote logs"
  failures=$((failures + 1))
else
  echo "PASS: a refused start writes nothing"
fi

if [[ "$failures" -gt 0 ]]; then
  echo "$failures test(s) failed"
  exit 1
fi
echo "all live-stack tests passed"
