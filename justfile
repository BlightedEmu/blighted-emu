# Developer commands for the Blighted New World emulator.
set shell := ["bash", "-euo", "pipefail", "-c"]

sweep_age_days := "7"

# Run every required check (spec 10.5) and the game-data guard (M1 spec section 6).
check:
    scripts/no-game-data.sh
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cargo deny check
    just test-scripts
    just test-web

# Run the shell script tests.
test-scripts:
    scripts/target-size.test.sh
    scripts/no-game-data.test.sh
    scripts/live-stack.test.sh

# Start a local test world (edge-http on 8443, director, 1 to 8 sims, gateway) and the live
# panel; 4 sims hold 16,000 players. Ctrl-C stops all. Build with `cargo build --release` and
# set NW_GAME_DIR first.
live sims="4":
    scripts/live-stack.sh {{sims}}

# Run the tests of the admin page code (Node built-in test runner, no package).
test-web:
    node --test 'bins/blighted-admin/web/*.test.mjs'

# Print the build folder size; fails above 10 GiB (spec 10.6).
size:
    scripts/target-size.sh target

# Delete build files older than 7 days (spec 10.6).
clean-old:
    cargo sweep --time {{sweep_age_days}}

compose := "docker compose -f deploy/docker-compose.yml --env-file deploy/.env"

# Start the infrastructure and wait until every service is healthy.
infra-up:
    {{compose}} up -d --wait

# Stop the infrastructure. Data volumes are kept.
infra-down:
    {{compose}} down

# Run the infrastructure smoke test (M0 exit criterion).
infra-test:
    deploy/smoke-test.sh

# Tests that need the Docker Compose PostgreSQL, Valkey and NATS. Run `just infra-up` first.
test-infra:
    cargo test -p nw-db -p nw-login -p nw-bus -p sim -p rep-gateway -p world-director -p blighted-admin --features nw-db/infra,nw-login/infra,nw-bus/infra,sim/infra,rep-gateway/infra,world-director/infra,blighted-admin/infra

# Tests that read your own New World install. Set NW_GAME_DIR first.
test-local:
    cargo test -p nw-formats -p nw-oodle -p content-tool --features nw-formats/local-data,nw-oodle/local-data,content-tool/local-data

# Regenerate the table structs in crates/nw-content/src/generated from your install.
codegen:
    cargo run -p content-tool -- codegen

# Build the content bundle into local-data/content/ from your install.
bundle:
    cargo run -p content-tool -- build

# Build one world file per level into local-data/content/ from your install. Run `just bundle` first.
world:
    cargo run -p content-tool -- world

# Write the open world map tiles into local-data/content/map/ from your install (README "Admin tools").
map-tiles:
    cargo run --release -p content-tool -- map-tiles

# Decode every capture ledger under local-data/captures (unpack the capture archives there first).
capture-check:
    find -L local-data/captures -name ledger.bin -print0 | xargs -0 cargo run -p content-tool -- capture-check
