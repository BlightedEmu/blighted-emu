#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Infrastructure smoke test (M0 exit criterion). Needs deploy/.env.
# Starts the stack and leaves it running. Stop it with: just infra-down
set -euo pipefail

readonly DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly COMPOSE_FILE="$DEPLOY_DIR/docker-compose.yml"
readonly ENV_FILE="$DEPLOY_DIR/.env"
readonly LOCALHOST="127.0.0.1"
readonly NATS_MONITOR_PORT=8222
readonly PROMETHEUS_PORT=9090
readonly GRAFANA_PORT=3000
readonly PERSIST_TABLE="smoke_persist"
readonly PERSIST_ROWS=1
readonly HTTP_OK=200
readonly SCRAPE_WAIT_SECONDS=20
readonly SCRAPE_POLL_SECONDS=2
readonly REQUIRED_SECRETS=(POSTGRES_USER POSTGRES_PASSWORD POSTGRES_DB GRAFANA_ADMIN_USER GRAFANA_ADMIN_PASSWORD)

compose() { docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" "$@"; }
# Fetch into a variable first: piping curl into "grep -q" can fail with SIGPIPE under pipefail.
http_get() { curl -fsS "$1" || true; }
# Read a value with Compose's own .env parser, so it matches what the containers get.
env_value() { compose config --environment | sed -n "s/^$1=//p"; }
pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1" >&2; exit 1; }

[[ -f "$ENV_FILE" ]] || fail "missing $ENV_FILE (copy deploy/.env.example and set passwords)"
bind_address="$(env_value BIND_ADDRESS)"
bind_address="${bind_address:-$LOCALHOST}"

# 1. Compose refuses to run when any one secret is missing, and names that secret.
partial_env="$(mktemp)"
trap 'rm -f "$partial_env"' EXIT
for secret in "${REQUIRED_SECRETS[@]}"; do
  grep -v "^${secret}=" "$ENV_FILE" > "$partial_env"
  if config_error="$(env -i PATH="$PATH" HOME="$HOME" docker compose -f "$COMPOSE_FILE" --env-file "$partial_env" config 2>&1 >/dev/null)"; then
    fail "compose accepted an env file without $secret"
  fi
  [[ "$config_error" == *"set $secret in deploy/.env"* ]] || fail "missing $secret gave the wrong error: $config_error"
done
pass "compose requires each of the ${#REQUIRED_SECRETS[@]} secrets"

# 2. Every published port binds to the configured address (default 127.0.0.1).
config_text="$(compose config)"
# grep -c exits 1 when the count is 0; "|| true" keeps set -e from stopping the script.
published_count="$(grep -c 'published:' <<<"$config_text" || true)"
bound_count="$(grep -c "host_ip: $bind_address" <<<"$config_text" || true)"
[[ "$published_count" -eq "$bound_count" ]] || fail "$published_count ports published, $bound_count bound to $bind_address"
pass "all $published_count ports bind to $bind_address"

# 3. Every service starts and becomes healthy.
compose up -d --wait
pass "all services healthy"

# 4. PostgreSQL data survives a new container (only the volume keeps it).
postgres_user="$(env_value POSTGRES_USER)"
postgres_db="$(env_value POSTGRES_DB)"
psql_exec() { compose exec -T postgres psql -U "$postgres_user" -d "$postgres_db" -tAc "$1"; }
psql_exec "DROP TABLE IF EXISTS $PERSIST_TABLE; CREATE TABLE $PERSIST_TABLE (id int); INSERT INTO $PERSIST_TABLE VALUES (1);" >/dev/null
# --renew-anon-volumes: the image has its own anonymous volume; only the named volume may keep data.
compose up -d --wait --force-recreate --renew-anon-volumes postgres
# A lost table makes the query fail; "|| true" lets the check below report it.
rows="$(psql_exec "SELECT count(*) FROM $PERSIST_TABLE;" 2>/dev/null || true)"
psql_exec "DROP TABLE IF EXISTS $PERSIST_TABLE;" >/dev/null
[[ "$rows" == "$PERSIST_ROWS" ]] || fail "postgres lost data when the container was re-created"
pass "postgres data survives container re-creation"

# 5. Valkey answers.
[[ "$(compose exec -T valkey valkey-cli ping)" == "PONG" ]] || fail "valkey did not answer PONG"
pass "valkey answers"

# 6. NATS is healthy.
[[ "$(http_get "http://$LOCALHOST:$NATS_MONITOR_PORT/healthz")" == *'"ok"'* ]] || fail "nats health check failed"
pass "nats healthy"

# 7. Prometheus scrapes itself.
deadline=$((SECONDS + SCRAPE_WAIT_SECONDS))
until [[ "$(http_get "http://$LOCALHOST:$PROMETHEUS_PORT/api/v1/query?query=up")" == *'"job":"prometheus"'* ]]; do
  (( SECONDS < deadline )) || fail "prometheus has no self-scrape result after $SCRAPE_WAIT_SECONDS s"
  sleep "$SCRAPE_POLL_SECONDS"
done
pass "prometheus scrapes itself"

# 8. Grafana is up and has the Prometheus data source.
grafana_health="$(http_get "http://$LOCALHOST:$GRAFANA_PORT/api/health")"
grep -Eq '"database": *"ok"' <<<"$grafana_health" || fail "grafana health check failed"
status="$(curl -s -o /dev/null -w '%{http_code}' -u "$(env_value GRAFANA_ADMIN_USER):$(env_value GRAFANA_ADMIN_PASSWORD)" \
  "http://$LOCALHOST:$GRAFANA_PORT/api/datasources/name/Prometheus")"
[[ "$status" -eq "$HTTP_OK" ]] || fail "grafana data source Prometheus missing (HTTP $status)"
pass "grafana has the Prometheus data source"

echo "all infrastructure checks passed"
