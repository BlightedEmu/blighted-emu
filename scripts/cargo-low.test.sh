#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Tests for cargo-low.sh.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly LOW_SCRIPT="$SCRIPT_DIR/cargo-low.sh"
readonly EXIT_OK=0
readonly EXIT_USAGE=2
readonly LOWEST_NICE=19

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

# A stand-in for cargo: it writes its arguments and its nice value.
fake_cargo="$work_dir/fake-cargo"
cat > "$fake_cargo" <<'EOF'
#!/usr/bin/env bash
echo "args: $*"
echo "nice: $(nice)"
EOF
chmod +x "$fake_cargo"

failures=0

expect() {
  local name="$1" ok="$2"
  if [[ "$ok" == yes ]]; then
    echo "PASS: $name"
  else
    echo "FAIL: $name"
    failures=$((failures + 1))
  fi
}

actual=0
"$LOW_SCRIPT" >/dev/null 2>&1 || actual=$?
expect "no arguments is a usage error" "$([[ "$actual" -eq "$EXIT_USAGE" ]] && echo yes)"

for scope in 0 1; do
  output="$(CARGO_LOW_SCOPE="$scope" CARGO_LOW_CARGO="$fake_cargo" \
    CARGO_LOW_LOCK="$work_dir/lock" "$LOW_SCRIPT" build --release -p sim)" || actual=$?
  expect "scope $scope: the arguments pass through" \
    "$([[ "$output" == *"args: build --release -p sim"* ]] && echo yes)"
  expect "scope $scope: cargo runs at the lowest priority" \
    "$([[ "$output" == *"nice: $LOWEST_NICE"* ]] && echo yes)"
done
expect "the lock file is made" "$([[ -e "$work_dir/lock" ]] && echo yes)"

if [[ "$failures" -ne 0 ]]; then
  echo "$failures test(s) failed"
  exit 1
fi
exit "$EXIT_OK"
