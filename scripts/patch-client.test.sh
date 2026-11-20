#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Tests for patch-client.py, using a small synthetic executable (no real game files).
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly PATCH_SCRIPT="$SCRIPT_DIR/patch-client.py"
# The tests build a synthetic Bin64 directory and never set up an assets/ sibling
# with the Lua pak, so every patch-client.py invocation runs the exe-only patch.
# The full tool is covered end to end by scripts/patch-client-lua.test.sh.
run_patch() {
    python3 "$PATCH_SCRIPT" --skip-lua "$@"
}
# The verify-setter signature the tool searches for, and the six bytes it replaces.
readonly SIGNATURE_HEX="8991580100004c898188010000c3"
readonly NOPS_HEX="909090909090"
# Sonic and Steam EndSession bodies (the Steam one with a stand-in call offset).
readonly SONIC_HEX="48895c24084889742418488954241057""4883ec20488bfa488bd9488b024c8b4008488d4a0841ffd0c6435801488b074885c0741f488b004885c0741041b802000000488d5708488d4f08ffd048c70700000000488b5c2430488b7424404883c4205fc3"
readonly STEAM_HEX="48895c2408488954241057""4883ec20488bfa488bd94883c130e812345678488b8b280200004885c97407488b01ff5030"
# AWS proxy parts: the ClientConfiguration constructor (stand-in call offset), the S3, Kinesis, STS,
# news and region ping set-ups (lead, NOP, replayed instructions, trail; the call offset is filled
# in), and the S3 verifySSL write.
readonly CTOR_HEX="48894c2408535657""4883ec30488bf933f6e81234567890c7472001000000"
readonly S3_LEAD_HEX="488d4c2450" S3_TAIL_HEX="904488bc24ac000000488b15"
readonly KINESIS_LEAD_HEX="488d4df0" KINESIS_TAIL_HEX="90c6454c00c7451001000000"
readonly STS_LEAD_HEX="488d4d10" STS_TAIL_HEX="90c6456c00488d442430"
readonly NEWS_LEAD_HEX="488d4df0" NEWS_TAIL_HEX="90c6454c00488d55f0"
readonly PING_LEAD_HEX="488d4c2430" PING_TAIL_HEX="908bc53886e80100000f95c0"
readonly S3_VERIFY_HEX="c684240802000001c744247001000000"
# The Steam GetTicket function and the start of its ticket handler (stand-in RIP and call offsets).
readonly GET_TICKET_HEX="40534883ec20488bd9c681e802000001488d0d11111111488d1511111111e811111111""83bbf002000000743d488d0d11111111ff15111111118b93f0020000488b08488b01ff9088000000""488d8b8c03000033d241b8000a0000e811111111c7838803000000000000488d0d11111111""ff151111111133d2488b08488b01ff50708983f00200004883c4205bc3"
readonly HANDLER_HEX="48895c2410488974241848897c2420554154415541564157488dac2450faffff4881ecb0060000""488bf2488bd94533ff4489bde0050000448b328b7a044488bde0050000488d0511111111""488944244044897c24480f28442440660f7f4424504c8d0511111111488d542450""488d8de0050000e811111111b8020000004438bde00500000f45f8488d0d11111111""443bb3f00200007411"
# The patched ticket request (from the CMP after the log call): store handle 0, build the response
# (handle 0, result OK, length 0) at [RSP+0x30], call the handler at VA 0x1400014A0, return, and
# INT3 to the end of the replaced bytes.
readonly OFFLINE_TICKET_HEX="c783f002000000000000c744243000000000c744243401000000c744243800000000""488d542430488bcbe8fefeffff4883c4205bc3"
readonly OFFLINE_TICKET_FILL=53
# The typed chat site: the name of the chat restriction variable in .rdata, and its creation in
# .text (LEA RDX,[name] / LEA RCX,[RBP+0x95D8] / CALL / MOV R8B,1 / LEA RDX,[RBP+0x95D8]).
readonly CHAT_NAME="javelin.social.enable-vpc-restrictions"
readonly CHAT_NAME_OFFSET=$((0xc40))
readonly CHAT_SITE_OFFSET=$((0x440))
readonly CHAT_FLAG_OFFSET=$((CHAT_SITE_OFFSET + 0x15))
# The shadow account check in .text (stand-in call offset): PUSH RBX / SUB RSP,0x20 / MOV RBX,RCX /
# CALL / TEST EAX,EAX / JNZ / CMP [RBX+0x25C],EAX / JNZ / MOV AL,1 / ADD RSP,0x20 / POP RBX / RET /
# XOR AL,AL / ADD RSP,0x20 / POP RBX / RET; the 1 of its MOV AL,1 at +0x1B.
readonly SHADOW_CHECK_HEX="40534883ec20488bd9e811111111""85c07510""39835c020000""7508b001""4883c4205bc3""32c04883c4205bc3"
readonly SHADOW_CHECK_OFFSET=$((0x410))
readonly SHADOW_YES_OFFSET=$((SHADOW_CHECK_OFFSET + 0x1b))
# AGID console variables: the enable bool is the same creation shape as the chat restriction
# variable (MOV R8B,1); the two cooldowns share an int32 shape (MOV R8D,imm32).
readonly AGID_ENABLE_NAME="javelin.enable-agid-account-linking"
readonly AGID_SECS_NAME="javelin.agid-link-modal-cooldown-secs"
readonly AGID_BUTTON_NAME="javelin.agid-link-modal-button-cooldown"
readonly AGID_ENABLE_NAME_OFFSET=$((0xC80))
readonly AGID_SECS_NAME_OFFSET=$((0xCB0))
readonly AGID_BUTTON_NAME_OFFSET=$((0xCE0))
readonly AGID_ENABLE_SITE_OFFSET=$((0x900))
readonly AGID_SECS_SITE_OFFSET=$((0x940))
readonly AGID_BUTTON_SITE_OFFSET=$((0x980))
# A bool creation site is 29 bytes: LEA RDX(7) + LEA RCX(7) + CALL(5) + MOV R8B imm8(3) + LEA RDX(7).
# An int32 site is 32 bytes: LEA RDX(7) + LEA RCX(7) + CALL(5) + MOV R8D imm32(6) + LEA RDX(7).
readonly AGID_ENABLE_FLAG_OFFSET=$((AGID_ENABLE_SITE_OFFSET + 0x15))
readonly AGID_SECS_IMM_OFFSET=$((AGID_SECS_SITE_OFFSET + 0x15))
readonly AGID_BUTTON_IMM_OFFSET=$((AGID_BUTTON_SITE_OFFSET + 0x15))
# Fake PE layout: image base, .text at RVA 0x1000 / file 0x200 (0xA00 bytes, through 0xC00),
# .rdata at RVA 0x2000 / file 0xC00 (0x200 bytes, through 0xE00).
readonly SIG_OFFSET=$((0x200))
readonly SONIC_OFFSET=$((0x220))
readonly STEAM_OFFSET=$((0x2a0))
readonly SLOT_OFFSET=$((0xc10))
readonly CTOR_OFFSET=$((0x300))
readonly S3_OFFSET=$((0x340))
readonly KINESIS_OFFSET=$((0x380))
readonly STS_OFFSET=$((0x3c0))
readonly S3_VERIFY_OFFSET=$((0x400))
readonly NEWS_OFFSET=$((0x480))
readonly PING_OFFSET=$((0x4c0))
# The ticket handler and GetTicket sit past the INT3 run the caves use.
readonly HANDLER_OFFSET=$((0x6a0))
readonly GET_TICKET_OFFSET=$((0x750))
readonly TICKET_REQUEST_OFFSET=$((GET_TICKET_OFFSET + 0x23))
# INT3 padding (0x500..0x6A0, room for five caves): the first cave is at file 0x500, VA
# 0x140001300.
readonly CAVE_OFFSET=$((0x500))
readonly CAVE_SLOT=$((0x50))
# The jumps that replace the NOP and the next instruction after each constructor call: to the
# caves at VA 0x140001300, 0x140001350, 0x1400013A0, 0x1400013F0 and 0x140001440.
readonly S3_JUMP_HEX="e9b101000090909090"
readonly KINESIS_JUMP_HEX="e9c2010000"
readonly STS_JUMP_HEX="e9d2010000"
readonly NEWS_JUMP_HEX="e962010000"
readonly PING_JUMP_HEX="e97101000090909090"
# The S3 cave: proxyHost "127.0.0.1" (three dwords), its size 9, proxyPort 47443, verifySSL 0,
# the replayed MOV [RSP+0xAC],R15B, then JMP back to VA 0x140001153.
readonly S3_CAVE_HEX="c780a00000003132372ec780a4000000302e302ec780a800000031000000""48c780b000000009000000c780c000000053b90000c680b801000000""4488bc24ac000000e90cfeffff"
# Little-endian VAs of the Sonic and Steam functions in the fake image.
readonly SONIC_VA_HEX="2010004001000000"
readonly STEAM_VA_HEX="a010004001000000"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
failures=0

pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; failures=$((failures + 1)); }

# Prints $3 bytes of file $1 at offset $2 as hex.
bytes_at() {
  dd if="$1" bs=1 skip="$2" count="$3" 2>/dev/null | xxd -p | tr -d '\n'
}

# Checks that the patched copy ($patched) has the hex $3 at offset $2.
check_bytes() {
  local what="$1" offset="$2" want="$3" got
  got="$(bytes_at "$patched" "$offset" $((${#want} / 2)))"
  if [[ "$got" == "$want" ]]; then
    pass "$what"
  else
    fail "$what (got $got)"
  fi
}

# Builds a fake Bin64 with a minimal PE NewWorld.exe: the verify signature (when $2 is yes), the
# Sonic and Steam EndSession bodies (the Steam one when $3 is yes), a vtable slot that points at
# the Steam function, the AWS proxy parts with INT3 padding (when $4 is yes), and the Steam
# GetTicket function and its ticket handler (when $5 is yes, the default).
make_bin64() {
  local dir="$1" with_sig="$2" with_steam="$3" with_proxy="$4" with_ticket="${5:-yes}" \
    with_agid="${6:-yes}"
  mkdir -p "$dir"
  python3 -I - "$dir/NewWorld.exe" "$with_sig" "$with_steam" "$with_proxy" \
    "$with_ticket" "$with_agid" "$GET_TICKET_HEX" "$HANDLER_HEX" "$SIGNATURE_HEX" "$SONIC_HEX" "$STEAM_HEX" "$STEAM_VA_HEX" "$CTOR_HEX" "$S3_LEAD_HEX" \
    "$S3_TAIL_HEX" "$KINESIS_LEAD_HEX" "$KINESIS_TAIL_HEX" "$STS_LEAD_HEX" "$STS_TAIL_HEX" \
    "$NEWS_LEAD_HEX" "$NEWS_TAIL_HEX" "$PING_LEAD_HEX" "$PING_TAIL_HEX" \
    "$S3_VERIFY_HEX" "$CHAT_NAME" "$CHAT_NAME_OFFSET" "$CHAT_SITE_OFFSET" \
    "$SHADOW_CHECK_HEX" "$SHADOW_CHECK_OFFSET" \
    "$AGID_ENABLE_NAME" "$AGID_ENABLE_NAME_OFFSET" "$AGID_ENABLE_SITE_OFFSET" \
    "$AGID_SECS_NAME" "$AGID_SECS_NAME_OFFSET" "$AGID_SECS_SITE_OFFSET" \
    "$AGID_BUTTON_NAME" "$AGID_BUTTON_NAME_OFFSET" "$AGID_BUTTON_SITE_OFFSET" <<'PY'
import struct, sys
(path, with_sig, with_steam, with_proxy, with_ticket, with_agid, get_ticket, handler, sig, sonic,
 steam, steam_va, ctor, s3_lead, s3_tail, kinesis_lead, kinesis_tail, sts_lead, sts_tail,
 news_lead, news_tail, ping_lead, ping_tail, s3_verify, chat_name, chat_name_offset, chat_site,
 shadow_check, shadow_check_offset,
 agid_enable_name, agid_enable_name_offset, agid_enable_site,
 agid_secs_name, agid_secs_name_offset, agid_secs_site,
 agid_button_name, agid_button_name_offset, agid_button_site) = sys.argv[1:]
# Fake PE: .text at file 0x200 size 0xA00 (RVA 0x1000), .rdata at file 0xC00 size 0x200 (RVA 0x2000).
TEXT_RAW = 0x200
TEXT_SIZE = 0xA00
RDATA_RAW = 0xC00
RDATA_SIZE = 0x200
IMAGE_LEN = RDATA_RAW + RDATA_SIZE
image = bytearray(IMAGE_LEN)
image[0:2] = b"MZ"
struct.pack_into("<I", image, 0x3C, 0x40)
image[0x40:0x44] = b"PE\0\0"
struct.pack_into("<HH", image, 0x44, 0x8664, 2)
struct.pack_into("<H", image, 0x40 + 20, 0xF0)
struct.pack_into("<HQ", image, 0x40 + 24, 0x20B, 0)
struct.pack_into("<Q", image, 0x40 + 24 + 24, 0x140000000)
table = 0x40 + 24 + 0xF0
for index, (name, rva, raw, size, flags) in enumerate((
    (b".text", 0x1000, TEXT_RAW, TEXT_SIZE, 0x60000020),
    (b".rdata", 0x2000, RDATA_RAW, RDATA_SIZE, 0x40000040),
)):
    entry = table + index * 40
    image[entry : entry + len(name)] = name
    struct.pack_into("<IIII", image, entry + 8, size, rva, size, raw)
    struct.pack_into("<I", image, entry + 36, flags)
def put(offset, hex_text):
    data = bytes.fromhex(hex_text)
    image[offset : offset + len(data)] = data
def va(offset):
    # .text raw 0x200 maps to VA 0x140001000.
    return 0x140001000 + (offset - TEXT_RAW)
def rdata_va_of(offset):
    # .rdata raw RDATA_RAW maps to RVA 0x2000 (VA 0x140002000).
    return 0x140002000 + (offset - RDATA_RAW)
if with_sig == "yes":
    put(0x200, sig)
put(0x220, sonic)
if with_steam == "yes":
    put(0x2A0, steam)
put(RDATA_RAW + 0x10, steam_va)
if with_proxy == "yes":
    put(0x300, ctor)
    for site, lead, tail in ((0x340, s3_lead, s3_tail), (0x380, kinesis_lead, kinesis_tail),
                             (0x3C0, sts_lead, sts_tail), (0x480, news_lead, news_tail),
                             (0x4C0, ping_lead, ping_tail)):
        call = site + len(bytes.fromhex(lead))
        offset = struct.pack("<i", va(0x300) - (va(call) + 5))
        put(site, lead + "e8" + offset.hex() + tail)
    put(0x400, s3_verify)
    image[0x500:0x800] = b"\xcc" * 0x300
if with_ticket == "yes":
    put(0x6A0, handler)
    put(0x750, get_ticket)
chat_name_offset, chat_site = int(chat_name_offset), int(chat_site)
image[chat_name_offset : chat_name_offset + len(chat_name) + 1] = chat_name.encode() + b"\0"
chat_name_va = rdata_va_of(chat_name_offset)
name_rel = struct.pack("<i", chat_name_va - (va(chat_site) + 7))
put(chat_site, "488d15" + name_rel.hex() + "488d8dd8950000" "e811111111" "41b001" "488d95d8950000")
put(int(shadow_check_offset), shadow_check)
# AGID variable creation sites and names.
def put_bool_creation(site, name_text, name_offset, default_byte):
    image[name_offset : name_offset + len(name_text) + 1] = name_text.encode() + b"\0"
    name_va = rdata_va_of(name_offset)
    rel = struct.pack("<i", name_va - (va(site) + 7))
    # LEA RDX,[name] / LEA RCX,[RBP+0x1020] / CALL stub / MOV R8B,imm8 / LEA RDX,[RBP+0x1020]
    put(site, "488d15" + rel.hex() + "488d8d20100000" "e811111111"
        "41b0" + f"{default_byte:02x}" + "488d9520100000")
def put_int32_creation(site, name_text, name_offset, default_imm32):
    image[name_offset : name_offset + len(name_text) + 1] = name_text.encode() + b"\0"
    name_va = rdata_va_of(name_offset)
    rel = struct.pack("<i", name_va - (va(site) + 7))
    imm = struct.pack("<I", default_imm32).hex()
    # LEA RDX,[name] / LEA RCX,[RBP+0x1020] / CALL stub / MOV R8D,imm32 / LEA RDX,[RBP+0x1020]
    put(site, "488d15" + rel.hex() + "488d8d20100000" "e811111111"
        "41b8" + imm + "488d9520100000")
if with_agid == "yes":
    put_bool_creation(int(agid_enable_site), agid_enable_name, int(agid_enable_name_offset), 1)
    put_int32_creation(int(agid_secs_site), agid_secs_name, int(agid_secs_name_offset), 0x0002A300)
    put_int32_creation(int(agid_button_site), agid_button_name, int(agid_button_name_offset), 0x14)
open(path, "wb").write(image)
PY
}

# 1) Patch creates the copy and the backup, and leaves the official unchanged.
bin64="$work_dir/good/Bin64"
patched="$bin64/BlightedNewWorld.exe"
make_bin64 "$bin64" yes yes yes
cp "$bin64/NewWorld.exe" "$work_dir/official.before"
run_patch "$bin64" >/dev/null
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

check_bytes "patched GetTicket calls the ticket handler with an empty ticket" \
  "$TICKET_REQUEST_OFFSET" "$OFFLINE_TICKET_HEX"
check_bytes "patched GetTicket fills the rest of the request with INT3" \
  $((TICKET_REQUEST_OFFSET + ${#OFFLINE_TICKET_HEX} / 2)) "$(printf 'cc%.0s' $(seq "$OFFLINE_TICKET_FILL"))"
check_bytes "patched GetTicket keeps the flag write and the log call" "$GET_TICKET_OFFSET" "${GET_TICKET_HEX:0:70}"
check_bytes "the ticket handler is unchanged" "$HANDLER_OFFSET" "$HANDLER_HEX"
check_bytes "the chat restriction variable starts off" "$((CHAT_FLAG_OFFSET - 2))" "41b000488d95d8950000"
check_bytes "the shadow account check answers 0" "$((SHADOW_YES_OFFSET - 1))" "b0004883c4205bc3"
check_bytes "the AGID link enable variable starts off" "$((AGID_ENABLE_FLAG_OFFSET - 2))" "41b000488d9520100000"
check_bytes "the AGID link cooldown secs variable starts at 0x7FFFFFFF" \
  "$((AGID_SECS_IMM_OFFSET - 2))" "41b8ffffff7f488d9520100000"
check_bytes "the AGID link button cooldown variable starts at 0x7FFFFFFF" \
  "$((AGID_BUTTON_IMM_OFFSET - 2))" "41b8ffffff7f488d9520100000"

# 2) --verify passes on the patched copy, and fails after tampering.
if run_patch "$bin64" --verify >/dev/null; then
  pass "--verify passes on a correct copy"
else
  fail "--verify passes on a correct copy"
fi
printf 'X' | dd of="$bin64/BlightedNewWorld.exe" bs=1 seek="$SIG_OFFSET" count=1 conv=notrunc 2>/dev/null
if run_patch "$bin64" --verify >/dev/null; then
  fail "--verify fails on a tampered copy"
else
  pass "--verify fails on a tampered copy"
fi

# The shadow account check must be part of --verify: tampering it back to 1 must fail.
run_patch "$bin64" >/dev/null
printf '\x01' | dd of="$bin64/BlightedNewWorld.exe" bs=1 seek="$SHADOW_YES_OFFSET" count=1 conv=notrunc 2>/dev/null
if run_patch "$bin64" --verify >/dev/null; then
  fail "--verify fails when the shadow account yes byte is tampered to 1"
else
  pass "--verify fails when the shadow account yes byte is tampered to 1"
fi

# --verify must also fail when any one of the three AGID gate bytes is tampered back.
run_patch "$bin64" >/dev/null
printf '\x01' | dd of="$bin64/BlightedNewWorld.exe" bs=1 seek="$AGID_ENABLE_FLAG_OFFSET" count=1 conv=notrunc 2>/dev/null
if run_patch "$bin64" --verify >/dev/null; then
  fail "--verify fails when the AGID link enable byte is tampered to 1"
else
  pass "--verify fails when the AGID link enable byte is tampered to 1"
fi
run_patch "$bin64" >/dev/null
printf '\x00\xa3\x02\x00' | dd of="$bin64/BlightedNewWorld.exe" bs=1 seek="$AGID_SECS_IMM_OFFSET" count=4 conv=notrunc 2>/dev/null
if run_patch "$bin64" --verify >/dev/null; then
  fail "--verify fails when the AGID cooldown secs is tampered back to 48 hours"
else
  pass "--verify fails when the AGID cooldown secs is tampered back to 48 hours"
fi
run_patch "$bin64" >/dev/null
printf '\x14\x00\x00\x00' | dd of="$bin64/BlightedNewWorld.exe" bs=1 seek="$AGID_BUTTON_IMM_OFFSET" count=4 conv=notrunc 2>/dev/null
if run_patch "$bin64" --verify >/dev/null; then
  fail "--verify fails when the AGID button cooldown is tampered back to 20 seconds"
else
  pass "--verify fails when the AGID button cooldown is tampered back to 20 seconds"
fi

# A build without any AGID variable creation sites must be refused loudly.
no_agid="$work_dir/no-agid/Bin64"
make_bin64 "$no_agid" yes yes yes yes no
if run_patch "$no_agid" >/dev/null 2>&1 \
  || [[ -e "$no_agid/BlightedNewWorld.exe" ]]; then
  fail "missing AGID variable creation sites are refused"
else
  pass "missing AGID variable creation sites are refused"
fi

# A build without the shadow account check must be refused loudly, with a non-zero exit.
no_shadow="$work_dir/no-shadow/Bin64"
make_bin64 "$no_shadow" yes yes yes
# Overwrite the shadow check bytes with INT3 so the signature cannot match.
dd if=/dev/zero bs=1 count="$((${#SHADOW_CHECK_HEX} / 2))" conv=notrunc 2>/dev/null \
  | tr '\0' '\xcc' \
  | dd of="$no_shadow/NewWorld.exe" bs=1 seek="$SHADOW_CHECK_OFFSET" conv=notrunc 2>/dev/null
if run_patch "$no_shadow" >/dev/null 2>&1 \
  || [[ -e "$no_shadow/BlightedNewWorld.exe" ]]; then
  fail "missing shadow account check signature is refused"
else
  pass "missing shadow account check signature is refused"
fi

# 3) --revert removes the patched copy but keeps the official exe.
run_patch "$bin64" --revert >/dev/null
if [[ ! -e "$bin64/BlightedNewWorld.exe" && -e "$bin64/NewWorld.exe" ]]; then
  pass "--revert removes only the patched copy"
else
  fail "--revert removes only the patched copy"
fi

# 4) A build without a signature is refused.
missing="$work_dir/missing/Bin64"
make_bin64 "$missing" no yes yes
if run_patch "$missing" >/dev/null 2>&1; then
  fail "missing verify signature is refused"
else
  pass "missing verify signature is refused"
fi
no_steam="$work_dir/no-steam/Bin64"
make_bin64 "$no_steam" yes no yes
if run_patch "$no_steam" >/dev/null 2>&1 || [[ -e "$no_steam/BlightedNewWorld.exe" ]]; then
  fail "missing Steam EndSession signature is refused"
else
  pass "missing Steam EndSession signature is refused"
fi

no_ticket="$work_dir/no-ticket/Bin64"
make_bin64 "$no_ticket" yes yes yes no
if run_patch "$no_ticket" >/dev/null 2>&1 || [[ -e "$no_ticket/BlightedNewWorld.exe" ]]; then
  fail "missing Steam GetTicket signature is refused"
else
  pass "missing Steam GetTicket signature is refused"
fi

# 5) Re-running is idempotent: a second patch rebuilds the same copy from the official.
run_patch "$bin64" >/dev/null
run_patch "$bin64" >/dev/null
if run_patch "$bin64" --verify >/dev/null; then
  pass "re-running stays correct"
else
  fail "re-running stays correct"
fi

# 6) Without --aws-proxy the AWS client set-ups stay as they are.
got="$(bytes_at "$bin64/BlightedNewWorld.exe" "$CAVE_OFFSET" 4)"
if [[ "$got" == "cccccccc" ]] && cmp -s <(bytes_at "$bin64/NewWorld.exe" "$S3_OFFSET" 64) \
  <(bytes_at "$bin64/BlightedNewWorld.exe" "$S3_OFFSET" 64); then
  pass "the AWS clients are unchanged without --aws-proxy"
else
  fail "the AWS clients are unchanged without --aws-proxy"
fi

# 7) --aws-proxy adds a jump and a cave per AWS client, and turns off the S3 verifySSL write.
run_patch "$bin64" --aws-proxy >/dev/null
check_bytes "S3 call returns into a jump to its cave" $((S3_OFFSET + 10)) "$S3_JUMP_HEX"
check_bytes "Kinesis call returns into a jump to its cave" $((KINESIS_OFFSET + 9)) "$KINESIS_JUMP_HEX"
check_bytes "STS call returns into a jump to its cave" $((STS_OFFSET + 9)) "$STS_JUMP_HEX"
check_bytes "news call returns into a jump to its cave" $((NEWS_OFFSET + 9)) "$NEWS_JUMP_HEX"
check_bytes "region ping call returns into a jump to its cave" $((PING_OFFSET + 10)) "$PING_JUMP_HEX"
check_bytes "S3 cave sets the proxy and verifySSL, replays, jumps back" "$CAVE_OFFSET" "$S3_CAVE_HEX"
check_bytes "Kinesis cave replays its instruction" $((CAVE_OFFSET + CAVE_SLOT + 58)) "c6454c00e9"
check_bytes "STS cave replays its instruction" $((CAVE_OFFSET + 2 * CAVE_SLOT + 58)) "c6456c00e9"
check_bytes "news cave replays its instruction" $((CAVE_OFFSET + 3 * CAVE_SLOT + 58)) "c6454c00e9"
check_bytes "region ping cave replays its instructions" $((CAVE_OFFSET + 4 * CAVE_SLOT + 58)) "8bc53886e8010000e9"
check_bytes "S3 verifySSL write stores 0" "$S3_VERIFY_OFFSET" "c684240802000000c7442470"
check_bytes "constructor is unchanged" "$CTOR_OFFSET" "${CTOR_HEX:0:40}"
check_bytes "base patches are kept" "$SIG_OFFSET" "$NOPS_HEX"
if run_patch "$bin64" --verify --aws-proxy >/dev/null; then
  pass "--verify --aws-proxy passes on a proxy copy"
else
  fail "--verify --aws-proxy passes on a proxy copy"
fi
printf 'X' | dd of="$patched" bs=1 seek="$((CAVE_OFFSET + 6))" count=1 conv=notrunc 2>/dev/null
if run_patch "$bin64" --verify --aws-proxy >/dev/null; then
  fail "--verify --aws-proxy fails on a tampered cave"
else
  pass "--verify --aws-proxy fails on a tampered cave"
fi
run_patch "$bin64" --revert >/dev/null
if [[ ! -e "$patched" ]] && cmp -s "$bin64/NewWorld.exe" "$bin64/NewWorld.exe.orig"; then
  pass "--revert removes the proxy copy and keeps the official exe"
else
  fail "--revert removes the proxy copy and keeps the official exe"
fi

# 8) --aws-proxy is refused when a set-up is missing; the base patch still works without it.
no_proxy="$work_dir/no-proxy/Bin64"
make_bin64 "$no_proxy" yes yes no
if run_patch "$no_proxy" --aws-proxy >/dev/null 2>&1 \
  || [[ -e "$no_proxy/BlightedNewWorld.exe" ]]; then
  fail "missing AWS client set-ups are refused with --aws-proxy"
else
  pass "missing AWS client set-ups are refused with --aws-proxy"
fi
if run_patch "$no_proxy" >/dev/null; then
  pass "the base patch needs no AWS client set-ups"
else
  fail "the base patch needs no AWS client set-ups"
fi

if [[ "$failures" -eq 0 ]]; then
  echo "all patch-client tests passed"
else
  echo "$failures patch-client test(s) failed"
  exit 1
fi
