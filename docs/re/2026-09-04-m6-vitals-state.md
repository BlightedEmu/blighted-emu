# Vitals replicated state

Date: 2026-09-04.

Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`.

This note is `MB::VitalsComponentReplicatedState` (type 15): which field is current health and which is max health.

The game was not run. Easy Anti-Cheat was not analyzed. No body bytes, names, ids, positions, tokens, or IP addresses are in this note.

Labels: **Observed**, **Inferred**, **Unknown**.

## Method

Same Ghidra settings as the mob-spawn note: existing project, read-only, analysis off, heap `16G`, `-max-cpu 10`, ten decompiler threads. REA `open_binary` was not called. No REA Evidence ID. The constructor decompile is `<re-scratch>/scratch-m6/out/fn_14671e040.c`. The registration calls were checked in the executable with `objdump`.

## Fields

**Observed.** Catalog unmarshal `FUN_1465b0dd0` (`0x1465b0dd0`) builds the object in `FUN_14671e040` (`0x14671e040`). The instance vtable is `0x14855ec60`. Virtual `+0x90` is `FUN_1417b4110` (`0x1417b4110`), the same bundle reader as type 10 and type 13.

**Observed.** `FUN_141775c60` (`0x141775c60`) appends a child to slot `n` when `n` is below 10. The constructor registers these names. The slot argument is in `r9`.

Slot 0 (`xor r9d, r9d`), in call order. The names were read from the executable at the `lea` targets:

| Order | Name | Field address in the object |
|---|---|---|
| 1 | `vitalsData` | `+0xec0` |
| 2 | `healthChangeFlags` | `+0x928` |
| 3 | `vitalsId` | `+0xf48` |
| 4 | `vitalsCategoryId` | `+0xf70` |
| 5 | `vitalsLevel` | `+0xf98` |
| 6 | `invulnerability` | `+0xfc0` |
| 7 | `displayImmuneWhenInvulnerable` | `+0xfe0` |
| 8 | `maxHealth` | `+0x1000` |

The string addresses are `0x14855f8b0`, `0x14855f8c0`, `0x14855f8d8`, `0x14855f8e8`, `0x14855f8f8`, and `0x1483e2bc0` for `maxHealth`. `healthChangeFlags` and `vitalsData` are the names in the decompiler at the matching calls.

**Observed.** These names are registered with a qword loaded from a global, not with a constant slot:

| Name | Global loaded into `r9` |
|---|---|
| `HealthAmount` | `0x14a563290` |
| `HealthMax` | `0x14a563290` |
| `HealthTickRate` | `0x14a563290` |
| `StaminaAmount`, `StaminaMax`, `StaminaTickRate` | `0x14a563298` |
| `ManaAmount`, `ManaMax`, `ManaTickRate` | `0x14a5632a0` |

A search of `.text` for a RIP-relative store found no store to `0x14a563290` or `0x14a563298`. The only store to `0x14a5632a0` is at `0x14671e752`, inside this constructor, and it is after the loads. The file bytes at those globals are not a slot index from 0 to 9.

**Observed.** `replicatedAfflictionsHotData` and `replicatedAfflictionsColdData` use the slot index returned by a second `FUN_141677dd0` in this constructor (`object + 0x1020`).

**Inferred.** Current health is the field named `HealthAmount`. One max is the field named `HealthMax` (same slot global as `HealthAmount`). A second max is the slot 0 child `maxHealth`. The wire slot of `HealthAmount` and `HealthMax` is the runtime value of `0x14a563290`. This pass did not find a writer that sets that global to a slot index, so the byte offset of current health inside a type 15 body is not fixed by the static image.

## Second pass

**Observed.** The already-analyzed Ghidra program, opened read-only with analysis left off, reports references for the three globals. `0x14a563290` has three references, all `READ`, all inside `FUN_14671e040` (`0x14671e325`, `0x14671e3da`, `0x14671e3f7`). `0x14a563298` has three `READ` references in the same function. `0x14a5632a0` has those reads plus one `WRITE` at `0x14671e752`. That write stores the slot index returned by `FUN_141677dd0` for this object. It does not write the health global.

**Observed.** A scan of the whole `NewWorld.exe` image for the 8-byte absolute address of `0x14a563290`, `0x14a563298`, and `0x14a5632a0` found no hits. A scan of every RIP-relative load, store, and `lea` in `.text` whose target falls in `0x14a563000` through `0x14a563fff` finds the health global only at the three reads above. Nearby `lea` sites (`0x14a563284`, `0x14a563288`, `0x14a56328c`) are magic-static guards. Their stores go to other addresses, not to `0x14a563290`.

**Observed.** The qword stored at `0x14a563290` in the image is `0x979980403fafb1b`. The preferred image base is `0x140000000`, so a base relocation adds nothing when the file is mapped there. That qword is not a slot index from 0 to 9.

**Observed.** `FUN_141775c60` appends a child only when the slot argument is below 10. `HealthAmount`, `HealthMax`, `HealthTickRate`, `StaminaAmount`, `StaminaMax`, and `StaminaTickRate` therefore do not become wire children. `ManaAmount`, `ManaMax`, and `ManaTickRate` do, on the extra slot, because that slot index is the value written to `0x14a5632a0`.

**Observed.** The slot 0 children that do register, and their readers:

| Child | Name | Reader | Bytes when present |
|---|---|---|---|
| 0 | `vitalsData` | `FUN_1469515b0` (`0x1469515b0`) | 1 byte, two `u64` big-endian, 1 byte |
| 1 | `healthChangeFlags` | `FUN_146951470` (`0x146951470`) | 1 byte |
| 2 | `vitalsId` | `FUN_1432d9460` (`0x1432d9460`) | `u32` big-endian |
| 3 | `vitalsCategoryId` | same reader | `u32` big-endian |
| 4 | `vitalsLevel` | `FUN_1417b3620` (`0x1417b3620`) | `u32` big-endian |
| 5 | `invulnerability` | `FUN_14279e210` (`0x14279e210`) | 1 byte |
| 6 | `displayImmuneWhenInvulnerable` | same reader | 1 byte |
| 7 | `maxHealth` | `FUN_142a42e10` (`0x142a42e10`) | `u16` big-endian |

**Observed.** On catacombs payloads whose first reached vitals member has slot mask 1, child 7 (`maxHealth`) is present on 14 of 4,088 bodies. The `u16` values in that small set include 0 and 32,768. The two `u64` values inside `vitalsData` were above 1,000,000 on all 2,678 samples that contained the blob, so they are not a health integer. Those wide values are omitted.

**Withdrawn in Round 3.** The inference that `HealthAmount` is absent from the replicated body is not the standing result. The globals sit past the raw bytes of `.data`, so the loader supplies zeros and the six named fields register on slot 0. Child 0 is current health. The table is in Round 3.

## Files read

- `docs/re/2026-09-02-replication-and-movement.md`
- `docs/re/2026-09-04-m6-mob-spawn-messages.md`
- Ghidra program `NewWorld.exe` in `<re-scratch>/ghidra`, read-only

## Round 3

The second pass above treated the file bytes at `0x14a563290` as the runtime slot index. That inference is withdrawn. Those addresses are past the raw bytes of `.data`, so the loader supplies zeros, and the six named fields do register.

### Slot globals

**Observed.** In `NewWorld.exe` the section `.data` has virtual address `0x9e7b000` and raw size `0x467000`. The raw bytes end at RVA `0xa2e2000`. `0x14a563290`, `0x14a563298`, and `0x14a5632a0` are RVA `0xa563290` and the two qwords after it. They are inside the virtual size and outside the raw size. A read-only Ghidra open shows the same three reads and one write already listed: health and stamina are only read, inside `FUN_14671e040`. The mana qword is written at `0x14671e752`, still inside that constructor.

**Inferred.** The Windows loader fills the gap between raw size and virtual size with zeros before any constructor runs. `FUN_141775c60` appends a child when its slot argument is below 10. A zero argument is slot 0, so `HealthAmount`, `StaminaAmount`, `HealthMax`, `HealthTickRate`, `StaminaMax`, and `StaminaTickRate` are appended. There is no store to the health or stamina qword anywhere in the analyzed reference list, so nothing in this image moves them off slot 0 before the constructor reads them.

**Observed.** `FUN_141677dd0` returns the number of slots already in the object, then appends one slot of 800 bytes. The base constructor `FUN_14162f3e0` calls it once, so the object already has slot 0. The vitals constructor calls it again and stores the return value, 1, into `0x14a5632a0` before `ManaAmount` is registered. Mana and the two affliction fields use that value. They are slot 1.

**Observed.** Call order in `FUN_14671e040`, and the slot each call uses when health and stamina are 0 and mana is 1:

| Order | Name | Slot |
|---|---|---|
| 1 | `HealthAmount` | 0 |
| 2 | `StaminaAmount` | 0 |
| 3 | `ManaAmount` | 1 |
| 4 | `replicatedAfflictionsHotData` | 1 |
| 5 | `replicatedAfflictionsColdData` | 1 |
| 6 | `vitalsData` | 0 |
| 7 | `healthChangeFlags` | 0 |
| 8 | `HealthMax` | 0 |
| 9 | `HealthTickRate` | 0 |
| 10 | `StaminaMax` | 0 |
| 11 | `StaminaTickRate` | 0 |
| 12 | `ManaMax` | 1 |
| 13 | `ManaTickRate` | 1 |
| 14 | `vitalsId` | 0 |
| 15 | `vitalsCategoryId` | 0 |
| 16 | `vitalsLevel` | 0 |
| 17 | `invulnerability` | 0 |
| 18 | `displayImmuneWhenInvulnerable` | 0 |
| 19 | `maxHealth` | 0 |

### Readers

**Observed.** The three amount fields and the three max fields are built by `LAB_143bb81a0`. That constructor stores vtable `0x1480ff160`. Virtual `+0x30` is `FUN_142a42f30`, which calls `FUN_14087a270`. That helper reads 4 bytes and byte-swaps them. The damage note already identifies that pattern as a big-endian `f32`.

**Observed.** The three tick-rate fields are built by `LAB_146742d50`. That constructor stores vtable `0x1480ff558`. Virtual `+0x30` is `FUN_142a42f80`, which calls `FUN_14087ae90`. The first instructions of `FUN_14087ae90` advance the cursor by 2 and load a `u16`. The replication-rate note already uses this helper as a big-endian `u16`.

**Observed.** `FUN_1417b43c0` reads one child byte, calls the child reader for each set bit among the next 7 children, and stops when bit 7 is clear. A child byte of 0 has no set child bit. The function still returns success. An empty child byte, including a second byte whose value is 0, means those children are absent and the list ends. It is not a second form of a child.

### Slot 0 table

Child index is the append order on slot 0. One size per child. The 4-byte floats are children 0 and 1. The 18-byte reader is child 2. The 1-byte flag reader is child 3. There is no second size for child 0 or child 1.

| Child | Name | Reader | Bytes | Type |
|---|---|---|---|---|
| 0 | `HealthAmount` | `FUN_142a42f30` | 4 | big-endian `f32`, current health |
| 1 | `StaminaAmount` | `FUN_142a42f30` | 4 | big-endian `f32` |
| 2 | `vitalsData` | `FUN_1469515b0` | 18 | 1 byte, two big-endian `u64`, 1 byte |
| 3 | `healthChangeFlags` | `FUN_146951470` | 1 | 1 byte |
| 4 | `HealthMax` | `FUN_142a42f30` | 4 | big-endian `f32` |
| 5 | `HealthTickRate` | `FUN_142a42f80` | 2 | big-endian `u16` |
| 6 | `StaminaMax` | `FUN_142a42f30` | 4 | big-endian `f32` |
| 7 | `StaminaTickRate` | `FUN_142a42f80` | 2 | big-endian `u16` |
| 8 | `vitalsId` | `FUN_1432d9460` | 4 | big-endian `u32` |
| 9 | `vitalsCategoryId` | `FUN_1432d9460` | 4 | big-endian `u32` |
| 10 | `vitalsLevel` | `FUN_1417b3620` | 4 | big-endian `u32` |
| 11 | `invulnerability` | `FUN_14279e210` | 1 | 1 byte |
| 12 | `displayImmuneWhenInvulnerable` | `FUN_14279e210` | 1 | 1 byte |
| 13 | `maxHealth` | `FUN_142a42e10` | 2 | big-endian `u16` |

### Capture fit

**Observed.** A walk of the ledger set (696 `ledger.bin` files on the second pass; the head-field pass below used 695) reached slot-0 type 15 bodies and scored two width rows. The row with 4-byte tick children ends on a following known class, or on the payload end, for 2,387,442 of 2,396,697 bodies (99.614%). The row in the table, with 2-byte tick children, ends that way for 2,396,636 of 2,396,697 bodies (99.997%). Failures of the 2-byte row: 61. The static 8-child row from the second pass of this note, and a row that appends six 4-byte fields after those eight, both end fewer bodies.

**Observed.** The member before a reached type 15, on the first pass, was type 11 on 2,736,690 bodies, type 2913 on 113,874, the start of the group on 38,937, type 3139 on 10,823, type 1528 on 1,563, type 10 on 779, and type 13 on 249. The type prefix of the member itself is 15. These bodies are class 15. They are not a walk that has slipped into class 1528.

**Observed.** Slot byte 2, which is slot 1 only, occurs on 460,589 type 15 bodies. Of those, 426,606 have only child 0 set, and every one of those ends after 4 bytes. That matches `ManaAmount`. The other slot-1 masks are child 1 only (26,066), children 1 and 2 (4,710), children 0 and 1 (2,325), and children 0, 1, and 2 (882). No fixed width from 1 to 48 bytes makes 99.9% of the 26,066 child-1-only bodies end on a known next class. The best single width, 27 bytes, ends 20,057 of them (76.9%). Those children are the affliction fields. Their internal layout is not settled here.

**Inferred.** The 18-byte and 1-byte readers were never a second form of the 4-byte children. They are children 2 and 3. The tick children are 2 bytes, not 4. Giving them 4 bytes is what left the 8,273 misses of the 4-byte tick row.

### Value check

**Observed.** On bodies the 4-byte-tick row could read, a fall in a float was compared with a type 2071 or type 3601 amount from the same connection in the previous 1.5 s. Child 0 fell and matched an amount on 10,473 of 224,643 falls. Child 1 fell 2,187 times and matched 0. Child 4 fell 426 times and matched 0. Child 5 fell 55 times and matched 0.

**Observed.** On the 2-byte-tick row, child 0 changed by more than 0.5 on 787,549 steps across 3,118 replication keys. Child 4, on keys that sent it, changed by more than 0.5 on 895 of 2,162 keys. Child 4 is therefore not a single constant for the life of a key. It is still the field that does not track the hit amount. Child 0 is the field that does.

**Inferred.** Child 0 is current health. Child 4 is `HealthMax` by registration order and by the fact that it does not fall with the hit amount. The 4.7% match rate means most falls are not equal to a captured 2071 or 3601 amount on that connection. Other damage, or a scale on the hit, can do that. The test does not promote any other child to current health: their match count is 0.

### Task 6, child presence and level

**Observed.** On the 2,396,697 slot-0 bodies of the second pass, how often each child bit is set:

| Child | Name | Bodies |
|---|---|---:|
| 0 | `HealthAmount` | 1,463,454 |
| 1 | `StaminaAmount` | 543,438 |
| 2 | `vitalsData` | 45,549 |
| 3 | `healthChangeFlags` | 331,304 |
| 4 | `HealthMax` | 24,701 |
| 5 | `HealthTickRate` | 22,066 |
| 6 | `StaminaMax` | 21,571 |
| 7 | `StaminaTickRate` | 16,710 |
| 8 | `vitalsId` | 49,183 |
| 9 | `vitalsCategoryId` | 62,955 |
| 10 | `vitalsLevel` | 1,267 |
| 11 | `invulnerability` | 404,399 |
| 12 | `displayImmuneWhenInvulnerable` | 394,928 |
| 13 | `maxHealth` | 66,609 |

**Observed.** `vitalsLevel` was seen on 39 replication keys. A later sample on the same key differed on 7 of those 39. The child is rare (1,267 bodies), so this is not a statement about every mob in the captures.

### Other notes

**Observed.** Aeternum-World (AGPL-3.0) describes a bundle record as an interest id, a member count, and members. That shape is the walk used here. Its packet note does not give this child table. A search of `local-data/first-light-main` found no vitals child layout. The other people's protocol notes in the open-world capture export name scoreboard messages and a housing input. They do not give these widths. No text was copied from them.

### Round 3 files

- `<re-scratch>/scratch-m6/round3/scan_round3.py`, log `scan-round3.log`
- `<re-scratch>/scratch-m6/round3/scan_fit2.py`, log `scan-fit2.log`
- `<re-scratch>/scratch-m6/round3/M6Round3.java`, output `ghidra-out/`
- Ghidra project `<re-scratch>/ghidra`, read-only, analysis off, 12 decompiler threads. REA `open_binary` was not called. No REA Evidence ID.
- `NewWorld.exe` disassembly of `FUN_14671e040`
