#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Patch the New World client's Lua script that opens the Amazon account-link
pop-up (AGID) at character select.

The target is ``lyshineui/main menu/landingscreenv2.luac`` inside the client
pak ``assets/DataStrm-part3.pak``. The function ``TryShowAGIDLink`` is a Lua
5.1 closure in that script that, once every gate passes, calls the inner
``ShowAGIDLinkModal`` broadcast. This tool replaces the body of each of
those two closures with a single ``RETURN 0 1`` instruction (return no
values), so the pop-up can never open from Lua.

The pak is a ZIP archive. Most entries are compressed with method 15
(Oodle Kraken); hundreds of entries in the same pak already use method 0
(stored), so this tool writes the patched entry back as a stored entry and
never needs the Oodle encoder. The decompressor runs as a subprocess of the
small Rust helper ``oodle-tool`` (``oozextract`` crate), which is already
built next to the research checkout.

The patched copy lives next to the original pak as
``DataStrm-part3.blighted.pak``. The official pak is never modified; the
tool backs it up to ``DataStrm-part3.pak.orig`` on the first run only.

Usage:
    scripts/patch-client-lua.py <assets-directory>
    scripts/patch-client-lua.py <assets-directory> --verify
    scripts/patch-client-lua.py <assets-directory> --revert
    scripts/patch-client-lua.py --oodle-tool <path> <assets-directory>

With ``--oodle-tool`` the caller names the Linux ``oodle-tool`` binary; the
default is ``$NW_OODLE_TOOL`` or the path next to the research checkout.
"""

import argparse
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from typing import NoReturn

# ---------------------------------------------------------------------------
# Paths and constants.
# ---------------------------------------------------------------------------

OFFICIAL_PAK_NAME = "DataStrm-part3.pak"
BACKUP_PAK_NAME = "DataStrm-part3.pak.orig"
PATCHED_PAK_NAME = "DataStrm-part3.blighted.pak"

TARGET_ENTRY = b"lyshineui/main menu/landingscreenv2.luac"

DEFAULT_OODLE_TOOL = os.path.expanduser("~/nw-re/out-charselect/q2-icon/oodle-tool/target/release/oodle-tool")

# Each Lua 5.1 function proto whose constants include ANY of these UTF-8
# strings is replaced with an early-return stub. The first string proves the
# gate-check closure (``TryShowAGIDLink``); the second proves the inner
# broadcast closure that actually opens the modal.
AGID_SIGNATURE_STRINGS = (
    b"javelin.enable-agid-account-linking",
    b"Showing AGID account link modal",
)

# ---------------------------------------------------------------------------
# ZIP format (standard, no zip64 for this pak).
# ---------------------------------------------------------------------------

EOCD_SIG = b"PK\x05\x06"
CD_SIG = b"PK\x01\x02"
LFH_SIG = b"PK\x03\x04"
EOCD_MAX_TAIL = 65557  # 22-byte EOCD + 0xFFFF comment max
EOCD_STRUCT = "<HHHHIIH"
EOCD_LEN = 22
LFH_STRUCT = "<HHHHHIIIHH"
LFH_LEN = 30
CD_STRUCT = "<HHHHHHIIIHHHHHII"
CD_LEN = 46
METHOD_STORE = 0
METHOD_KRAKEN = 15

# ---------------------------------------------------------------------------
# Lua 5.1 binary format.
# ---------------------------------------------------------------------------

LUA_SIGNATURE = b"\x1bLua"
LUA_VERSION = 0x51
LUA_FORMAT_OFFICIAL = 0x00
LUA_LITTLE_ENDIAN = 1
LUA_SIZEOF_INT = 4
LUA_SIZEOF_SIZE_T = 8
LUA_SIZEOF_INSTRUCTION = 4
LUA_SIZEOF_NUMBER = 8
LUA_INTEGRAL_FLAG = 0  # IEEE double
LUA_HEADER = struct.pack(
    "<4sBBBBBBBB",
    LUA_SIGNATURE, LUA_VERSION, LUA_FORMAT_OFFICIAL, LUA_LITTLE_ENDIAN,
    LUA_SIZEOF_INT, LUA_SIZEOF_SIZE_T, LUA_SIZEOF_INSTRUCTION,
    LUA_SIZEOF_NUMBER, LUA_INTEGRAL_FLAG,
)
LUA_HEADER_LEN = 12

# Lua 5.1 instruction layout (iABC): op (6 bits) | A (8) | C (9) | B (9).
LUA_OP_RETURN = 30
RETURN_A = 0
RETURN_B = 1  # B-1 = 0 returned values
RETURN_C = 0

# Lua 5.1 constant type tags.
LUA_TNIL = 0
LUA_TBOOLEAN = 1
LUA_TNUMBER = 3
LUA_TSTRING = 4

# Many New World luac files start with a two-byte "0x04 0x00" prefix before
# the Lua signature (Amazon's custom loader expects it). The patcher keeps
# the prefix bit-identical so the loader is not surprised.
AMAZON_LUAC_PREFIX_MAX = 8


def encode_return(a: int, b: int, c: int) -> int:
    """Pack a Lua 5.1 RETURN instruction."""
    return (
        (b & 0x1FF) << 23
        | (c & 0x1FF) << 14
        | (a & 0xFF) << 6
        | (LUA_OP_RETURN & 0x3F)
    )


EARLY_RETURN_INSTRUCTION = encode_return(RETURN_A, RETURN_B, RETURN_C)

# ---------------------------------------------------------------------------
# Error helpers.
# ---------------------------------------------------------------------------


def fail(message: str) -> NoReturn:
    sys.exit(f"error: {message}")


# ---------------------------------------------------------------------------
# ZIP reading.
# ---------------------------------------------------------------------------


class Entry:
    """One central-directory entry from the pak, already parsed."""

    __slots__ = (
        "name", "method", "mtime", "mdate", "crc", "csize", "usize",
        "lhoff", "version_made", "version_need", "flags", "iattr", "eattr",
        "extra", "comment", "cd_offset",
    )

    def __init__(self, raw: bytes, cd_offset: int) -> None:
        if raw[:4] != CD_SIG:
            raise ValueError("bad central directory signature")
        fields = struct.unpack_from(CD_STRUCT, raw, 4)
        (self.version_made, self.version_need, self.flags, self.method,
         self.mtime, self.mdate, self.crc, self.csize, self.usize,
         nlen, elen, clen, _dstart, self.iattr, self.eattr,
         self.lhoff) = fields
        start = 4 + struct.calcsize(CD_STRUCT)
        self.name = raw[start:start + nlen]
        self.extra = raw[start + nlen:start + nlen + elen]
        self.comment = raw[start + nlen + elen:start + nlen + elen + clen]
        self.cd_offset = cd_offset

    def record_length(self) -> int:
        return CD_LEN + len(self.name) + len(self.extra) + len(self.comment)

    def serialise(self) -> bytes:
        header = struct.pack(
            CD_STRUCT, self.version_made, self.version_need, self.flags,
            self.method, self.mtime, self.mdate, self.crc, self.csize,
            self.usize, len(self.name), len(self.extra), len(self.comment),
            0, self.iattr, self.eattr, self.lhoff,
        )
        return CD_SIG + header + self.name + self.extra + self.comment


def read_eocd(data: bytes) -> "tuple[int, int, int]":
    """``(cd_offset, cd_size, eocd_offset)`` for the plain (not zip64) EOCD."""
    tail_start = max(0, len(data) - EOCD_MAX_TAIL)
    tail = data[tail_start:]
    idx = tail.rfind(EOCD_SIG)
    if idx < 0:
        fail("end-of-central-directory not found in pak")
    eocd = tail_start + idx
    (_disk, _cd_disk, _entries_disk, _entries_total, cd_size, cd_off, clen
     ) = struct.unpack_from(EOCD_STRUCT, data, eocd + 4)
    if cd_off == 0xFFFFFFFF or cd_size == 0xFFFFFFFF:
        fail("pak uses zip64; this tool does not")
    if eocd + EOCD_LEN + clen != len(data):
        fail("pak has trailing bytes after EOCD")
    return cd_off, cd_size, eocd


def parse_central_directory(data: bytes, cd_off: int, cd_size: int
                            ) -> "list[Entry]":
    entries = []
    offset = cd_off
    end = cd_off + cd_size
    while offset < end:
        if data[offset:offset + 4] != CD_SIG:
            fail(f"bad central directory record at 0x{offset:X}")
        peek_fields = struct.unpack_from(CD_STRUCT, data, offset + 4)
        nlen = peek_fields[9]
        elen = peek_fields[10]
        clen = peek_fields[11]
        length = CD_LEN + nlen + elen + clen
        entries.append(Entry(data[offset:offset + length], offset))
        offset += length
    if offset != end:
        fail("central directory length does not match its records")
    return entries


def find_target_entry(entries: "list[Entry]", name: bytes) -> Entry:
    matches = [e for e in entries if e.name.lower() == name.lower()]
    if not matches:
        fail(f"entry not found in pak: {name.decode(errors='replace')}")
    if len(matches) > 1:
        fail(f"entry appears {len(matches)} times in pak: "
             f"{name.decode(errors='replace')}")
    return matches[0]


def read_local_file_header(data: bytes, lhoff: int
                           ) -> "tuple[int, int, int]":
    """``(data_offset, nlen, elen)`` for the local file header at ``lhoff``."""
    if data[lhoff:lhoff + 4] != LFH_SIG:
        fail(f"bad local file header at 0x{lhoff:X}")
    (_vneed, _flags, _method, _mtime, _mdate, _crc, _csize, _usize, nlen, elen
     ) = struct.unpack_from(LFH_STRUCT, data, lhoff + 4)
    return lhoff + LFH_LEN + nlen + elen, nlen, elen


# ---------------------------------------------------------------------------
# Oodle Kraken decompression (external Linux helper).
# ---------------------------------------------------------------------------


def resolve_oodle_tool(cli_path: "str | None") -> str:
    beside_script = os.path.join(os.path.dirname(os.path.abspath(__file__)), "oodle-tool")
    for candidate in (cli_path, os.environ.get("NW_OODLE_TOOL"), beside_script, DEFAULT_OODLE_TOOL):
        if candidate and os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
    fail(f"oodle-tool not found or not executable (looked beside the script, "
         f"in $NW_OODLE_TOOL, and at {DEFAULT_OODLE_TOOL})")


def oodle_decompress(oodle_tool: str, compressed: bytes, usize: int) -> bytes:
    with tempfile.TemporaryDirectory() as workspace:
        src = os.path.join(workspace, "in.comp")
        dst = os.path.join(workspace, "out.bin")
        with open(src, "wb") as handle:
            handle.write(compressed)
        result = subprocess.run(
            [oodle_tool, src, dst, str(usize)],
            capture_output=True,
        )
        if result.returncode != 0:
            fail("oodle-tool failed: "
                 f"{result.stderr.decode(errors='replace').strip()}")
        with open(dst, "rb") as handle:
            return handle.read()


# ---------------------------------------------------------------------------
# Lua 5.1 binary parser and serialiser (round-trip).
# ---------------------------------------------------------------------------


class LuaReader:
    __slots__ = ("data", "offset")

    def __init__(self, data: bytes, offset: int) -> None:
        self.data = data
        self.offset = offset

    def take(self, length: int) -> bytes:
        end = self.offset + length
        if end > len(self.data):
            fail("Lua binary ended unexpectedly")
        out = self.data[self.offset:end]
        self.offset = end
        return out

    def u8(self) -> int:
        return self.take(1)[0]

    def i32(self) -> int:
        return struct.unpack("<i", self.take(LUA_SIZEOF_INT))[0]

    def size_t(self) -> int:
        return struct.unpack("<Q", self.take(LUA_SIZEOF_SIZE_T))[0]

    def u32(self) -> int:
        return struct.unpack("<I", self.take(LUA_SIZEOF_INSTRUCTION))[0]

    def f64(self) -> float:
        return struct.unpack("<d", self.take(LUA_SIZEOF_NUMBER))[0]

    def lua_string(self) -> bytes:
        length = self.size_t()
        if length == 0:
            return b""
        return self.take(length)


class LuaProto:
    """One Lua 5.1 function prototype, in a form we can edit and write back."""

    __slots__ = (
        "source", "linedefined", "lastlinedefined", "nups", "nparams",
        "is_vararg", "maxstack", "code", "constants", "protos", "lines",
        "locals", "upnames",
    )

    def __init__(self) -> None:
        self.source: bytes = b""
        self.linedefined = 0
        self.lastlinedefined = 0
        self.nups = 0
        self.nparams = 0
        self.is_vararg = 0
        self.maxstack = 2
        self.code: "list[int]" = []
        self.constants: "list[object]" = []
        self.protos: "list[LuaProto]" = []
        self.lines: "list[int]" = []
        self.locals: "list[tuple[bytes, int, int]]" = []
        self.upnames: "list[bytes]" = []


def check_lua_header(data: bytes) -> int:
    """Return the offset of the Amazon prefix end (= start of Lua signature).

    Many client luac files start with ``\\x04\\x00`` before the Lua
    signature. The tool preserves whatever prefix it sees, checks the Lua
    header for the known shape and reports its length.
    """
    sig_index = data.find(LUA_SIGNATURE)
    if sig_index < 0 or sig_index > AMAZON_LUAC_PREFIX_MAX:
        fail("Lua 5.1 signature not found at the start of the script")
    expected = LUA_HEADER
    if data[sig_index:sig_index + LUA_HEADER_LEN] != expected:
        fail("Lua 5.1 header does not match the known shape")
    return sig_index


def read_proto(reader: LuaReader) -> LuaProto:
    proto = LuaProto()
    proto.source = reader.lua_string()
    proto.linedefined = reader.i32()
    proto.lastlinedefined = reader.i32()
    proto.nups = reader.u8()
    proto.nparams = reader.u8()
    proto.is_vararg = reader.u8()
    proto.maxstack = reader.u8()
    sizecode = reader.i32()
    proto.code = [reader.u32() for _ in range(sizecode)]
    sizek = reader.i32()
    for _ in range(sizek):
        tag = reader.u8()
        if tag == LUA_TNIL:
            proto.constants.append(None)
        elif tag == LUA_TBOOLEAN:
            proto.constants.append(bool(reader.u8()))
        elif tag == LUA_TNUMBER:
            proto.constants.append(reader.f64())
        elif tag == LUA_TSTRING:
            proto.constants.append(reader.lua_string())
        else:
            fail(f"unknown Lua constant tag {tag} at offset {reader.offset}")
    sizep = reader.i32()
    proto.protos = [read_proto(reader) for _ in range(sizep)]
    sizeline = reader.i32()
    proto.lines = [reader.i32() for _ in range(sizeline)]
    sizeloc = reader.i32()
    for _ in range(sizeloc):
        name = reader.lua_string()
        start = reader.i32()
        end = reader.i32()
        proto.locals.append((name, start, end))
    sizeup = reader.i32()
    proto.upnames = [reader.lua_string() for _ in range(sizeup)]
    return proto


def write_lua_string(buffer: bytearray, value: bytes) -> None:
    if not value:
        buffer += struct.pack("<Q", 0)
        return
    buffer += struct.pack("<Q", len(value))
    buffer += value


def write_proto(buffer: bytearray, proto: LuaProto) -> None:
    write_lua_string(buffer, proto.source)
    buffer += struct.pack("<ii", proto.linedefined, proto.lastlinedefined)
    buffer += bytes((proto.nups, proto.nparams, proto.is_vararg,
                     proto.maxstack))
    buffer += struct.pack("<i", len(proto.code))
    for instruction in proto.code:
        buffer += struct.pack("<I", instruction & 0xFFFFFFFF)
    buffer += struct.pack("<i", len(proto.constants))
    for value in proto.constants:
        if value is None:
            buffer += bytes((LUA_TNIL,))
        elif isinstance(value, bool):
            buffer += bytes((LUA_TBOOLEAN, 1 if value else 0))
        elif isinstance(value, float):
            buffer += bytes((LUA_TNUMBER,)) + struct.pack("<d", value)
        elif isinstance(value, bytes):
            buffer += bytes((LUA_TSTRING,))
            write_lua_string(buffer, value)
        else:
            fail(f"cannot serialise Lua constant of type {type(value).__name__}")
    buffer += struct.pack("<i", len(proto.protos))
    for child in proto.protos:
        write_proto(buffer, child)
    buffer += struct.pack("<i", len(proto.lines))
    for line in proto.lines:
        buffer += struct.pack("<i", line)
    buffer += struct.pack("<i", len(proto.locals))
    for name, start, end in proto.locals:
        write_lua_string(buffer, name)
        buffer += struct.pack("<ii", start, end)
    buffer += struct.pack("<i", len(proto.upnames))
    for name in proto.upnames:
        write_lua_string(buffer, name)


def walk_protos(proto: LuaProto) -> "list[LuaProto]":
    out = [proto]
    for child in proto.protos:
        out.extend(walk_protos(child))
    return out


def stub_proto(proto: LuaProto) -> None:
    """Replace ``proto``'s body with a one-instruction early return.

    ``nups``, ``nparams`` and ``is_vararg`` stay the same so callers that
    already closed over this proto keep working. Upvalue debug names are
    cleared along with every other debug array."""
    proto.code = [EARLY_RETURN_INSTRUCTION]
    proto.maxstack = max(proto.maxstack, proto.nparams + 1)
    proto.lines = [proto.linedefined]
    proto.locals = []
    proto.upnames = []


def plan_lua_patches(root: LuaProto, needles: "tuple[bytes, ...]"
                     ) -> "list[tuple[int, LuaProto, bytes]]":
    """Each proto that holds any needle as a string constant, paired with
    the needle that matched. Returned in walk order."""
    plan = []
    for index, proto in enumerate(walk_protos(root)):
        for needle in needles:
            if any(isinstance(k, bytes) and needle in k
                   for k in proto.constants):
                plan.append((index, proto, needle))
                break
    return plan


def patch_lua_script(raw: bytes, needles: "tuple[bytes, ...]"
                     ) -> "tuple[bytes, list[tuple[int, bytes]]]":
    """Rewrite each matched proto's body to RETURN. Return the full new
    luac bytes and the ``(walk_index, needle)`` list of touched protos."""
    prefix_end = check_lua_header(raw)
    reader = LuaReader(raw, prefix_end + LUA_HEADER_LEN)
    root = read_proto(reader)
    if reader.offset != len(raw):
        fail(f"trailing bytes after Lua script: {len(raw) - reader.offset}")
    plan = plan_lua_patches(root, needles)
    if not plan:
        fail("no AGID-related Lua function found in the script")
    touched = []
    for index, proto, needle in plan:
        stub_proto(proto)
        touched.append((index, needle))
    out = bytearray(raw[:prefix_end])
    out += LUA_HEADER
    write_proto(out, root)
    return bytes(out), touched


# ---------------------------------------------------------------------------
# Pak rewriting.
# ---------------------------------------------------------------------------


def build_patched_pak(original_data: bytes, cd_off: int, cd_size: int,
                      eocd_offset: int, entries: "list[Entry]",
                      target: Entry, new_entry_bytes: bytes
                      ) -> bytes:
    """Build the patched pak:

    1. Keep everything before the original central directory verbatim. The
       patched entry's old local header and compressed data stay in place
       as dead space — the pak loader follows the central directory, never
       the dead header, so this is safe.
    2. Append a new local file header + the new STORED script body right
       where the original central directory starts.
    3. Rewrite the central directory with the one patched entry updated.
    4. Append a fresh EOCD.
    """
    fixed_prefix = original_data[:cd_off]
    new_lhoff = len(fixed_prefix)

    new_crc = zlib.crc32(new_entry_bytes) & 0xFFFFFFFF
    new_csize = len(new_entry_bytes)
    new_usize = len(new_entry_bytes)

    lfh = struct.pack(
        LFH_STRUCT,
        target.version_need, target.flags, METHOD_STORE,
        target.mtime, target.mdate, new_crc, new_csize, new_usize,
        len(target.name), len(target.extra),
    )
    lfh_record = LFH_SIG + lfh + target.name + target.extra

    patched_target = Entry.__new__(Entry)
    patched_target.name = target.name
    patched_target.method = METHOD_STORE
    patched_target.mtime = target.mtime
    patched_target.mdate = target.mdate
    patched_target.crc = new_crc
    patched_target.csize = new_csize
    patched_target.usize = new_usize
    patched_target.lhoff = new_lhoff
    patched_target.version_made = target.version_made
    patched_target.version_need = target.version_need
    patched_target.flags = target.flags
    patched_target.iattr = target.iattr
    patched_target.eattr = target.eattr
    patched_target.extra = target.extra
    patched_target.comment = target.comment
    patched_target.cd_offset = 0

    new_cd = bytearray()
    for entry in entries:
        if entry is target:
            new_cd += patched_target.serialise()
        else:
            new_cd += entry.serialise()
    new_cd_off = new_lhoff + len(lfh_record) + len(new_entry_bytes)

    eocd_tail = original_data[eocd_offset + EOCD_LEN:]
    # Preserve the archive comment that followed the old EOCD, if any.
    (_disk, _cd_disk, _entries_disk, _entries_total, _cd_size_old, _cd_off_old,
     clen) = struct.unpack_from(EOCD_STRUCT, original_data, eocd_offset + 4)
    comment = eocd_tail[:clen]

    new_eocd = struct.pack(
        EOCD_STRUCT, 0, 0, len(entries), len(entries),
        len(new_cd), new_cd_off, len(comment),
    )
    return fixed_prefix + lfh_record + new_entry_bytes + bytes(new_cd) + EOCD_SIG + new_eocd + comment


def verify_patched_pak(patched_data: bytes, needles: "tuple[bytes, ...]",
                       oodle_tool: str, expect_method_store: bool) -> bool:
    """Re-open the patched pak, pull the entry back, parse it, prove every
    matched proto's body is the one-instruction early-return."""
    cd_off, cd_size, _eocd = read_eocd(patched_data)
    entries = parse_central_directory(patched_data, cd_off, cd_size)
    target = find_target_entry(entries, TARGET_ENTRY)
    data_offset, _nlen, _elen = read_local_file_header(patched_data, target.lhoff)
    compressed = patched_data[data_offset:data_offset + target.csize]
    if target.method == METHOD_STORE:
        decoded = compressed
    elif target.method == METHOD_KRAKEN:
        decoded = oodle_decompress(oodle_tool, compressed, target.usize)
    else:
        fail(f"patched entry has unexpected method {target.method}")
    if expect_method_store and target.method != METHOD_STORE:
        return False
    if zlib.crc32(decoded) & 0xFFFFFFFF != target.crc:
        return False
    if len(decoded) != target.usize:
        return False
    prefix_end = check_lua_header(decoded)
    reader = LuaReader(decoded, prefix_end + LUA_HEADER_LEN)
    root = read_proto(reader)
    if reader.offset != len(decoded):
        return False
    protos = walk_protos(root)
    found_any = False
    for proto in protos:
        for needle in needles:
            if any(isinstance(k, bytes) and needle in k
                   for k in proto.constants):
                found_any = True
                if (len(proto.code) != 1
                        or proto.code[0] != EARLY_RETURN_INSTRUCTION):
                    return False
                break
    return found_any


# ---------------------------------------------------------------------------
# Top-level flows.
# ---------------------------------------------------------------------------


def make_patched(official: str, backup: str, patched: str,
                 oodle_tool: str) -> None:
    with open(official, "rb") as handle:
        data = handle.read()
    cd_off, cd_size, eocd = read_eocd(data)
    entries = parse_central_directory(data, cd_off, cd_size)
    target = find_target_entry(entries, TARGET_ENTRY)
    data_offset, _nlen, _elen = read_local_file_header(data, target.lhoff)
    compressed = data[data_offset:data_offset + target.csize]
    if target.method == METHOD_STORE:
        decoded = compressed
    elif target.method == METHOD_KRAKEN:
        decoded = oodle_decompress(oodle_tool, compressed, target.usize)
    else:
        fail(f"target entry uses unexpected method {target.method}")
    if zlib.crc32(decoded) & 0xFFFFFFFF != target.crc:
        fail("decoded CRC does not match the central directory CRC")
    if len(decoded) != target.usize:
        fail("decoded length does not match the central directory length")
    patched_bytes, touched = patch_lua_script(decoded, AGID_SIGNATURE_STRINGS)
    for index, needle in touched:
        print(f"patched Lua proto {index} (matched {needle.decode()!r})")

    if not os.path.exists(backup):
        shutil.copy2(official, backup)
        print(f"backup written: {backup}")
    else:
        print(f"backup kept: {backup}")

    new_pak = build_patched_pak(data, cd_off, cd_size, eocd, entries,
                                target, patched_bytes)
    temporary = patched + ".tmp"
    with open(temporary, "wb") as handle:
        handle.write(new_pak)
        handle.flush()
        os.fsync(handle.fileno())
    shutil.copymode(official, temporary)
    os.replace(temporary, patched)

    with open(patched, "rb") as handle:
        patched_data = handle.read()
    if not verify_patched_pak(patched_data, AGID_SIGNATURE_STRINGS,
                              oodle_tool, expect_method_store=True):
        sys.exit("error: verification of the patched pak failed")
    print(f"wrote {patched} ({len(touched)} proto(s) stubbed)")


def verify_patched(official: str, patched: str, oodle_tool: str) -> int:
    if not os.path.isfile(patched):
        print(f"missing: {patched}")
        return 1
    with open(patched, "rb") as handle:
        patched_data = handle.read()
    good = verify_patched_pak(patched_data, AGID_SIGNATURE_STRINGS,
                              oodle_tool, expect_method_store=True)
    print("patched pak:", "OK" if good else "WRONG")
    return 0 if good else 1


def revert(patched: str) -> None:
    if os.path.exists(patched):
        os.remove(patched)
        print(f"removed: {patched}")
    else:
        print("nothing to remove")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Stub the AGID Lua closures in the New World client pak.",
    )
    parser.add_argument("assets", help="the game assets directory holding "
                        f"{OFFICIAL_PAK_NAME}")
    parser.add_argument("--oodle-tool", help="path to the Linux oodle-tool "
                        "binary (oozextract); default $NW_OODLE_TOOL or the "
                        "one next to the research checkout")
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--verify", action="store_true",
                       help="only check the patched pak; write nothing")
    group.add_argument("--revert", action="store_true",
                       help="delete the patched pak (keeps the official one)")
    arguments = parser.parse_args()

    official = os.path.join(arguments.assets, OFFICIAL_PAK_NAME)
    backup = os.path.join(arguments.assets, BACKUP_PAK_NAME)
    patched = os.path.join(arguments.assets, PATCHED_PAK_NAME)
    oodle_tool = resolve_oodle_tool(arguments.oodle_tool)

    if arguments.revert:
        revert(patched)
        return
    if not os.path.isfile(official):
        sys.exit(f"error: not found: {official}")
    if arguments.verify:
        sys.exit(verify_patched(official, patched, oodle_tool))
    make_patched(official, backup, patched, oodle_tool)


if __name__ == "__main__":
    main()
