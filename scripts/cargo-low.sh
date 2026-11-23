#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Runs one cargo command so that it cannot freeze the game servers on the same PC: one build
# at a time under the shared build lock, at the lowest CPU and disk priority, and in its own
# memory group that the kernel slows down above a memory cap. On 2026-11-22 sim builds next to
# the running stack froze sim-1 for up to 11 s and dropped every player.
#
# Usage: scripts/cargo-low.sh <cargo arguments>, for example
#   scripts/cargo-low.sh build --release -j 8 -p sim
# Environment:
#   CARGO_LOW_MEMORY_HIGH  the memory cap of the build's group (default 16G)
#   CARGO_LOW_SCOPE        0: no memory group (for a machine without a user systemd)
#   CARGO_LOW_LOCK         the lock file (default target/.cargo-build.lock)
#   CARGO_LOW_CARGO        the cargo program (default cargo; the tests use a stand-in)
set -euo pipefail

readonly EXIT_USAGE=2
readonly LOCK_WAIT_S=3600
readonly LOWEST_NICE=19
readonly IDLE_IO_CLASS=3
readonly LOW_WEIGHT=20
readonly DEFAULT_MEMORY_HIGH="16G"
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_DIR="$(dirname "$SCRIPT_DIR")"

if [[ $# -eq 0 ]]; then
  echo "usage: scripts/cargo-low.sh <cargo arguments>" >&2
  exit "$EXIT_USAGE"
fi

lock="${CARGO_LOW_LOCK:-$REPO_DIR/target/.cargo-build.lock}"
cargo_program="${CARGO_LOW_CARGO:-cargo}"
memory_high="${CARGO_LOW_MEMORY_HIGH:-$DEFAULT_MEMORY_HIGH}"
mkdir -p "$(dirname "$lock")"

low=(nice -n "$LOWEST_NICE" ionice -c "$IDLE_IO_CLASS" "$cargo_program" "$@")

use_scope() {
  [[ "${CARGO_LOW_SCOPE:-1}" != 0 ]] \
    && command -v systemd-run >/dev/null \
    && systemctl --user show-environment >/dev/null 2>&1
}

if use_scope; then
  exec flock -w "$LOCK_WAIT_S" "$lock" systemd-run --user --scope --quiet \
    -p MemoryHigh="$memory_high" -p CPUWeight="$LOW_WEIGHT" -p IOWeight="$LOW_WEIGHT" \
    "${low[@]}"
fi
exec flock -w "$LOCK_WAIT_S" "$lock" "${low[@]}"
