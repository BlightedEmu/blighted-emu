#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Prints the size of a Cargo target folder. Exits 2 when it is above the limit (spec 10.6).
# Usage: target-size.sh [target-folder]   (default: target)
# TARGET_SIZE_LIMIT_GIB overrides the limit (used by the tests).
set -euo pipefail

readonly DEFAULT_LIMIT_GIB=10
readonly BYTES_PER_GIB=$((1024 * 1024 * 1024))
readonly EXIT_OVER_LIMIT=2
readonly LIMIT_GIB="${TARGET_SIZE_LIMIT_GIB:-$DEFAULT_LIMIT_GIB}"

target_dir="${1:-target}"

if [[ ! -d "$target_dir" ]]; then
  echo "target size: 0.00 GiB (no folder at $target_dir)"
  exit 0
fi

size_bytes="$(du -sb "$target_dir" | cut -f1)"
size_gib="$(awk -v bytes="$size_bytes" -v per_gib="$BYTES_PER_GIB" 'BEGIN { printf "%.2f", bytes / per_gib }')"
echo "target size: ${size_gib} GiB (limit ${LIMIT_GIB} GiB)"

if (( size_bytes > LIMIT_GIB * BYTES_PER_GIB )); then
  echo "WARNING: $target_dir is above ${LIMIT_GIB} GiB. Run 'just clean-old'." >&2
  exit "$EXIT_OVER_LIMIT"
fi
