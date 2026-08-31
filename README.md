# Blighted New World Emulator

A preservation server emulator for New World: Aeternum. The official servers shut down on 2027-01-31.

- Players use their own legal copy of the final PC client. This project never distributes Amazon game files.
- The emulator runs on standard dedicated servers, with no AWS dependency.
- It is non-commercial.

Design: the architecture spec is kept locally (not in git).
Research: [docs/NEW_WORLD_EMULATOR_RESEARCH.md](docs/NEW_WORLD_EMULATOR_RESEARCH.md)

## Licences

| Part | Licence |
|---|---|
| Server programs and core crates | AGPL-3.0-or-later ([LICENSE](LICENSE)) |
| `launcher` | GPL-3.0-or-later |
| `nw-formats` | MIT OR Apache-2.0 |
| `nw-oodle` | GPL-3.0-or-later |

Each crate states its licence in its `Cargo.toml`. `cargo deny check` enforces the policy in `deny.toml`.

## Requirements

- Rust (the version in `rust-toolchain.toml` is installed automatically by rustup)
- Docker with Compose v2
- `cargo install --locked cargo-deny cargo-sweep just`

## Quick start

```bash
cp deploy/.env.example deploy/.env
```

Edit `deploy/.env` and set new passwords. Then:

```bash
just check
just infra-up
just infra-test
```

## Commands

| Command | What it does |
|---|---|
| `just check` | Format, lint, test, dependency policy, script tests |
| `just size` | Prints the build folder size; fails above 10 GiB |
| `just clean-old` | Deletes build files older than 7 days |
| `just infra-up` / `just infra-down` | Starts / stops PostgreSQL, Valkey, NATS, Prometheus, Grafana |
| `just infra-test` | Infrastructure smoke test |
| `just test-local` | Tests that read your New World install (`NW_GAME_DIR`) |
| `just codegen` | Regenerates the table structs in `crates/nw-content/src/generated` |
| `just bundle` | Builds the content bundle into `local-data/content/` |

## Content data

The server reads game rules from your own New World install. Nothing from the game is stored in this repository; only the table shapes (names and types) are generated into `crates/nw-content/src/generated`.

```bash
export NW_GAME_DIR="/path/to/steamapps/common/New World"
just bundle
```

`just bundle` writes `local-data/content/bundle-<build>.bin`, then reloads it and checks every table and row. The pinned client build is in `config/client-build.toml`.

### Recovery

`content-tool` links `nw-content`, so it cannot run while the generated code does not compile. Removing the `generated` module lines from `crates/nw-content/src/lib.rs` is not enough, because `bundle.rs` and `content-tool build` use `Tables`. Instead:

1. Restore the committed generated files: `git checkout -- crates/nw-content/src/generated`.
2. Fix the cause (usually a change to `bins/content-tool/src/codegen`).
3. Run `just codegen` again.

Codegen stops with an error when a table type name would give an invalid Rust name or a name that the generated code already uses, so a new client build cannot write code that does not compile for that reason.

## Rules for contributors

- Never commit game files, captures, or `deploy/.env`. Put local game data in `local-data/` (ignored by git).
- Never connect a changed client to Amazon's servers.
- Every new copyleft crate must be added to `[licenses] exceptions` in `deny.toml`.
