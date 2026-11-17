#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Installs the realm data pack (the capture-made tables) into the content folder, then builds the
# tables made from them on this machine (`content-tool content build --derived`: the derived
# tables of the content registry, in build order; the AI data only when Server.pak is there).
# Build the install tables first (README, "Content data": just content).
# Usage: realm-data.sh [--rebuild] [--replace] pack.zip
#   --rebuild build the derived tables again even when they are fresh
#   --replace replace tables of the content folder that differ from the pack's
set -euo pipefail

readonly EXIT_USAGE=2
readonly CLIENT_BUILD_FILE="config/client-build.toml"

root="${REALM_DATA_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
read -r -a content_tool <<< "${REALM_DATA_CONTENT_TOOL:-cargo run --release -p content-tool --}"

rebuild_args=()
replace_args=()
zip=""
for arg in "$@"; do
  case "$arg" in
    --rebuild) rebuild_args=(--rebuild) ;;
    --replace) replace_args=(--replace) ;;
    -*) echo "unknown option: $arg" >&2; exit "$EXIT_USAGE" ;;
    *)
      if [[ -n "$zip" ]]; then
        echo "only one pack zip" >&2
        exit "$EXIT_USAGE"
      fi
      zip="$arg"
      ;;
  esac
done

# A relative zip path starts at the caller's folder; the commands below run in the repo folder.
if [[ -n "$zip" && "$zip" != /* ]]; then
  zip="$PWD/$zip"
fi

cd "$root"
build="$(sed -n 's/^steam_build_id = "\([0-9]*\)"$/\1/p' "$CLIENT_BUILD_FILE")"
if [[ -z "$build" ]]; then
  echo "no steam_build_id in $CLIENT_BUILD_FILE" >&2
  exit "$EXIT_USAGE"
fi

if [[ -z "$zip" ]]; then
  echo "realm-data: give the pack zip: just realm-data realm-data-$build.zip" >&2
  exit "$EXIT_USAGE"
fi

"${content_tool[@]}" realm-pack install "${replace_args[@]}" "$zip"
"${content_tool[@]}" content build --derived "${rebuild_args[@]}"
