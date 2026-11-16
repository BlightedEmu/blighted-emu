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

# A fresh folder with the client build file and a stub content tool.
# STUB_FAIL_INSTALL=1 makes the stub's install fail.
setup() {
  rm -rf "$work_dir/root"
  mkdir -p "$work_dir/root/config"
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
run_script pack.zip > /dev/null
pass_if "installs_the_given_zip" logged "realm-pack install pack.zip"
pass_if "builds_the_derived_tables_after_the_install" \
  test "$(sed -n '2p' "$work_dir/log")" = "content build --derived"
pass_if "runs_two_commands" test "$(wc -l < "$work_dir/log")" -eq 2

setup
run_script --rebuild --replace pack.zip > /dev/null
pass_if "rebuild_is_passed_to_the_build" logged "content build --derived --rebuild"
pass_if "replace_is_passed_to_the_install" logged "realm-pack install --replace pack.zip"

setup
status=0
STUB_FAIL_INSTALL=1 run_script pack.zip > /dev/null 2>&1 || status=$?
pass_if "stops_when_install_fails" test "$status" -ne 0
pass_if "stops_before_the_derived_tables" not_logged "content build"

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
