#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Starts a local test world for the live panel: edge-http (HTTPS on port 8443), world-director,
# 1 to 8 sims, rep-gateway, and then blighted-admin in the foreground. Ctrl-C stops all of them.
# Usage: live-stack.sh [sims]   (default 4 sims of up to 4,000 players each, so 16,000 fit;
# run `cargo build --release` first; NW_GAME_DIR names the game install)
# It needs the git-ignored local files below. The servers get the secrets of the env file;
# blighted-admin does not (its bot batches read the env file themselves). Logs go to LOG_DIR.
# LIVE_STACK_ROOT overrides the repository folder (used by the tests).
set -euo pipefail

readonly EXIT_USAGE=2
readonly EXIT_MISSING=3
# The director splits the map into strips of equal width, west to east. With 5 or more sims the
# westmost strip (x below 2,048 m) is sea with no start point, so that sim stays empty.
readonly DEFAULT_SIMS=4
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
# The sim metrics ports that local-data/nw-testclient.toml reads (port 9103 is the director's).
readonly SIM_METRICS_PORTS=(9102 9104 9105 9106 9107 9108 9109 9110)
readonly GATEWAY_METRICS_URL="http://127.0.0.1:9101/metrics"
readonly DIRECTOR_METRICS_URL="http://127.0.0.1:9103/metrics"
readonly SNAPSHOT_INTERVAL_MS=250
readonly SIM_TICK_THREADS=4
# Bots do not spread evenly over the sims' cells: each sim takes twice its usual 2,000.
readonly SIM_MAX_PLAYERS=4000
# Bots log in fast and all from this PC: room for 10,000 of them (the M5 load test settings).
readonly LOGIN_FAIL_LIMIT=50000
readonly GATEWAY_WORKERS=12
readonly GATEWAY_MAX_CONNECTIONS=12000
readonly HANDSHAKES_PER_IP_PER_SECOND=1000
# The sim joins after the director runs; the gateway starts after the sim has its cells.
readonly DIRECTOR_START_S=2
readonly SIM_START_S=8
readonly PROGRAMS=(edge-http world-director sim rep-gateway blighted-admin nw-testclient)

sims="${1:-$DEFAULT_SIMS}"
if [[ ! "$sims" =~ ^[1-9][0-9]*$ ]] || ((sims > ${#SIM_METRICS_PORTS[@]})); then
  echo "live stack: sims must be 1 to ${#SIM_METRICS_PORTS[@]}, not '$sims'" >&2
  exit "$EXIT_USAGE"
fi

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
metrics_urls="\"$GATEWAY_METRICS_URL\", \"$DIRECTOR_METRICS_URL\""
for ((index = 0; index < sims; index++)); do
  sim_id="sim-$((index + 1))"
  port="${SIM_METRICS_PORTS[index]}"
  start_server "$sim_id" env NW_SIM_SIM_ID="$sim_id" NW_SIM_METRICS_LISTEN="127.0.0.1:$port" \
    NW_SIM_TICK_THREADS="$SIM_TICK_THREADS" NW_SIM_MAX_PLAYERS="$SIM_MAX_PLAYERS" \
    NW_SIM_ADMIN__SNAPSHOT_INTERVAL_MS="$SNAPSHOT_INTERVAL_MS" "$BIN/sim"
  metrics_urls+=", \"http://127.0.0.1:$port/metrics\""
done
sleep "$SIM_START_S"
start_server gateway env NW_GATEWAY_WORKERS="$GATEWAY_WORKERS" \
  NW_GATEWAY_LIMITS__MAX_CONNECTIONS="$GATEWAY_MAX_CONNECTIONS" \
  NW_GATEWAY_LIMITS__HANDSHAKES_PER_IP_PER_SECOND="$HANDSHAKES_PER_IP_PER_SECOND" \
  "$BIN/rep-gateway" serve

echo "$sims sims run; logs in $LOG_DIR. Open $PANEL_URL. Ctrl-C stops everything."
NW_TESTCLIENT_LOGIN_URL="$LOGIN_URL" NW_TESTCLIENT_GAME_URL="$GAME_URL" \
  BLIGHTED_ADMIN_LIVE__METRICS_URLS="[$metrics_urls]" \
  "$BIN/blighted-admin" serve
