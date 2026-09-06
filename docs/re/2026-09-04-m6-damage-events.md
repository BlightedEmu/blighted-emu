# Hits and damage events

Date: 2026-09-04.

Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`.

This note decodes `DamageReceiverComponentClientFacet_OnDamageDealt` (2071), `VitalsComponentClientFacet_OnDamage` (3601), and `DamageReceiverComponentReplicatedState` (1528).

The game was not run. Easy Anti-Cheat was not analyzed. No body bytes, names, ids, positions, tokens, or IP addresses are in this note. Float spreads are rounded.

Labels: **Observed**, **Inferred**, **Unknown**.

## Method

Same Ghidra settings as the mob-spawn note: heap `16G`, `-max-cpu 10`, ten decompiler threads, existing project, analysis off. REA `open_binary` was not called. No REA Evidence ID.

Capture checks used the reliable-frame join. Sessions: training dummy, one catacombs session, one player-death session.

## 1. Type 2071

**Observed.** Unmarshal `FUN_14656ce90` (`0x14656ce90`) calls `FUN_1465b4b50` (`0x1465b4b50`). The body reader is `FUN_1415095d0` then `FUN_145ce7230` (`0x145ce7230`).

| Step | Reader | Size |
|---|---|---|
| 1 | `FUN_1415095d0` | two `u64` big-endian |
| 2 | `FUN_140878610` | 16 raw bytes |
| 3 | `FUN_14087ad90` | one `u64` big-endian |
| 4 | `FUN_14087a220` | one `u32` big-endian |
| 5 | `FUN_14087a220` | one `u32` big-endian |
| 6 | `FUN_140878610` | 2 raw bytes |
| 7 | `FUN_14087b5c0` | prefix count |
| 8 | `FUN_145b56640` | that many items |

**Observed.** One item is one byte, then two values passed through `FUN_14087a270` (`0x14087a270`). That function byte-swaps a `u32`. `Ordinal_14` is the swap. The two words are big-endian `f32` values: the constant word `1065353216` is the float `1.0`.

**Observed.** Every type 2071 body in the three sessions is 60 bytes, and byte 50 is the prefix value 1, so there is one item.

| Session | Bodies | Failures of this size rule |
|---|---:|---:|
| Training dummy | 93 | 0 |
| Catacombs | 1,011 | 0 |
| Player-death session | 99 | 0 |

Total 1,203 bodies. The fixed head is 50 bytes, the prefix is 1 byte, and the item is 9 bytes. 50 + 1 + 9 = 60. The reader ends on the last byte of each of these bodies.

**Observed.** On the dummy, the item tag byte is 11 on 86 bodies and 14 on 7 bodies. The first float is 0 at the low end, 82.4 at the median, and 2790.7 at the high end (93 samples). The second float is 1.0 on all 93. In the catacombs the second float runs from 0.55 to 0.82 (1,011 samples). In the death session it runs from 0.69 to 0.98 (99 samples).

**Inferred.** The first float is the hit amount. The second float is a scale that stays 1 against a training dummy and drops below 1 in a real fight. The tag byte is a small kind code, not an entity id. This pass does not name the kind.

**Observed.** The same 60-byte layout was applied to every type 2071 body in the 695 ledgers, not only the three fight sessions. 100,831 bodies have length 60 and a prefix count below 8.

**Observed.** Each 4-byte word at offsets 0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, and 44 was read big-endian and little-endian. The `u16` at offset 48 and the tag byte at offset 51 were read as well. Each value was compared with crc32 of these id sets: ability `id` (702), ability `weaponTag` (32), creature `id` (8,673), creature `VitalsID` (8,673), creature `VitalsCategories` (14,111), item `id` (37,559), status-effect `id` (4,521). Both the raw spelling and the lower-case spelling are in each set.

**Observed.** Match counts are 0 for every field against every set, with two exceptions. Big-endian offset 20 matches an item id on 5 of 100,831 bodies. Little-endian offset 32 matches an item id on 5 of 100,831 bodies. Ability id and weapon tag match 0 bodies on every field, including offsets 40 and 44. Five matches do not name the weapon of a hit. No guid table was used. The local ability records have `id` and `weaponTag` only.

**Observed.** The first record of the latest client type 1589 on the same connection, when it is under 1 s old, supplies attack bits 9, 10, 13, and 15. All 100,831 hits had such a mask. The first-float spread while each bit is set is in the combat note. A hit can set more than one of these bits, so the four spreads are not a partition. They are not a weapon split.

## 2. Type 3601

**Observed.** Unmarshal `FUN_146567bf0` (`0x146567bf0`) calls `FUN_14658c360` (`0x14658c360`). After the two `u64` values it reads one `u64` big-endian, then `FUN_1465e66d0` (`0x1465e66d0`): 2 raw bytes, one swapped `u32`, `FUN_1415acfc0` (three swapped `u32` values), and `FUN_1466a1d40`. That last function reads a prefix count and calls `FUN_145ce6260` once per item.

**Observed.** Body sizes: catacombs 79 of 80 are 52 bytes and 1 is 61 bytes. The death session is 16 of 17 at 52 bytes and 1 at 61 bytes.

**Observed.** `FUN_145ce6260` (`0x145ce6260`) reads one byte and two big-endian floats, 9 bytes. The head before the item count is 42 bytes. Across all 695 ledgers, type 3601 has 29,937 bodies and 29,937 end on `42 + prefix + 9 × count`. Failures: 0.

## 3. Type 1528

**Observed.** `MB::DamageReceiverComponentReplicatedState`. Unmarshal `FUN_14659bbf0` (`0x14659bbf0`) builds the object in `FUN_1466f8310` (`0x1466f8310`). The instance vtable is `0x148551480`. Virtual `+0x90` is `FUN_1417b4110`, the same bundle reader as type 15.

**Observed.** The constructor registers three children: `isBlockActive`, `blockWeaponSlotAlias`, and `debugCritChance`.

**Unknown.** Each child's wire size, and an exact-end count. The fight walks stopped on this class (30 times in the cave, 15 times in the dummy) and did not decode it. It is not the per-hit damage. That is type 2071.

## Files read

- `docs/re/2026-09-02-replication-and-movement.md`
- `docs/re/2026-09-04-m6-vitals-state.md`
- Ghidra program `NewWorld.exe` in `<re-scratch>/ghidra`, read-only
