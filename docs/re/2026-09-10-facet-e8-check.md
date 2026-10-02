# The facet `+0xe8` is-local check

Date: 2026-09-10. Build: Steam `25810808`. Image base `0x140000000`.

Offline only. The game was not run for this note. Easy Anti-Cheat was not
analyzed. No body bytes, names, ids, positions, tokens, or IP addresses are in
this note. Capture data is counts, flag values, and equality of fields.

Labels: **Observed**, **Inferred**, **Unknown**.

Method. Ghidra project `<re-scratch>/ghidra-25810808`, program
`NewWorld.exe`, headless `-readOnly -noanalysis`, `MAXMEM=24G`, `-max-cpu 28`.
New scripts in `<re-scratch>/scripts`: `VtWindow.java` (vtable window,
pointer search, function range, instruction window), `MkDecomp.java` (makes a
function in memory, not saved, then decompiles), `OffsetWriters.java`,
`OffsetUses.java`, `SmallCallers.java`, `SlotScan.java`. Outputs under
`<re-scratch>/out-world-entry/` (`vtw`, `live`, `w318`, `role`,
`rolecall`, `cid`, `bus`, `busvt`, `busfn`, `cidw`, `c1140e`, `msgd`).
Capture check: a scratch Rust tool on `nw-capture` and `nw-reflect` that
prints counts only. It ran on the 112 open-world sessions that have a ledger.

## Answer

**Observed.** The live facet vtable starts at `0x148536b60`. The facet
constructor `FUN_146712b40` writes it. Slot `+0xe8` is `FUN_14684e020`:
`return *(int *)(facet + 0x318) == 1`. The earlier table `0x148536a98` is not a
vtable start. `FUN_14174d0c0` is a type-id cast and is not in the live path.

**Observed.** Only `FUN_14692b0e0` sets `facet + 0x318` to a role value:
`1` when the 3935 `characterId` text is the same as the local character id
text, else `2`. The constructor sets `0`. The local text comes from the
`GameConnection` field `character_id` (`+0x1140`). That field is the
`CharacterId` string of the login token.

**Observed.** On retail, the token `CharacterId` text is the 36-character,
lowercase, dashed text of the 1628 first presence id: 22 of 22 tokens.

**Inferred.** This server sends 3935 `characterId` as flag `0x01` (no dashes)
with the entity raw id. That text is 32 hex digits of the entity number. It is
never the same as the token text (the character UUID with dashes). So the role
is `2`, `+0xe8` is false, and `OnEnable` does not call `PlayerRegistry +0x10`.

**Fix.** Send the 3935 slot-2 `characterId` with flag `0x05` and the 16 bytes
of the player's character UUID (`Uuid::as_bytes()` order). See section 6.

## 1. The live vtable and `+0xe8`

**Observed.** `FUN_1468773a0` (`OnEnable`) pointer occurs once in the image, at
`0x148536bb8`. Data window `0x148536a00..0x148536d00`:

- `0x148536b60` is referenced by `FUN_146712b40` (`*param_1 = 0x148536b60`).
- `0x148536cd0` is referenced by `FUN_146712b40` (`param_1[5]`, second base).
- `0x148536a30`, `0x148536a98`, `0x148536b40`, `0x148536b50` are referenced
  only by destructors and their callers. Those entries are `_purecall` rows or
  destructor pointers of other tables.

**Observed.** From start `0x148536b60`:

| Slot | Function | Body |
|---|---|---|
| `+0x58` | `FUN_1468773a0` | `OnEnable` |
| `+0xb0` | `FUN_14683dba0` | applies a 3935 state to the facet |
| `+0xe0` | `FUN_1467742d0` | called by `OnEnable` |
| `+0xe8` | `FUN_14684e020` | `facet+0x318 == 1` |
| `+0xf0` | `FUN_14684e1b0` | `facet+0x318 != 0` |

**Observed.** `FUN_14684e020` and `FUN_14684e1b0` had no function in the
project. `MkDecomp.java` made them in memory. Both are three instructions
(`CMP dword [RCX+0x318]`, `SETZ`/`SETNZ`, `RET`).

**Observed.** `FUN_146712b40` writes `facet+0x318 = 0`
(`0x146712e6e MOV dword ptr [RDI+0x318],EBP`).

## 2. Who sets the role at `facet+0x318`

**Observed.** A scan of stores to `[reg+0x318]` in `0x146000000..0x147000000`
found 147 rows. In facet code, only `FUN_14692b0e0` stores a role:

- `local = FUN_140fca680(handler +0x248)`: an optional value from an EBus
  (bus getter `FUN_141027fe0`).
- If no value: role `2`.
- Else role `1` when `strlen` and bytes of `local.text` equal
  `param_2` text (`memcmp`), else role `2`.
- When role is not `1`, it tags the entity as not `LocalPlayer`.

**Observed.** Only caller: `FUN_14683dba0` (facet slot `+0xb0`). It calls
`FUN_14692b0e0(facet, state+0x7d0, state+0x880)` when the member at
`state+0x7c0` or `state+0x870` reports a change (`+0x60`).

**Observed.** 3935 constructor `FUN_146713160`: `param_1[0xf8]` (`+0x7c0`) is
`characterId` (vtable `PTR_FUN_1481e3e38`, text at `+0x7d0`, an
`AZStd::string`). `param_1[0x10e]` (`+0x870`) is `characterName` (text at
`+0x880`). Both are on slot 2 (`param_1[0x1e8]`).

**Observed.** `characterId` reader `FUN_1434cb580` calls `FUN_14087ac50`:

- Flag byte bit 0 clear: read a string (`FUN_1407daaf0`).
- Bit 0 set: read 16 bytes, then make text with
  `FUN_14147fad0(id, buf, 0x27, bit1, bit2, bit3)`.

**Observed.** `FUN_14147fad0`: bit 1 adds braces. Bit 2 adds a dash before
bytes 4, 6, 8, 10. Bit 3 makes upper case, else lower case. Bytes are written
in stored order, two hex digits each.

**Inferred.** Flag `0x01` gives 32 lowercase hex digits. Flag `0x05` gives the
canonical lowercase UUID text `8-4-4-4-12`.

## 3. The local character id (bus `+0x248`)

**Observed.** The bus handler is the game object (destructor `FUN_14640cb20`).
Its interface at object `+0x28` uses vtable `0x1484fc720`. Slot `+0x248` is
`FUN_1464367b0`. It reads `P = *(this+0x1b8)` (first `GameConnection`) and
returns `P+0x12f0` (text, 16 bytes, flag) only when `P+0x1300` (text length)
is not 0 and `P+0x1530` (connection state) is not 0.

**Observed.** `FUN_1464271e0` sets `P+0x12f0 = FUN_146408970(copy of P+0x1140)`.
`FUN_146408970` keeps the text as it is and parses the 16-byte form
(`FUN_140875a20`). CVar `javelin.impersonate-character-id` can replace it.

**Observed.** `FUN_14643d830` logs `P+0x1100` as `rep_address`, `+0x1120`
as `world_id`, `+0x1140` as `character_id`, `+0x1160` as `persona_id`.
That is the order of the login token strings (`RepAddress`, `WorldId`,
`CharacterId`, `PersonaId`; `crates/nw-reflect/src/registration/token.rs`).

**Inferred.** The local text is the token `CharacterId`. This server's login
puts `Uuid::to_string()` there (`crates/nw-login/src/game.rs`,
`bins/rep-gateway/src/registration.rs:116`): lowercase with dashes.

**Observed.** 1628 does not feed this check. `FUN_1415435f0` dispatches the
1628 handler with `(msg+0x10, +0x50, +0x74, +0x98, +0xc0, +0x100)`. The second
presence (`msg+0x100`) goes to actor `+0xb20` (`FUN_145aa0b60`). The first
presence (`msg+0x10`) is used only on the debug branch (`param_6+10 != 0`).

## 4. Captures (counts and equality only)

Sessions with a ledger: 112. Sessions with a 1628: 27 (38 contexts).

| Check | Result |
|---|---|
| 1628 first presence flag | `0x05` on 38 of 38 |
| 1628 second presence flag | `0x05` on 38 of 38 |
| 1628 first id equals second id | 0 of 38 |
| V3 login token `CharacterId` length | 36 on 22 of 22 |
| token text equals dashed text of 1628 first id | 22 of 22 |
| 3935 bodies that hold the 1628 first id | 224 bodies, 24 sessions; byte before it `0x05` on all 224 |
| 3935 slot-2-only bodies (`0x04`, child 0 set): byte after the child byte | `0x05` on 2,698 + 902 + 54 + 2 bodies |
| 3935 slot-2-only bodies whose first presence is the 1628 first id | 0 (those bodies are other players) |
| 3935 bodies that hold the 1628 second id | 2,396 bodies, 24 sessions; byte before it `0x05` |

**Inferred.** The 1628 first id is the own character UUID. Retail sends it in
the own 3935 body with flag `0x05`. The 1628 second id is shared by many
players. It sits after `characterName`, so it is likely `homeWorldId`.

## 5. What this server sends now

**Observed.** `bins/sim/src/world/members.rs:114` sets
`character_id = raw_id_of_entity(entity)`.
`crates/nw-messages/src/player_components.rs` writes it with
`PRESENCE_FLAG_FIXED = 0x01`. Text on the client: 32 hex digits of the entity
number. Token text: the character UUID with dashes. Length 32 against 36:
the `memcmp` branch is not reached. Role is `2`.

**Observed.** `crates/nw-messages/src/self_ident.rs` writes the 1628 first
presence as flag `0x01` with the entity raw id, and the second presence as
flag `0x01` with 16 zero bytes. This is not the cause of the `+0xe8` result.

## 6. Exact bytes the server must send

Own player 3935 (component 9) slot-2 body, 20 bytes after the member header:

| Offset | Bytes | Meaning |
|---|---|---|
| 0 | `0x04` | slot byte: slot 2 only |
| 1 | `0x09` | children 0 (`characterId`) and 3 (`playerConnected`) |
| 2 | `0x05` | presence flag: bit 0 (16 bytes) + bit 2 (dashes), lowercase |
| 3 to 18 | 16 bytes | `player.character.as_bytes()` (the login `Uuid`) |
| 19 | `0x01` | `playerConnected = true` |

**Inferred.** With these bytes, the client text equals the token text,
`FUN_14692b0e0` sets role `1`, `+0xe8` is true, and `OnEnable` calls
`PlayerRegistry +0x10` (if the deferred lookup at entity `+0x2b8` is not null).

Recommended for parity with retail (not needed by `+0xe8`):

- 1628 first presence: flag `0x05`, same 16 UUID bytes.
- 1628 second presence: flag `0x05`, the world UUID bytes. The use of actor
  `+0xb20` is **Unknown**.

Keep the spawn card GdeRef and type 164 on `raw_id_of_entity`. They are the
offline-cache key and do not take part in this check.

## 7. Residual risk

**Unknown.** The order of `OnEnable` and the first `FUN_14683dba0` call. The
role is set only when the 3935 state is applied. If `OnEnable` runs first, it
reads role `0` and takes the false path. No other recovered writer calls
`PlayerRegistry +0x10` later. Retail sends the own 3935 in the same spawn
group as the card. This server does the same. Confidence that the fix above
is enough: medium-high.

**Unknown.** The deferred lookup `FUN_1417db510(entity+0x2b8)` must also be
non-null on the true path. It was not traced in this pass.

## Files read

- `docs/re/2026-09-10-local-player-check.md`
- `local-data/morning-test-plan.md` (not kept)
- `crates/nw-messages/src/{player_components.rs,self_ident.rs,slots.rs}`
- `bins/sim/src/world/members.rs`
- `crates/nw-reflect/src/registration/token.rs`
- `local-data/azoth-catalog/packets/3935.json`
