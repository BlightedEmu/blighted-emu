# Component tables reached by the bundle walk

Date: 2026-09-06.

Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`.

Offline only. The game was not run. Easy Anti-Cheat was not analyzed. No body bytes, names, ids, positions, tokens, or IP addresses are in this note.

The four classes are the ones the replication-rate note and the admin capture viewer name as stops. Round 3 already gave slot 0 widths for three of them. This note names the children and re-fits them on the walk that also passes slot-1 vitals.

Labels: **Observed**, **Inferred**, **Open**.

Ghidra was the existing project, read-only, analysis off, 14 decompiler threads. REA `open_binary` was not called. No REA Evidence ID.

## 1. Class 4297, `MB::StaminaComponentReplicatedState`

**Observed.** Constructor `FUN_14671a230`. Six `FUN_141775c60` calls. Each sets the slot argument with `xor r9d, r9d`, so each child is slot 0. Each child object is built with vtable `0x1480ff160`. Virtual `+0x30` is `FUN_142a42f30`, the big-endian `f32` reader, 4 bytes.

**Observed.** Names, in registration order. The first two are the bytes at `0x14855e1c0` and `0x147fec6c4`. The other four are the string operands in the constructor.

| Child | Name | Reader | Bytes | Type |
|---|---|---|---:|---|
| 0 | `cur` | `FUN_142a42f30` | 4 | big-endian `f32` |
| 1 | `max` | `FUN_142a42f30` | 4 | big-endian `f32` |
| 2 | `winded` | `FUN_142a42f30` | 4 | big-endian `f32` |
| 3 | `regen` | `FUN_142a42f30` | 4 | big-endian `f32` |
| 4 | `multMax` | `FUN_142a42f30` | 4 | big-endian `f32` |
| 5 | `multRegen` | `FUN_142a42f30` | 4 | big-endian `f32` |

**Observed.** On the 696 ledgers this walk reached 2,721,822 bodies and every one ended on a following member header or on the payload end. Bodies the reader refused: 0. Bodies that parsed and then missed the next header: 0.

## 2. Class 2913, `MB::CombatStatusComponentReplicatedState`

**Observed.** Constructor `FUN_1466f1f10`. Four children, each with slot argument 0.

| Child | Name | Reader | Bytes | Type |
|---|---|---|---:|---|
| 0 | `replicatedInCombatFlag` | `FUN_1417b3cb0` | 1 | byte, must be 0 or 1 |
| 1 | `replicatedInPvpCombatFlag` | `FUN_1417b3cb0` | 1 | byte, must be 0 or 1 |
| 2 | `replicatedCombatLoggedOutTimePoint` | `FUN_14279e480` | 8 | big-endian `u64` |
| 3 | `replicatedCombatConcludedTimePoint` | `FUN_14279e480` | 8 | big-endian `u64` |

`FUN_1417b3cb0` reads one byte and fails when the byte is 2 or more. `FUN_14279e480` calls `FUN_14087ad90`, which reads 8 bytes and byte-swaps them (`FUN_1461679c0`).

**Observed.** Reached bodies 444,539. Ended on a next header or the payload end: 444,539. Refused: 0. Edge misses: 0.

## 3. Class 3139, `MB::DelayedEventComponentReplicatedState`

**Observed.** Constructor `FUN_143066370`. Two children, each with slot argument 0.

| Child | Name | Reader | Bytes | Type |
|---|---|---|---:|---|
| 0 | `totalDelayDuration` | `FUN_142a42f80` | 2 | big-endian `u16` (`FUN_14087ae90`) |
| 1 | `delayedEventCompleted` | `FUN_1417b3cb0` | 1 | byte, must be 0 or 1 |

**Observed.** Reached bodies 652,456. Ended on a next header or the payload end: 652,456. Refused: 0. Edge misses: 0.

## 4. Class 3451, `Javelin::GroupDataComponentReplicatedState`

**Observed.** Constructor `FUN_1433d8bf0`. The base object already has slot 0. The constructor calls `FUN_141677dd0` five times and stores the returned slot indexes at object offset `0xb60`. Those five calls append slots 1 through 5.

**Observed.** A registration whose slot argument is `xor r9d, r9d` is slot 0. A registration whose slot argument is loaded from `0xb60` plus the loop index is one of slots 1 through 5. The five dynamic slots each receive the same nine names, in this order:

| Child | Name |
|---|---|
| 0 | `groupMemberWorldPositionRelative` |
| 1 | `groupMemberWorldPositionAbsolute` |
| 2 | `groupMemberGameModeIndex` |
| 3 | `groupMemberFaction` |
| 4 | `groupMemberLevel` |
| 5 | `groupMemberWaypointPosition` |
| 6 | `groupMemberPingData` |
| 7 | `groupMemberPingTargetPosition` |
| 8 | `groupMemberHouseIds` |

**Observed.** Slot 0 receives the leader positions first, then five copies of each of `groupMemberRole`, `groupMemberMatchmakingRole`, `groupMemberHealth`, `groupMemberMana`, and `groupMemberFlags` (the copies are interleaved: one role, one matchmaking role, one health, one mana, one flags, then the next copy), then `groupLeaderPingData`, then five `raidMemberPosition`, then five `groupMemberBaseData`, then these singles: `groupId`, `groupDungeonTime`, `groupRaidGroupIndexField`, `groupNameIndex`, `groupIconIndex`, `groupColorIndex`, `groupObjectiveIndex`, `groupDungeonState`, `groupGameModeIdx`, `groupGameModeDifficulty`, `joinQueueThrottleActive`.

**Observed.** Two slot-0 readers were identified from the object vtables in the constructor. `groupLeaderWorldPositionRelative` uses vtable `0x1481ce088`, virtual `+0x30` `FUN_1433f5410`, three bytes. `groupLeaderWorldPositionAbsolute` uses vtable `0x1480ff408`, virtual `+0x30` `FUN_142a433d0`: two big-endian `f32` and one big-endian `u16`, 10 bytes. The other children were not given a size in this round.

**Observed.** The fit walk reached 908,890 class 3451 bodies and stopped on all of them. A header scan of the same stop recorded the slot byte. Slot byte 1 (only bit 0) is 103,935 bodies. In the 16 most common slot bytes the largest count is 111,617. Those 16 values already sum to more than 789,000 bodies. No one slot byte is 99.9% of the reached set. The child masks on slot byte 0 or 1 also do not collapse: the largest mask count in that subset is 10,792.

**Open after round 4.** A child size for every slot-0 child and every dynamic-slot child, checked to 99.9% of the 908,890 bodies. The constructor order and the slot operand are known. The header scan shows the body is not one slot. The remaining readers were not decompiled, so a width fit was not run.

## Files

- `<re-scratch>/scratch-m6/round4/scan_round4.py`, log `scan-round4.log`
- `<re-scratch>/scratch-m6/round4/scan_group.py`, log `scan-group.log`
- `<re-scratch>/scratch-m6/round4/M6Round4.java`, output `ghidra-out/`
- `docs/re/2026-09-04-m6-mob-replication-rate.md`
- `docs/re/2026-09-04-admin-capture-viewer.md`
