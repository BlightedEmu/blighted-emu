# M6-A exit test

Date: 2026-09-04. Code: the M6-A commits on `main` after `73f00e8`, with the fixes of the final review (`0913230`).

This note records the exit test of spec `2026-09-04-m6a-combat-engine-design.md` section 1.2. The game was not run. M6-A has no client message for combat: the client does not see mobs, and players attack only through the server's attack function (tests).

## Result

| Criterion | Result |
|---|---|
| 1. `cargo test --workspace` and `just test-infra` pass | Pass. 722 and 267 tests, 0 failed. |
| 2. `content-tool combat-check` | Pass with the rule of the final review (see below): every spawn without rows is explained; 99.3 % of the nwdb creature records are explained. The rule as first written (95 % direct match within 1) is not met: 83.1 %. |
| 3. Server tests of spec section 11.3 | Pass (see below). |
| 4. Live: 8 sims with every mob, 5,000 bots | Pass, on the third run, with shorter bot lanes (see below). |
| 5. Fight stress test | Pass. Tick p99 5.13 ms. |
| 6. This record | Yes. |

## 2. Combat check

**Observed.** `content-tool combat-check` on the bundle and the world file of build 22469132:

- 115,937 vitals spawns. 389 have level 0 (the level rows run from 1 to 80); 66 spawns of 8 ids are in no vitals table. Together 455 rows are missing for 443 spawns; the sims do not spawn those and count them (`sim_mobs_unspawned_total`). The meaning of level 0 is Unknown.
- nwdb.info: 8,650 creature records read (23 files unreadable). 7,188 match our max health within 0.1 % (at least 1). 1,403 more match it at the level of their `VitalsLevelVariantData` row: nwdb computes a variant mob's health at that row's own `level` column, while its `Level` field and the emulator use the spawn level. The other 59 differ by 2 to 3 points (nwdb's rounding).
- The first run of the check found the category mods: a mob type's mods include the `VitalsModifierData` mods of its creature type (for example `Solo+`: health 2, damage 1.2). Without them every `Solo+` mob had half of nwdb's health.

**Later finding.** The build 22469132 world file is deleted. `local-data/content/` now holds build 25810808 files only. This check cannot be re-run as written.

**Unknown.** Whether the live game builds a variant mob at its spawn level or at the variant row's level. The captures do not show a mob's max health per type (`docs/re/2026-09-04-m6-combat-reference-values.md` section 2).

## 3. Server tests

On NATS, PostgreSQL and real `sim` loops:
- `a_player_killed_by_a_mob_lives_again_and_is_saved`: a mob kills a player; the death is saved at once where it happened; the player moves again after its respawn.
- `a_mob_of_the_other_sim_kills_a_player_through_the_bus`: a mob of the east sim kills a player of the west sim; the hit goes over the bus.

The rest of section 11.3 runs on the world state (a mob killed and respawned, sleep and wake, cells that add and remove mobs, the attack refusals, handoffs that keep health), because a player attack has no loop path before M6-B. Two mutation checks show that the loop tests fail without the death save, without the player respawn, and without the hit sending.

## 4. Live test

Set-up as the M5 load step (`docs/re/2026-09-04-m5-load-test.md`): one PC, release builds, `edge-http`, `world-director` (cells of 1,024 m), eight `sim` processes (4 tick threads each), `rep-gateway` (12 workers), `nw-testclient swarm --bots 5000` at 50 bots per second, each bot 320 s. New for M6-A: every `sim` loads the combat rows and the mobs of its cells.

**Observed.** The sims spawned 115,494 mobs (877 to 30,596 per sim; the easternmost stripe holds none) and left 443 spawns without rows.

Start points: a throwaway search of the world file chose flat land on the sim borders with no vitals spawn within the bot area plus 70 m (sight 20 m; the largest vision offset in `AITargetingData` is 0; leash 50 m), so no mob ever sees a bot.

| Run | Start points | Bot lanes | Max players per sim | Result |
|---|---|---|---|---|
| 1 | 8 (all such points for lanes of ± 150 m) | ± 150 m | 2,000 | Fail: "the sim is full" (27,828 refused handoffs). 7 of the 8 points lie on two borders. |
| 2 | 8 | ± 150 m | 5,000 | Fail: 2,400 handoffs failed, worst bundle gap 302 s, 4.5 % idle CPU. 625 bots per start crowd two borders; three sims had no bot. |
| 3 | 13 (at most 4 per border) | ± 60 m | 5,000 | Pass: every swarm check. |

**Observed (run 3).** The swarm printed: 5,000 bots started in 105.2 s, failed to start 0, fewest crossings 30, worst bundle gap 101 ms, fewest bundles 0.9966, handoffs 157,899 (failed 0), within one tick: handoffs 1.0000, ticks 1.0000; 0 UDP receive buffer errors; every check passed. The lowest idle CPU was 19.9 %.

**Observed (run 3).** Every mob stayed asleep for the whole run (sampled every 10 s). The mob part of a tick took 0.04 to 0.26 ms on average, against a whole tick of 1.6 to 7.1 ms (about 2 to 4 %). Highest CPU (100 % = one core, the eight sims added together): `sim` 435 %, `rep-gateway` 451 %, `nw-testclient` 499 %, `edge-http` 121 %, `nats-server` 32 %, `world-director` 2 %. Memory of each `sim` with all its mobs: 699 to 780 MiB.

**Observed (runs 2 and 3).** The mobs did not cause the failures of run 2: their part of the tick was 0.35 ms of 9.5 ms in the busiest sim.

## 5. Fight stress

`cargo test --release -p sim fight_stress -- --ignored --nocapture`: 1,000 players, each next to one awake level-20 mob, in one `sim` with 4 tick threads, attacking every 500 ms, mobs respawning after 5 s, 1,800 ticks. 51,274 hits and 1,728 kills. Tick p50 2.90 ms, p99 5.13 ms, max 49.67 ms.

## What this proves

- Every mob of the open world lives in the sims, sleeps at almost no cost, and does not change what 5,000 players get: every M5 swarm check passes with all mobs loaded.
- 1,000 fights at once in one `sim` fit in about a sixth of a tick (p99).
- The combat loop works on the real loop and across sims: a mob kills a player of its own or of another sim, the death is saved at once, and the player lives again.
- Mob health from the datasheets matches nwdb for 99.3 % of its records once the variant level is taken into account.

## What this does not prove

- Anything the client sees: there is no combat message before M6-B.
- Awake mobs under live load: in the live run no mob woke, by design (no mob may see a bot). The stress test covers awake mobs in one `sim`.
- The live game's numbers: damage per weapon, mob health per type, and a mob's respawn time are Unknown (the offline study could not join them). Player death to respawn in the captures is 2.4 to 2.6 s (4 pairs); the config still has 10 s, for M6-B to change.
- A player who respawns next to a strong mob is killed again and again (final review I3); M6-B brings real respawn points.
