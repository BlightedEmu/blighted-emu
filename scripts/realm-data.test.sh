#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Tests for realm-data.sh with a stub content tool that logs its arguments.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly DATA_SCRIPT="$SCRIPT_DIR/realm-data.sh"
readonly BUILD="12345"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
failures=0

pass_if() {
  local name="$1"
  shift
  if "$@"; then
    echo "PASS: $name"
  else
    echo "FAIL: $name"
    failures=$((failures + 1))
  fi
}

# A fresh folder with the client build file, an empty content folder and a stub content tool.
# STUB_FAIL_INSTALL=1 makes the stub's install fail.
setup() {
  rm -rf "$work_dir/root"
  mkdir -p "$work_dir/root/config" "$work_dir/root/local-data/content"
  echo "steam_build_id = \"$BUILD\"" > "$work_dir/root/config/client-build.toml"
  cat > "$work_dir/stub.sh" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$STUB_LOG"
if [[ "$1" == realm-pack && "${STUB_FAIL_INSTALL:-0}" == 1 ]]; then
  exit 1
fi
STUB
  chmod +x "$work_dir/stub.sh"
  : > "$work_dir/log"
}

run_script() {
  env REALM_DATA_ROOT="$work_dir/root" REALM_DATA_CONTENT_TOOL="$work_dir/stub.sh" \
    STUB_LOG="$work_dir/log" "$DATA_SCRIPT" "$@"
}

logged() {
  grep -qx -- "$1" "$work_dir/log"
}

not_logged() {
  ! grep -q -- "$1" "$work_dir/log"
}

setup
output="$(run_script pack.zip)"
pass_if "installs_the_given_zip" logged "realm-pack install pack.zip"
pass_if "builds_missing_derived_tables" logged "creature-animations"
pass_if "builds_effect_areas_last_of_the_tables" \
  test "$(sed -n '5p' "$work_dir/log")" = "effect-areas"
pass_if "no_server_pak_skips_ai_data_and_exits_0" not_logged "ai-data"
pass_if "no_server_pak_names_the_depot_download" \
  test "${output##*Server.pak not found}" != "$output"

setup
touch "$work_dir/root/local-data/content/creature-animations-$BUILD.json"
run_script pack.zip > /dev/null
pass_if "skips_existing_derived_tables" not_logged "creature-animations"

setup
touch "$work_dir/root/local-data/content/creature-animations-$BUILD.json"
mkdir -p "$work_dir/root/local-data/server-pak"
touch "$work_dir/root/local-data/server-pak/Server.pak"
run_script --rebuild --replace pack.zip > /dev/null
pass_if "rebuild_builds_them_again" logged "creature-animations"
pass_if "replace_is_passed_to_the_install" logged "realm-pack install --replace pack.zip"
pass_if "server_pak_builds_ai_data" logged "ai-data"

setup
status=0
STUB_FAIL_INSTALL=1 run_script pack.zip > /dev/null 2>&1 || status=$?
pass_if "stops_when_install_fails" test "$status" -ne 0
pass_if "stops_before_the_derived_tables" not_logged "creature-animations"

setup
status=0
run_script --what > /dev/null 2>&1 || status=$?
pass_if "an_unknown_option_is_refused" test "$status" -eq 2

setup
status=0
message="$(run_script 2>&1)" || status=$?
pass_if "no_zip_is_refused" test "$status" -eq 2
pass_if "no_zip_names_the_pack_of_the_build" \
  test "${message#*realm-data-$BUILD.zip}" != "$message"
pass_if "no_zip_installs_nothing" not_logged "realm-pack"

if [[ "$failures" -gt 0 ]]; then
  echo "$failures realm-data test(s) failed"
  exit 1
fi
echo "all realm-data tests passed"
