#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Fails when git tracks game files or content bundles (M1 spec section 6).
# Usage: no-game-data.sh [repository-folder]   (default: current folder)
set -euo pipefail

readonly BUNDLE_MAGIC="NWCB"
readonly MAGIC_LENGTH=4
readonly EXIT_FOUND=1

repo="${1:-.}"
violations=()

while IFS= read -r -d '' path; do
  case "$path" in
    local-data/* | *.pak | *.datasheet | *.loc.xml)
      violations+=("$path")
      continue
      ;;
  esac
  file="$repo/$path"
  if [[ -f "$file" ]] && [[ "$(head -c "$MAGIC_LENGTH" "$file" | tr -d '\0')" == "$BUNDLE_MAGIC" ]]; then
    violations+=("$path")
  fi
done < <(git -C "$repo" ls-files -z)

if [[ "${#violations[@]}" -gt 0 ]]; then
  printf 'game data tracked by git: %s\n' "${violations[@]}" >&2
  exit "$EXIT_FOUND"
fi
echo "no game data tracked"
