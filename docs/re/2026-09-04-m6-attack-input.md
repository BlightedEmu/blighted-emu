# Attack bits in the client input

Date: 2026-09-04.

Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`.

This note says which bits of type 1589 change when the player attacks. The body layout is already in `docs/re/2026-09-04-movement-message.md` and in `crates/nw-messages/src/client_input.rs`. This note does not restate that layout. It counts mask bits.

The game was not run. Easy Anti-Cheat was not analyzed. No body bytes, names, ids, positions, tokens, or IP addresses are in this note.

Labels: **Observed**, **Inferred**, **Unknown**.

## Method

The record reader is `FUN_142a25d90`, as in the movement message note. A throwaway script decoded every type 1589 body with that layout and counted the 24 mask bits. It printed rates only. Three sessions: the training dummy, one greatsword skill session, and one movement-only session from the clean running set. Every decoded body in these sessions ended on the last byte (`exact` equals the message count).

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

**Unknown.** The weapon and the attack id inside those bits. The later rows are gated by the record prefix as well as the mask. This pass counted mask bits, not which prefix bit selects a light attack or a heavy attack. A per-weapon split needs a datasheet id that this count did not join.

## Files read

- `docs/re/2026-09-04-movement-message.md`
- `crates/nw-messages/src/client_input.rs`
- Ledgers for the three sessions above, counts only
