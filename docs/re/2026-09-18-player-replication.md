# Player replication: what other players see

Date: 2026-09-18. Build: Steam `25810808`. Labels: **Observed**, **Inferred**, **Unknown**.

Sources: retail ledgers `duel_capture`, `3v3_arena_group`, `arvaysa_ES_attacker_war_45vs45`,
`arvaysa_walk_running_crouch_prone`, `brim_runtoggle_spam`, `arv_charged_attack_hold`
(`local-data/captures/open-world/`); scratch scanners over them (counts, child values and a few
bodies; no capture bytes kept in the repo besides the test vectors below); the client decompile
(`local-data/re-sweep`, strings only).

## 1. Retail remote players

**Observed** (scanner over the three PvP captures):

| Measure | Duel | 3v3 arena | War 45v45 |
|---|---|---|---|
| Remote ALC gap p50 / p90 / p99 | 17 / 36 / 317 ms | 16 / 19 / 50 ms | 33 / 135 / 849 ms |
| Bundle bytes p50 / p99 | 280 / 788 | 183 / 473 | 791 / 1,028 |

- A nearby remote player gets a type-11 state in every retail tick (about 60 Hz). In the war,
  far players come less often and every datagram stays under about 1 KB.
- Remote ALC children: `idRel` 0 and `timeOffsetRel` 1 on almost every state, `worldPosRel` 10,
  `rotation` 11, `lookDir` 12, the slayer children 13-24, the Mannequin scope children 28, 33
  and 34, `idAbs`/`timeOffsetAbs` 25/26, `worldPosAbs` 27 now and then, `timeAnchor` 40.
- A retail remote state (duel, 51302 ms): `01e7c00e80c5fd7fffffff47441a28c5ab1846121ebc452b6f312ba9`
  = `idRel` c5, `timeOffsetRel` fd, `slayerSeqTimeRel` 7f, `worldPosRel` at the anchor, `rotation`
  4744, `slayerStateId` 1a, `slayerStateIdStarted` 28c5, `slayerSequenceId` ab18, `worldPosAbs`.
- `slayerStateIdStarted` (child 14) is the full ALC state number in which the slayer state
  started: `14=28ad` comes with `idRel` `ad`; its high byte runs on at 30 per second.
- Other remote classes after the card: vitals 15, paperdoll 3183 (weapon draws, gear swaps),
  damage receiver 1528 (block on and off), delayed events 3139, interactor 3752.

## 2. Slayer states (Inferred meanings)

"Slayer" is the client's SlayerScript state machine (`SlayerScriptComponent`, ...); the state ids
index the character's script.

**Observed** (own retail states against the own inputs):

- In `brim_runtoggle_spam` each press of input slot value 1 bit 24 flips slot value 0 bit 14;
  the own state's `slayerStateId` goes `1a` (sequence `aa2b`) when the bit is set and `1b`
  (sequence `b625`, sometimes `9a15`, `bd15`) when it clears, 24 times each.
- In `arvaysa_walk_running_crouch_prone`, the player moved at a median 2.49 m/s (p90 6.35) in
  state `1a` and 1.95 m/s (p90 2.15) in `1b`; `1f` (sequence `b910`) follows a start of movement;
  `07` (sequence `8d4c`) once, with `dataBits` `d1`.

**Inferred:** bit 14 of slot value 0 is the run toggle's mode, `1a` running and `1b` walking. Only
these four states appear in the crouch-and-prone capture, so the crouch and prone states are
**Unknown**; so are the states of attacks, dodges and abilities (the duel's remote `07`/`08` came
with `slayerStateIdStarted` in child 17 and children 44 and 48).

## 3. What the sim sends (built 2026-09-18)

- Each tick every player becomes one replica that all its viewers share
  (`bins/sim/src/world/replica.rs`): its look, the items that show on it (paperdoll only; the bag
  stays private), its health, its block, its camera yaw as facing and look direction, its run
  toggle as slayer state `1a`/`1b`, all on its own ALC clock (`OwnClock`).
- Card: the player prefab with the look, paperdoll and health, and a full first ALC state on the
  player's clock anchor. Later: `AlcState::remote_step` every time the player goes out (number,
  time, anchor, place, facing, look direction, slayer state), plus the vitals change, the death
  or new life, the paperdoll after a draw or gear change, and `isBlockActive`.
- Priority (`RemotePlan`): within 30 m or in combat every tick, within 60 m every second tick,
  farther every fourth; at most 24 remote players per viewer per tick (about 1 KB of states,
  retail's p99 datagram); a new player's card and any change always go; card refreshes and the
  longest-waiting players go first.
- Ghosts across sim borders carry the same replica (`nw_bus::PlayerGhost`, at most 240 bytes per
  ghost), so a border player keeps moving, turning, blocking and changing weapons for the players
  of the other sim.

## 4. Measuring sync

`nw-testclient swarm` with `[swarm] block_toggle_ms` set: each bot changes its block that often;
every bot times the delay from another bot's change to its sight in its own bundles. The summary
line gives the action-to-visible p50 and p99, and the check `action-to-visible p99 within its
limit` uses `max_sync_latency_ms` (100 ms: two 33 ms ticks and the loopback network). Run it as
in the M5 load note (`docs/re/2026-09-04-m5-load-test.md`), for example:

```
NW_SECRET_TESTCLIENT_SWARM_PASSWORD=... target/release/nw-testclient swarm --bots 5000
```

## 5. Unknown

- The slayer states of crouch, prone, sprint, attacks, dodges, blocks and abilities, and the
  meaning of `slayerSequenceId`.
- The Mannequin scope children (28, 33, 34) of a remote player: the player fragment and action
  numbers of attacks and emotes. Without them other players see no swing animation.
- `lookDir` with pitch: we send the yaw only, in the `rotation` form.
- The durability, bind flag and instance id a remote paperdoll needs: a ghost's items carry
  none (Inferred: they do not show).
