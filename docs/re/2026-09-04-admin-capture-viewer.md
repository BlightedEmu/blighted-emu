# Admin tools part A: capture viewer exit test

Date: 2026-09-04. Code: the commits on `main` after `a602171`, to the commit that adds this record.

This note records the exit test of spec `2026-09-04-admin-tools-design.md` section 1.3, part A. It gives counts only: no capture bytes, ids, names, positions, tokens or addresses.

## Result

| Criterion | Result |
|---|---|
| A1. `cargo test --workspace`, `just test-web`, `just check` pass | Pass (`just check` exit 0, with `just test-web`: 17 page tests). |
| A2. `just capture-check` with type 11 read and written back to the same bytes | Type 11: pass (83,148,612 members, 0 byte differences). The check as a whole finds 46 new failures, all in vitals members (see 2). |
| A3. Map tiles written and lined up with the relief | All 3,029 readable tiles written. Alignment: pass at 9 of 10 places (see 3). |
| A4. Largest ledger opens in under 30 s | Pass: 1.25 s. |
| A5. Our own session opens and compares | Pass (see 4). |
| A6. This record | Yes. |

## 1. Program checks

- `just check`: game data guard, `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace`, `cargo deny check`, the shell script tests and `just test-web` pass. The first run found one line from the capture page task that `cargo fmt` changes; it is fixed in its own commit.
- `cargo test -p content-tool --features local-data the_install_has_level_1_tiles_of_256_m` passes on the install (8 levels; formats BC1 and BC3 only; 2,156 level 1 tiles; highest level 1 index 55).

## 2. Capture check

**Observed.** `capture-check` over the 695 ledgers, release build:

| | Before (`a602171`) | After the type 11 reader |
|---|---:|---:|
| Bundle members read and written back, type 10 | 34,433 | 79,668 |
| type 11 | 0 (no reader) | 83,148,612 |
| type 13 | 16,192 | 77,411 |
| type 15 | 2 | 3,766 |
| Payloads read to the end | 1,076 | 12,059,144 |
| Decode failures | 1 | 47 |

The move of the ledger code into `nw-capture` and the shared decoder gave the same output, line for line, as before each change.

**Observed.** Every new failure is in a bundle group of 2 members: type 11, then type 15 (vitals). Before the type 11 reader the walk stopped at type 11, so these vitals members were never read. In 19 of the 20 records checked, the vitals member has slot byte `0x01` and one child byte `0x04` (only child 2, `vitalsId`, which the reader reads as 4 bytes). In 13 of them, 18 bytes follow the child byte to the end of the payload. 18 bytes is the size the vitals note gives `vitalsData`.

**Inferred.** The slot 0 child order or sizes of `VitalsSlot0` (M6-B) do not fit this form. A separate task was flagged to find the true layout and to check what `sim` sends.

**Observed.** On the largest session the walk stops most at classes 15 (vitals, 150,278 walks), 4297 (`MB::StaminaComponentReplicatedState`, 92,906) and 3451 (`Javelin::GroupDataComponentReplicatedState`, 50,572): the readers still missing.

## 3. Map tiles

**Observed.** `content-tool map-tiles`: 3,031 tiles in 8 levels (level 1 a 56 x 56 grid of 256 m, 14,336 m in all); BC1 760, BC3 2,269. The Oodle decoder cannot read 2 level 1 tiles (`y020_x047`, `y023_x021`); they are left out and listed. The run takes 5 s and writes 1.2 GB of PNG files into `local-data/content/map/`.

**Observed.** Tile rows count from the south edge: the four level 6 tiles form one whole continent only with row 1 above row 0. The map page with the relief over the tiles shows the coastlines on top of each other.

**Observed.** A throwaway script compared, at level 3 (4 m per pixel), the relief's water (height under the ocean level, 40 m) with the transparent part of the map tiles:

- Whole world: 80.5 % of the pixels agree with no shift; the best shift is 0 m east and 8 m north (80.6 %). With the tile rows the other way round, 71.6 % agree.
- Ten places (the coast tiles where the map draws at least 90 % of the relief land; coast pixels within 100 m of open water on the map, so that lakes the map paints are left out): median distance from the relief coast to the map's edge 8.0, 25.6, 8.9, 17.0, 8.0, 14.4, 8.0, 8.9, 11.3 and 11.3 m. 9 of 10 places are within 20 m. The one at 25.6 m is a tile on the south edge of the map.

**Inferred.** The 80 % agreement, not more, comes from land that the map does not draw (outside the playable area) and from water that it paints (lakes, rivers). The cause of the 25.6 m place is Unknown.

## 4. Capture viewer

**Observed.** `blighted-admin` (then named `nw-admin`) lists 695 sessions (as `capture-check`), 385 with a `metadata.json` name and all 410 open-world sessions with the title of their Discord post.

**Observed.** The largest ledger (307 MB, 1,083,337 records, 1,086,769 messages, 1,160 tracks, 312,322 player points) opens in 1.25 s; the bytes of a message near its end come in 0.32 s. The program uses 1.2 GB of memory with the world layer and that session.

**Observed.** In the browser, a training dummy session shows 79 message types, 9 entities with their paths, 26 walk stop classes, the bytes of a bundle with its members marked, and a compare table with a catacombs session (125 rows, 64 with a ratio). The console shows no error.

**Observed.** After the review fixes, the page also shows the largest session (133 message types, 1,160 entities, the message list); before them, a list of 312,322 player points overflowed the browser's call stack. A request with another `Host` (a page whose name was rebound to 127.0.0.1) gets 403.

**Observed.** Our own session: one `sim`, `world-director`, `rep-gateway` and `edge-http` (port 8443), then `nw-testclient --ledger <file> pair`. Every pair check passed. The ledger (653 KB) opens with 3,639 records, 3,553 messages, 18 tracks, 1,602 player points and no stream failure. Compared with the training dummy session: ours has 14 message types, the recorded one 79; 13 are in both, 66 only in the recorded one, and 1 (type 3601) only in ours. Server to client: ours 11, recorded 41. Client to server: ours 3, recorded 38.

## What this proves

- The capture viewer reads every recorded session and our own, and shows types, members with their bytes, paths and a comparison.
- The type 11 reader is byte-exact on 83 million members; with it, 12 million bundles are read to the end instead of 1,076.
- The client's map tiles line up with the world file's heights (best whole-map shift 8 m).

## What this does not prove

- That the vitals reader is right: the new failures show it is not, for one form (separate task).
- What the 66 message types that our session lacks do: the viewer shows them; decoding them is later work.
- The live panel (part B).
