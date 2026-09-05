# Movement message records

Date: 2026-09-04.

This note continues `docs/re/2026-09-02-replication-and-movement.md` section 3. The fixed head, the record mask, and bits 0 to 7 keep the sizes from that note, except the prefix width below. The carrier is in `docs/re/2026-09-02-carrier-captures.md`.

Labels: **Observed** means the binary or a capture showed the fact. **Inferred** means a conclusion from those facts. **Unknown** means the files do not show it. **Corrected** means this pass replaces an earlier size.

The game was not run. No new traffic was recorded. Easy Anti-Cheat was not analyzed. This pass used the existing decompile files and a disassembler on `NewWorld.exe`. It did not start a new Ghidra analysis. No body bytes, names, ids, positions, tokens, chat, or IP addresses are in this note.

## 1. Wide prefix

**Observed.** `FUN_14087b5c0` (`0x14087b5c0`) matches `prefix_at`. The first byte below `0x80` is one byte. Larger first bytes add 1, 2, 3, or 4 extra bytes. A first byte at or above `0xF0` is 5 bytes in total.

**Observed.** `FUN_14087b770` (`0x14087b770`) uses the same widths through a first byte below `0xF8`. The function compares the first byte with `0x80`, `0xC0`, `0xE0`, `0xF0`, `0xF8`, `0xFC`, `0xFE`, and `0xFF`.

**Observed.** A first byte from `0xF8` to `0xFB` reads 6 bytes. `0xFC` or `0xFD` reads 7 bytes. `0xFE` reads 8 bytes. `0xFF` reads 9 bytes. The payload bytes are little-endian. The low bits of the tag join the value when the tag is below `0xFE`. Tag `0xFE` and tag `0xFF` contribute no value bits.

**Corrected.** The bootstrap note and `binary.md` say `FUN_14087b770` stops after 4 extra bytes. The binary reads the longer forms above. This note follows the binary.

**Observed.** Record bit 0 calls `FUN_14087b770`. `FUN_142a2ea20` (`0x142a2ea20`) calls `FUN_14087b770` while two or more slots remain. One leftover slot calls `FUN_14087b5c0`. The four scaled bytes after those prefixes stay 4 bytes.

**Observed.** Fixed-head step 5 also calls `FUN_14087b770`. On these captures that first byte is below `0xF8` on every body. That head step keeps the old width here.

**Observed.** The first record has a bit 0 prefix at or above `0xF8` on 23,839 of the older bodies and on 295,375 of the open-world bodies.

**Corrected.** The prior decoder used `prefix_at` for bit 0 and for the pair slots. That read is short when the first byte is at or above `0xF8`. The short read moves bit 2 and bit 7. After the wide read, the first-record bit 2 failures are 0. The older first-record bit 7 map failures are 0. The prior counts were 32 and 78,394.

## 2. Carried record state

**Observed.** `FUN_142a25d90` (`0x142a25d90`) calls `FUN_142a20960` (`0x142a20960`) before the mask read. The copy source is the previous record. The first record copies a zero block of `0x490` bytes.

**Observed.** A clear bit 0 does not write a new prefix. Later tests use the prefix that the copy left in place. The tests use the low 32 bits.

**Observed.** The gate byte is at record offset `0x380`. A clear gate bit does not write it. Row 5 runs when that byte is not 0.

**Observed.** The gate bit index moves forward by 1 when the bit is clear and when the bit is set. The instruction is the increment at `0x142a261c8`.

**Corrected.** The prior decoder left the cursor on the gate bit when row 5 did not run. It also tested one bit for rows 14, 15, and 16. The binary tests three bits.

## 3. Fields after bit 7

The cursor starts at bit 9. A clear mask bit consumes 0 bytes. The prefix bits below are bits of the carried prefix from mask bit 0.

| Order | Width when the test passes | Test |
|---|---|---|
| 1 | three prefix integers | Prefix bit 0 is set. The reader is `FUN_14087b5c0`. |
| 2 | 10 | Prefix bit 11 is set and mask bit 9 is set. The cursor becomes 10. |
| 3 | 10, then 0 or 24 | Prefix bit 8 is set. The cursor bit gates a position. The next bit gates 16 raw bytes and one `u64` big-endian. The cursor then moves by 2. |
| 4 | 0 or 1 | The cursor bit. One `u8` gate. The cursor then moves by 1. |
| 5 | 0 or 10, then 0 or 32 | The gate byte is not 0. The cursor bit gates a position. The next bit gates `FUN_1415acf00` (`0x1415acf00`), 32 bytes. The cursor then moves by 2. |
| 6 | 0 or 8 | Prefix bit 14 is set. The cursor bit gates 8 raw bytes. The cursor then moves by 1. |
| 7 | prefix count, then `count * 104` | Always. The count is `FUN_14087b5c0`. The limit is `0x2000000`. The reader is `FUN_142a1a310` (`0x142a1a310`). |
| 8 | prefix count, then `count * 192` | Always. The reader is `FUN_142a1a0d0` (`0x142a1a0d0`). |
| 9 | prefix count, then `count * 96` | Always. The reader is `FUN_142a19e60` (`0x142a19e60`). |
| 10 | 0 or 10 | Prefix bit 15 is set. The cursor bit gates a position. The cursor then moves by 1. |
| 11 | 0 or 6 | The cursor bit. Three binary16 values. The cursor stays on this bit. |
| 12 | 8 | Prefix bit 16 is set. One `u64` big-endian. |
| 13 | 1 | Prefix bit 18 is set. One `u8`. |
| 14 | 0 or 1 | Mask bit cursor+1. One `u8`. |
| 15 | 0 or 4 | Mask bit cursor+2. One `u32` big-endian. |
| 16 | 0 or 2 | Prefix bit 0 is set and mask bit cursor+3 is set. One binary16. |

**Observed.** `FUN_14416eb00` (`0x14416eb00`) is always read after bit 7 and before bit 8. Tag `3` reads 1 byte. Any other tag reads 18 bytes. The function returns success after that read.

**Observed.** Mask bit 8 calls `FUN_1435ebc30` (`0x1435ebc30`). The flag byte must be `0` or `1`. Value `0` reads 1 byte. Value `1` reads the long body in the Widths section of `binary.md`.

**Observed.** Row 12 occurs 0 times in these captures. This pass did not count how many bit 8 flags are `1`. Every body still ends with 0 bytes left, so each flag byte that the reader consumed was `0` or `1` and the following body fit.

## 4. Bit 7 base

**Observed.** Bit 7 calls `FUN_142a19b20` (`0x142a19b20`). It reads `f32` big-endian, `f32` big-endian, and `u16` big-endian. The z constants are `-100` and `1000`.

**Observed.** `FUN_142a236c0` (`0x142a236c0`) stores that x and y from the transform at record offset `0xE0`. The function does not add a region origin, a cell origin, or a grid square to x or y.

**Observed.** The same function clamps z. z at or above `1000` is replaced by a value next to `1000`. z at or below `-100` is replaced by a value next to `-100`. The nudge vector at `0x14a30fd50` is initialized to about `1.2e-7` on each axis. x and y stay as the transform stored them.

**Corrected.** The 2 September check said the finite bit 7 floats span only 0 to 1,000. That check used the short prefix. With the wide prefix, older bit 7 x values reach the bucket above 8,192 and at or below 16,384 (3,032,920 reads). Older y values reach that same bucket (993,456 reads). Open-world reads fill the buckets between 1,000 and 4,096 as well.

**Inferred.** The two floats are world x and world y. The wire does not add a second base.

**Observed.** On the clean running session, 23,693 of 23,711 bit 7 x values are above 4,096 and at or below 8,192. The same count of y values is above 8,192 and at or below 16,384. 18 x values and 18 y values are at or below 128.

**Observed.** That session has 21,528 type 8 bundles. The first member is type 11 on 21,384 bundles. Type 13 is the first member on 0 bundles.

**Unknown.** The distance from bit 7 to that player's own type 13 position in the clean session. The member header has no length for the type 11 body. This pass did not decode the type 11 reader. A later type 13 member stays unread.

**Observed.** The older ledgers reach 1,527 type 13 positions as the first member. 126 files also have client movement. 991 pairs fall inside 1 s for the actor whose samples stay nearest to the client.

**Observed.** 531 of those pairs have a horizontal distance above 64 and at or below 128. 145 are above 32 and at or below 64. A multiple of 128, 256, 512, 1,000, 1,024, 2,048, or 4,096 does not move the main group into the 0 to 8 band.

**Inferred.** That nearest type 13 actor is not shown to be the local player. The serializer still has no added base.

**Observed.** 7 bit 7 reads on the open-world ledgers have a negative x or a negative y. They are finite. They are not above 14,336. The older ledgers have 0 such reads. The reader does not reject a negative x or y.

## 5. Capture counts

**Observed.** Older ledgers: 285 files. Type 1589 bodies: 2,203,200. Bodies with 0 bytes left: 2,203,200.

**Observed.** Open-world ledgers: 410 files. Type 1589 bodies: 8,406,681. Bodies with 0 bytes left: 8,406,681.

## 6. Client time

**Observed.** The record reader does not convert a clock. Row 12, the optional `u64` big-endian, occurs 0 times.

**Observed.** The `u16` big-endian after the mask uses buckets from 0 through 64,512. That is the full `u16` range in 1,024-wide buckets.

**Unknown.** A client time inside the record. No reader in the record converts a time, and the `u16` range does not show a clock. The bytes do not identify another time field.

## Status

| Item | Status | Still missing |
|---|---|---|
| Wide prefix on bit 0 and on pair slots | Complete | none |
| Carried prefix and gate | Complete | none |
| Cursor after bit 8 | Complete | none |
| `FUN_14416eb00` size | Complete | none |
| Bit 8 long body on captures | Partial | This pass did not count flag value `1`. |
| Bit 7 world x and y | Complete | none |
| Distance to the clean-session type 13 position | Unknown | Type 11 body length. Type 13 is not the first member. |
| Client time inside the record | Unknown | The reader has no clock field. |

## Check

**Observed.** On 2026-09-04 a second, separate script decoded every client type 1589 body of the 695 ledgers again. It was written from this note, the widths it cites (`binary.md`), and the decompiled record reader `FUN_142a25d90`. It printed counts and rounded ranges only.

| Item | Older ledgers (285) | Open-world ledgers (410) |
|---|---|---|
| Type 1589 bodies | 2,203,200 | 8,406,681 |
| Bodies that end with 0 bytes left | 2,203,200 | 8,406,681 |
| Records | 5,913,808 | 26,191,317 |
| Mask bit 0 prefixes in the wide form (first byte at or above `0xF8`), all records | 24,946 | 318,068 |
| Bit 8 flag `0` | 245,032 | 526,016 |
| Bit 8 flag `1` (the long body) | 627,936 | 502,476 |
| Bit 7 reads | 4,742,120 | 22,272,407 |
| Bit 7 reads with x and y finite and inside 0 to 14,336 | 4,742,120 | 22,272,400 |

**Observed.** The 7 open-world bit 7 reads outside that range have a negative x or y. They are the 7 reads of section 4.

**Observed.** Per ledger, the median bit 7 x and y is within 250 m of the median position of the type 13 members that are first in their payload on 59 of 126 older ledgers and on 194 of 305 open-world ledgers that have both.

**Observed.** Over capture time, the last bit 7 position of each message moves at the walk and sprint speeds of `docs/re/2026-09-04-movement-numbers.md` (that note's Check section has the numbers).

**Inferred.** Sections 1 to 3 give the complete layout of type 1589 for these captures, and the bit 7 x and y are world positions. The bit 8 item of the Status table is now complete: 1,130,412 long bodies decode.

## Files read

- `docs/re/2026-09-02-replication-and-movement.md`
- `docs/re/2026-09-04-bootstrap-message-bodies.md`
- `docs/re/2026-09-02-carrier-captures.md`
- `<re-scratch>/scratch-replication/binary.md`
- `<re-scratch>/scratch-replication/movement.md`
- `<re-scratch>/scratch-post-reg/q1_parse.py`
- Decompile files under `<re-scratch>/scratch-replication/out2`, `out5`, and `out-widths`
- `NewWorld.exe` at `<game-dir>/Bin64/NewWorld.exe`
- Ledgers under `local-data/captures/**/ledger.bin` and `local-data/captures/open-world/**/ledger.bin`

## Type 11 pass

The game was not run. This pass used the existing read-only Ghidra project and the clean running ledgers. It did not redo speeds, gaps, falls, or jumps.

**Observed.** Virtual `+0x90` on the type 11 instance is `FUN_142a436b0` (`0x142a436b0`). The instance vtable is `0x1480ff900`. The bundle path calls this slot only.

**Observed.** The body starts with one byte. Bit 0 and bit 1 select two slots. The other bits of that byte are not tested. A selected slot then has one wide prefix integer (`FUN_14087b770`). The low 54 bits of that integer are child presence bits. Slot 0 has 54 children. Slot 1 has 9 children. A set bit reads that child. A clear bit reads 0 bytes.

**Observed.** Child sizes, in constructor order. `u8` is 1 byte. `u16` is 2 bytes. `f32` is 4 bytes big-endian. `u64` is 8 bytes big-endian. `pos3` is 3 bytes. `pos10` is `f32`, `f32`, and `u16` big-endian. `half` is 2 bytes. `prefix` is the short prefix integer. `blob33` is a short prefix count below 33, then that many bytes. `blob177` is a short prefix count below 177, then that many bytes. `frag` is 12 bytes. `bounds` is 1 flag byte. Value 0 stops. Value 1 reads 16 bytes, one `u8`, and one more flag byte. `quat` is the reader `FUN_14087a8d0` (`0x14087a8d0`): 1 tag byte, then 0 to 3 bytes. The extra count is 3 when tag bit 1 is clear and 2 when it is set, then minus 1 when bit 2 is set, then minus 1 when bit 3 is set, then minus 1 when bit 4 is set and the count is not 0.

Slot 0: `u8`, `u8`, then four times `u8` `half`, then `pos3`, `quat`, `quat`, then four times `prefix` `u16` `prefix`, then `u8`, `u8`, `pos10`, `u8`, `blob33`, `u8`, `u8`, `prefix`, `blob177`, `frag`, `f32`, `half`, `half`, `u8`, `u8`, `u64`, `u8`, `u8`, `u8`, `u8`, `u8`, `prefix`, `u8`, `prefix`, `u8`, `u8`, `u8`, `u8`, `f32`.

Slot 1: `u8`, `u8`, `blob177`, `blob33`, `u8`, `u8`, `u8`, `bounds`, `u8`.

**Observed.** On the clean running ledgers this skip accepts 21,384 type 11 bodies. Failures: 0. The next member header, when one exists, is a real class index.

**Observed.** Type 13 members that sit after type 11 in the same group: 0. 21,069 of those groups end at type 11. 315 groups have another class next. That next class is not type 13.

**Observed.** Every type 11 group in this session uses one group id. That same group id is also the first member of other payloads, including the player component (39 payloads). The player-component body skip accepts all 39. The member after that component is not type 13.

**Unknown.** The horizontal distance from bit 7 to that player's type 13. No type 13 position was reached. The same-context member after the player component still has an unread body, so the walk cannot get to a later type 13. There is no distance bin.

**Unknown.** Rotation mask bit 7. This pass did not search it again. The binary has no read for it, and the bit was clear on all 1,538 reached members.

**Observed.** The member after the player component is type 4355, `ChatBrokerActor::InitMsg`, on 21 of the 39 payloads, and type 3133, `MB::EntitlementComponentReplicatedState`, on 18. The index is the `typeindex.json` list position. The name is the `uuid_to_class.json` join for that UUID.

**Observed.** Type 3133 virtual `+0x90` is `FUN_1417b4110` (`0x1417b4110`). The instance vtable is `0x148174800`. The body is one slot byte. Bit 0 selects the only slot. That slot has three children: two counted lists, then one byte that is `0` or `1`. This skip accepts all 18 bodies. Failures: 0.

**Observed.** The member after those 18 bodies is not type 13. It is type 4236, `MB::StatusEffectsComponentReplicatedState`, on 10 payloads, type 5485, `Javelin::SeasonsRewardsReplicatedState`, on 6 payloads, and type 1525, `MB::StatMultiplierTableComponentReplicatedState`, on 2 payloads. Reached type 13 members: 0. There is no distance bin. The walk cannot reach type 13 without the body reader of that next member.

**Observed.** Type 4355 virtual `+0x90` is `FUN_14437ba10` (`0x14437ba10`). It does not read a sized body. It asks the cursor for `ChatBroker` and then calls virtual `+0x68`. Those 21 payloads stay unread. The walk cannot reach a later type 13 in them without that reader.
