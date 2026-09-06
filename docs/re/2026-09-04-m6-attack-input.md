# Attack bits in the client input

Date: 2026-09-04.

Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`.

This note says which bits of type 1589 change when the player attacks. The body layout is already in `docs/re/2026-09-04-movement-message.md` and in `crates/nw-messages/src/client_input.rs`. This note does not restate that layout. It counts mask bits.

The game was not run. Easy Anti-Cheat was not analyzed. No body bytes, names, ids, positions, tokens, or IP addresses are in this note.

Labels: **Observed**, **Inferred**, **Unknown**.

## Method

The record reader is `FUN_142a25d90`, as in the movement message note. A throwaway script decoded every type 1589 body with that layout and counted the 24 mask bits. It printed rates only. Three sessions: the training dummy, one greatsword skill session, and one movement-only session from the clean running set. Every decoded body in these sessions ended on the last byte (`exact` equals the message count).

**Observed.** A second pass decoded all 695 ledgers with the same layout: 10,609,881 bodies, 10,603,968 exact (99.94%), 32,088,391 records. Global mask counts: bit 0 on 11,233,086 records, bit 1 on 10,436,019, bit 4 on 19,755,385, bit 5 on 10,412,385, bit 6 on 19,242,811, bit 7 on 26,999,901, bit 11 on 10,114,445, bit 8 on 1,901,277, bit 9 on 1,005,539, bit 10 on 1,128,298, bit 12 on 91,190, bit 13 on 1,003,434, bit 14 on 172, bit 15 on 493,606. Bits 16 through 23 are clear on every record in all 695 ledgers.

## Bits

**Observed.** Share of records with the bit set. A record is one entry inside a type 1589 body.

| Bit | Training dummy (7,442 records) | Greatsword session (15,489 records) | Movement only (2,562 records) |
|---:|---:|---:|---:|
| 0 | 0.363 | 0.237 | 0.352 |
| 1 | 0.319 | 0.222 | 0.269 |
| 2 | 0.056 | 0.022 | 0.134 |
| 4 | 0.640 | 0.230 | 0.620 |
| 5 | 0.317 | 0.222 | 0.269 |
| 6 | 0.671 | 0.397 | 0.307 |
| 7 | 0.905 | 0.531 | 0.699 |
| 8 | 0.001 | 0 | 0 |
| 9 | 0.012 | 0.014 | 0 |
| 10 | 0.015 | 0.018 | 0 |
| 11 | 0.318 | 0.224 | 0.269 |
| 12 | 0 | 0.0003 (4 records) | 0 |
| 13 | 0.033 | 0.063 | 0 |
| 15 | 0.023 | 0.037 | 0 |

Bits 3, 14, and 16 through 23 were clear on every record in these three sessions.

**Observed.** From the movement message note, bits 0 to 8 are the fixed fields. Bit 7 is the position. The cursor for the later rows starts at bit 9. Bit 6 is the two `f32` values.

**Inferred.** An attack is not a new message. It is mask bits that stay clear while the player only runs. Those bits are 9, 10, 13, and 15. Bit 8 is almost unused (11 dummy records). Bit 12 appears on 4 greatsword records only. Bit 6 is set more often in the dummy (0.671) than while only running (0.307), so it rises during a fight, and it is also set while running. It is not an attack by itself.

**Observed.** Prefix bits gate the attack rows (`RowLayout` in `crates/nw-messages/src/client_input.rs`, same constants in the movement record reader). When mask bit 9 is set, prefix bit 8 is set on 762,728 of 1,005,539 records and prefix bit 0 on 229,170. When mask bit 10 is set: prefix 0 on 366,302, prefix 8 on 268,721, prefix 15 on 68,328, prefix 18 on 377 of 1,128,298. When mask bit 13 is set: prefix 0 on 730,852 and prefix 8 on 267,214 of 1,003,434. When mask bit 15 is set, prefix bit 0 is set on all 493,606. The common attack row signatures (mask bits, prefix bits, rows, gate byte, records): (9)+(8)+(row3pos)+0 on 478,272; (9,10,13)+(8)+(row3pos,row3id,row14)+0 on 241,906; (13)+(0)+(row1,row14,row16)+0 on 241,560; (10,15)+(0)+(row1,row5pos,row16)+2 on 109,712; (10)+(15)+(row10,row14)+0 on 45,916; (9,10,13,15)+(0)+(row1,row4,row5pos,row5block,row14,row16)+2 on 143,205. Row 14 is 1 on 10,386,893 of 10,400,748 row-14 records.

**Observed.** No row, slot, or bit-8-string field matches a datasheet crc. Checked fields: row 1 (three prefix ints, 1,361,708 records), row 3 `u64` (268,721 records, 173 distinct values), row 6 (0 records), row 12 (0 records), row 13 (618,903 records, 16 distinct values from 1 to 12 and above), row 14, row 15 (12,218 records, 15 distinct values), row 16, every slot value (2,987 distinct values over 314,617,060 samples), and 7,911,960 bit-8 long-body strings. Sets: ability `id` (702), ability `weaponTag` (32), creature `id` (8,673), creature `VitalsID` (8,673), creature `VitalsCategories` (14,111), item `id` (37,559), status-effect `id` (4,521), raw and lower-case spellings. Match count 0 on every field against every set.

**Inferred.** The weapon is not in type 1589. No row carries a weapon datasheet id over 32,088,391 records, so the weapon comes from server-side loadout state, not from the input rows. The attack id is the (mask, prefix, rows) signature with the row-14 tag of 1: prefix bit 8 selects the target rows (row 3 position and row 3 id), prefix bit 0 selects the action rows, and prefix bit 15 selects the aimed position (row 10). Row 13 (prefix bit 18) is not attack-tied: it rides on 618,903 records while attack-bit records carry prefix 18 on 377 records only.

**Observed.** Hit damage by preceding attack family (type 2071 first float, latest attack record within 1 s, 54,975 joined hits): prefix-8 target family median 599.1 (10,542 hits), prefix-0 gate-2 full-action family median 1,151.2 (11,075 hits), prefix-0 gate-0 action family median 228.6 (28,474 hits), no-prefix vector family median 698.1 (4,650 hits), bit-12 skill family median 684.5 (234 hits).

**Inferred.** Light attacks are the prefix-0 gate-0 action rows (row 1, row 14, row 16 singletons): most common (28,474 joined hits) and weakest (median 228.6). Heavy attacks are the prefix-0 gate-2 full rows (row 4 gate byte, row 5 position and block): rarer (11,075 joined hits) and about five times harder (median 1,151.2). The target, aim, and vector rows ride on both. Bit 12 is the skill family: aimed position (row 10) or the row-15 hash on 91,190 records, with 15 distinct row-15 values that match no local datasheet id.

## Files read

- `docs/re/2026-09-04-movement-message.md`
- `crates/nw-messages/src/client_input.rs`
- All 695 ledgers, counts only
