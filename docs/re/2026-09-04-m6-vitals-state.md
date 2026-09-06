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

**Inferred.** Max health on the wire is the slot 0 child named `maxHealth`, a big-endian `u16`, and it is usually absent. Current health would have been `HealthAmount`. That field is not in the replicated body. A capture of type 15 does not carry current health.

**Unknown.** A runtime store to `0x14a563290` from code that is not in this image. The searches above cover every RIP-relative access in `.text`, every absolute address immediate in the file, and every Ghidra reference in the analyzed program. A later method would have to watch the global change while the process runs. That requires running the client, which this pass does not do.

## Files read

- `docs/re/2026-09-02-replication-and-movement.md`
- `docs/re/2026-09-04-m6-mob-spawn-messages.md`
- Ghidra program `NewWorld.exe` in `<re-scratch>/ghidra`, read-only
- `NewWorld.exe` disassembly of `FUN_14671e040`
