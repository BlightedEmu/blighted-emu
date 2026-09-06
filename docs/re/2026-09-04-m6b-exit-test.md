# M6-B exit test

Date: 2026-09-04. Code: the M6-B commits on `main` after `0756db7`, with the fixes of the final review (`4828897`).

This note records the exit test of spec `2026-09-04-m6b-combat-wire-design.md` section 1.2. The game was not run. The test client of this project played the client side.

## Result

| Criterion | Result |
|---|---|
| 1. `cargo test --workspace` and `just test-infra` pass | Pass. 793 and 295 tests, 0 failed. |
| 2. `content-tool capture-check`: 0 differences | Pass. 0 codec differences on 695 ledgers (see below). |
| 3. Server tests of spec section 10.3 | Pass (see below). |
| 4. Live `nw-testclient hunt` | Pass with 90 bots, not 1,000: the user's choice after amendment 5 of the spec (see below). |
| 5. The M6-A live run (`swarm --bots 5000`) | Pass: every check. Positions only (see below). |
| 6. This record | Yes. |

## 2. Byte check

**Observed.** `content-tool capture-check` on the 695 decrypted ledgers (24,944,621 server and 11,079,061 client datagrams), at `17a111a`; later commits do not change a reader:

- Bodies decoded and encoded to the same bytes: 2071: 103,098; 3601: 30,548; 4299: 291; 1938: 316 (and the M4 types as before).
- Bundle members decoded and encoded to the same bytes: type 10: 34,433; type 13: 16,192; type 15: 2.
- 0 codec differences. One failure is left: in `starstone_barrows_group_run_2`, more than 1,024 reliable frames wait for a frame that the recording lost. It is in the capture, not in a reader.

**Observed.** The vitals notes give child 0 (`vitalsData`) 18 bytes and child 1 (`healthChangeFlags`) 1 byte. Single-member bundles of 10 to 29 bytes hold each of them in 4 bytes, with a first byte in the `f32` range (child 0: `0x43` to `0x4a`; child 1: `0x3b` to `0x3e`). Children 8 to 13 of slot 0 appear on 567 members. The reader treats all of these as not sized, and the bundle walk stops there, as at a class without a reader. That is why only 2 type 15 members are checked. The server sends only children 2, 3, 4 and 7.

**Unknown.** What selects the 4-byte form of children 0 and 1, and the sizes of children 8 to 13.

## 3. Server tests

On NATS, PostgreSQL and real `sim` loops:

- `an_attack_input_hits_a_mob_and_the_gateway_gets_a_2071`: an input with attack bit 9 next to a mob gets the gateway a 2071 on the message channel.
- `a_killed_player_gets_a_4299_and_respawns_on_request`: a mob kills a player; the gateway gets a 4299; the player cannot move until its 1938, then it moves again.
- `a_mob_enters_a_players_bundles_with_its_card`: a mob comes into a player's bundles with its card (type 10), vitals (type 15) and position, then with its position.

Mutation checks: each of these fails without the new wiring (no attack from the input, no outbox in the tick, no card, no 1938 path). The rest of section 10.3 runs on the world state (`bins/sim/src/world/attack.rs`, `world.rs`, `members.rs`).

## 4. Live hunt

**Set-up.** As the M6-A live test (`docs/re/2026-09-04-m6a-exit-test.md`): one PC, release builds, `edge-http`, `world-director` (cells of 1,024 m), eight `sim` processes (4 tick threads each), `rep-gateway` (12 workers). The sims ran with `combat.mob_respawn_ms = 5000` for the run only (the committed value is 360,000), so that 90 bots had mobs to fight for 180 s.

**Hunt starts.** A throwaway search of the world file (grid 25 m; ground slope at most 0.5 within 10 m; above the ocean) chose the spots with 15 to 60 spawns of level 1 to 5 within 40 m and no stronger spawn within 70 m (sight plus leash). It found three:

| Start (x, y) | Spawns of level 1 to 5 within 40 m | Sim |
|---|---|---|
| 7171.3, 3160.2 | 17 | sim-5 (3 m from the sim-4 border) |
| 9696.3, 2260.2 | 17 | sim-6 |
| 7971.3, 4685.2 | 15 | sim-5 |

Spots with more than 60 such spawns (up to 1,180 within 40 m) are stacks of spawn points. Without the 70 m rule there are 10 spots.

**Why 90 bots (spec amendment 5).** Each player sees its 47 nearest entities, bots and mobs together. More than about 30 bots on one start see only other bots and never a mob. The user chose 30 bots per start on the three starts. `[hunt]` now refuses more than 30 bots per start.

**Observed.** `nw-testclient hunt`, 90 new characters at 10 per second, 180 s each:

- bots 90, failed to start 0; hits (2071) 5,928; hits taken (3601) 1,067; deaths (4299) 75; lived again after the 1938: 73 (the other two died less than the proof wait before the end); mob deaths 818; moves that arrived after a death 5 (counted apart since the final review); worst bundle gap 54 ms.
- Every check of section 9.3 passed: every bot registered, no bot was closed, every bot hit a mob, mobs died (818, at least 9 needed), every dead bot lived again after its request, the bundles never stopped, no move of a living bot was refused, tick p99 within one tick, every metrics page read. 0 UDP receive buffer errors.
- Ticks within 33 ms, by sim: 100 % on all eight. Mean tick: 0.20 ms (sim-4), 0.59 ms (sim-5), 0.38 ms (sim-6), at most 0.02 ms on the others. Mob deaths by sim: sim-4 117 (the mobs west of the border at the first start, hit through the bus), sim-5 468, sim-6 233.
- The lowest idle CPU was 89.9 %.

## 5. Swarm regression

**Set-up.** The same stack with the committed mob respawn and the 13 mob-free start points of the M6-A run; `nw-testclient swarm --bots 5000` at 50 bots per second, each bot 320 s.

**Observed.** 5,000 bots started in 105.2 s, failed to start 0, fewest crossings 31, worst bundle gap 100 ms, fewest bundles 0.9966, handoffs 158,321 (failed 0), within one tick: handoffs 1.0000, ticks 1.0000; 0 UDP receive buffer errors; every check passed. Ticks within 33 ms, by sim: 100 % on all eight; mean tick 1.71 to 7.40 ms. No mob was awake at the end. The lowest idle CPU was 27.3 %.

**What it leaves out.** The bots stand away from every mob, so every mob sleeps: no spawn card, vitals block or combat message is sent, and each bot gets only the positions of up to 47 players. It shows that M6-B did not break the M5 and M6-A paths. It is not a scale result for the game.

## What this proves

- The combat loop works on the wire: a client sees mobs (card, vitals, positions), attacks with input bits, gets its hits (2071), the hits on it (3601) and its death (4299), and lives again on its request (1938), on the real loop and across a sim border.
- Our codecs of types 10, 15 (the sized children), 2071, 3601, 4299 and 1938 read every reached captured body and write the same bytes.

## What this does not prove

- That the real game client accepts our spawn card, vitals block and message heads: their ids and heads are our own values (spec F9). The game client cannot connect to this server.
- Scale with combat: the hunt had 90 bots. The swarm run sends positions only: every mob sleeps, and no card, vitals or combat message is sent. It is a regression check, not a scale result.
- The live game's numbers: damage per weapon, mob health per type, and a mob's respawn time are still Unknown; the run used a 5 s mob respawn.
