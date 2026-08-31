#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Tests for target-size.sh.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SIZE_SCRIPT="$SCRIPT_DIR/target-size.sh"
readonly EXIT_OK=0
readonly EXIT_OVER_LIMIT=2
readonly ZERO_LIMIT_GIB=0

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

failures=0

expect_exit() {
  local name="$1" expected="$2"
  shift 2
  local actual=0
  "$@" >/dev/null 2>&1 || actual=$?
  if [[ "$actual" -eq "$expected" ]]; then
    echo "PASS: $name"
  else
    echo "FAIL: $name (expected exit $expected, got $actual)"
    failures=$((failures + 1))
  fi
}

expect_exit "missing target folder is size zero" "$EXIT_OK" \
  "$SIZE_SCRIPT" "$work_dir/does-not-exist"

mkdir -p "$work_dir/small"
echo "x" > "$work_dir/small/file"
expect_exit "small folder is under the default limit" "$EXIT_OK" \
  "$SIZE_SCRIPT" "$work_dir/small"

expect_exit "folder above the limit fails" "$EXIT_OVER_LIMIT" \
  env TARGET_SIZE_LIMIT_GIB="$ZERO_LIMIT_GIB" "$SIZE_SCRIPT" "$work_dir/small"

if [[ "$failures" -gt 0 ]]; then
  echo "$failures test(s) failed"
  exit 1
fi
echo "all target-size tests passed"
