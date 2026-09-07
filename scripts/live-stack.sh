#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Starts a local test world for the live panel: edge-http (HTTPS on port 8443), world-director,
# one sim, rep-gateway, and then blighted-admin in the foreground. Ctrl-C stops all of them.
# Usage: live-stack.sh   (run `cargo build --release` first; NW_GAME_DIR names the game install)
# It needs the git-ignored local files below. The servers get the secrets of the env file;
# blighted-admin does not (its bot batches read the env file themselves). Logs go to LOG_DIR.
# LIVE_STACK_ROOT overrides the repository folder (used by the tests).
set -euo pipefail

readonly EXIT_MISSING=3
readonly ROOT="${LIVE_STACK_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
readonly BIN="target/release"
readonly ENV_FILE="local-data/m3-test.env"
readonly EDGE_CONFIG="local-data/edge-http/edge.toml"
readonly DIRECTOR_CONFIG="local-data/world-director.toml"
readonly LOG_DIR="local-data/logs/live"
readonly EDGE_LISTEN="0.0.0.0:8443"
readonly LOGIN_URL="https://login.faqfirebase.com:8443"
readonly GAME_URL="https://game.faqfirebase.com:8443"
readonly PANEL_URL="http://127.0.0.1:9180/live"
readonly SIM_ID="sim-1"
readonly SIM_METRICS_LISTEN="127.0.0.1:9102"
readonly SNAPSHOT_INTERVAL_MS=250
readonly LOGIN_FAIL_LIMIT=5000
readonly HANDSHAKES_PER_IP_PER_SECOND=200
# The sim joins after the director runs; the gateway starts after the sim has its cells.
readonly DIRECTOR_START_S=2
readonly SIM_START_S=8
readonly PROGRAMS=(edge-http world-director sim rep-gateway blighted-admin nw-testclient)

cd "$ROOT"

missing=()
for file in "$ENV_FILE" "$EDGE_CONFIG" "$DIRECTOR_CONFIG"; do
  [[ -f "$file" ]] || missing+=("$file")
done
for program in "${PROGRAMS[@]}"; do
  [[ -x "$BIN/$program" ]] || missing+=("$BIN/$program")
done
[[ -n "${NW_GAME_DIR:-}" ]] || missing+=("NW_GAME_DIR")
if ((${#missing[@]} > 0)); then
  echo "live stack: missing ${missing[*]}" >&2
  echo "Run 'cargo build --release', set NW_GAME_DIR, and keep the local-data files." >&2
  exit "$EXIT_MISSING"
fi

mkdir -p "$LOG_DIR"
pids=()
stop_all() {
  kill "${pids[@]}" 2>/dev/null || true
  wait "${pids[@]}" 2>/dev/null || true
}
trap stop_all EXIT

# Starts one server with the secrets of the env file; its output goes to LOG_DIR/<name>.log.
start_server() {
  local name="$1"
  shift
  (
    set -a
    # shellcheck source=/dev/null
    . "$ENV_FILE"
    set +a
    exec "$@"
  ) >"$LOG_DIR/$name.log" 2>&1 &
  pids+=("$!")
}

start_server edge env NW_EDGE_HTTPS_LISTEN="$EDGE_LISTEN" \
  NW_EDGE_AUTH__LOGIN_FAIL_LIMIT="$LOGIN_FAIL_LIMIT" \
  "$BIN/edge-http" --config "$EDGE_CONFIG" serve
start_server director "$BIN/world-director" --config "$DIRECTOR_CONFIG"
sleep "$DIRECTOR_START_S"
start_server "$SIM_ID" env NW_SIM_SIM_ID="$SIM_ID" NW_SIM_METRICS_LISTEN="$SIM_METRICS_LISTEN" \
  NW_SIM_ADMIN__SNAPSHOT_INTERVAL_MS="$SNAPSHOT_INTERVAL_MS" "$BIN/sim"
sleep "$SIM_START_S"
start_server gateway env \
  NW_GATEWAY_LIMITS__HANDSHAKES_PER_IP_PER_SECOND="$HANDSHAKES_PER_IP_PER_SECOND" \
  "$BIN/rep-gateway" serve

echo "Servers run; logs in $LOG_DIR. Open $PANEL_URL. Ctrl-C stops everything."
NW_TESTCLIENT_LOGIN_URL="$LOGIN_URL" NW_TESTCLIENT_GAME_URL="$GAME_URL" \
  "$BIN/blighted-admin" serve
