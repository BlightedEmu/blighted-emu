# Movement numbers

Date: 2026-09-02.

This note gives player speeds for the server move check. Mount speeds stay in `crates/nw-content/src/generated/mount_movement_data.rs`. Type 13 positions and the type 8 header are in `docs/re/2026-09-02-replication-and-movement.md`. This note does not decode type 1589 bodies.

The game was not run. No new traffic was recorded. Easy Anti-Cheat was not analyzed. One Ghidra pass was read-only, with analysis off, 10 decompiler threads, and a 24 GB heap. No body bytes, names, ids, exact positions, tokens, chat, or IP addresses are in this note.

Labels: **Observed** means a file, the binary, or a ledger showed the fact. **Inferred** means a conclusion from those facts. **Unknown** means the files do not show it.

## 1. Datasheet speeds

**Observed.** The field name is `Speed`. In `playeractions_nav.actionlist` a test says `GreaterThanOrEqual 1.65 Walk Speed`, and the value is `1.65`.

**Inferred.** The unit is metres per second. The action file does not contain the text `m/s`. Terrain height in this client is in metres.

The action lists are in `SharedDataStrm-part7.pak`. Encumbrance limits are in `SharedDataStrm-part5.pak`. Stat multipliers and common status effects are in `SharedDataStrm-part6.pak`.

| Number | Value | Unit | File | Field |
|---|---|---|---|---|
| Walk | 1.65 | m/s | `playeractions_nav.actionlist` | `Stand_Walk` `Speed` |
| Walk minimum | 1.40 | m/s | same file | `Stand_Walk` `MinSpeed` |
| Run | 3.25 | m/s | same file | `ServerValidatedCanRun` `Speed` |
| Run, weapon out | 3.25 | m/s | same file | `Combat_Nav_Stand` `Speed` |
| Sprint | 5.5 | m/s | `playeractions_sprint.actionlist` | `ServerValidatedCanSprint` `Speed` |
| Sprint, armed | 5.5 | m/s | same sprint file | `Stand+Armed` `Speed` |
| Crouch walk | 0.825 | m/s | nav file | `Crouch_Walk` `Speed` |
| Crouch, combat | 1.25 | m/s | nav file | `Combat_Nav_Crouch` `Speed` |
| Crouch sprint | 2.5 | m/s | sprint file | `Crouch` `Speed` |
| Prone walk | 0.1875 | m/s | nav file | `Prone_Walk` `Speed` |
| Swim | 3.0 | m/s | `playeractions_swim.actionlist` | `Swim_Nav` `Speed` |
| Wade sprint | 2.5 | m/s | sprint file | `Wade` `Speed` |
| Bow hip fire | 2.25 | m/s | nav file | `Bow_Hipfire_Movespeed` `Speed` |
| Bow aim | 1.65 | m/s | nav file | `Bow_ChargedAim` `Speed` |
| Sprint turn cap | 6 | m/s | sprint file | `InheritVelocityMaxSpeed` |

**Observed.** Player equip-load speed multipliers are all `1`. The fields are `EquipLoadMoveSpeedMultFast`, `Normal`, `Slow`, and `Overburdened` in `javelindata_encumbrancelimits.datasheet`. The player row has `EncumbranceBaseMax` 2000 and `EquipLoadBaseMax` 500.

**Observed.** `MoveSpeedMod` and `SprintSpeedMod` in `javelindata_statmultipliers.datasheet` each span 0 to 1.5. Common haste is `MoveSpeedMod` 0.1. Potion haste is `SprintSpeedMod` 0.4. Road sprint boost is `SprintSpeedMod` 0.15. Exhausted is `SprintSpeedMod` -0.15.

**Observed.** Sprint start delay is `Sprint_Delay_Timer` `BaseDuration` 0.85 seconds. Light combat is 0.85 seconds. Medium is 0.65 seconds. Heavy is 0.5 seconds.

**Observed.** Ghidra stores 3.25 at `0x14807ecc8` (`Nav_Walk`) and 5.5 at `0x14807ee28` (`Nav_Sprint`).

**Unknown.** `m_EffectiveHeightAboveGround` and `m_stepUpMaxDetectionHeight` have offsets and no number in this pass.

## 2. Capture speeds

**Observed.** The script read 695 ledger files with 10 processes. Reachable type 13 positions: 11,168. Tracks: 8,662. Decode failures: 0. The movement-test files, including the clean running and walking folder, have 0 reachable type 13 positions. A later member stays unread when an earlier body has no length.

**Observed.** A short snap is a horizontal step of about 2 m or more in at most 50 ms. That speed is at least 40 m/s. Fifteen such steps are removed. Seven more steps of at least 20 m/s in the 50 ms to 100 ms bin are removed. Fast travel is a step of 400 m or more on a gap longer than 5 seconds. Those steps sit outside the 100 ms window.

**Observed.** Kept steps in the at-most-100 ms window: 765. Median: 0.0 m/s to 0.1 m/s. 99th percentile: 0.0 m/s to 0.1 m/s. Maximum: 3.9 m/s to 4.0 m/s. That maximum is one step.

**Inferred.** That one step is the fastest reachable type 13 step under the teleport rule. It is not a repeated run. It sits above the datasheet run speed of 3.25 m/s and below the sprint speed of 5.5 m/s.

**Unknown.** A sustained run speed from the clean running capture. Those files have no reachable type 13 position.

## 3. Type 1589 timing

**Observed.** Message times only. The movement-test files have 9,365 client type 1589 messages and 9,300 gaps. The median gap is 30 ms to 40 ms. That bin holds 5,757 gaps. The 99th percentile is 60 ms to 70 ms.

**Observed.** Each of the 10 movement-test files has one gap of 1.5 s to 2.5 s. After that gap, the longest gap is 150 ms to 160 ms. Two gaps fall in that bin.

**Inferred.** The 1.5 s to 2.5 s gap is a break in the file. The longest gap in the continuous part is 150 ms to 160 ms.

**Unknown.** Which gaps fall while the player is moving. Those files have no reachable type 13 position.

## 4. Fall and jump

**Observed.** A fall here is a drop of at least 1 m, a horizontal move under 2 m, and an interval of at most 500 ms. That set has 0 steps. Downward steps in the at-most-50 ms bin: 10. Every drop is under 1 m. The fastest is 33.8 m/s to 33.9 m/s. One step.

**Inferred.** A drop under 1 m in at most 50 ms does not separate a fall from a short correction.

**Unknown.** The highest fall speed of a drop of at least 1 m.

**Observed.** A jump here is a rise of at least 0.3 m that comes back down within 1.5 s, with no teleport step. Completed rises: 0. Eight rises of at least 0.3 m have a horizontal move of at least 2 m. The tallest is 4.4 m to 4.5 m. The teleport rule removes them.

**Unknown.** The highest jump height that stays under the teleport rule.

## Summary

| Number | Value | Source | Status |
|---|---|---|---|
| Walk | 1.65 m/s | `Stand_Walk` `Speed` | Observed value. Inferred unit |
| Run | 3.25 m/s | `ServerValidatedCanRun` `Speed` | Observed value. Inferred unit |
| Sprint | 5.5 m/s | `ServerValidatedCanSprint` `Speed` | Observed value. Inferred unit |
| Crouch walk | 0.825 m/s | `Crouch_Walk` `Speed` | Observed |
| Swim | 3.0 m/s | `Swim_Nav` `Speed` | Observed |
| Equip-load speed | 1 | four `EquipLoadMoveSpeedMult*` fields | Observed |
| Haste cap | 0 to 1.5 | `MoveSpeedMod` `Max` | Observed |
| Sprint start delay | 0.85 s | `Sprint_Delay_Timer` | Observed |
| Highest honest capture speed | 3.9 m/s to 4.0 m/s | 765 type 13 steps. 22 snaps removed | Observed. One step |
| Send interval | 30 ms to 40 ms | 9,300 gaps in the movement test | Observed |
| Longest continuous send gap | 150 ms to 160 ms | 2 gaps, after one long break per file | Inferred |
| Highest fall speed | Unknown | 0 drops of at least 1 m | Unknown |
| Highest jump height | Unknown | 0 completed rises under the teleport rule | Unknown |

## Server defaults

| Setting | Default | Reason |
|---|---|---|
| `run_speed_mps` | 3.25 | That is `ServerValidatedCanRun`. |
| `speed_margin` | 1.7 | Sprint is 5.5 m/s. 5.5 / 3.25 is about 1.7. The fastest kept capture step is under 4.0 m/s, so it passes this cap. |
| `burst_seconds` | Unknown | The 0.85 s figure is a sprint start delay. It is not a speed-burst window. The longest continuous send gap is 0.16 s. |
| `ground_tolerance_m` | Unknown | The height-above-ground field has no number in this pass. |
| `max_height_above_ground_m` | Unknown | No completed jump stayed under the teleport rule. |

## Check

**Observed.** On 2026-09-04 a separate script measured client speeds from type 1589 itself. It used the record layout of `docs/re/2026-09-04-movement-message.md` (checked there: every body of the 695 ledgers decodes with 0 bytes left). It took the last bit 7 position of each message, sampled positions at least 250 ms and at most 1 s apart in one capture context, and left out steps longer than 50 m. It printed a histogram only.

**Observed.** Samples: 1,375,026. Below 0.25 m/s: 448,822. The bins of 0.25 m/s with the most moving samples:

| Speed | Share of moving samples |
|---|---|
| 1.50 to 1.75 m/s | 3.3 % (a peak between flatter bins) |
| 5.25 to 5.50 m/s | 6.5 % (the largest bin) |
| 5.50 to 5.75 m/s | 5.2 % |
| 6.00 to 6.25 m/s | 3.3 % |
| 6.25 to 6.50 m/s | 3.3 % |

**Observed.** 35.8 % of moving samples are below 5.0 m/s. 35.2 % are at 7.0 m/s or more. 2.6 % are at 20 m/s or more.

**Observed.** Capture-time gaps between client type 1589 messages of one context, all 695 ledgers: 10,608,615 gaps; the 30 to 40 ms bin holds 69.6 %; 99.755 % are at most 70 ms; 1,978 are 300 ms or more.

**Inferred.** The walk peak (1.65 m/s) and the sprint peak (5.5 m/s) of section 1 are the speeds players move at. The 6.0 to 6.5 m/s peak matches sprint with a 15 % `SprintSpeedMod` (5.5 x 1.15 = 6.3), for example the road sprint boost.

**Inferred.** Section 2's highest honest speed (3.9 to 4.0 m/s, from 765 type 13 steps) is far below what players reach. A cap of `run_speed_mps` x (1 + `speed_margin`) = 5.5 m/s (the Server defaults table) would refuse the 6.0 to 6.5 m/s peak.

**Unknown.** What the samples at 7 m/s and more are. Mounts and speed effects are two candidates; this check does not tell them apart.

**Observed.** A second run of the same type 1589 script followed the client's own positions per capture context (last bit 7 position of each message, height from the raw `u16` over -100 to 1,000 m). It printed statistics only.

| Item | Result |
|---|---|
| `arvaysa_running_forward` (clean capture), speed of moving samples | median 5.38 m/s, 90 % at most 5.55 m/s |
| `arvaysa_walking_forward` (clean capture), speed of moving samples | median 3.19 m/s, 90 % at most 3.51 m/s |
| Gaps between messages while the player moves, 15 clean movement files | largest 101 ms |
| Gaps while the player moves, all 695 ledgers | 5,572,712 gaps; 99 % at most 65 ms |
| Falls of at least 1 m (height never rises, no step faster than 30 m/s) | 29,251; median drop 1.9 m, 99 % at most 27.9 m, largest 128.6 m |
| Average fall speed of those falls | median 1.6 m/s, 99 % at most 10.0 m/s |
| Jumps (a rise of at least 0.3 m at 2 m/s or more, back within 0.3 m in 2 s) | 56,517; median 0.47 m, 99 % at most 2.90 m |

**Inferred.** Section 2's open items have answers from the player's own positions: the sustained run speed (3.2 m/s, the datasheet 3.25), the sprint (5.4 m/s, the datasheet 5.5), the gaps while moving, falls and jumps. The largest "jump" (85 m) and the largest fall speed (107 m/s) are lifts or teleports, not jumps or falls.

**Inferred.** A fall can start more than 64 m above the ground below it (the largest is 128.6 m). A server that refuses positions more than 64 m above the terrain can refuse such a fall.

## Files read

- `playeractions_nav.actionlist`, `playeractions_sprint.actionlist`, and `playeractions_swim.actionlist` in `SharedDataStrm-part7.pak`
- `javelindata_encumbrancelimits.datasheet` in `SharedDataStrm-part5.pak`
- `javelindata_statmultipliers.datasheet` and `javelindata_statuseffects_common.datasheet` in `SharedDataStrm-part6.pak`
- `docs/re/2026-09-02-replication-and-movement.md`
- `docs/re/2026-09-02-carrier-captures.md`
- 695 ledgers under `local-data/captures/`
- Ghidra program `NewWorld.exe` in `<re-scratch>/ghidra`, read-only

## Height fields pass

This pass searched only the two height fields. It did not redo speeds, gaps, falls, or jumps.

**Observed.** `m_EffectiveHeightAboveGround` is bound in `FUN_140d3aa50` (`0x140d3aa50`) at offset `0x30`. `m_stepUpMaxDetectionHeight` is bound in `FUN_14416afd0` (`0x14416afd0`) at offset `0x18`. Each call passes a cleared default holder. The same holder is cleared for the other fields in those functions. The holder is not a stored height.

**Observed.** Those two strings have no other code reference. A search of float loads in `FUN_140d3aa50`, `FUN_14416afd0`, and the functions from `0x140d00000` to `0x140d40000` and from `0x144150000` to `0x144180000` found no constant stored into these fields. The text `CharacterController` is not in the image. The names `groundTolerance`, `stepOffset`, `stepHeight`, and `maxJumpHeight` are not in the image.

**Observed.** `FUN_14416d520` (`0x14416d520`) uses the constant `0.5` in a reciprocal step. It does not store that constant into offset `0x18`. `FUN_14416c200` (`0x14416c200`) writes a sum of two runtime floats to offset `0x30` of a different object.

**Unknown.** The number of `m_EffectiveHeightAboveGround`, and the number of `m_stepUpMaxDetectionHeight`. A static read cannot give either number. The only functions that name the fields record the name and the offset. They do not record a constant, and this pass did not find a function that computes the value.
