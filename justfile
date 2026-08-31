# Developer commands for the Blighted New World emulator.
set shell := ["bash", "-euo", "pipefail", "-c"]

sweep_age_days := "7"

# Run every required check (spec 10.5).
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cargo deny check
    just test-scripts

# Run the shell script tests.
test-scripts:
    scripts/target-size.test.sh

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
