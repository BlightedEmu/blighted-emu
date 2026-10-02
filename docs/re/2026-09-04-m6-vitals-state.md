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

## Round 4

The 14-child slot-0 table is unchanged. This round sizes slot 1, and it separates the capture-check failures that follow a vitals member from a change of those widths.

### Slot 1 table

**Observed.** Slot 1 is the slot appended by `FUN_141677dd0` inside `FUN_14671e040`. Registration order on that slot:

| Child | Name | Reader | Bytes | Type |
|---|---|---|---:|---|
| 0 | `ManaAmount` | `FUN_142a42f30` | 4 | big-endian `f32` |
| 1 | `replicatedAfflictionsHotData` | `FUN_14694edf0` | list | grammar below |
| 2 | `replicatedAfflictionsColdData` | `FUN_14694eb20` | list | grammar below |
| 3 | `ManaMax` | `FUN_142a42f30` | 4 | big-endian `f32` |
| 4 | `ManaTickRate` | `FUN_142a42f80` | 2 | big-endian `u16` (`FUN_14087ae90`) |

`FUN_1417b4110` tests only the low bits for the slots that exist. Vitals has two slots, so the slot byte uses bit 0 and bit 1. Bit 0 is slot 0. Bit 1 is slot 1. A body with both bits set reads slot 0, then slot 1.

### Affliction list

**Observed.** Both list readers start with a short prefix integer (`FUN_14087b5c0`). The hot reader is vtable `0x14855eab0` at virtual `+0x30`. The cold reader is vtable `0x14855eb50` at virtual `+0x30`.

**Observed.** When that count is not 0, the reader takes the items in groups of at most 8. Each group is one byte, then that many items. Bit `i` of the group byte selects the long form of item `i`. A clear bit is the short form. Each item, short or long, is one byte, then an id. The id is one byte: 0 reads nothing more; 1 reads a wide prefix integer (`FUN_14087b770`); any other value makes the reader fail. The long form is then appended.

**Observed.** The hot long form is `FUN_145ce61b0`: two big-endian binary16 values (`FUN_14087ae90`, 2 bytes each) and two big-endian `u64` values (`FUN_14087ad90`, byte swap `FUN_1461679c0`, 8 bytes each). 20 bytes.

**Observed.** The cold long form is `FUN_145ce6130`: one binary16, then `FUN_1407d7c20` (one byte, then one byte that must be 0 or 1). 4 bytes. A last byte of 2 or more fails the reader.

**Observed.** When the first count is 0, the reader does not stop. It calls virtual `+0x78`. Hot: `FUN_1469518e0`. Cold: `FUN_1469517e0`. That call reads one id, then a short prefix count. The count must be at most `0x2000000`. Each item is one byte plus the long form of that list. There is no short form on this path.

### Slot 1 fit

**Observed.** On the 696 ledgers, a body whose slot byte is 2 (slot 1 only) ends on a following known class, or on the payload end, for 460,589 of 460,589 bodies. Misses: 0. By child mask:

| Mask | Children | Bodies |
|---|---|---:|
| 1 | 0 | 426,606 |
| 2 | 1 | 26,066 |
| 3 | 0, 1 | 2,325 |
| 6 | 1, 2 | 4,710 |
| 7 | 0, 1, 2 | 882 |

**Observed.** A body whose slot byte is 3 (slot 0 then slot 1) ends the same way for 47,276 of 47,276. Misses: 0. Masks on slot 1: 1 on 44,573, 2 on 1,141, 6 on 689, 7 on 529, 3 on 291, and 31 (children 0 through 4) on 53. Children 3 and 4 occur on those 53 bodies and use the 4-byte and 2-byte sizes above.

**Observed.** The same walk still ends the slot-0-only table for 2,399,847 bodies and misses 61. The miss count is the round-3 remainder. The slot-1 grammar does not move those 61.

**Observed.** Item count on the hot list, using the inner count when the first prefix is 0: 0 on 2,774 lists, 1 on 33,884, 2 on 28. No hot list in this walk had an item count above 2.

**Observed.** A hot-list item count that rose on the same replication key did so 571 times. 390 of those 571 rises were within 1.5 s of a type 2071 or type 3601 on that connection. Hits of those two types: 131,258. The join is the connection clock. It is not a join to one ability, and it does not label a damage-over-time ability.

### Failures after a vitals member

**Observed.** Of the 176 capture-check failures, 67 have a vitals member as the member before the failure. The slot byte of that vitals member is 1. The 14-child read of that member succeeds. The failure is the next member.

**Observed.** 26 of the 67 are "encoded member differs" on a member of class 10, 11, or 13. The body of that member, read and written by its reader, is the same bytes. The difference is the prefix in front of the body. 22 of the 26 use a longer prefix than the shortest form: a type id in 2, 3, or 4 bytes where the shortest form is 1 byte (18 members), or a key in 5 bytes written as 3 (3 members) or in 4 bytes written as 3 (1 member). The other 4 are class 11, the key prefix stays 5 bytes, the type prefix stays 1 byte, and the header bytes still differ. Those 4 were not split further.

**Inferred.** Those 26 are not a second size of a vitals child. The vitals body already ends on a member whose own body round-trips. Changing a vitals width would move that member.

**Observed.** 32 of the 67 fail in class 13, "position slot: unknown bits". 8 fail in class 11 with a count above 32 (6) or above 176 (2). 1 fails in class 11 with a flag byte other than 0 or 1. Masking the position slot byte down to bit 0 lets the position reader finish on 17 of the 32 and still fail on 15. That is not a rule for all 32. The position note already says other slot bits were clear on the members it reached. `FUN_1417b4110` tests only as many low bits as the object has slots. These 32 are not settled by that fact alone: 15 still fail after the mask.

**Open after round 4.** A single width change, or a pad after the children, that makes all 41 reader failures (32 position, 8 count, 1 flag) end, and that leaves the 2,399,847 slot-0 fits in place. The width search on an earlier walker accepted many widths at once, because a later type 11 body can absorb the shifted bytes (`probe-summary-widths.txt`). It did not name one child. The 26 prefix differences are not part of this open item.

### Bodies that follow type 11 and then fail inside vitals

**Observed.** 57 capture-check failures are class 15, "body ends early", and the member before them is class 11. The failure file's split of those 57 is 37 that need 4 bytes and have 2, 15 that need 4 and have 0, and 5 that need 2 and have 1. The framed payload ends inside the field.

**Observed.** On the body located after that class 11 member, `VitalsSlot0::read` returns the same truncation, 57 of 57. A second walker with the same 14 widths reports a finished body of 5 to 16 bytes on those 57 and does not name the short child. The two walks do not agree, so this round does not assign the missing bytes to one child. Slot byte on that located body is 1, not 3. Slot 1 is not the missing tail.

### Round 4 files

- `<re-scratch>/scratch-m6/round4/scan_round4.py`, log `scan-round4.log`
- `<re-scratch>/scratch-m6/round4/probe.rs`, summaries `probe-summary.txt` and `probe-summary-widths.txt`
- `<re-scratch>/scratch-m6/round4/M6Round4.java`, `M6Round4b.java`, `M6Round4c.java`, `M6Round4d.java`, output `ghidra-out/`
- Ghidra project `<re-scratch>/ghidra`, read-only, analysis off, 14 decompiler threads. REA `open_binary` was not called. No REA Evidence ID.

## Round 5

The 14-child slot 0 table is unchanged. This round tests the 89 capture-check failures that follow a slot-0 vitals member. A layout change is accepted only when it clears those 89 and still ends the 2,399,847 bodies that already fit.

### Static readers

**Observed.** `FUN_1417b4110` reads one byte and tests only as many low bits as the object has slots. Vitals has two slots, so only bit 0 and bit 1 are tested. `FUN_1417b43c0` reads one byte per group of at most 7 children. Bit 7 reads another child byte. A success path does not skip a child whose bit is set.

**Observed.** Each slot-0 child reader, and the helper it calls, has one success length:

| Child | Reader | Helper | Bytes on success |
|---|---|---|---:|
| 0, 1, 4, 6 | `FUN_142a42f30` | `FUN_14087a270` | 4 |
| 5, 7 | `FUN_142a42f80` | `FUN_14087ae90` | 2 |
| 2 | `FUN_1469515b0` | `FUN_140878610`, `FUN_14087ad90`, `FUN_14087a190` | 18 |
| 3 | `FUN_146951470` | `FUN_140878610` | 1 |
| 8, 9 | `FUN_1432d9460` | `FUN_14087a220` | 4 |
| 10 | `FUN_1417b3620` | `FUN_14087a220` | 4 |
| 11, 12 | `FUN_14279e210` | `FUN_14087a190` | 1 |
| 13 | `FUN_142a42e10` | `FUN_14087a1c0` | 2 |

A failed helper returns failure for the child. It does not return success after fewer bytes. There is no value check, version, or second meaning of the child byte in these functions.

### The 36 encoded differences

**Observed.** All 36 are in the 89. The member after the vitals member reads, and its body bytes match the bytes the reader writes back. By class: 16 of class 10, 11 of class 11, 7 of class 13, 2 of class 15. The tail after that member walks on 35 of the 36. Key length 1 on 25, 3 on 2, 4 on 1, 5 on 8. Type-prefix length 1 on 9, 2 on 9, 3 on 15, 4 on 3. The type prefix is longer than the shortest form on 27. The key length 1 is the shortest form of the key value that was read.

**Inferred.** Those 36 are not a vitals body that was read too long. The vitals member already ends on a member whose body matches, and the bytes after that member still walk on 35 of 36. A shift of the boundary would move that member.

### The 40 position failures and the 12 count failures

**Observed.** 40 fail in class 13, "position slot: unknown bits". 11 of the slot bytes have bit 0 clear. 29 have bit 0 set. Clearing every slot bit except bit 0 lets the position reader finish on 21 of the 40. The tail then walks on 20 of those 21. The other 19 still fail on the child byte. Clearing child bits above the three position children, and skipping one extra child byte when bit 7 is set, lets 9 of those 19 finish. The tail walks on 8 of the 9. 10 of the 40 still fail.

**Observed.** 12 fail on a count. 7 name limit 32. 4 name limit 176. 1 names another limit. The limits 32 and 176 are the client blob limits from round 4 (`FUN_142a43140` below 33, `FUN_142a43230` below 177).

### Searches that did not name a shorter child

**Observed.** For each of the 53 reader failures in the 89, the start of the failing member was moved by 1 to 8 bytes in both directions. A candidate counted only when the key length was 3, 4, or 5, the type id was the shortest form, and a known class read and wrote the same bytes. 49 of the 53 had no such start. The same search out to 24 bytes left 48 of those 49 with no such start.

**Observed.** For the vitals body before those failures, each set child was given every width below its table width, and each pair of set children was shortened the same way. 79 of the 81 vitals bodies in this set had no width that produced a key of 3 to 5 bytes and a known class that wrote the same bytes. The child masks are not one mask. The most common is `0x22ff` on 35 of the 89. Of all 219 failures, the scene counts are 160 other, 29 war, 25 OPR, 4 arena, and 1 invasion.

**Open after round 5.** One vitals rule that clears all 89 and still ends the 2,399,847 slot-0 bodies. The static readers have one success length each. The 36 encoded differences are a prefix form, not a child width. The position-bit ignore rule clears 30 of the 40 position failures and leaves 10. The count failures are the client limits on 11 of the 12. No shorter child is shared by the rest.

**Later finding.** This item is solved. The child bytes and the child data alternate: one child byte, then the data of the children it sets, then the next child byte. With that order a decoder ends every type-15 body. **Inferred.** This explains the 89 failures. See `2026-09-10-player-appearance-and-vitals.md` section 1. The code is `crates/nw-messages/src/slots.rs`.

### Round 5 files

- `<re-scratch>/scratch-m6/round5/src/main.rs`, summary `probe-summary.txt`
- `<re-scratch>/scratch-m6/round5/M6Round5.java`, output `ghidra-out/`
- Ghidra project `<re-scratch>/ghidra`, read-only, analysis off, 14 decompiler threads. REA `open_binary` was not called. No REA Evidence ID.
