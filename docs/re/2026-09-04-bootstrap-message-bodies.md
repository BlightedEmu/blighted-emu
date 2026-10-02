# Bootstrap message bodies

Date: 2026-09-04.

This note gives the body field layout of the first server messages after login. Message order, timing, and sizes are in `docs/re/2026-09-02-post-registration-sequence.md`. The carrier and the envelope are in `docs/re/2026-09-02-carrier-captures.md`. This note does not repeat those facts.

The body starts after the type index. Offsets below are body offsets. Integers are big-endian unless a row says otherwise. A 16-byte field is raw bytes. The reader does not swap those bytes.

Labels: **Observed** means a capture or the binary showed the fact. **Inferred** means a conclusion from those facts. **Unknown** means the files do not show it.

The game was not run. No new traffic was recorded. Easy Anti-Cheat was not analyzed. The Ghidra project was opened read-only with analysis left off. Ten decompiler threads ran. The heap was 16 GB or 24 GB. No body bytes, names, ids, positions, or tokens are in this note.

## Shared readers

**Observed.** `FUN_14087a220` (`0x14087a220`) reads 4 bytes and byte-swaps them. That is a `u32` big-endian.

**Observed.** `FUN_14087a270` (`0x14087a270`) reads 4 bytes and byte-swaps them through `FUN_146167960`. The capture bits of those fields are clean floats. Those fields are `f32` big-endian.

**Observed.** `FUN_14087ad90` (`0x14087ad90`) reads 8 bytes. `FUN_1461679c0` (`0x1461679c0`) byte-swaps that value. That is a `u64` big-endian.

**Observed.** `FUN_140878610` (`0x140878610`) copies a fixed count of raw bytes.

**Observed.** `FUN_14087b5c0` (`0x14087b5c0`) and `FUN_14087b770` (`0x14087b770`) read the same prefix code as the envelope type index. A first byte at or below `0x7F` is the value. Larger first bytes add 1, 2, 3, or 4 extra bytes. This is not a LEB128 varint.

**Later finding.** That width is wrong for `FUN_14087b770`. It also reads 6-, 7-, 8- and 9-byte forms (first byte `0xF8` and above). `FUN_14087b5c0` keeps the 1- to 5-byte forms. See `2026-09-04-movement-message.md` section 1.

**Observed.** `FUN_14087a1c0` (`0x14087a1c0`) reads a `u16` and byte-swaps it.

## 446 and 1034

Classes: `CoatlicueTimingConfigurationConnectedMsg` and `CalendarConnectedMsg`.

**Observed.** Both bodies are 72 bytes on all 43 contexts. Unmarshal for 446 is `FUN_14646c070` (`0x14646c070`). Unmarshal for 1034 is `FUN_14646bf10` (`0x14646bf10`). Both call `FUN_1415b4de0` (`0x1415b4de0`). That function calls `FUN_1415acc30` (`0x1415acc30`) twice.

**Observed.** Each `FUN_1415acc30` reads one `u32` big-endian, then 16 raw bytes, then 16 raw bytes. Two calls are 72 bytes.

| Offset | Size | Type | Same on all 43 | Value |
|---|---|---|---|---|
| 0 | 4 | `u32` big-endian | yes, and it differs between the two classes | omitted |
| 4 | 16 | raw | yes, and it differs between the two classes | omitted |
| 20 | 16 | raw | yes, and it differs between the two classes | omitted |
| 36 | 4 | `u32` big-endian | no. The same value is on both classes in one context. | varies |
| 40 | 16 | raw | no. The same value is on both classes in one context. | varies |
| 56 | 16 | raw | yes, and the two classes share it | omitted |

**Corrected.** A first-light layout put a 4-byte id, a 32-byte blob, and a 36-byte trailer in this body. The reader does not use that split.

The omitted values are id-sized or sit in an id-sized block. They are not printed.

## 1628 PlayerManagerSelfIdentificationMsg

**Observed.** The body is 12,702 bytes on all 43 contexts. It arrives in 12 parts. Unmarshal is `FUN_1414fd530` (`0x1414fd530`). The handler `FUN_146454c00` (`0x146454c00`) reads the message after that unmarshal.

**Observed.** The first call is `FUN_14087aab0` (`0x14087aab0`). It reads one `u8`. On all 43 contexts that byte is `5`. Bit 0 of that value is set, so the same function then copies 16 raw bytes. Those 16 bytes have 2 values on the 43 contexts.

**Inferred.** The 16 bytes are one id per character. The captures hold two characters.

**Observed.** The next three calls are `FUN_1415acc30`. Each call is one `u32` big-endian plus two 16-byte raw fields.

| Offset | Size | Type | Same on all 43 | Value |
|---|---|---|---|---|
| 0 | 1 | `u8` | yes | 5 |
| 1 | 16 | raw | no | varies |
| 17 | 36 | one `u32` big-endian and two raw 16-byte fields | yes | omitted |
| 53 | 4 | `u32` big-endian | no | varies |
| 57 | 16 | raw | no | varies |
| 73 | 16 | raw | yes | omitted |

**Observed.** From offset 89 the remaining 12,613 bytes are the same on all 43 contexts. The reader then calls `FUN_1415acc30` again, `FUN_1415ba4a0` (`0x1415ba4a0`), and `FUN_14087aab0` again. `FUN_1415ba4a0` reads one `u32` big-endian, one further block, then two `u8` flags that must be `0` or `1`.

**Unknown.** The field tree inside the constant tail. The bytes are omitted because the block is large and can hold ids. A later pass can name each helper call without printing those bytes.

**Corrected.** A 21-byte minimum wire form does not match this body. The empty 4-byte form is type 1617, not this message.

## 164 ClientAddEntryMsg

**Observed.** Unmarshal is `FUN_1407c8a70` (`0x1407c8a70`). It copies 16 raw bytes. The body is 16 bytes on all 43 contexts. The 16 bytes have 2 values on the 43 contexts.

**Observed.** On all 43 contexts, this id is not the 16-byte id at offset 1 of type 1628.

| Offset | Size | Type | Same on all 43 | Value |
|---|---|---|---|---|
| 0 | 16 | raw | no | varies |

**Confirmed.** The body is one 16-byte id. The value is omitted.

## 1617 ReceivePlayerSpawnPointMsg

**Observed.** Unmarshal is `FUN_1414e3930` (`0x1414e3930`). It does not read the stream. The body length is 0 on all 43 contexts.

| Offset | Size | Type | Same on all 43 | Value |
|---|---|---|---|---|
| none | 0 | empty | yes | empty |

**Confirmed.** The body has no fields.

## 3236 SyncNotifications

**Observed.** The first body in each context is 122 bytes on 22 contexts and 154 bytes on 21 contexts. Inside each length, every byte is the same across those contexts. Unmarshal is `FUN_143197fe0` (`0x143197fe0`). The body reader is `FUN_14319a370` (`0x14319a370`). It reads one nested block, then one `u8` that must be `0` or `1`.

**Unknown.** The fields inside the nested block. The two templates are constant, and the bytes are omitted. The class UUID text for a tighter reader tie was found, and the reader still calls `FUN_143026230` (`0x143026230`) for that block. That helper was not decompiled in this pass.

## 335 TimeSynchMsg

**Observed.** Unmarshal is `FUN_1407cc8f0` (`0x1407cc8f0`). The body reader is `FUN_14087ad90`. The body is 8 bytes. It is one `u64` big-endian. On all 43 contexts the low 4 bytes change between the first copy and the next copy. The value is a time. It is not printed.

| Offset | Size | Type | Same on all 43 | Value |
|---|---|---|---|---|
| 0 | 8 | `u64` big-endian | no | varies |

**Corrected.** A split into a separate clock word and a nonce word is not what the reader does. The low half changes every copy. The high half can stay the same for one interval. Both halves are one `u64`.

## 1635 LevelInfoChangedMsg

**Observed.** The handler is `FUN_146446800` (`0x146446800`). It reads an in-memory object. It compares a `u64` at object `+0xa8`. It reads a flag at object `+0xa2`. The body length of the first message is 106, 112, or 157. Two copies in one context have the same bytes. The bytes differ across contexts.

**Unknown.** The wire field list. The hyphenated class UUID is not in the image, so this pass did not tie an unmarshal function to the class. A fixed offset list cannot be confirmed while the length changes. The bytes are omitted.

## 4246 SyncTimeChange

**Observed.** Unmarshal is `FUN_1440713a0` (`0x1440713a0`). The body reader is `FUN_144071dc0` (`0x144071dc0`). It reads two `u64` big-endian values, then nine 4-byte big-endian values, then one `u64` big-endian, then four `f32` big-endian values. The nine 4-byte values are three `f32` values and then six `u32` values. `FUN_1469ad860` (`0x1469ad860`) reads those nine. The body is 76 bytes on all 43 contexts. The first two copies in a context are the same.

| Offset | Size | Type | Same on all 43 | Value |
|---|---|---|---|---|
| 0 | 8 | `u64` big-endian | no | varies |
| 8 | 8 | `u64` big-endian | no | varies |
| 16 | 4 | `f32` big-endian | yes | 6 |
| 20 | 4 | `f32` big-endian | yes | -1 |
| 24 | 4 | `f32` big-endian | yes | 0.00046913 |
| 28 | 4 | `u32` big-endian | yes | 21300 |
| 32 | 4 | `u32` big-endian | yes | 0 |
| 36 | 4 | `u32` big-endian | yes | 65100 |
| 40 | 4 | `u32` big-endian | yes | 0 |
| 44 | 4 | `u32` big-endian | yes | 3600 |
| 48 | 4 | `u32` big-endian | yes | 1800 |
| 52 | 8 | `u64` big-endian | yes | omitted. It is a time. |
| 60 | 4 | `f32` big-endian | yes | 0.16667 |
| 64 | 4 | `f32` big-endian | yes | 0.16667 |
| 68 | 4 | `f32` big-endian | yes | 0.83333 |
| 72 | 4 | `f32` big-endian | yes | 0.83333 |

**Confirmed.** The 16 bytes before the floats vary across contexts and stay fixed inside one context. They are two ids. The numeric block matches the reader widths.

**Corrected.** A layout that stores one `u64` across offsets 32 and 40 does not match the reader. Those are `u32` fields. The value `0` is its own field.

## 4247 SyncDayPhase

**Observed.** Unmarshal is `FUN_1440716c0` (`0x1440716c0`). `FUN_1415095d0` (`0x1415095d0`) reads two `u64` big-endian values. `FUN_14087a220` then reads one `u32` big-endian. The body is 20 bytes on all 43 contexts.

| Offset | Size | Type | Same on all 43 | Value |
|---|---|---|---|---|
| 0 | 8 | `u64` big-endian | no | varies |
| 8 | 8 | `u64` big-endian | no | varies |
| 16 | 4 | `u32` big-endian | yes | 2 |

**Confirmed.** The last field is the constant `2`. The first 16 bytes differ across contexts.

## 6310 UpdatePhasing

**Observed.** The first body in a context is 36 bytes on 38 contexts and 87 bytes on 5 contexts. Bytes differ across contexts.

**Unknown.** The wire field list. The hyphenated class UUID is not in the image, so this pass did not tie an unmarshal function to the class. A length step of 17 in the wider capture set is not, by itself, a field boundary.

## 8 ReplicatedStateBundle

This section is the outer header only. It does not decode entity state.

**Observed.** Unmarshal is `FUN_14646c2b0` (`0x14646c2b0`). The header reader is `FUN_146b07870` (`0x146b07870`). The header was checked on every type 8 body in the 285 ledgers. The count is 4,709,247. Every body matched. The payload length consumed the rest of the body.

Wire order:

| Step | Type | Rule |
|---|---|---|
| 1 | `u8` | The value is `0` or `1`. |
| 2 | prefix-coded integer | Present only when step 1 is `1`. The reader is `FUN_14087b770`. |
| 3 | `u8` | One byte. |
| 4 | `u8` | One byte. |
| 5 | `u8` | The value is `0` or `1`. |
| 6 | `u8` | The value is `0` or `1`. |
| 7 | optional block | Present only when step 6 is `1`. `FUN_146a6ddb0` (`0x146a6ddb0`) reads two prefix-coded counts. The second count is at most 100. That many `u16` big-endian values follow. |
| 8 | prefix-coded integer | Payload length. |
| 9 | raw bytes | That many bytes. This note does not decode them. |

**Corrected.** A header that uses LEB128, then three fixed bytes, then a count of indexes, does not match these bodies. Source of that older header shape: Aeternum-World `docs/Network/packet_system.md`. The prefix code above is the code in the binary.

**Unknown.** Names for steps 3 and 4. The reader stores them as plain bytes. The capture was not used to print them, because they can change per bundle.

## Layout status

| Message | Layout status | Still missing |
|---|---|---|
| 446 and 1034 | Complete | none |
| 1628 | Partial | Field tree of the constant tail after offset 89. |
| 164 | Complete | none |
| 1617 | Complete | none |
| 3236 | Partial | Fields inside the nested block. |
| 335 | Complete | none |
| 1635 | Unknown | Wire reader. The class UUID text is not in the image. |
| 4246 | Complete | none |
| 4247 | Complete | none |
| 6310 | Unknown | Wire reader. The class UUID text is not in the image. |
| 8 outer header | Complete | Entity bytes inside the payload. That decode was out of scope. |

## Check

**Observed.** On 2026-09-02 a second, separate script decoded the 285 ledgers again. It printed counts and the constants in this note only. It confirmed these facts:

- Every body length.
- The fixed and changing slices of 446, 1034, 1628, and 4246.
- The byte `5` in 1628.
- The constants of 4246.
- The value `2` in 4247 on the 43 login contexts. Contexts that start in the middle of a session also show `0`.
- The type 8 header rule on 4,709,282 bodies, with 0 failures.

The two scripts count type 8 bodies a little differently. The reason is that the reliable order and the chunk join differ between them.

## Files read

- `docs/re/2026-09-02-post-registration-sequence.md`
- `docs/re/2026-09-02-carrier-captures.md`
- `local-data/Aeternum-World-main/docs/Network/packet_system.md`
- `local-data/Aeternum-World-main/Catalog/uuid_to_class.json`
- `<game-dir>/typeindex.json`
- `local-data/azoth-catalog/packets/<type>.json` for the types in this note
- First-light codec files under `local-data/first-light-main/server/javelin/` for claimed sizes and field splits only
- `local-data/first-light-main/docs/post-v3-sequence.md` and `post-v3-flow.md`
- Ledgers under `local-data/captures/**/ledger.bin`
- Ghidra program `NewWorld.exe` in `<re-scratch>/ghidra`, read-only
