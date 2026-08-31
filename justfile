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
