#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Write the ``latest.json`` of a launcher release.

The launcher's updater reads this file from the newest GitHub Release. It names
the new version, the release notes, and for each system the download address
of the update file and its signature (the ``.sig`` file the Tauri build writes
beside it). The release workflow (``.github/workflows/launcher-release.yml``)
runs this script after both builds pass.

A missing update file or signature is an error: a release without it would
break the update of that system.
"""

import argparse
import json
import sys
from pathlib import Path

# The updater's names of the two systems the launcher is built for.
WINDOWS_PLATFORM = "windows-x86_64"
LINUX_PLATFORM = "linux-x86_64"
# The signature file beside each update file.
SIGNATURE_SUFFIX = ".sig"
# Exit code of a missing file.
EXIT_MISSING = 1


def platform_entry(update_file: Path, base_url: str) -> dict:
    """The download address and the signature of one update file."""
    signature_file = update_file.with_name(update_file.name + SIGNATURE_SUFFIX)
    if not update_file.is_file():
        raise FileNotFoundError(f"update file missing: {update_file}")
    if not signature_file.is_file():
        raise FileNotFoundError(f"signature missing: {signature_file}")
    return {
        "signature": signature_file.read_text(encoding="utf-8").strip(),
        "url": f"{base_url.rstrip('/')}/{update_file.name}",
    }


def manifest(version: str, notes: str, pub_date: str, base_url: str,
             windows: Path, linux: Path) -> dict:
    """The content of ``latest.json``."""
    return {
        "version": version,
        "notes": notes,
        "pub_date": pub_date,
        "platforms": {
            WINDOWS_PLATFORM: platform_entry(windows, base_url),
            LINUX_PLATFORM: platform_entry(linux, base_url),
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--version", required=True, help="launcher version, for example 0.1.0")
    parser.add_argument("--notes", default="", help="release notes, one '- ' line per change")
    parser.add_argument("--pub-date", required=True, help="release time, RFC 3339")
    parser.add_argument("--base-url", required=True, help="download address of the release files")
    parser.add_argument("--windows", required=True, type=Path, help="the Windows setup file")
    parser.add_argument("--linux", required=True, type=Path, help="the Linux AppImage")
    parser.add_argument("--out", required=True, type=Path, help="the latest.json to write")
    args = parser.parse_args()
    try:
        content = manifest(args.version, args.notes, args.pub_date, args.base_url,
                           args.windows, args.linux)
    except FileNotFoundError as error:
        print(f"error: {error}", file=sys.stderr)
        return EXIT_MISSING
    args.out.write_text(json.dumps(content, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
