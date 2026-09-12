#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Make the patched copy of the New World client that the emulator needs.

This tool writes a patched copy, ``BlightedNewWorld.exe``, next to the official
``NewWorld.exe`` (which it never modifies). The copy has two changes:

1. Certificate bypass. The real client refuses the emulator's self-signed
   game-server certificate. The tool finds OpenSSL ``SSL_CTX_set_verify`` by a
   fixed byte signature and replaces its six-byte ``verify_mode`` write with
   ``NOP`` instructions. A fresh ``SSL_CTX`` starts with ``verify_mode`` 0
   (``SSL_VERIFY_NONE``), so no certificate is verified.

2. Exit to desktop. The Steam auth manager's ``EndSession`` only stores the quit
   callback and waits for the Omni persona client, which the emulator setup does
   not have, so "Exit to desktop" hangs. The Sonic auth manager's ``EndSession``
   runs the callback at once. The tool points the Steam ``EndSession`` vtable
   slot at the Sonic function (one 8-byte pointer in ``.rdata``). Both functions
   are found by byte signatures; the vtable slot is the single pointer to the
   Steam function.

Neither change alters the file length. See ``docs/client-patch.md`` for the
procedure, the reasoning, and how to re-derive a signature if a build changes it.

Usage:
    scripts/patch-client.py <Bin64-directory>
    scripts/patch-client.py <Bin64-directory> --verify
    scripts/patch-client.py <Bin64-directory> --revert
"""

import argparse
import os
import re
import shutil
import struct
import sys
from typing import NoReturn

# SSL_CTX_set_verify(ctx, mode, callback):
#   MOV dword ptr [RCX + 0x158], EDX   89 91 58 01 00 00   <- the verify_mode write (patched out)
#   MOV qword ptr [RCX + 0x188], R8    4C 89 81 88 01 00 00
#   RET                                C3
SIGNATURE = bytes.fromhex("899158010000" "4c898188010000" "c3")
# The leading instruction we replace (the verify_mode write).
VERIFY_MODE_WRITE = bytes.fromhex("899158010000")
NOPS = b"\x90" * len(VERIFY_MODE_WRITE)

# Sonic auth manager EndSession (build 25810808: FUN_145a8d310): calls the quit callback, sets
# the flag at this+0x58, then frees the callback. The whole body has no address in it.
SONIC_END_SESSION = bytes.fromhex(
    "48895c2408" "4889742418" "4889542410" "57" "4883ec20" "488bfa" "488bd9"
    "488b02" "4c8b4008" "488d4a08" "41ffd0" "c6435801" "488b07" "4885c0" "741f"
    "488b00" "4885c0" "7410" "41b802000000" "488d5708" "488d4f08" "ffd0"
    "48c70700000000" "488b5c2430" "488b742440" "4883c420" "5f" "c3"
)
# Steam auth manager EndSession (build 25810808: FUN_145a8d380): stores the callback at
# this+0x30, calls the persona client at this+0x228 when it is set, and never runs the callback.
# The four bytes after E8 are the build-specific call offset.
STEAM_END_SESSION = re.compile(
    re.escape(bytes.fromhex("48895c2408" "4889542410" "57" "4883ec20" "488bfa" "488bd9" "4883c130"))
    + b"\xe8.{4}"
    + re.escape(bytes.fromhex("488b8b28020000" "4885c9" "7407" "488b01" "ff5030")),
    re.DOTALL,
)
POINTER_LEN = 8

OFFICIAL_NAME = "NewWorld.exe"
BACKUP_NAME = "NewWorld.exe.orig"
PATCHED_NAME = "BlightedNewWorld.exe"

SEE_DOC = "see docs/client-patch.md"

# PE layout.
PE_OFFSET_FIELD = 0x3C
FILE_HEADER_LEN = 24
SECTION_COUNT_FIELD = 6
OPTIONAL_HEADER_SIZE_FIELD = 20
IMAGE_BASE_FIELD = 24
SECTION_HEADER_LEN = 40
SECTION_FIELDS_OFFSET = 8


def fail(message: str) -> NoReturn:
    sys.exit(f"error: {message} ({SEE_DOC})")


def find_unique(data: bytes, pattern: "bytes | re.Pattern[bytes]", what: str) -> int:
    """Offset of the single match of `pattern`, or exit with an error."""
    if isinstance(pattern, bytes):
        pattern = re.compile(re.escape(pattern), re.DOTALL)
    matches = [match.start() for match in pattern.finditer(data)]
    if not matches:
        fail(f"{what} signature not found; the client build may have changed")
    if len(matches) > 1:
        fail(f"{what} signature found more than once; aborting")
    return matches[0]


def virtual_address(data: bytes, file_offset: int) -> int:
    """The virtual address of `file_offset` in the PE image `data`, or exit with an error."""
    try:
        pe = struct.unpack_from("<I", data, PE_OFFSET_FIELD)[0]
        sections = struct.unpack_from("<H", data, pe + SECTION_COUNT_FIELD)[0]
        optional_size = struct.unpack_from("<H", data, pe + OPTIONAL_HEADER_SIZE_FIELD)[0]
        image_base = struct.unpack_from("<Q", data, pe + FILE_HEADER_LEN + IMAGE_BASE_FIELD)[0]
        header = pe + FILE_HEADER_LEN + optional_size
        for index in range(sections):
            entry = header + index * SECTION_HEADER_LEN + SECTION_FIELDS_OFFSET
            virtual_size, rva, raw_size, raw_start = struct.unpack_from("<IIII", data, entry)
            if raw_start <= file_offset < raw_start + raw_size:
                return image_base + rva + file_offset - raw_start
    except struct.error:
        fail("not a readable PE file")
    fail(f"file offset 0x{file_offset:X} is in no section")


def plan_patches(data: bytes) -> "list[tuple[str, int, bytes]]":
    """The (name, file offset, new bytes) edits that make the patched copy from `data`."""
    verify_site = find_unique(data, SIGNATURE, "certificate-verify")
    sonic = virtual_address(data, find_unique(data, SONIC_END_SESSION, "Sonic EndSession"))
    steam = virtual_address(data, find_unique(data, STEAM_END_SESSION, "Steam EndSession"))
    slot = find_unique(data, struct.pack("<Q", steam), "Steam EndSession vtable slot")
    return [
        ("certificate bypass", verify_site, NOPS),
        ("exit to desktop", slot, struct.pack("<Q", sonic)),
    ]


def read_file(path: str) -> bytes:
    with open(path, "rb") as handle:
        return handle.read()


def is_patched(original: bytes, data: bytes) -> bool:
    """Whether `data` is `original` with every planned edit."""
    if len(data) != len(original):
        return False
    return all(
        data[site : site + len(new)] == new for _, site, new in plan_patches(original)
    )


def make_patched(official: str, backup: str, patched: str) -> None:
    """Back up the official exe once and write the patched copy from its bytes."""
    original = read_file(official)
    edits = plan_patches(original)
    for name, site, _ in edits:
        print(f"{name}: patch site at file offset 0x{site:X}")

    if not os.path.exists(backup):
        shutil.copy2(official, backup)
        print(f"backup written: {backup}")
    else:
        print(f"backup kept: {backup}")

    buffer = bytearray(original)
    for _, site, new in edits:
        buffer[site : site + len(new)] = new
    temporary = patched + ".tmp"
    with open(temporary, "wb") as handle:
        handle.write(buffer)
        handle.flush()
        os.fsync(handle.fileno())
    shutil.copymode(official, temporary)
    os.replace(temporary, patched)

    if not is_patched(original, read_file(patched)):
        sys.exit("error: verification of the patched copy failed")
    print(f"wrote {patched} (certificate bypass, exit to desktop)")


def verify_patched(official: str, patched: str) -> int:
    """Return 0 when the patched copy is present and correctly patched, else 1."""
    if not os.path.isfile(patched):
        print(f"missing: {patched}")
        return 1
    good = is_patched(read_file(official), read_file(patched))
    print("patched copy:", "OK" if good else "WRONG")
    return 0 if good else 1


def revert(patched: str) -> None:
    if os.path.exists(patched):
        os.remove(patched)
        print(f"removed: {patched}")
    else:
        print("nothing to remove")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Make BlightedNewWorld.exe, the patched client copy the emulator needs."
    )
    parser.add_argument("bin64", help="the game Bin64 directory that holds NewWorld.exe")
    group = parser.add_mutually_exclusive_group()
    group.add_argument(
        "--verify", action="store_true", help="only check the patched copy; write nothing"
    )
    group.add_argument(
        "--revert", action="store_true", help="delete the patched copy (keeps the official exe)"
    )
    arguments = parser.parse_args()

    official = os.path.join(arguments.bin64, OFFICIAL_NAME)
    backup = os.path.join(arguments.bin64, BACKUP_NAME)
    patched = os.path.join(arguments.bin64, PATCHED_NAME)

    if arguments.revert:
        revert(patched)
        return
    if not os.path.isfile(official):
        sys.exit(f"error: not found: {official}")
    if arguments.verify:
        sys.exit(verify_patched(official, patched))
    make_patched(official, backup, patched)


if __name__ == "__main__":
    main()
