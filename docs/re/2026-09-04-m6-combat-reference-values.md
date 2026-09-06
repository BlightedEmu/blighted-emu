# Combat reference numbers

Date: 2026-09-04.

Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`.

This note is the fidelity table for damage, mob health, mob damage, and respawn times. Values are counts and spreads. No names, ids, positions, or payload bytes.

The game was not run. Easy Anti-Cheat was not analyzed.

Labels: **Observed**, **Inferred**, **Unknown**.

The decoders are the damage note, the vitals note, and the death note. A number is here only when those decoders read the field.

## 1. Damage per hit on a training dummy

**Observed.** Type 2071, one item, first float. The damage note checks that every dummy body is 60 bytes and ends on that item.

| Sample | Count | Lowest | Median | Highest |
|---|---:|---:|---:|---:|
| All dummy hits, first float | 93 | 0 | 82.4 | 2790.7 |
| Second float | 93 | 1.0 | 1.0 | 1.0 |

The tag byte is 11 on 86 hits and 14 on 7 hits.

**Inferred.** The first float is the hit amount. The second float is a scale. It does not move on the dummy.

**Observed.** Splitting hits by weapon failed on the full ledger set as well as on the three fight sessions. The damage note lists every 4-byte field, the `u16` at offset 48, and the tag byte. Against crc32 of ability `id` (702) and `weaponTag` (32) the match count is 0 on all 100,831 bodies. The only non-zero item-id matches are 5 bodies at big-endian offset 20 and 5 bodies at little-endian offset 32. There is no per-weapon row.

The item kind byte on the dummy is 11 (86 hits) or 14 (7 hits). The first-float spread above is for all 93 hits together. A per-kind split was not stable enough to add a second row: kind 14 has only 7 samples and was not separated in the stored aggregate.

**Observed.** The same 100,831 hits, joined to attack bits 9, 10, 13, and 15 on the first record of the latest client type 1589 within 1 s. A hit can set more than one bit.

| Bit set | Hits | Lowest | Median | Highest |
|---|---:|---:|---:|---:|
| 9 | 34,380 | 0 | 799.8 | 23,914.2 |
| 10 | 35,399 | 0 | 772.5 | 23,914.2 |
| 13 | 43,184 | 0 | 683.1 | 27,289.5 |
| 15 | 23,912 | 0 | 1,221.0 | 23,914.2 |

## 2. Mob max health by type and level

**Observed.** `HealthAmount` is not a wire child. The vitals note records the search: no static write to `0x14a563290`, and the image qword is not a slot index, so `FUN_141775c60` drops the field. `maxHealth` is a big-endian `u16` on slot 0 and was present on 14 of 4,088 catacombs vitals bodies that used only that slot. Those 14 values are not a per-type table (they include 0 and 32,768). No datasheet id matched the `vitalsId` word against crc32 of the 8,673 local creature `VitalsID` strings.

**Observed.** More id fields were compared on every slot-0 type 15 the walk could read. Child 2, both endian orders: 28,708 bodies, match 0 against every id set in the damage note. Child 4, both orders: 16,966 bodies, match 0. Child 7 (`maxHealth` `u16`): 17,270 bodies, match 0. Child 3 big-endian matches a vitals category on 1 of 134,650 bodies. Child 3 little-endian matches an item id on 1 of 134,650 bodies. The four `u32` slices of the 18-byte vitals data, on 528,454 bodies, match at most 55 bodies to an item id and at most 6 bodies to a creature id or a vitals id. Those counts do not make a per-creature health table.

There is no mob-max-health row. `maxHealth` is on the wire for 17,270 slot-0 bodies, and none of those `u16` values match a datasheet id. The vitals id words do not match crc32 of the 8,673 creature ids.

## 3. Damage a mob deals per hit

**Observed.** No field in the 60-byte type 2071 body matches a creature id or a vitals id. The match count is 0 on all 100,831 bodies for those two sets. The attack-bit table above mixes every hit. It is not damage dealt by a mob. A mob-only row would need one of those id joins. The joins are 0.

## 4. Time from a mob death to its respawn

**Observed.** One point-spawner slice, already read in the world-format note, has min and max respawn cooldown 360.0 seconds. That is one asset, not a capture clock.

**Observed.** Thirteen gaps between type 10 members that share a replication key, from 30 s to 2 hours, in the four fight sessions. Lowest 30.3 s, median 56.9 s, highest 159.6 s. The 360 s slice cooldown is not in that set.

**Observed.** The next type 10 on the same connection after a type 4299, when type 1938 has not yet been seen, is 157 gaps: lowest 0 s, median 2.154 s, highest 171.974 s. The death note states why that gap is not one mob. The 13 same-key gaps above are unchanged.

**Unknown.** Which gap is one mob's death respawn. The offline search is finished. Remove types 566, 1517, and 1791 occur 0 times. No vitals or death-recap field joins a creature id to the replication key. The missing fact is a death event on that same key. The readers and the ledgers do not contain it.

## 5. Time from a player death to respawn

**Observed.** Type 1938 appears in the death session (2 bodies) and the catacombs (3 bodies). The death note checks that those five bodies end on the last byte. Type 3950 was not in those sessions. All 280 type 4299 bodies in the 695 ledgers end on the last byte. Failures: 0. The death note has the reader.

**Observed.** Four pairs of type 4299 then type 1938 on one connection context. Lowest 2.423 s, median 2.545 s, highest 2.646 s. Type 3950 was absent (count 0) in the four fight sessions, so the death instant is type 4299, not type 3950.

**Observed.** Death-recap numeric fields, 280 bodies, same id sets as the damage note. Match 0, except the `u32` in `FUN_145ce8b20` (item id on 89 bodies) and the last `u32` of `FUN_14669cf60` (status-effect id on 81 bodies). The zero fields are `f120-u32`, `f120-word-a`, `f120-word-b`, `f120-u16-a`, `f120-u16-b`, `block-1`, `block-2`, `ce7570-u16`, `cf60-u32-a`, `cf60-u32-b`, `ce-u16`, `b4-u32-a`, `b4-u32-b`, `b4-u32-c`, `list-a`, `list-b`, and `list-c`. Neither non-zero field is a creature id, so neither field starts a mob respawn clock.

## Files read

- `docs/re/2026-09-04-m6-vitals-state.md`
- `docs/re/2026-09-04-m6-damage-events.md`
- `docs/re/2026-09-04-m6-death-and-respawn.md`
- `docs/re/2026-09-04-m6-spawners.md`
- `docs/re/2026-09-04-m6-attack-input.md`
