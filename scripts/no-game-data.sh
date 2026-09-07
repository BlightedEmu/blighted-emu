#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Fails when git tracks game files, content bundles, world files, textures or capture ledgers
# (M1 spec section 6, M1b spec section 9, admin tools spec section 7). A ledger holds other
# players' data, or the login token of our own test session.
# Usage: no-game-data.sh [repository-folder]   (default: current folder)
set -euo pipefail

readonly BUNDLE_MAGIC="NWCB"
readonly WORLD_MAGIC="NWWF"
readonly LEDGER_MAGIC="NWDL"
readonly MAGIC_LENGTH=4
readonly EXIT_FOUND=1

repo="${1:-.}"
violations=()

# Git runs before the loop, so a git failure stops the script (set -e).
tracked_list="$(mktemp)"
trap 'rm -f "$tracked_list"' EXIT
git -C "$repo" ls-files -z > "$tracked_list"

while IFS= read -r -d '' path; do
  case "$path" in
    local-data/* | *.pak | *.datasheet | *.loc.xml | *.heightmap | *.metadata | *.capitals.json | *.[dD][dD][sS] | ledger.bin | */ledger.bin)
      violations+=("$path")
      continue
      ;;
  esac
  file="$repo/$path"
  if [[ -f "$file" ]]; then
    magic="$(head -c "$MAGIC_LENGTH" "$file" | tr -d '\0')"
    if [[ "$magic" == "$BUNDLE_MAGIC" || "$magic" == "$WORLD_MAGIC" || "$magic" == "$LEDGER_MAGIC" ]]; then
      violations+=("$path")
    fi
  fi
done < "$tracked_list"

if [[ "${#violations[@]}" -gt 0 ]]; then
  printf 'game data tracked by git: %s\n' "${violations[@]}" >&2
  exit "$EXIT_FOUND"
fi
echo "no game data tracked"
