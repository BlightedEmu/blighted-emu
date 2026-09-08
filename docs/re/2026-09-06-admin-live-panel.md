# Admin tools part B: live panel exit test

Date: 2026-09-06. Code: the commits on `main` after `0782951`, to the commit that adds this record.

This note records the exit test of spec `2026-09-04-admin-tools-design.md` section 1.3, part B. It gives counts only: no ids, names, positions, tokens or addresses.

## Result

| Criterion | Result |
|---|---|
| B1. `cargo test --workspace`, `just test-web`, `just check`, `just test-infra` pass | Pass. |
| B2. 8 sims with mobs: 1,000 bots from the buttons, on the map, one followed, all stopped; every tile has a value; tick p99 of every sim within one tick | Pass (see 2). |
| B3. Snapshot on, sim with 1,000 players: extra tick time under 0.5 ms average (`sim_snapshot_seconds`) | Not met: 2.35 ms per snapshot (0.32 ms when spread over the ticks). See 3. |
| B4. This record | Yes. |

## 1. Program checks

`just check` (game data guard, `cargo fmt --check`, clippy with `-D warnings`, `cargo test --workspace`, `cargo deny check`, the shell script tests, `just test-web` with 30 page tests) exits 0. `just test-infra` exits 0, with the new `a_published_heartbeat_and_snapshot_reach_the_view` (the panel reads a heartbeat and a snapshot from the Docker Compose NATS).

## 2. Live test with 8 sims

**Observed.** Set-up: `scripts/live-stack.sh 8` (edge-http on 8443, the director, 8 sims with 4 tick threads each and snapshots every 250 ms, the gateway with 12 workers, the panel). The sims hold 115,494 mobs together. Release build.

From the page, the "+100" button was clicked 10 times, one click per 2 s (the batches start one after another). Results:

- 1,000 players and 1,000 bots on the map; 0 bots failed to start. The report of the first batch: 232 handoffs, 0 failed; every bot got at least 99.88 % of its bundles; the worst bundle gap 74 ms.
- A player row was selected: the selection shows the bot and the map follows it.
- "Stop all": all 10 batches closed, printed their reports and ended in about 2 s; 0 were killed after the 30 s grace.
- Every tile had a value with 1,000 bots: tick average 4.2 ms and p99 20 ms (every sim together); 28,444 packets in and 41,124 out per second; 2.46 MiB/s in and 15.43 MiB/s out; 990,977 bundle members per second; 31,504 mobs awake and 83,990 asleep; CPU in % of one core: sims 155, gateway 98, test client 141, director 0, login server 0.
- Bots of the swarm do not fight. Mobs that they wake kill some of them; the panel shows those as dead. So this is a full load (mobs, cards, vitals, combat), not positions only.

Tick time per sim over 60 s at 1,000 bots (1,800 ticks per sim; p99 is the bucket bound), after the change of section 4:

| Sim (players) | Snapshots on: average / p99 | Snapshots off: average / p99 | Snapshot average |
|---|---|---|---:|
| 1 (0, sea) | 0.02 / 1 ms | 0.02 / 1 ms | 0.013 ms |
| 2 (about 120) | 1.68 / 5 ms | 1.49 / 5 ms | 0.073 ms |
| 3 (about 118) | 4.28 / 10 ms | 4.13 / 10 ms | 0.258 ms |
| 4 (200) | 9.00 / 20 ms | 8.56 / 20 ms | 0.786 ms |
| 5 (about 85) | 1.54 / 5 ms | 1.59 / 5 ms | 0.081 ms |
| 6 (about 218) | 6.95 / 20 ms | 6.48 / 10 ms | 0.576 ms |
| 7 (about 185) | 5.46 / 10 ms | 5.02 / 10 ms | 0.443 ms |
| 8 (74) | 2.75 / 5 ms | 3.16 / 5 ms | 0.213 ms |

Every sim stays within one tick (33 ms) in both runs. With snapshots on, the tick average of a sim is at most 0.47 ms higher (sim 6); the runs differ by noise of the same size (sim 8 is faster with snapshots on).

## 3. Snapshot cost at 1,000 players on one sim

**Observed.** `scripts/live-stack.sh 1`: one sim owns the whole map; the same 10 batches of 100 put all 1,000 bots on it. Over 60 s:

| | Snapshots on | Snapshots off |
|---|---|---|
| Ticks in 60 s | 1,782 | 1,800 |
| Tick average | 22.46 ms | 24.07 ms |
| Tick p99 (bucket) | 50 ms | 33 ms |
| Awake mobs | 23,085 | (not shown with snapshots off) |
| `sim_snapshot_seconds` average | 2.351 ms (240 snapshots) | none |

The snapshot time grows with the awake mobs, not with the players: this sim sends 23,085 awake mobs in each snapshot. Spread over the ticks, it adds 2.351 ms × 240 / 1,782 = 0.32 ms per tick on average, under the 0.5 ms of B3. One snapshot takes 2.35 ms, above 0.5 ms. One sim with 1,000 players and 23,000 awake mobs is at its limit with or without snapshots (tick average 22 to 24 ms of 33 ms); the p99 bucket is one step higher with snapshots on.

`cargo test --release -p sim snapshot_speed -- --ignored --nocapture` builds the snapshot of 23,000 awake mobs in 0.30 ms on its own. In the running sim the mobs lie apart in memory and the PC also runs the bots and the gateway, so the same work takes about 8 times as long.

**Result.** The spec measures B3 with `sim_snapshot_seconds`, which is the time of one snapshot: 2.35 ms against 0.5 ms, so B3 is not met. Spread over the ticks it is 0.32 ms per tick, and the tick p99 bucket went from 33 ms to 50 ms with snapshots on. The cause is the number of awake mobs (23,085), and the time of one snapshot grows with them. With 8 sims the busiest sim takes 0.79 ms per snapshot (section 2). Ways to meet B3 are a longer snapshot interval, a snapshot of a part of the mobs per interval, or a mob table laid out in one block of memory; none is done in part B.

## 4. Changes made by the live checks

The live checks of part B found these faults. Each fix has a test that failed first:

1. An until-stopped swarm printed its report but did not exit: the runtime waited for the blocking read of standard input. Fixed in the test client's `main`.
2. Every until-stopped bot failed with "the walk takes too long": its route asked for 24 h, but a route may take at most 1 h. Its route is now half the longest walk, and the bot starts a new one where it stands.
3. Ctrl-C did not stop the panel while the live page was open: the server waited for the live stream, which never ends. The stream now ends when the server closes.
4. The snapshot and the mob gauges walked the awake mobs once per state (6 walks). They now walk them once. On the busiest of 8 sims the snapshot went from 1.40 ms to 0.79 ms.

## 5. Larger runs by the user

**Observed by the user, not measured here.** With the live panel, about 7,000 bots ran on this PC; above about 7,000, bots began to drop. The cause is Unknown. The M5 load record (`docs/re/2026-09-04-m5-load-test.md`) found the same kind of limit at 10,000 positions-only bots, with the bots and the servers on one PC.
