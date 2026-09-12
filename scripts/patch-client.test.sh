#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Tests for patch-client.py, using a small synthetic executable (no real game files).
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly PATCH_SCRIPT="$SCRIPT_DIR/patch-client.py"
# The verify-setter signature the tool searches for, and the six bytes it replaces.
readonly SIGNATURE_HEX="8991580100004c898188010000c3"
readonly NOPS_HEX="909090909090"
# Sonic and Steam EndSession bodies (the Steam one with a stand-in call offset).
readonly SONIC_HEX="48895c24084889742418488954241057""4883ec20488bfa488bd9488b024c8b4008488d4a0841ffd0c6435801488b074885c0741f488b004885c0741041b802000000488d5708488d4f08ffd048c70700000000488b5c2430488b7424404883c4205fc3"
readonly STEAM_HEX="48895c2408488954241057""4883ec20488bfa488bd94883c130e812345678488b8b280200004885c97407488b01ff5030"
# Fake PE layout: image base, .text at RVA 0x1000 / file 0x200, .rdata at RVA 0x2000 / file 0x300.
readonly SIG_OFFSET=$((0x200))
readonly SONIC_OFFSET=$((0x220))
readonly STEAM_OFFSET=$((0x2a0))
readonly SLOT_OFFSET=$((0x310))
# Little-endian VAs of the Sonic and Steam functions in the fake image.
readonly SONIC_VA_HEX="2010004001000000"
readonly STEAM_VA_HEX="a010004001000000"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
failures=0

pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; failures=$((failures + 1)); }

# Builds a fake Bin64 with a minimal PE NewWorld.exe: the verify signature (when $2 is yes), the
# Sonic and Steam EndSession bodies (the Steam one when $3 is yes), and a vtable slot that
# points at the Steam function.
make_bin64() {
  local dir="$1" with_sig="$2" with_steam="$3"
  mkdir -p "$dir"
  python3 -I - "$dir/NewWorld.exe" "$with_sig" "$with_steam" "$SIGNATURE_HEX" "$SONIC_HEX" \
    "$STEAM_HEX" "$STEAM_VA_HEX" <<'PY'
import struct, sys
path, with_sig, with_steam, sig, sonic, steam, steam_va = sys.argv[1:]
image = bytearray(0x400)
image[0:2] = b"MZ"
struct.pack_into("<I", image, 0x3C, 0x40)
image[0x40:0x44] = b"PE\0\0"
struct.pack_into("<HH", image, 0x44, 0x8664, 2)
struct.pack_into("<H", image, 0x40 + 20, 0xF0)
struct.pack_into("<HQ", image, 0x40 + 24, 0x20B, 0)
struct.pack_into("<Q", image, 0x40 + 24 + 24, 0x140000000)
table = 0x40 + 24 + 0xF0
for index, (name, rva, raw) in enumerate(((b".text", 0x1000, 0x200), (b".rdata", 0x2000, 0x300))):
    entry = table + index * 40
    image[entry : entry + len(name)] = name
    struct.pack_into("<IIII", image, entry + 8, 0x100, rva, 0x100, raw)
def put(offset, hex_text):
    data = bytes.fromhex(hex_text)
    image[offset : offset + len(data)] = data
if with_sig == "yes":
    put(0x200, sig)
put(0x220, sonic)
if with_steam == "yes":
    put(0x2A0, steam)
put(0x310, steam_va)
open(path, "wb").write(image)
PY
}

# 1) Patch creates the copy and the backup, and leaves the official unchanged.
bin64="$work_dir/good/Bin64"
make_bin64 "$bin64" yes yes
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

got="$(dd if="$bin64/BlightedNewWorld.exe" bs=1 skip="$SLOT_OFFSET" count=8 2>/dev/null | xxd -p)"
if [[ "$got" == "$SONIC_VA_HEX" ]]; then
  pass "patched copy points the Steam EndSession slot at the Sonic function"
else
  fail "patched copy points the Steam EndSession slot at the Sonic function (got $got)"
fi
got="$(dd if="$bin64/BlightedNewWorld.exe" bs=1 skip="$STEAM_OFFSET" count=20 2>/dev/null | xxd -p)"
if [[ "$got" == "${STEAM_HEX:0:40}" ]]; then
  pass "patched copy keeps the Steam function body"
else
  fail "patched copy keeps the Steam function body (got $got)"
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

# 4) A build without a signature is refused.
missing="$work_dir/missing/Bin64"
make_bin64 "$missing" no yes
if python3 "$PATCH_SCRIPT" "$missing" >/dev/null 2>&1; then
  fail "missing verify signature is refused"
else
  pass "missing verify signature is refused"
fi
no_steam="$work_dir/no-steam/Bin64"
make_bin64 "$no_steam" yes no
if python3 "$PATCH_SCRIPT" "$no_steam" >/dev/null 2>&1 || [[ -e "$no_steam/BlightedNewWorld.exe" ]]; then
  fail "missing Steam EndSession signature is refused"
else
  pass "missing Steam EndSession signature is refused"
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
