# Replication and movement

Date: 2026-09-02.

This note gives the layout inside a type 8 payload, and the layout of the client movement message. The type 8 outer header is in `docs/re/2026-09-04-bootstrap-message-bodies.md`. This note starts at the payload bytes after the prefix-coded payload length. Carrier and message order are in the 2 September carrier and sequence notes.

The game was not run. No new traffic was recorded. Easy Anti-Cheat was not analyzed. The Ghidra project was opened read-only. Ten decompiler threads ran. The heap was 16 GB or 24 GB. No body bytes, names, ids, exact positions, tokens, chat, or IP addresses are in this note.

Labels: **Observed** means the binary or the captures showed the fact. **Inferred** means a conclusion from those facts. **Unknown** means the files do not show it.

## 1. Type 8 payload members

**Observed.** The payload walk is `FUN_14175ccc0` (`0x14175ccc0`). It calls `FUN_146af20d0` (`0x146af20d0`) for each group. The loop stops when two remaining-byte counters are equal.

One group:

| Step | Type | Rule |
|---|---|---|
| 1 | prefix-coded integer | The caller keeps the low 16 bits. |
| 2 | `u8` | Member count. |
| 3 | member | Step 2 times. Reader `FUN_146af2340` (`0x146af2340`). |

One member:

| Step | Type | Rule |
|---|---|---|
| 1 | prefix-coded integer | The code does not name this field. |
| 2 | type id | `FUN_1461acfe0` (`0x1461acfe0`). |
| 3 | body | Virtual `+0x90` on the type object. |

**Observed.** The type id is one prefix-coded integer. Value `0` means the next 16 bytes are raw. Any other value is an index into a table of 16-byte ids. The stride is 16. No extra uuid follows a non-zero index.

**Corrected.** Source: Aeternum-World `docs/Network/packet_system.md`. That note uses LEB128 for these integers. This binary uses the same prefix code as the outer header. That code is not LEB128.

**Inferred.** The capture walk treated a non-zero type id as a `typeindex.json` slot. The joined class names match the catalog. Type 13 bodies then decode. The 16-byte table entries are those class ids.

**Observed.** 285 ledgers. Type 8 messages: 4,709,247. Outer-header failures: 0. Single-member payloads: 3,984,498. Multi-member payloads: 724,749. A single-member payload has one group and a count of 1. Its body size is the bytes after the member header.

**Unknown.** Body size of a later member in a multi-member payload. The header has no length field. A later body needs that class's own reader.

### Twenty most common classes

The count is every reached member header. The median is the single-member body size only. A class with no single-member payload has an unknown size.

| Class | Type | Count | Single-member | Median |
|---|---|---|---|---|
| `MB::ALCReplicatedState` | 11 | 4,669,175 | 3,976,280 | 58 |
| `MB::PlayerComponentReplicatedState` | 3935 | 18,477 | 0 | Unknown |
| `Javelin::GlobalMapDataManagerComponentReplicatedState` | 3780 | 4,463 | 4,458 | 1,732 |
| `MB::DamageReceiverComponentReplicatedState` | 1528 | 4,163 | 0 | Unknown |
| `MB::LandClaimManagerComponentReplicatedState` | 3086 | 2,584 | 2,548 | 3,992 |
| `MB::GdeMetadataReplicatedState` | 10 | 2,477 | 387 | 281 |
| `MB::PositionInTheWorldReplicatedState` | 13 | 1,538 | 6 | 15 on the decoded bodies |
| `MB::ItemManagementComponentReplicatedState` | 5437 | 1,176 | 0 | Unknown |
| `MB::DelayedEventComponentReplicatedState` | 3139 | 1,094 | 25 | 5 |
| `Javelin::GroupDataComponentReplicatedState` | 3451 | 770 | 612 | 704 |
| `MB::PlayerHousingReplicatedState` | 2768 | 579 | 0 | Unknown |
| `MB::WarDataComponentReplicatedState` | 1739 | 564 | 33 | 3,755 |
| `MB::GatherableControllerReplicatedState` | 12 | 526 | 4 | 554 |
| `MB::InteractReplicatedState` | 2930 | 445 | 51 | 6 |
| `MB::AbilityInstanceTrackingComponentReplicatedState` | 2267 | 384 | 0 | Unknown |
| `MB::VariationComponentReplicatedState` | 14 | 281 | 0 | Unknown |
| `MB::TimelineComponentReplicatedState` | 1647 | 268 | 32 | 45 |
| `Javelin::PlayerArenaReplicatedState` | 2406 | 148 | 0 | Unknown |
| `Javelin::GameModeReplicatedState` | 2343 | 146 | 16 | 8 |
| `Javelin::InstancedSlayerScriptReplicatedState` | 6234 | 146 | 0 | Unknown |

Common single-member sizes for type 11 are 5 (251,006) and 9 (109,905).

## 2. PositionInTheWorldReplicatedState

**Observed.** Type index 13. Unmarshal is `FUN_145ce5f90` (`0x145ce5f90`). The bundle path calls virtual `+0x90` only. That function is `FUN_1417b4110` (`0x1417b4110`). The full unmarshal also has a second bitmap and one `0`/`1` flag per field. The bundle path does not read those.

**Corrected.** Source: Aeternum-World. Its dataset address `0x1417B417C` sits inside `FUN_1417b4110`. The entry is `0x1417b4110`.

The class registers three children in slot 0: `position`, `rotation`, `scale`.

| Step | Type | Rule |
|---|---|---|
| 1 | `u8` | Slot byte. Bit 0 set means slot 0 follows. Other bits were clear on every reached member. |
| 2 | `u8` | Present when bit 0 of step 1 is set. Bits 0, 1, and 2 select the three children. Bit `0x80` means another bitmap byte. That bit was clear on every reached member. |

**Observed.** Position reader `FUN_145cea190` (`0x145cea190`). x is `f32` big-endian. y is `f32` big-endian. z is `u16` big-endian. The range constants are `-100` and `2000`. The decode is `u16 * (1/65535) * (max - min) + min`.

**Observed.** Rotation reader `FUN_14087b250` (`0x14087b250`). One mask byte. A present component is `u16` big-endian, mapped to `[-1, 1]`. `w` is not on the wire. `w` is the square root of `max(0, 1 - x*x - y*y - z*z)`, then the four values are normalized.

| Mask bit | Set | Clear |
|---|---|---|
| 0 | x is 0. No `u16`. | See bit 3. |
| 3 | x is 1, and only when bit 0 is clear. No `u16`. | Read one `u16`. |
| 1 | y is 0. No `u16`. | See bit 4. |
| 4 | y is 1, and only when bit 1 is clear. No `u16`. | Read one `u16`. |
| 2 | z is 0. No `u16`. | See bit 5. |
| 5 | z is 1, and only when bit 2 is clear. No `u16`. | Read one `u16`. |
| 6 | `w` is negated. No bytes. | `w` stays non-negative. |

**Unknown.** Mask bit 7. The table has no read for it. The bit was clear on all 1,538 reached members.

**Observed.** Scale reader `FUN_14087b0b0` (`0x14087b0b0`). Each value is one binary16 (`u16` big-endian). Tag `0` stores four `f32` values of `1` and reads no value. Tag `1` reads one binary16 and copies it to four slots. Any other tag reads three binary16 values.

**Corrected.** Source: Aeternum-World. That note requires tag `2` for three values. This reader uses three values for every tag other than `0` and `1`.

**Observed.** The walk reached 1,538 type 13 members. Leftover-zero count: 1,538. Fail count: 0. Of those, 1,527 have a position, 1,454 have a rotation, and 119 have a scale. Scale tag 1 occurs 62 times. Another tag occurs 57 times. Tag 0 occurs 0 times.

**Observed.** Rounded outward, x spans 0 to 14,000 and y spans 0 to 10,000. z raw `u16`, rounded outward to 1,024, spans 4,096 to 22,528. The samples include neither 0 nor 65,535.

**Unknown.** Type 13 members that follow a different class. Those earlier bodies have no length, so the walk cannot reach them. 4,709,241 payloads stop at the first member that is not type 13.

## 3. Client movement

**Observed.** Type 1589 is `ActionListComponentServerFacet_ReceiveInputFromClient`. Count: 2,203,200. Every message is client to server, channel 1, and unreliable. Body length is 63 to 720. The median is 278. Unmarshal is `FUN_144867c70` (`0x144867c70`).

Wire order:

| Step | Type | Rule |
|---|---|---|
| 1 | `u64` big-endian | |
| 2 | `u64` big-endian | |
| 3 | fixed head | `FUN_142a2e6b0` (`0x142a2e6b0`). See the head table. |
| 4 | `u8` | Record count. |
| 5 | record | Step 4 times. `FUN_142a25d90` (`0x142a25d90`). |

Fixed head:

| Step | Type |
|---|---|
| 1 to 4 | four `u8` |
| 5 to 7 | three prefix-coded integers |
| 8 | `u8` |
| 9 | `FUN_1461ac530` (`0x1461ac530`) |
| 10 | `u8` |
| 11 to 13 | three prefix-coded integers |
| 14 | `u8` |
| 15 | `FUN_142a18620` (`0x142a18620`) |
| 16 and 17 | two prefix-coded integers |

**Observed.** Step 9 reads a `u8` that must be `0` or `1`. Value `1` reads a prefix integer. A `u64` big-endian always follows. Step 15 reads a prefix count of 0 through 16, two prefix integers per item, then two more prefix integers.

Each record starts with 3 raw mask bytes and one `u16` big-endian. Later fields follow the mask.

| Gate | Size when set |
|---|---|
| bit 0 | prefix integer |
| bit 1 | 1 byte |
| bit 2 | 1 byte, value `0` or `1` |
| bit 3 | 1 byte |
| bit 4 | `u16` big-endian. Range constants are `0` and `10`. |
| bit 5 | `f32` big-endian |
| bit 6 | two `f32` big-endian |
| bit 7 | two `f32` big-endian and one `u16` big-endian. z uses range constants `-100` and `1000`. |
| bit 8 | `FUN_1435ebc30` (`0x1435ebc30`) |

**Observed.** Bit 7 is the position inside the record. `FUN_14416eb00` (`0x14416eb00`) is always read after that bit. Its tag `3` reads 1 byte. Any other tag reads 18 bytes. That function can carry the same position form.

**Observed.** After bit 8 the reader uses a cursor that starts at bit 9. A clear bit consumes 0 bytes. The next test uses the following bit. Row 11 is three binary16 values when its bit is set, 6 bytes. It does not read a new flag byte.

**Observed.** A fixed record of 14, 15, or 16 bytes does not match these bodies. The best fixed rule fits 328,404 bodies and fails 1,874,796.

**Observed.** The binary layout, including the row 11 rule, ends with 0 bytes left on 445 bodies. Fail count: 2,202,755.

**Observed.** The check that stops the most bodies is the prefix count before `FUN_142a1a310` (`0x142a1a310`). It stops 854,698 bodies. The offset median is 128. The mask bit 8 flag in `FUN_1435ebc30` stops 742,241 bodies. The offset median is 126. The flag byte is not `0` or `1` at that point in the decoder.

**Unknown.** Why those two checks fail on most bodies. The functions are decompiled. The capture decoder still stops. A later pass has to align those two reads with the bytes at offsets 126 and 128. This note does not guess a new size.

**Unknown.** Another client-to-server message that carries position. No client-to-server class name contains Position, Move, or Velocity. The binary tied a position read to type 1589 and to `FUN_14416eb00` inside that record. It did not tie a second message type.

## Status

| Item | Status | Still missing |
|---|---|---|
| Type 8 member header | Complete | none |
| Type 8 later-member body size | Unknown | A length field is not in the header. Each other class needs its own reader. |
| Position class on the bundle path | Complete | Mask bit 7. It was never set. The table has no read for it. |
| Type 13 members after another class | Unknown | Those earlier bodies have no length. |
| Type 1589 head and mask bits 0 to 7 | Complete | none |
| Type 1589 full body on the captures | Partial | Bit 8 flag and the prefix count before `FUN_142a1a310` fail on most bodies. |
| Another client position message | Unknown | No second type was tied to a position reader. |

## Check

**Observed.** On 2026-09-02 a second, separate script decoded the 285 ledgers again. It printed counts and rounded ranges only.

- The first member header is valid on all type 8 payloads (4,709,282 in that script; the chunk join differs a little from the first script).
- Type 13 alone in its payload: 6 bodies decode with 0 bytes left. Type 13 first in a longer payload: on all 1,529, the bytes after the body start a valid member header. The position ranges are the same as in section 2.
- Type 1589: the fixed head decodes on all 2,203,200 bodies.
- The mask bytes of a record are little-endian: bit 0 is bit 0 of the first byte. With the other byte order, no record has bit 7.
- With bits 0 to 6 at the sizes in section 3, the first record has bit 7 on 2,189,999 bodies. On 2,111,605 of them, x and y are finite and inside 0 to 14,336. On 78,394, they are not. On 32 bodies, bit 2 is not 0 or 1.
- The finite x and y of bit 7 span only 0 to 1,000, rounded outward. Type 13 positions span 0 to 14,000. The raw `u16` z of bit 7 spans the full 0 to 65,535.

**Inferred.** The bit 7 floats are not the world position as such. They can be relative to a region or another base. A size before bit 7 is also still wrong for some records.

**Unknown.** The meaning of the bit 7 floats, and the cause of the 78,394 values that are not on the map.

## Files read

- `docs/re/2026-09-04-bootstrap-message-bodies.md`
- `docs/re/2026-09-02-carrier-captures.md`
- `docs/re/2026-09-02-post-registration-sequence.md`
- `local-data/Aeternum-World-main/docs/Network/packet_system.md`
- `local-data/Aeternum-World-main/Catalog/uuid_to_class.json`
- `<game-dir>/typeindex.json`
- `local-data/first-light-main/server/javelin/chunked_stream_08.py` and `action_history_635.py` for size claims only
- `local-data/azoth-catalog/packets/<type>.json` for the class purpose text
- Ledgers under `local-data/captures/**/ledger.bin`
- Ghidra program `NewWorld.exe` in `<re-scratch>/ghidra`, read-only
