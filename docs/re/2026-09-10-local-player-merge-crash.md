# Local-player merge crash

Date: 2026-09-10. Build: Steam `25810808`. Image base `0x140000000`.

Offline only. The game was not run for this note. The fault values come from the existing Proton log `~/steam-1063730.log`. Easy Anti-Cheat was not analyzed. No body bytes, names, ids, positions, tokens, or IP addresses are in this note. The field name `idRel` is a static string in the client.

Labels: **Observed**, **Inferred**, **Unknown**.

Method. `open_binary` on `Bin64/NewWorld.exe` (sha256 `30fe6461f929ba44178376a72f0765dc5da28ccba20e5006a60a229babd74033`) started a new Ghidra analysis. That analysis did not finish inside the tool limit (330 s). No REA Evidence ID was returned. `close_binary` was called after the investigation. The function text below is from the existing current-build project `<re-scratch>/ghidra-25810808`, program `/NewWorld.exe`, opened read-only with analysis off. The fault instruction was read from that listing and checked against the log registers.

## Answer

**Observed.** The read fault at `0x14172de81` is in `FUN_14172dce0`. The bad value is the incoming state's qword at `+0x10`. On this crash that qword is `0x148100c94`, the address of the static field name `idRel`.

**Observed.** `idRel` is written by the ALC state constructor `FUN_142a36d50`. The generic merge reads that qword as a pointer to 800-byte slots. The load of the next pointer faults.

**Inferred.** The stored object is a generic replicated state. The incoming object is an ALC state. The generic merge runs because a state for the same cache key already exists. Confidence is high for the bad field and the call path. Confidence is medium for which earlier record stored the generic state.

## 1. The three functions

**Observed.** Crash log, thread `014c`. Read of `0x0000006c65526471`. `rip = 0x14172de81`. `rax = 0x0000006c65526469`. `r8 = 0x148100c94`. `r14 = 0`. Instruction: `mov r8, qword ptr [rax+r14+8]`.

**Observed.** `rax` is the first eight bytes at `r8`. Those bytes are the characters of `idRel` and three zero bytes. `r8` is the address of that string in the image. The same address is the only data reference to the string. Both references are in `FUN_142a36d50` at `0x142a37632` and `0x142a37639`.

**Observed.** Return addresses in the log:

| Return | Function | Role |
|---|---|---|
| `0x141790861` | `FUN_141790730` | cache update, calls the merge |
| `0x14171a17c` | `FUN_141718de0` | per-member state handler |
| `0x14175e1b0` | `FUN_14175dbe0` | bundle member walk |
| `0x14643fdda` | `FUN_14643fa70` | unexpected-message handler |
| `0x146457270` | `FUN_146457150` | `ProcessPendingReliableMsgQueue` |

### `FUN_14172dce0` (the merge)

**Observed.** Windows register roles at entry:

| Register | Role |
|---|---|
| `rcx` (`r13`) | stored state. The call is `vtable + 0x10` on this object. |
| `rdx` | output shared pointer. |
| `r8` (`r15`) | pointer to the incoming shared pointer. `*r8` is saved at `[rsp+0x50]`. |
| `r9` (`rdi`) | `DAT_14a436d10`, passed through to each child. |
| stack byte | flag from the caller. |

**Observed.** The function calls `vtable + 0xb8` on the stored state and keeps the clone in `rbp`. It counts stored slots as `(end - begin) / 800` from `+0x10` and `+0x18`. For slot 0 it then does:

```
r8 = [incoming + 0x10] + slot_offset
rax = [r8]
r8 = [rax + child_offset + 8]
```

`child_offset` is 0 at the fault (`r14 = 0`). `slot_offset` is 0 on the first slot (`r9` is cleared at `0x14172ddf4`). So `[incoming + 0x10]` is `0x148100c94`.

**Observed.** The same loop loads the clone's child first (`rsi` at the fault is a heap pointer, `0x2049cc987d0`). That load did not fault. The fault is the incoming side only.

**Observed.** The child call that the faulting load prepares is virtual `+0x38`. The second argument comes from the stored slot. The third argument comes from the incoming slot.

### `FUN_141790730` (the cache)

**Observed.** Call from `0x14171a177`. `r8d` is the component index (`esi` after the prologue). `r9` is the incoming shared pointer. `rcx` and `rdx` are passed unchanged into `FUN_1416beb20`. That function hashes `rdx` in the table owned by `rcx` and returns a tree. The tree is then searched by the component index.

**Observed.** The state pointer lives at `node + 0x20`. If it is null, the function copies the incoming shared pointer in and calls virtual `+0x30`. If it is not null, it calls virtual `+0x10`. This crash took the second call. The stored object's `vtable + 0x10` is `FUN_14172dce0`.

**Inferred.** `rcx` is the cache owner and `rdx` is the entity key. The component index is a separate argument. The names of those two objects are not recovered here.

### `FUN_141718de0` (the handler)

**Observed.** Signature from the decompiler: `(param_1, ushort param_2, member_vector param_3, param_4, flag param_5)`. `FUN_14175dbe0` calls it once per bundle member.

**Observed.** One branch logs `GDE not present, but is managed (offline). Storing state in offline cache.` A later branch in the same member loop logs `Merging with state present in offline cache.` and also calls `vtable + 0x10`. This crash returned into `FUN_141790730`, so the faulting call is the cache update, not that earlier direct call.

**Observed.** Another branch logs `Received new local player metadata, updating m_localPlayersServerGDERef.`

## 2. What the ALC constructor puts at `+0x10`

**Observed.** `FUN_142a36d50` is the current-build ALC state constructor. It builds two embedded slots at `param_1 + 0x10` (`param_1 + 2`), each `0x3b0` bytes, with element constructor `0x142a38cb0`.

**Observed.** `0x142a38cb0` begins with `mov qword ptr [rcx+0x360], rcx`. The name cursor of a slot is the slot address itself.

**Observed.** After the slots exist, `FUN_142a36d50` writes the pointer `idRel` through that cursor, then adds `0x10`. The first qword of the first embedded slot becomes `0x148100c94`. That qword is the state's `+0x10`.

**Observed.** The ALC vtable stored by this constructor is `0x148100b10`. Its `+0x10` is `FUN_142a3f7e0`. Its `+0xb8` is `FUN_142a3bff0`. Neither is `FUN_14172dce0`.

**Inferred.** The incoming object was built by `FUN_142a36d50`. The stored object was not. A generic state uses a pointer to 800-byte slots at `+0x10`. An ALC state uses that same offset as the first name pointer. The generic merge cannot take an ALC object as the incoming record.

## 3. A `0x00` body

**Observed.** Prior note `docs/re/2026-09-10-player-component-slots.md`: every class in the 56-class table has 1 to 4 slots, the generic reader consumes one slot byte, and `0x00` is the empty body. The constructor runs in unmarshal before the body is consumed. This note does not repeat that class table.

**Inferred.** A `0x00` body of a generic class yields a constructed state. The cache stores that object on the first insert (`virtual +0x30`). A later generic record for the same key takes `FUN_14172dce0` and walks real slot vectors.

**Observed.** The ALC reader is not that generic reader. Its constructor writes `idRel` at `+0x10` for every body, including the 15-byte position body. The body contents do not remove that pointer.

## 4. The pending queue and message order

**Observed.** `FUN_14643fa70` appends the message to the vector at `param_1 + 0xd0` when the early context flag at `+0x1a2` is clear. For a bundle whose context byte matches `param_1 + 0x80`, it logs `HandleUnexpectedMessage skipping ReceiveStateBundle for bundle message due to clientContext == nullptr` and returns. `FUN_146457150` later logs `ProcessPendingReliableMsgQueue processing %zu messages from m_pendingUnexpectedMsgs` and delivers each saved message.

**Inferred.** The queue is the client's own path for a reliable message that arrives before the context exists. The crash is in that later delivery. The queue does not create the bad `+0x10` value.

**Observed.** Prior note `docs/re/2026-09-08-world-entry-live.md`: in `NH-MSQ-10.2-Quest`, each type `1635` is followed on channel 1 by the large reliable bundle. The current emulator sends the player bundle before `1635`, so the client queues it and applies it on the offline path (`GDE not present`).

**Inferred.** Apply-after-`1635` matches the capture order and can leave this offline handler. It does not change the ALC layout. If a generic state is already stored under the ALC key, the same merge still faults.

## 5. GdeRef and the local-player id

**Observed.** This faulting function does not compare a GdeRef. It merges two state objects.

**Observed.** `FUN_141718de0` contains the log text for `m_localPlayersServerGDERef`. Prior note: that branch runs when the interest id is 1 and a check on the entity (`+0x850`, virtual `+0x20`) is true.

**Unknown.** Whether this run took the offline path because the GdeRef, the `ClientAddEntry` id, or the type `1628` id did not match. The log line that would show the branch was not recovered from the fault registers.

## 6. Minimal local-player group

**Inferred.** The crashing pair is a generic stored state plus an ALC incoming state under one cache key. The safe shape is:

1. One GDE card (component 0, type 10) with a real asset id.
2. One ALC record (component 16, type 11) as the first state stored for that key. The 15-byte `pos10` body is the smallest body that still carries a position (prior note). The constructor still runs.
3. One `0x00` body for each generic class in the 56-class table, each at its own component index. Those indexes do not include 16.

**Inferred.** Do not store a generic state at the ALC key before the ALC record. The 56 empty records do not use index 16. A second bundle, a mob group, or a client seed can still occupy that key. The fault proves that some generic state was already there.

**Unknown.** The component index in this crash. `esi` held it inside `FUN_141790730`, and the fault frame no longer has it. The static evidence identifies the incoming object as the ALC state, not the index number.

**Unknown.** Whether the client builds a stored state for every prefab component before the bundle, including the indexes the table skips (3, 7, 8, 11, 15, 21, 23, 30, 37, 38, 40, 52, 53, 54, 58, 59, 63, 64, 69, 75). Crash 1 in the prior note shows that a facet apply on a record that never unmarshalled is unsafe. This crash shows a different failure: a record that did unmarshal (the ALC) merged into a generic object.

**Unknown.** Which of the 56 generic classes need a non-empty body after the merge succeeds. The merge itself only needs constructed slot vectors.

## Files read

- `local-data/handoff-merge-crash-re.md` (not kept)
- `local-data/handoff-player-components-re.md` (reference index) (not kept)
- `docs/re/2026-09-10-player-component-slots.md`
- `docs/re/2026-09-08-world-entry-live.md`
- `crates/nw-messages/src/player_components.rs` (index list only)
- `~/steam-1063730.log` (fault registers and return addresses)
- Ghidra project `<re-scratch>/ghidra-25810808`, program `/NewWorld.exe`, read-only
- Client image `Bin64/NewWorld.exe`

REA `open_binary` returned no Evidence ID. The decompiler output used here is from the existing project, not from the timed-out REA analysis.
