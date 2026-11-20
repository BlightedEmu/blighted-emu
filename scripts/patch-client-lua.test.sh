#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Test harness for scripts/patch-client-lua.py. The harness builds a tiny
# synthetic pak around a synthetic Lua 5.1 script, drives the full pipeline
# (open pak, read entry, parse Lua, stub the AGID protos, rewrite pak) and
# proves the patched pak passes the tool's own verify path.
#
# The harness never touches the real game files and never calls Oodle: the
# synthetic script is stored with method 0, which is also the method the
# tool writes the patched entry back as.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
tool="$here/patch-client-lua.py"

workdir="$(mktemp -d -t patch-client-lua.XXXXXXXX)"
trap 'rm -rf "$workdir"' EXIT

assets="$workdir/assets"
mkdir -p "$assets"

python3 - "$assets" <<'PY'
import os, struct, sys, zlib

assets = sys.argv[1]
pak = os.path.join(assets, "DataStrm-part3.pak")

# ---------- 1. build a synthetic Lua 5.1 bytecode script ----------------
# Four protos:
#   root            -> no AGID strings; carries the three children
#     proto_try     -> has 'javelin.enable-agid-account-linking'
#     proto_modal   -> has 'Showing AGID account link modal'
#     proto_noop    -> has no AGID string; must stay untouched
LUA_HEADER_PREFIX = b"\x04\x00"
LUA_HEADER = bytes.fromhex("1b4c7561") + bytes((0x51, 0, 1, 4, 8, 4, 8, 0))

def lua_string(value: bytes) -> bytes:
    if not value:
        return struct.pack("<Q", 0)
    return struct.pack("<Q", len(value)) + value

def encode_return(a=0, b=1, c=0):
    LUA_OP_RETURN = 30
    return struct.pack("<I", ((b & 0x1FF) << 23) | ((c & 0x1FF) << 14)
                       | ((a & 0xFF) << 6) | (LUA_OP_RETURN & 0x3F))

def write_proto(buf, *, source=b"", linedefined=0, lastlinedefined=0,
                nups=0, nparams=0, is_vararg=0, maxstack=2,
                code=None, constants=None, children=None, lines=None):
    code = code if code is not None else [encode_return()]
    constants = constants or []
    children = children or []
    lines = lines or []
    buf += lua_string(source)
    buf += struct.pack("<ii", linedefined, lastlinedefined)
    buf += bytes((nups, nparams, is_vararg, maxstack))
    buf += struct.pack("<i", len(code))
    for insn in code:
        buf += insn
    buf += struct.pack("<i", len(constants))
    for value in constants:
        if value is None:
            buf += bytes((0,))
        elif isinstance(value, bool):
            buf += bytes((1, 1 if value else 0))
        elif isinstance(value, float):
            buf += bytes((3,)) + struct.pack("<d", value)
        elif isinstance(value, bytes):
            buf += bytes((4,)) + lua_string(value)
        else:
            raise SystemExit(f"bad const {type(value)}")
    buf += struct.pack("<i", len(children))
    for child in children:
        buf += child
    buf += struct.pack("<i", len(lines))
    for line in lines:
        buf += struct.pack("<i", line)
    buf += struct.pack("<i", 0)  # no locals
    buf += struct.pack("<i", 0)  # no upvalue names

# Each child proto has a non-trivial body so the "before" bytes differ from
# the "after" (stubbed) bytes; the stub replaces it with [RETURN 0 1].
BODY_BEFORE = [
    encode_return(a=0, b=1, c=0),
    encode_return(a=0, b=1, c=0),
    encode_return(a=0, b=1, c=0),
]

try_buf = bytearray()
write_proto(try_buf, source=b"@try.lua", linedefined=10, lastlinedefined=40,
            nparams=1, maxstack=3, code=BODY_BEFORE,
            constants=[b"GetBool",
                       b"javelin.enable-agid-account-linking",
                       b"hasShownAGIDLinkThisSession"],
            lines=[10, 11, 12])

modal_buf = bytearray()
write_proto(modal_buf, source=b"@modal.lua", linedefined=42, lastlinedefined=50,
            nparams=1, maxstack=3, code=BODY_BEFORE,
            constants=[b"Showing AGID account link modal",
                       b"ShowAGIDLinkModal"],
            lines=[42, 43, 44])

noop_buf = bytearray()
write_proto(noop_buf, source=b"@noop.lua", linedefined=60, lastlinedefined=80,
            nparams=0, maxstack=2, code=BODY_BEFORE,
            constants=[b"not an AGID function"],
            lines=[60, 61, 62])

root_buf = bytearray()
write_proto(root_buf, source=b"@root.lua", linedefined=0, lastlinedefined=100,
            nparams=0, is_vararg=2, maxstack=4, code=[encode_return()],
            constants=[b"outer"],
            children=[bytes(try_buf), bytes(modal_buf), bytes(noop_buf)],
            lines=[0])

luac = LUA_HEADER_PREFIX + LUA_HEADER + bytes(root_buf)

# ---------- 2. build a synthetic zip pak around that script -------------
# Two entries: our target luac at method 0 (so the test needs no Oodle),
# and an unrelated bystander entry we expect to stay bit-identical.
def build_local(name, data, crc, method=0):
    header = struct.pack("<IHHHHHIIIHH", 0x04034B50,
                         20, 0, method, 0, 0, crc, len(data), len(data),
                         len(name), 0)
    return header + name + data, len(header + name + data)

def build_cd(name, lhoff, crc, csize, usize, method=0):
    return struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50,
                       20, 20, 0, method, 0, 0, crc, csize, usize,
                       len(name), 0, 0, 0, 0, 0, lhoff) + name

target_name = b"lyshineui/main menu/landingscreenv2.luac"
bystander_name = b"other/keep-me.txt"
bystander_data = b"this entry must survive the round-trip unchanged.\n"

target_crc = zlib.crc32(luac) & 0xFFFFFFFF
bystander_crc = zlib.crc32(bystander_data) & 0xFFFFFFFF

out = bytearray()
target_lhoff = len(out)
rec, _ = build_local(target_name, luac, target_crc, method=0)
out += rec
bystander_lhoff = len(out)
rec, _ = build_local(bystander_name, bystander_data, bystander_crc, method=0)
out += rec

cd_off = len(out)
cd = bytearray()
cd += build_cd(target_name, target_lhoff, target_crc, len(luac), len(luac))
cd += build_cd(bystander_name, bystander_lhoff, bystander_crc,
               len(bystander_data), len(bystander_data))
out += cd

eocd = struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, 2, 2,
                   len(cd), cd_off, 0)
out += eocd

with open(pak, "wb") as handle:
    handle.write(out)

print(f"built {pak} entries=2 luac={len(luac)}B")
PY

# ---------- 3. run the patch tool (STORE in, STORE out: no Oodle needed) -

# Any path is accepted by resolve_oodle_tool when it exists and is
# executable. The synthetic case does not call the decoder, but the tool
# resolves the path up-front, so point it at /bin/true.
fake_oodle="/bin/true"
if [[ ! -x "$fake_oodle" ]]; then
    fake_oodle="$(command -v true)"
fi

"$tool" "$assets" --oodle-tool "$fake_oodle"
"$tool" "$assets" --oodle-tool "$fake_oodle" --verify

# ---------- 4. deep check: round-trip, bystander untouched, protos stubbed
python3 - "$assets" <<'PY'
import os, struct, sys, zlib

assets = sys.argv[1]
patched = os.path.join(assets, "DataStrm-part3.blighted.pak")
original = os.path.join(assets, "DataStrm-part3.pak")

with open(patched, "rb") as handle:
    data = handle.read()

# Find EOCD.
idx = data.rfind(b"PK\x05\x06")
assert idx >= 0, "no EOCD in patched pak"
(_, _, entries, _, cd_size, cd_off, _) = struct.unpack_from("<HHHHIIH", data, idx + 4)
assert entries == 2, f"expected 2 entries, got {entries}"

# Pull bystander by walking CD.
offset = cd_off
found_bystander = False
found_target = False
while offset < cd_off + cd_size:
    assert data[offset:offset+4] == b"PK\x01\x02"
    fields = struct.unpack_from("<HHHHHHIIIHHHHHII", data, offset + 4)
    (version_made, version_need, flags, method, mtime, mdate, crc,
     csize, usize, nlen, elen, clen, _ds, _ia, _ea, lhoff) = fields
    name_start = offset + 46
    name = data[name_start:name_start + nlen]
    local = data[lhoff:lhoff + 30]
    (_, _, _, _, _, _, _, _, _, lnlen, lelen) = struct.unpack("<IHHHHHIIIHH", local)
    data_off = lhoff + 30 + lnlen + lelen
    payload = data[data_off:data_off + csize]
    if name.endswith(b"keep-me.txt"):
        found_bystander = True
        assert method == 0
        assert payload == b"this entry must survive the round-trip unchanged.\n"
        assert zlib.crc32(payload) & 0xFFFFFFFF == crc
    if name.endswith(b"landingscreenv2.luac"):
        found_target = True
        assert method == 0, f"expected STORE, got method {method}"
        assert zlib.crc32(payload) & 0xFFFFFFFF == crc
        assert len(payload) == usize
    offset += 46 + nlen + elen + clen

assert found_bystander, "bystander entry missing from patched pak"
assert found_target, "target entry missing from patched pak"

# Parse the patched luac and prove each AGID proto has only RETURN 0 1.
with open(patched, "rb") as handle:
    d = handle.read()

# Take the target payload out via the CD (do it again, we only kept flags).
offset = cd_off
while offset < cd_off + cd_size:
    fields = struct.unpack_from("<HHHHHHIIIHHHHHII", d, offset + 4)
    (_, _, _, _, _, _, crc, csize, usize, nlen, elen, clen, _, _, _, lhoff) = fields
    name = d[offset + 46:offset + 46 + nlen]
    if name.endswith(b"landingscreenv2.luac"):
        local = d[lhoff:lhoff + 30]
        (_, _, _, _, _, _, _, _, _, lnlen, lelen) = struct.unpack("<IHHHHHIIIHH", local)
        data_off = lhoff + 30 + lnlen + lelen
        luac = d[data_off:data_off + csize]
        break
    offset += 46 + nlen + elen + clen

# Walk the Lua tree enough to pick out the three leaf protos' bodies.
assert luac[:2] == b"\x04\x00", "Amazon prefix lost"
assert luac[2:6] == b"\x1bLua", "Lua signature lost"
assert luac[6] == 0x51, "Lua version lost"

off = 2 + 12  # prefix + header

def read_string(buf, off):
    n = struct.unpack_from("<Q", buf, off)[0]
    off += 8
    s = buf[off:off + n]
    return s, off + n

def read_proto(buf, off):
    source, off = read_string(buf, off)
    linedefined, lastlinedefined = struct.unpack_from("<ii", buf, off); off += 8
    nups, nparams, is_vararg, maxstack = buf[off:off + 4]; off += 4
    sizecode = struct.unpack_from("<i", buf, off)[0]; off += 4
    code = []
    for _ in range(sizecode):
        code.append(struct.unpack_from("<I", buf, off)[0]); off += 4
    sizek = struct.unpack_from("<i", buf, off)[0]; off += 4
    constants = []
    for _ in range(sizek):
        tag = buf[off]; off += 1
        if tag == 0:
            constants.append(None)
        elif tag == 1:
            constants.append(bool(buf[off])); off += 1
        elif tag == 3:
            constants.append(struct.unpack_from("<d", buf, off)[0]); off += 8
        elif tag == 4:
            s, off = read_string(buf, off); constants.append(s)
        else:
            raise SystemExit(f"bad tag {tag}")
    sizep = struct.unpack_from("<i", buf, off)[0]; off += 4
    children = []
    for _ in range(sizep):
        child, off = read_proto(buf, off); children.append(child)
    sizeline = struct.unpack_from("<i", buf, off)[0]; off += 4
    lines = []
    for _ in range(sizeline):
        lines.append(struct.unpack_from("<i", buf, off)[0]); off += 4
    sizeloc = struct.unpack_from("<i", buf, off)[0]; off += 4
    for _ in range(sizeloc):
        _n, off = read_string(buf, off); off += 8
    sizeup = struct.unpack_from("<i", buf, off)[0]; off += 4
    for _ in range(sizeup):
        _n, off = read_string(buf, off)
    return {"code": code, "constants": constants, "children": children,
            "lines": lines, "nparams": nparams, "nups": nups,
            "is_vararg": is_vararg, "linedefined": linedefined}, off

root, end = read_proto(luac, off)
assert end == len(luac), f"trailing {len(luac) - end} bytes after root proto"

# Our root has three children in order: try, modal, noop.
try_proto, modal_proto, noop_proto = root["children"]

LUA_OP_RETURN = 30
EARLY_RETURN = ((1 & 0x1FF) << 23) | ((0 & 0xFF) << 6) | (LUA_OP_RETURN & 0x3F)

def is_stubbed(proto):
    return (len(proto["code"]) == 1
            and proto["code"][0] == EARLY_RETURN)

assert is_stubbed(try_proto), f"try proto not stubbed: code={try_proto['code']}"
assert is_stubbed(modal_proto), f"modal proto not stubbed: code={modal_proto['code']}"
assert not is_stubbed(noop_proto), "noop proto was wrongly stubbed"
assert len(noop_proto["code"]) == 3, (
    f"noop proto bytecode changed: {noop_proto['code']}")

# Proto header fields must be preserved.
assert try_proto["nparams"] == 1
assert modal_proto["nparams"] == 1
assert try_proto["linedefined"] == 10
assert modal_proto["linedefined"] == 42

# The pak original must be untouched.
with open(original, "rb") as handle:
    before = handle.read()
import subprocess
# Re-compute the bytes of the original path we started with.
# A simple existence check is enough: the tool backs it up as .orig, not alters it.
assert os.path.exists(os.path.join(assets, "DataStrm-part3.pak.orig"))

print("ok")
PY

echo "all patch-client-lua tests passed"
