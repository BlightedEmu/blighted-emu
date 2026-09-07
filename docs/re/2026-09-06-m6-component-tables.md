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

## Round 5

**Observed.** The object has six slots. The base constructor supplies slot 0. `FUN_1433d8bf0` calls `FUN_141677dd0` five times and stores slots 1 to 5 at object offset `0xb60`. `FUN_1417b4110` tests one low bit per slot. Six slots use one byte. Bits 0 to 5 are the slots. Bit 0 is slot 0. Bits 1 to 5 are the five dynamic slots, in order. The function does not test bits 6 and 7.

**Observed.** Slot 0 has 49 children. A child byte holds 7 children. Bit 7 reads another child byte. Forty-nine children are 7 child bytes when the last byte does not continue. Each dynamic slot has 9 children: one child byte for the first 7, and a second byte for the last 2. The same nine readers are used on every dynamic slot.

**Observed.** Readers, from the vtable stored on the child object. A fixed length is the success path. A failed read does not return a shorter success.

Slot 0, in registration order:

| Child | Name | Reader | Bytes |
|---|---|---|---:|
| 0 | `groupLeaderWorldPositionRelative` | `FUN_1433f5410` | 3 |
| 1 | `groupLeaderWorldPositionAbsolute` | `FUN_142a433d0` | 10 |
| 2, 7, 12, 17, 22 | `groupMemberRole` | `FUN_14279e210` | 1 |
| 3, 8, 13, 18, 23 | `groupMemberMatchmakingRole` | `FUN_14279e210` | 1 |
| 4, 9, 14, 19, 24 | `groupMemberHealth` | `FUN_14279e210` | 1 |
| 5, 10, 15, 20, 25 | `groupMemberMana` | `FUN_14279e210` | 1 |
| 6, 11, 16, 21, 26 | `groupMemberFlags` | `FUN_1433f5310` | 2 |
| 27 | `groupLeaderPingData` | `FUN_1433f5170` | see below |
| 28 to 32 | `raidMemberPosition` | `FUN_1433f5520` | 6 |
| 33 to 37 | `groupMemberBaseData` | `FUN_1433f5120` | see below |
| 38 | `groupId` | `FUN_1433f5360` | 16 |
| 39 | `groupDungeonTime` | `FUN_14279e480` | 8 |
| 40 | `groupRaidGroupIndexField` | `FUN_14279e210` | 1 |
| 41 | `groupNameIndex` | `FUN_14279e210` | 1 |
| 42 | `groupIconIndex` | `FUN_14279e210` | 1 |
| 43 | `groupColorIndex` | `FUN_14279e210` | 1 |
| 44 | `groupObjectiveIndex` | `FUN_14279e210` | 1 |
| 45 | `groupDungeonState` | `FUN_14279e210` | 1 |
| 46 | `groupGameModeIdx` | `FUN_14279e210` | 1 |
| 47 | `groupGameModeDifficulty` | `FUN_14279e210` | 1 |
| 48 | `joinQueueThrottleActive` | `FUN_1417b3cb0` | 1, must be 0 or 1 |

`FUN_142a433d0` reads two big-endian `f32` values and one big-endian `u16` (`FUN_14087adf0`). `FUN_1433f5520` reads three big-endian binary16 values (`FUN_14087ae90`). `FUN_1433f5360` reads 16 bytes. `FUN_14279e480` reads 8 bytes.

Dynamic slots, in registration order. The same row is used for slots 1 to 5:

| Child | Name | Reader | Bytes |
|---|---|---|---:|
| 0 | `groupMemberWorldPositionRelative` | `FUN_1433f5410` | 3 |
| 1 | `groupMemberWorldPositionAbsolute` | `FUN_142a433d0` | 10 |
| 2 | `groupMemberGameModeIndex` | `FUN_1433f5260` | 1, or 2 when the first byte is 1 |
| 3 | `groupMemberFaction` | `FUN_14279e210` | 1 |
| 4 | `groupMemberLevel` | `FUN_14279e210` | 1 |
| 5 | `groupMemberWaypointPosition` | `FUN_1433f53c0` | 8 |
| 6 | `groupMemberPingData` | `FUN_1433f51c0` | see below |
| 7 | `groupMemberPingTargetPosition` | `FUN_1433f5210` | 9 |
| 8 | `groupMemberHouseIds` | `FUN_1433f4d60` | see below |

`FUN_1433f53c0` calls `FUN_14087acf0`, two big-endian `f32` values. `FUN_1433f5210` calls `FUN_145ce7bb0`, one big-endian `u64` and one byte. `FUN_1433f5260` fails when the first byte is 2 or more. A first byte of 0 is the whole child. A first byte of 1 reads one more byte.

**Observed.** `groupLeaderPingData` calls `FUN_145ce79f0`, which calls `FUN_145ce7aa0` and then reads one byte. `groupMemberPingData` calls `FUN_145ce7aa0` only. On the success path, `FUN_145ce7aa0` reads one byte that must be at most 14, then three big-endian `f32` values (`FUN_1415acfc0`), then one byte that must be 0 or 1, then one byte. A tag above 14 fails the reader. That is 15 bytes, or 16 for the leader.

**Observed.** `groupMemberBaseData` calls `FUN_145ce6b10`, which calls `FUN_14087aab0`. That helper reads one flag byte. Bit 0 set reads 16 bytes. Bit 0 clear calls `FUN_1407da950`.

**Observed.** `groupMemberHouseIds` reads a short prefix count. A count above 0 reads items in groups of at most 8. Each group is one byte. Each item is a wide prefix integer (`FUN_14087b770`) and an id (`FUN_1461acca0`: one byte, and a wide prefix integer when that byte is 1). A set bit then reads the long form `FUN_145ce6bf0`. A count of 0 calls virtual `+0x78` (`FUN_1433f5610`): one id, then a short prefix count that must be at most `0x2000000`, then that many long forms. `FUN_145ce6bf0` plus `FUN_1415b6a00` reads 16 bytes, two big-endian `u64` values, 16 bytes, and one big-endian `u64`. 56 bytes. A byte of 2 or more in `FUN_1461acca0` fails the reader.

**Observed.** The fit walk reached 907,611 class 3451 bodies. Round 4 reached 908,890 with the same stop. The difference is 1,279. The grammar above ends on a following header, by the same next-header test as round 4, for 609,928 of the 907,611 (67.2%). A stricter test, next class one of 10, 11, 13, 15, 2913, 3139, 3451, or 4297, or the payload end, ends 608,905. The stops that are not that end:

| Stop | Bodies |
|---|---:|
| house list | 151,767 |
| header after a full body | 72,685 |
| game-mode child | 40,287 |
| member ping | 27,795 |
| leader ping | 2,107 |
| 1-byte child | 1,467 |
| 2-byte flags | 1,008 |
| other named children | 550 |
| mask or slot parse | 17 |

The 550 other named stops are the 9-byte child 267, the flag byte 122, base data 62, the 10-byte child 49, the 3-byte child 30, the 8-byte child 18, and the 6-byte child 2.

**Open after round 5.** A table that ends at least 99.9% of the reached bodies. The slot byte, the child-byte rule, and the fixed lengths above are known. The house list, the game-mode child, and the ping success path do not end the bodies in the table. Methods: decompile of every registration constructor and of virtual `+0x30`; the 56-byte long form; the empty-list path; an extra child byte when bit 7 stays set after the last child. The fit numbers are the log.

## Files

- `<re-scratch>/scratch-m6/round4/scan_round4.py`, log `scan-round4.log`
- `<re-scratch>/scratch-m6/round4/scan_group.py`, log `scan-group.log`
- `<re-scratch>/scratch-m6/round4/M6Round4.java`, output `ghidra-out/`
- `<re-scratch>/scratch-m6/round5/scan_fit3451.py`, log `scan-fit3451.log`
- `<re-scratch>/scratch-m6/round5/M6Round5.java`, `M6Round5b.java`, `M6Round5c.java`, `M6Round5d.java`, `M6Round5e.java`, `M6Round5f.java`
- `docs/re/2026-09-04-m6-mob-replication-rate.md`
- `docs/re/2026-09-04-admin-capture-viewer.md`
