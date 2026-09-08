#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Make a certificate-verify-bypassed copy of the New World client.

The real client refuses the emulator's self-signed game-server certificate and
fails world entry with a transport security error. This tool writes a patched
copy, ``BlightedNewWorld.exe``, next to the official ``NewWorld.exe`` (which it
never modifies). The patched copy does not verify the server certificate, so it
accepts the emulator gateway.

The patch finds the OpenSSL ``SSL_CTX_set_verify`` function by a fixed byte
signature rather than a hard-coded address, so it keeps working when a game
update moves the code. It then replaces the six-byte ``verify_mode`` write with
``NOP`` instructions. A fresh ``SSL_CTX`` starts with ``verify_mode`` 0
(``SSL_VERIFY_NONE``); leaving the write out keeps it there, so no certificate
is verified. The rest of the function (the verify-callback write and the
``RET``) is untouched, and the file length does not change.

See ``docs/client-patch.md`` for the full procedure, the reasoning, and how to
re-derive the signature if a future build changes it.

Usage:
    scripts/patch-client.py <Bin64-directory>
    scripts/patch-client.py <Bin64-directory> --verify
    scripts/patch-client.py <Bin64-directory> --revert
"""

import argparse
import os
import shutil
import sys

# SSL_CTX_set_verify(ctx, mode, callback):
#   MOV dword ptr [RCX + 0x158], EDX   89 91 58 01 00 00   <- the verify_mode write (patched out)
#   MOV qword ptr [RCX + 0x188], R8    4C 89 81 88 01 00 00
#   RET                                C3
SIGNATURE = bytes.fromhex("899158010000" "4c898188010000" "c3")
# The leading instruction we replace (the verify_mode write).
VERIFY_MODE_WRITE = bytes.fromhex("899158010000")
NOPS = b"\x90" * len(VERIFY_MODE_WRITE)

OFFICIAL_NAME = "NewWorld.exe"
BACKUP_NAME = "NewWorld.exe.orig"
PATCHED_NAME = "BlightedNewWorld.exe"

SEE_DOC = "see docs/client-patch.md"


def find_patch_site(data: bytes) -> int:
    """Offset of the single `SSL_CTX_set_verify` signature, or exit with an error."""
    first = data.find(SIGNATURE)
    if first < 0:
        sys.exit(
            f"error: patch signature not found; the client build may have changed ({SEE_DOC})"
        )
    if data.find(SIGNATURE, first + 1) >= 0:
        sys.exit(f"error: patch signature found more than once; aborting ({SEE_DOC})")
    return first


def read_file(path: str) -> bytes:
    with open(path, "rb") as handle:
        return handle.read()


def make_patched(official: str, backup: str, patched: str) -> None:
    """Back up the official exe once and write the patched copy from its bytes."""
    original = read_file(official)
    site = find_patch_site(original)
    print(f"patch site at file offset 0x{site:X}")

    if not os.path.exists(backup):
        shutil.copy2(official, backup)
        print(f"backup written: {backup}")
    else:
        print(f"backup kept: {backup}")

    buffer = bytearray(original)
    buffer[site : site + len(VERIFY_MODE_WRITE)] = NOPS
    temporary = patched + ".tmp"
    with open(temporary, "wb") as handle:
        handle.write(buffer)
        handle.flush()
        os.fsync(handle.fileno())
    shutil.copymode(official, temporary)
    os.replace(temporary, patched)

    check = read_file(patched)
    if len(check) != len(original) or check[site : site + len(NOPS)] != NOPS:
        sys.exit("error: verification of the patched copy failed")
    print(f"wrote {patched} (certificate verification bypassed)")


def verify_patched(official: str, patched: str) -> int:
    """Return 0 when the patched copy is present and correctly patched, else 1."""
    if not os.path.isfile(patched):
        print(f"missing: {patched}")
        return 1
    original = read_file(official)
    site = find_patch_site(original)
    data = read_file(patched)
    good = len(data) == len(original) and data[site : site + len(NOPS)] == NOPS
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
        description="Make BlightedNewWorld.exe, a certificate-verify-bypassed client copy."
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
