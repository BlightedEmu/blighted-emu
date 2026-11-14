#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Installs the realm data pack (the capture-made tables) into local-data/content, then builds the
# tables made from them on this machine. Build the install tables first (README, "Content data":
# just bundle, just world, just design, just collision and the others).
# Usage: realm-data.sh [--rebuild] [--replace] pack.zip
#   --rebuild build the derived tables again even when they exist
#   --replace replace tables of the content folder that differ from the pack's
set -euo pipefail

readonly EXIT_USAGE=2
readonly CLIENT_BUILD_FILE="config/client-build.toml"
readonly CONTENT_DIR="local-data/content"
readonly SERVER_PAK="local-data/server-pak/Server.pak"
# The tables built from the pack tables and the install, in build order (each needs the ones
# before it): creature animations and catacomb tiles read the mob prefabs, dungeon scripts read
# the breach tiers, effect areas read the dungeon scripts, the AI data reads Server.pak and the
# creature animations.
readonly DERIVED_TABLES=(creature-animations catacomb-tiles dungeon-scripts effect-areas)
readonly AI_DATA="ai-data"

root="${REALM_DATA_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
read -r -a content_tool <<< "${REALM_DATA_CONTENT_TOOL:-cargo run --release -p content-tool --}"

rebuild=false
replace_args=()
zip=""
for arg in "$@"; do
  case "$arg" in
    --rebuild) rebuild=true ;;
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

needs_build() {
  [[ "$rebuild" == true || ! -f "$CONTENT_DIR/$1-$build.json" ]]
}

for table in "${DERIVED_TABLES[@]}"; do
  if needs_build "$table"; then
    "${content_tool[@]}" "$table"
  else
    echo "$table: kept (use --rebuild to build it again)"
  fi
done

if [[ ! -f "$SERVER_PAK" ]]; then
  echo "Server.pak not found: get it with a depot download of build $build into $SERVER_PAK, then run just realm-data again"
elif [[ "$rebuild" == true || ! -f "$CONTENT_DIR/$AI_DATA-$build.bin" ]]; then
  "${content_tool[@]}" "$AI_DATA"
else
  echo "$AI_DATA: kept (use --rebuild to build it again)"
fi
