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

### 2.1 Slayer slots, sequences and actions (second pass)

**Observed** (own retail states against own inputs: `duel_capture`, `arv_charged_attack_hold`,
`arvaysa_walk_running_crouch_prone`, `Hive bow and spear in duels`; scanners `pcorr`, `pact`,
`panim` in scratch):

- Children 13-24 are four slayer slots of (`slayerStateId`, `slayerStateIdStarted`,
  `slayerSequenceId`). Slot 1 carries the movement and attack states; slot 2 (16-18) holds
  `2c`/`2d`; slot 4 (22-24) toggled `20`/sequence `b533` and `00`/`00` in the duel with no block
  input near.
- An attack does not change `slayerStateId` when the state stays: the client sends a new
  `slayerStateIdStarted` (the current ALC state number) and a new `slayerSequenceId`, and child
  17 repeats the start. Holding attack in the charged capture cycled four sequences, as prefix
  values 1336, 5050, 2992, 5056 (bytes `b814 ba4e b02e 804f`), and the charged release gave 4313
  (`9943`). The duel's attacks used 1336, 5050, 5056 and others (`9407`, `a507`, `9509`, `9e04`,
  `aa05`): the ids are shared between captures, so they name actions, not a per-player counter.
- The decoded state ids seen are small (1, 7, 8, 12, 13, 26, 27, 31, 44, 45); the sequence ids run
  to about 5,000. The player CAGE grid (`playerdata/cage/player.grid`) has 59 aliases, 63 action
  lists and 530 actions (541 named actions in the lists): neither its alias order nor its action
  order gives these numbers (Attack is alias 7 and Dodge 12, which fit two states; 26 and 27, the
  run toggle's, do not). No pak file holds the slayer script; the client builds it at run time
  (`SlayerScriptComponent`, `SlayerScript::Module::SlayerScriptData`).
- Crouch and prone: the capture's crouch toggles (input bit groups `v1b45 v2b45 v4b1 v6b6 v8b28
  v8b56` and `v1b32 v1b49 v3b23 v3b56 v4b32 v7b34`) changed no slayer state, no `globalFragTags`
  and no Mannequin scope of the own state.

**Unknown:** the table from (weapon, action, combo step) to slayer state and sequence, and how the
client shows crouch to others. The captures name too few actions per weapon, and the weapon in
the hand is not readable from them (the own card's paperdoll lies past members our walk cannot
read). Without the table the sim sends no attack, dodge or crouch state: a wrong sequence id
would play another action's animation. Ways to learn it: the client's slayer script tables at
run time (a probe started with the game, not attached to it), or a capture of each weapon's
light, heavy and charged attacks and a dodge with the weapon named.

### 2.2 The client names the actions it starts (third pass)

Source: the retail newchar capture (`local-data/captures/own/newchar-linux-2026-09-12`),
decoded fields of input 1589 against the own ALC slot-0 children, scratch scanners `rowone`,
`rowmap` and `chain`.

- Observed: input record row 1 (three prefix integers, `InputRecord::row_1`) is non-zero only
  when the client starts a slayer action. Each of the 73 such rows came back in the next own
  ALC state (dt 30 to 38 ms; once 84 to 87 ms):
  - row 1 value 0 is the sequence: child 15;
  - row 1 value 1, low 16 bits, is the state: child 13, sent only when it changed;
  - row 1 value 1, bits 16 to 31, is the second slot's state, sent as that value plus one
    (child 16: `0x0024` came back as `0x25`; `0xffff`, none, is 0);
  - row 1 value 2 is the second slot's sequence: child 18, with child 16 only when not 0;
  - children 14 and 17 (started) are the record's counter.
  Example, record 2286: row 1 `[1038, 0x0024001b, 5008]`, next own state `13=1b 14=08ee
  15=8e10 16=25 17=08ee 18=904e`. An earlier byte search (`inseq`) missed this: the row is
  prefix-coded on its own, not the bytes of the ALC child.
- Observed: the server made 481 further own slayer changes with no client row: the movement
  states (state 26 sequence 2794 = run, state 27 with 2415, 1370, 1405 and others = walk;
  states 31, 7 and 9) and fixed follow-ups of client actions, one to four states later (0 to
  132 ms): 5047 then 487, 5055 then 4603, 5051 then 484, 5056 then 468, 5050 then 485, 3120
  then 2030, 1221 then 463 (5 to 13 times each, never another).
- Observed: child 2 (`slayerSeqTimeRel`) is one raw byte. It went `0x80`, `0x83`, `0x86`, …
  about 3 per 33 ms state after an action, and was `0x7f` or `0x7d` in other echo states.
  Its unit and meaning: Unknown; the sim does not send it.
- Inferred from the client code: neither lock-on gate reads the own slayer state.
  `FUN_142c95760` sends bus query 26 to the candidate target entity (its own handlers answer),
  and `FUN_14171b010` tests relation flag bits 2 and 4 (`+0x34`) of both sides. So echoing the
  own slayer state is not expected to make the client pick lock targets.

## 3. What the sim sends (built 2026-09-18)

- Each tick every player becomes one replica that all its viewers share
  (`bins/sim/src/world/replica.rs`): its look, the items that show on it (the public paperdoll
  only, as retail: every remote paperdoll of the duel and the 3v3 arena sets slot 2 alone), its
  health, its block, its camera yaw as facing, its view as look direction, its lock or homing
  target, its run toggle as slayer state `1a`/`1b`, all on its own ALC clock (`OwnClock`).
- `lookDir` (child 12) is a unit direction, not a facing quat: `(-sin yaw cos pitch, cos yaw cos
  pitch, -sin pitch, 0)` from the input's view angles (record bit 6: yaw, pitch), in the
  smallest-three form with w flagged 0 (Observed: own duel states against the inputs; remote tags
  `0x10`, `0x18`, `0x30`, `0x39` the same form).
- Card: the player prefab with the look, paperdoll and health, and a full first ALC state on the
  player's clock anchor. Later: `AlcState::remote_step` every time the player goes out (number,
  time, anchor, place, facing, look direction, slayer state), plus the vitals change, the death
  or new life, the paperdoll after a draw or gear change, and `isBlockActive`.
- Priority (`RemotePlan`): within 30 m or in combat every tick, within 60 m every second tick,
  farther every fourth; at most 24 remote players per viewer per tick (about 1 KB of states,
  retail's p99 datagram); a new player's card and any change always go; card refreshes and the
  longest-waiting players go first.
- Who a player sees (`Interest`): those that fight it first (one targets the other, or both are
  in combat), then the nearest, at most `max_visible_players`. That is no bundle limit (bundles
  split by bytes; the per-tick budget above bounds the bytes): 200 by default, at most 500. Each
  seen player costs a slot and a card refresh every 30 bundles, so 500 seen means about 17 cards
  per tick. Retail's 45-vs-45 war showed one client 180 other players.
- Ghosts across sim borders carry the same replica (`nw_bus::PlayerGhost`, at most 240 bytes per
  ghost: the view, the target and the slot and item of each item that shows), so a border player
  keeps moving, turning, blocking and changing weapons for the players of the other sim.

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

- The slayer states the server makes on its own (movement states 31, 7 and 9, and the
  follow-ups of actions other than those of section 2.2) and their triggers; crouch and prone;
  the meaning of child 2.
- The slayer state and sequence of each action (section 2.1). Retail remote Mannequin scopes
  (children 28, 33, 34) came only in a few states of the duel (`33=0700cc2c15000053` with tags
  `0210...`), not with the attacks.

## 6. Corrections (2026-09-20, movement study)

Source: the retail newchar capture (`local-data/captures/own/newchar-retail-2026-09-18`), the
autorun capture (`brim_runtoggle_spam`), scratch scanners in `local-data/scratch-work/movement2`.

- **Observed:** slot value 0 bit 14 is the forward navigation input (`ForwardNav` in the action
  lists), not a run toggle: it is set in all 12,756 newchar records with the forward key and in
  no other, and in the autorun capture each press of value 1 bit 24 (autorun) turned it on with
  no movement key, while the own place moved forward at the sprint speed. The slayer states `1a`
  and `1b` of section 2 follow the start and the stop of a move, not a mode. The sim now moves
  the body forward on it and sends the movement states of `bins/sim/src/world/slayer.rs`
  (start `1a`/`aa2b`, back start `1b`/`9814`, stop `1b`/`bd15`, restart `1b`/`b625`, idle
  `1f`/`b910` 39 records after a stop: the most common sequence of each transition; the rest
  vary with what the capture does not show, Unknown).
- **Observed:** every own state is numbered with the counter of the newest input record the
  server moved the body by (39,955 of 39,955 distinct own numbers; 36 ms median after the
  record); a state repeats unchanged until a newer record comes. `slayerStateIdStarted` is the
  counter of the record that started the state. The sim numbers own and remote states so
  (`StateNumber`, members.rs); clients that number no records keep the 33 ms clock.
- **Observed:** child 2 (`slayerSeqTimeRel`) and child 3 (`slayerSeqTimeAbs`, a `half`) give the
  time into the slot-1 sequence: `abs + (rel - 0x7f) × 4/255 s`, the sum of the record frame
  times since the start record. A new `abs` comes with each start and every 2 s. The own states
  carry it now (`SequenceClock`, alc.rs). Unknown: the remote states' child 2 (the duel's starts
  carry `7e` and `7f`), and children 4 to 9.
