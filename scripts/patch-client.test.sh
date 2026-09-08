#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Tests for patch-client.py, using a small synthetic executable (no real game files).
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly PATCH_SCRIPT="$SCRIPT_DIR/patch-client.py"
# The verify-setter signature the tool searches for, and the six bytes it replaces.
readonly SIGNATURE_HEX="8991580100004c898188010000c3"
readonly VERIFY_WRITE_HEX="899158010000"
readonly NOPS_HEX="909090909090"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
failures=0

pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; failures=$((failures + 1)); }

# Builds a fake Bin64 with a NewWorld.exe: filler, the signature, more filler.
make_bin64() {
  local dir="$1" with_sig="$2"
  mkdir -p "$dir"
  {
    printf 'AAAABBBBCCCCDDDD'
    if [[ "$with_sig" == "yes" ]]; then printf '%s' "$SIGNATURE_HEX" | xxd -r -p; fi
    printf 'EEEEFFFFGGGGHHHH'
  } >"$dir/NewWorld.exe"
}

# Offset (bytes) of the signature inside the fake exe: the 16-byte filler prefix.
readonly SIG_OFFSET=16

# 1) Patch creates the copy and the backup, and leaves the official unchanged.
bin64="$work_dir/good/Bin64"
make_bin64 "$bin64" yes
cp "$bin64/NewWorld.exe" "$work_dir/official.before"
python3 "$PATCH_SCRIPT" "$bin64" >/dev/null
if cmp -s "$bin64/NewWorld.exe" "$work_dir/official.before"; then
  pass "official exe unchanged"
else
  fail "official exe unchanged"
fi
if cmp -s "$bin64/NewWorld.exe.orig" "$work_dir/official.before"; then
  pass "backup equals the original"
else
  fail "backup equals the original"
fi
got="$(dd if="$bin64/BlightedNewWorld.exe" bs=1 skip="$SIG_OFFSET" count=6 2>/dev/null | xxd -p)"
if [[ "$got" == "$NOPS_HEX" ]]; then
  pass "patched copy has NOPs at the verify-mode write"
else
  fail "patched copy has NOPs (got $got)"
fi
official_size="$(stat -c %s "$bin64/NewWorld.exe")"
patched_size="$(stat -c %s "$bin64/BlightedNewWorld.exe")"
if [[ "$official_size" == "$patched_size" ]]; then
  pass "patched copy keeps the file length"
else
  fail "patched copy keeps the file length"
fi
# The bytes after the patched six must still match the original signature tail.
tail_got="$(dd if="$bin64/BlightedNewWorld.exe" bs=1 skip="$((SIG_OFFSET + 6))" count=8 2>/dev/null | xxd -p)"
if [[ "$tail_got" == "4c898188010000c3" ]]; then
  pass "patched copy keeps the callback write and RET"
else
  fail "patched copy keeps the callback write and RET (got $tail_got)"
fi

# 2) --verify passes on the patched copy, and fails after tampering.
if python3 "$PATCH_SCRIPT" "$bin64" --verify >/dev/null; then
  pass "--verify passes on a correct copy"
else
  fail "--verify passes on a correct copy"
fi
printf 'X' | dd of="$bin64/BlightedNewWorld.exe" bs=1 seek="$SIG_OFFSET" count=1 conv=notrunc 2>/dev/null
if python3 "$PATCH_SCRIPT" "$bin64" --verify >/dev/null; then
  fail "--verify fails on a tampered copy"
else
  pass "--verify fails on a tampered copy"
fi

# 3) --revert removes the patched copy but keeps the official exe.
python3 "$PATCH_SCRIPT" "$bin64" --revert >/dev/null
if [[ ! -e "$bin64/BlightedNewWorld.exe" && -e "$bin64/NewWorld.exe" ]]; then
  pass "--revert removes only the patched copy"
else
  fail "--revert removes only the patched copy"
fi

# 4) A build without the signature is refused.
missing="$work_dir/missing/Bin64"
make_bin64 "$missing" no
if python3 "$PATCH_SCRIPT" "$missing" >/dev/null 2>&1; then
  fail "missing signature is refused"
else
  pass "missing signature is refused"
fi

# 5) Re-running is idempotent: a second patch rebuilds the same copy from the official.
python3 "$PATCH_SCRIPT" "$bin64" >/dev/null
python3 "$PATCH_SCRIPT" "$bin64" >/dev/null
if python3 "$PATCH_SCRIPT" "$bin64" --verify >/dev/null; then
  pass "re-running stays correct"
else
  fail "re-running stays correct"
fi

if [[ "$failures" -eq 0 ]]; then
  echo "all patch-client tests passed"
else
  echo "$failures patch-client test(s) failed"
  exit 1
fi
