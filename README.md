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
| `just test-infra` | Tests that need PostgreSQL and Valkey, with the login and gateway walk (run `just infra-up` first) |
| `just test-local` | Tests that read your New World install (`NW_GAME_DIR`) |
| `just codegen` | Regenerates the table structs in `crates/nw-content/src/generated` |
| `just bundle` | Builds the content bundle into `local-data/content/` |
| `just world` | Builds one world file per level into `local-data/content/` (run `just bundle` first) |
| `just capture-check` | Decodes every capture ledger under `local-data/captures/` and prints counts |

## Content data

The server reads game rules from your own New World install. Nothing from the game is stored in this repository; only the table shapes (names and types) are generated into `crates/nw-content/src/generated`.

```bash
export NW_GAME_DIR="/path/to/steamapps/common/New World"
just bundle
just world
```

`just bundle` writes `local-data/content/bundle-<build>.bin`, then reloads it and checks every table and row. The pinned client build is in `config/client-build.toml`.

`just world` writes `local-data/content/world-<build>-<level>.bin` for each of the 41 levels (about 620 MB in all), reloads each file, and prints the record counts, the skipped metadata fields, and the spawn IDs that no datasheet row has.

### Recovery

`content-tool` links `nw-content`, so it cannot run while the generated code does not compile. Removing the `generated` module lines from `crates/nw-content/src/lib.rs` is not enough, because `bundle.rs` and `content-tool build` use `Tables`. Instead:

1. Restore the committed generated files: `git checkout -- crates/nw-content/src/generated`.
2. Fix the cause (usually a change to `bins/content-tool/src/codegen`).
3. Run `just codegen` again.

Codegen stops with an error when a table type name would give an invalid Rust name or a name that the generated code already uses, so a new client build cannot write code that does not compile for that reason.

## Capture check

`content-tool capture-check` decodes capture ledgers (`ledger.bin`, decrypted carrier datagrams) with `nw-carrier` and `nw-reflect` and prints counts only: datagrams, messages per type index, registration response values, and decode failures. It exits with an error when any datagram or message does not decode. It also decodes the server messages of the M4 types (`nw-messages`), encodes them again, and counts those that give the same bytes; a difference is a failure. The facts it checks are in `docs/re/2026-09-02-carrier-captures.md`.

Ledgers hold data of other players. Keep them in git-ignored `local-data/captures/`; never commit them or paste message contents.

```bash
export NW_GAME_DIR="/path/to/steamapps/common/New World"
just capture-check
```

To print class names next to type indexes, add `--names local-data/Aeternum-World-main/Catalog/uuid_to_class.json` (Aeternum-World, AGPL-3.0); the tool joins it to `typeindex.json` in your install by UUID.

## Login server and launcher

`edge-http` is the HTTPS login server. Players log in with the `launcher` program, by username and password or with Discord (one mode per server, `[auth] mode` in `config/edge.toml`).

The server needs a domain name of your own with two host names that point to the server:

- `login.<domain>`: the launcher API and the channel document that the game downloads first.
- `game.<domain>`: the game routes. Every game request carries the game session key in its path (`/s/<session>/...`).

Get one normal public certificate that covers both names (for example from Let's Encrypt with `certbot` or `lego`). The DNS challenge works also when the names point to a LAN address. Put the PEM files where `[tls]` points. The game always uses port 443, so `https_listen` must use port 443. On Linux, give the program that right once after each build: `sudo setcap 'cap_net_bind_service=+ep' target/debug/edge-http`.

Secrets come from environment variables, never from the config file:

| Variable | Value |
|---|---|
| `NW_SECRET_SIGNING_KEY` | Entry ticket key: `openssl rand -hex 32` |
| `NW_SECRET_GLOBAL_DATABASE_PASSWORD` | Password of `[global_database]` |
| `NW_SECRET_WORLD_1_DATABASE_PASSWORD` | Password of the first world database (the name is set by `database_password_env`) |
| `NW_SECRET_DISCORD_CLIENT_SECRET` | Discord client secret, Discord mode only |

```bash
cargo run -p edge-http -- migrate   # creates missing databases and applies migrations
cargo run -p edge-http -- serve
cargo run -p launcher
```

In Discord mode, add `https://login.<domain>/launcher/discord/callback` as a redirect URL of your Discord application.

`edge-http` logs every request that no route serves (`request for an unknown route`, with the session key masked). These lines show what the client asks for that the server does not answer yet.

### M2 exit test (manual)

Do this once in password mode and once in Discord mode (`[auth] mode`).

1. Start the services (`just infra-up`), set the secrets, then run `edge-http migrate` and `edge-http serve`.
2. Start Steam. Start the launcher, type your domain, and log in (or make an account).
3. Click Play. The game starts through Steam with `-ChannelService https://login.<domain>`, `-LoginChannelOverride <game session key>` and `-AuthBackend nw`. The last one makes the game send its Steam ticket to our `/steam` route; in `Game.log`, `Persona failed to initialize!` or `Omni metadata is NOT present` means the game took another auth path.
4. The game must reach the character select screen. Make a character.
5. Close the game. Click Play again. The new character must be in the list.
6. Save the `Game.log` lines of the login steps and the `edge-http` log lines of the run in a new note in `docs/re/` as evidence. Remove your account name, Steam ids and IP addresses first.

If the game stops before character select, keep the `Game.log` lines and the `request for an unknown route` lines: they show the next step to build.

## World gateway and test client

`rep-gateway` is the UDP entry point of a world. The game connects to the `rep_address` in `config/edge.toml` after the login queue. The gateway runs DTLS 1.2 and the GridMate carrier, checks the entry ticket in the registration message, answers with a status, and keeps the connection alive with pings. Each ticket works one time only.

`nw-testclient` does what the game does up to that point: it logs in over HTTPS, makes a character when the account has none, waits in the queue, and registers with the gateway.

The game itself cannot connect to the gateway. It trusts only one certificate that is built into the game, and this project does not change that. The test client trusts the gateway's own certificate instead.

Set up the gateway once:

1. In `config/rep-gateway.toml`, set `world_id` to the `[[world]] id` in `config/edge.toml`. The `rep_address` in `config/edge.toml` must reach `udp_listen` (the example files match).
2. Make the gateway's certificate and key. The files go to `local-data/rep-gateway/` and are never replaced:

```bash
cargo run -p rep-gateway -- generate-certificate
```

Secrets come from environment variables:

| Variable | Value |
|---|---|
| `NW_SECRET_SIGNING_KEY` | The same entry ticket key as `edge-http` |
| `NW_SECRET_TESTCLIENT_PASSWORD` | Password of the test account (`nw-testclient` only) |

```bash
cargo run -p rep-gateway -- serve
cargo run -p nw-testclient
```

`nw-testclient` reads `config/nw-testclient.toml`. Set `login_url` and `game_url` to your domain. The account is made on the first run.

The gateway serves Prometheus metrics on `metrics_listen` (`rep_gateway_*`). Keep that address internal.

### M3 exit test

1. `cargo test --workspace` passes.
2. `just capture-check` reads every local ledger with 0 decode errors.
3. Start the services (`just infra-up`), set the secrets, then start `edge-http serve` and `rep-gateway serve`.
4. Run `cargo run -p nw-testclient`. It must print `registration status 0`, then about 60 echoed pings (at least half of one per second of hold) with `closed by server: false`, then a non-zero status for the second registration with the same ticket. The exit code must be 0.

`just test-infra` runs the same walk without the network: the test client logs in through the `edge-http` routes on the Docker PostgreSQL and Valkey, then registers with a gateway on a loopback port.

## Rules for contributors

- Never commit game files, captures, or `deploy/.env`. Put local game data in `local-data/` (ignored by git).
- Never connect a changed client to Amazon's servers.
- Every new copyleft crate must be added to `[licenses] exceptions` in `deny.toml`.
