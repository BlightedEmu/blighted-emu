# How often mob members are sent, and how far

Date: 2026-09-04.

Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`.

This note measures how often mob members are sent, and how far from the player the client starts and stops receiving them. Distances would be in metres only. No positions are written here.

The game was not run. Easy Anti-Cheat was not analyzed.

Labels: **Observed**, **Inferred**, **Unknown**.

## Method

The walk is the type 8 payload walk from the replication note, plus the type 11 skip from the movement message note and the type 10 reader from the spawn note. It stops at the first member that is not type 10 or type 11. Session length is the last ledger time minus the first, in seconds. Times and positions are not printed.

## Rates

**Observed.** Members the walk accepted, and type 15 members that stopped the walk.

| Session | Seconds | Bundles | Type 11 accepted | Type 10 accepted | Type 15 stops |
|---|---:|---:|---:|---:|---:|
| Training dummy | 97.0 | 5,551 | 11,442 | 16 | 791 |
| Cave | 142.6 | 6,120 | 27,282 | 52 | 19 |
| Catacombs | 985.5 | 58,915 | 126,225 | 55 | 9,838 |

Type 11 failures: 0. Type 10 failures: 0.

**Inferred.** Type 11 is not a mob-only stream. The replication note shows it is the most common class, including the player. About 118 type 11 members per second in the dummy, 191 in the cave, and 128 in the catacombs. Type 10 is the spawn card. It is rare after the entity exists: 0.16 per second in the dummy, 0.36 in the cave, and 0.06 in the catacombs. Type 15 stops are a lower bound on vitals updates the walk reached, about 8 per second in the dummy and 10 per second in the catacombs. The cave barely stops on vitals (19 times in 143 seconds).

## Distance

**Observed.** A second walk kept the player's last type 1589 bit 7 position per connection context, and the positions inside type 11 (`pos10`) and type 13. A position within 2 m of that player position is the player's own body and is left out. The player position had to be under 1 s old. The same replication key starting again after a gap of 5 s or more is a new appearance. Only the distance in metres is kept.

Four sessions: the training dummy, the cave, one catacombs session, and one player-death session.

| Event | Samples | Lowest | Median | Highest |
|---|---:|---:|---:|---:|
| First time a non-player position is sent | 160 | 2 m | 23 m | 445 m |
| Last time that position is sent | 160 | 2 m | 17 m | 445 m |

Three first-seen distances were above 2,000 m and are not in the highest bin above. The highest listed is the highest 1 m bin at or under 2,000 m.

**Inferred.** Mobs in these sessions start being sent at a median of 23 m and are still sent at a median of 17 m on the last sample. The far edge in these samples is 445 m. That is the interest distance these captures support. It is not a single hardcoded radius from `region.slicedata`.

## What still stops the walk

**Observed.** After type 10, type 11, type 13, and a slot-0 type 15 body, the walk stops on the next class. The most common stops in the four sessions:

| Type | Class | Stops |
|---|---|---:|
| 4297 | `MB::StaminaComponentReplicatedState` | 14,185 |
| 15 | type 15 bodies whose slot mask was not only slot 0 | 8,874 |
| 5620 | `MB::MountComponentReplicatedState` | 8,405 |
| 3451 | `Javelin::GroupDataComponentReplicatedState` | 3,807 |
| 4236 | `MB::StatusEffectsComponentReplicatedState` | 3,731 |
| 2913 | `MB::CombatStatusComponentReplicatedState` | 2,962 |
| 3139 | `MB::DelayedEventComponentReplicatedState` | 2,893 |
| 3935 | `MB::PlayerComponentReplicatedState` | 2,160 |

**Observed.** Each of these classes uses the generic bundle reader `FUN_1417b4110` at instance vtable `+0x90`. The child slot is the 4th argument of `FUN_141775c60`. The search was the immediate form `xor r9d, r9d`, a RIP-relative load of the child vtable, and a memory load of the slot from the object. There is no second width table.

**Observed.** Type 4297, constructor `FUN_14671a230`. Six children, each registered with slot 0. Each child reader is `0x142a42f30`, a big-endian `f32` (4 bytes). On the 695 ledgers the slot-0 reader lands on a valid next header for 2,408,570 bodies. 15 bodies set another slot bit and still stop the walk.

**Observed.** Type 3139, constructor `FUN_143066370`. Two children, both slot 0. `totalDelayDuration` is `FUN_14087ae90`, a big-endian `u16`. `delayedEventCompleted` is one byte that must be 0 or 1. 468,717 bodies land on a valid next header. 22 bodies do not. 7 bodies set another slot bit. The walk can pass the 468,717 bodies.

**Observed.** Type 2913, constructor `FUN_1466f1f10`. Four children, each with slot 0 (`xor r9d, r9d` before every call). Two flag bytes, then two `u64` values (`0x14279e480`). 444,078 bodies land on a valid next header. 5 bodies set another slot bit. Boundary failures: 0. The walk can pass the slot-0 bodies.

**Observed.** Type 5620, constructor `FUN_1438d7000`, 16 registrations. The slot is `mov 0x3120/0x3128(%reg), %r9` after `FUN_141677dd0`. It is not an immediate. The walk cannot pass this body.

**Observed.** Type 4236, constructor `FUN_144013900`, 9 registrations. The slot is loaded from object offsets `0x1e58`, `0x1e60`, or `0x1e68`. It is not an immediate. The walk cannot pass this body.

**Observed.** Type 3451, constructor `FUN_1433d8bf0`, 30 registrations. Some calls use slot 0. Others use `mov 0xb60(%rsi,%rdi,1), %r9`. That index is not a constant. A slot-0-only skip does not describe the body. The walk cannot pass this body.

**Observed.** Type 3935, constructor `FUN_146711e90`, 20 registrations. The first slot is the return value of `FUN_141677dd0`. Later slots are loaded from object offset `0xf40`. They are not immediates. The walk cannot pass this body. No player fields were copied.

Type 15 with slot mask 1 was already read (2,122 bodies accepted, 0 of those ran off the end when the following header parsed). The walk still stops on type 15 when the slot mask is not only slot 0, and on types 5620, 3451, 4236, and 3935.

## Files read

- `docs/re/2026-09-02-replication-and-movement.md`
- `docs/re/2026-09-04-movement-message.md`
- `docs/re/2026-09-04-m6-mob-spawn-messages.md`
- `docs/re/2026-09-04-m6-alc-state.md`
- `docs/re/2026-08-31-world-and-object-stream-formats.md`

## Round 3

### Interest count

**Observed.** Distinct replication keys were counted per connection. One window is every bundle whose ledger timestamp is the same millisecond. A second window is 100 ms. Keys are counted only while the walk is still aligned. A key after an unsized member in the same payload is not in the count, so a window total is a lower bound on that window.

**Observed.** Same-millisecond window, 695 ledgers: the largest count is 43. By capture kind the largest is 43 outside war, OPR, invasion, and arena; 23 in a war capture; 23 in an invasion; 21 in an OPR; 14 in an arena. One bundle, keys seen before the walk stopped: largest 35. One bundle the walk finished: largest 26.

**Observed.** 100 ms window, 696 ledgers: the largest count is 45. By kind: 45 outside those four kinds, 28 in a war, 25 in an OPR, 23 in an invasion, 14 in an arena.

**Observed.** In the same-millisecond windows the largest number of keys that carried class 3935 is 1, and the largest number that carried class 10 or class 15 and not 3935 is 6. Those labels do not partition the 43 keys. Most keys in a full window are neither of those two labels. The measurement does not show a separate player cap and a separate mob cap. It shows one shared count of entity keys.

**Observed.** Distance from the connection's type 1589 position to a parsed entity position, in windows of at least 30 keys, has 8 kept samples and 1 dropped sample. The dropped sample was farther than every entity still present. That is one sample. It does not establish a keep rule.

### Client constant

**Observed.** A read-only scan of `.text` for a register compared with immediate 47, then a decompile of the 17 functions that contain those sites (`M6Cmp47.java`), finds no entity limit. The uses are the character `/` (byte value 47) and the length 47 of an AWS header string. `RelevanceRadius` is a reflected property name in `FUN_14416b800`. The function copies a default; it does not compare a count with 47. `VisibleCount` in the image is a UI-cursor string, not a replication cap.

**Inferred.** These captures never reach 47 entity keys in one connection window. The largest aligned count is 45 in 100 ms, which is below 47. The client image, on the searches above, does not contain a 47-entity interest constant. The emulator's 47 is not confirmed or contradicted by a reached cap. It is above every count this walk could align.

**Open after round 3.** Which entities are kept when a cap is reached. The distance sample is 8 kept and 1 dropped. A rule needs the entities the server did not send. This walk cannot see those. Methods and counts are in `docs/re/2026-09-04-m6-round-3.md`.
