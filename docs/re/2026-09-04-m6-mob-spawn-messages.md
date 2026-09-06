# How a mob appears and goes away

Date: 2026-09-04.

Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`.

This note is the reader for `MB::GdeMetadataReplicatedState` (type 10) and the remove messages that take an entity off the client. It does not repeat the type 8 member header. That header is in `docs/re/2026-09-02-replication-and-movement.md`.

The game was not run. Easy Anti-Cheat was not analyzed. No new traffic was recorded. No body bytes, names, ids, positions, tokens, or IP addresses are in this note.

This note follows the repository and the client binary.

Labels: **Observed** means the binary or the captures showed the fact. **Inferred** means a conclusion from those facts. **Unknown** means the files do not show it.

## Method

The existing Ghidra project `<re-scratch>/ghidra` (program `NewWorld.exe`) was opened read-only with analysis left off. The heap was `GHIDRA_HEADLESS_MAXMEM=16G`. The analyzer CPU count was `-max-cpu 10`, and the JVM property `cpu.core.override` was 10. Ten decompiler threads ran. REA `open_binary` was not called. Its schema has no memory or CPU fields, and a call would import the same executable into a second project. There is no REA Evidence ID for this pass. The decompiler output is under `<re-scratch>/scratch-m6/out` and `out2`. The headless logs are `ghidra.log` and `ghidra2.log` in that folder.

Class unmarshal addresses were taken from Aeternum-World `Catalog/uuid_to_class.json` (AGPL-3.0) and checked in this binary. Each address is a function in this image.

Captures checked for an exact read of type 10 and type 11, with the reliable-frame join from the carrier note: one training-dummy session (97.0 s), one cave session (142.6 s), and one catacombs session (985.5 s). Counts only.

## 1. Create: type 10

**Observed.** Type 10 is `MB::GdeMetadataReplicatedState`. The catalog unmarshal is `FUN_1415136e0` (`0x1415136e0`). It builds the object in `FUN_14161c070` (`0x14161c070`) and then calls `FUN_146160ae0` (`0x146160ae0`). That function calls virtual `+0x90`, then `+0xa0`, then `+0xb0`.

**Observed.** The instance vtable stored by the constructor is `0x148041178`. Virtual `+0x90` is `FUN_1417b4110` (`0x1417b4110`). The replication note already established that the bundle path calls `+0x90` only. The same function is the bundle reader for type 13.

**Observed.** `FUN_1417b4110` reads one byte per group of up to 8 slots. A set bit reads that slot. `FUN_1417b43c0` (`0x1417b43c0`) reads one byte per group of up to 7 children. Bit 7 of that byte means another child byte follows. A set child bit calls virtual `+0x30` on that child.

**Observed.** The base constructor `FUN_14162f3e0` (`0x14162f3e0`) calls `FUN_141677dd0` (`0x141677dd0`) once, so the object starts with one slot. The type 10 constructor then pushes two children onto that slot, in this order:

| Child | Name in the constructor | Reader `+0x30` | Bytes when present |
|---|---|---|---|
| 0 | `AssetId` | `FUN_1417b3670` (`0x1417b3670`) | 16 raw bytes, then one `u32` big-endian |
| 1 | `GdeRef` | `FUN_1417b3ae0` (`0x1417b3ae0`) | 16 raw bytes |

`FUN_14087a220` (`0x14087a220`) byte-swaps the `u32`. The 16-byte reads are `FUN_140878610` with a length of 16. `ReplicationCategory` is registered on a different list (`param + 0x680`). It is not one of these two children.

**Inferred.** A type 10 body on the bundle path is one slot byte, then the child bytes for slot 0 when bit 0 is set. Bit 0 of the child byte selects `AssetId` (20 bytes). Bit 1 selects `GdeRef` (16 bytes). A clear bit reads nothing. The 16 raw bytes are the asset id and the entity id. This note does not copy them.

**Observed.** The same reader was run on all 695 ledgers. It accepts a type 10 member when that member is reached before any class the walk cannot read. Reached bodies: 66,223. Failures: 0. A failure means the reader ran off the end of the payload. The three fight sessions inside that total: training dummy 16, cave 52, catacombs 55.

**Unknown.** Type 10 members that sit after a class this walk cannot read. Those bodies were not reached, because the member header has no length. The sub-id inside `AssetId`. The Azoth field note says the `u32` is always 2. This pass did not print values.

## 2. Remove

**Observed.** The catalog names these server messages. Each address is a real function in this image. The body reader is the function each unmarshal calls after it builds the object.

| Type | Class | Body reader |
|---|---|---|
| 566 | `Aoi::GhostBaseTrait::GridRemoveActorMsg` | `FUN_1415b7940` (`0x1415b7940`) |
| 1517 | `Aoi::GhostExTrait::GridRemoveActorAoiMsg` | `FUN_1415b7860` (`0x1415b7860`) |
| 1791 | `Aoi::GlobalEntityListenerTrait::GlobalRemoveActorMsg` | `FUN_1415b6ab0` (`0x1415b6ab0`) |
| 1414 | `Aoi::GhostBaseTrait::GridRemoveActorsBatchMsg` | `FUN_1415b7ad0` (`0x1415b7ad0`) |
| 161 | `Aoi::GhostExTrait::GridRemoveActorsAoiBatchMsg` | `FUN_1415b7a20` (`0x1415b7a20`) |

**Observed.** Type 1791 reads 16 raw bytes, one `u64` big-endian (`FUN_14087ad90`, byte swap `FUN_1461679c0`), and 16 raw bytes. Type 566 reads one byte, one `u32` big-endian, then `FUN_1415acc30` (one `u32` big-endian and two 16-byte copies), then `FUN_1417aef20`. Type 1517 uses the same head and ends in `FUN_1417aced0` (16 raw bytes, one `u64` big-endian, `FUN_1415acfc0`, then two flag bytes that must be 0 or 1).

**Inferred.** Type 1791 is the short remove: an id, a `u64`, and a second id. Types 566 and 1517 are the area-of-interest removes. The client needs one of these, plus the absence of further type 10 updates, to drop a non-player entity. Which of the three a mob despawn uses was not separated in the captures.

**Observed.** A count of these type indexes across all 695 ledgers, with the reliable-frame join: type 566 count 0, type 1517 count 0, type 1791 count 0. There is no body to fail an exact-end check. The readers are decompiled above and were not applied to a captured body because none exist in this set. A mob leaving the interest set in these captures is not one of these three messages.

## Status

| Item | Status | Still missing |
|---|---|---|
| Type 10 child order and sizes | Reached bodies 66,223, failures 0 | Bodies after an unread class |
| `AssetId` sub-id value | Unknown | A value census that prints only the `u32` |
| Remove message readers | Decompiled. Count in 695 ledgers is 0 | No captured body to check |
| Which remove a mob uses | Unknown | A count of these types around a mob leaving the interest set |

## Files read

- `docs/re/2026-09-02-replication-and-movement.md`
- `docs/re/2026-09-04-movement-message.md`
- `docs/re/2026-09-04-bootstrap-message-bodies.md`
- `docs/re/2026-09-02-carrier-captures.md`
- `local-data/Aeternum-World-main/Catalog/uuid_to_class.json` (AGPL-3.0, addresses only)
- Ghidra program `NewWorld.exe` in `<re-scratch>/ghidra`, read-only
