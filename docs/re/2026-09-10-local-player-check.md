# Why the portrayed player is not local (InGame)

Date: 2026-09-10. Build: Steam `25810808`. Image base `0x140000000`.

Offline only. The game was not run for this note. Easy Anti-Cheat was not
analyzed. No body bytes, names, ids, positions, tokens, or IP addresses are in
this note. Capture data is counts, sizes, slot bits, and equality of fields.

Labels: **Observed**, **Inferred**, **Unknown**.

Method. Current-build Ghidra project `<re-scratch>/ghidra-25810808`,
program `/NewWorld.exe`, headless `-readOnly -noanalysis`, heap 32 GB, worker
copies `ghidra-25810808-w2`/`w3`/`w4`. Scripts `RefsDecomp.java`, `Callers.java`,
`DumpVtSlot.java`. Captures: `local-data/captures/open-world/*/fragments.jsonl`
and ledgers (equality only). REA `open_binary` was not used in this pass; a
prior pass on `Bin64/NewWorld.exe` timed out at 330 s with no Evidence ID.

## Answer

**Observed.** `WaitingForPlayerSpawn` becomes `InGame` when `PlayerRegistry`
virtual `+0x30` (`FUN_141026220`) returns a non-null entity.

**Observed.** `PlayerComponentClientFacet::OnEnable` (`FUN_1468773a0`, string at
`0x148559140`) is the only recovered writer of that registry through virtual
`+0x10`. It calls `+0x10` only when facet virtual `+0xe8` returns true and a
deferred lookup at entity `+0x2b8` (`FUN_1417db510`) is non-null.

**Observed.** The first 3935 body of the local player is never `0x00`. On
`NH-MSQ-10.2-Quest` the first slot-1 body is 87 bytes and the slot byte is 6
(slots 1 and 2 set, slot 0 clear). Later bodies are mostly slot byte 2 (slot 1
only). This server now sends slot byte `0x04` with `characterId` and
`playerConnected`.

**Observed.** Facet vtable start `0x148536a98` (destructor `FUN_146752e00`
writes it). `OnEnable` is slot `+0x120`. Slot `+0xe8` is `FUN_1468b6360`, also
copied at `+0xf0`. That function compares a 16-byte UUID to
`PlayerComponentClientFacet` (`FUN_1469493c0`) and then to the base class
(`FUN_141799010`); else it calls `FUN_14174d0c0`. `OnEnable` calls `+0xe8`
with one argument (the facet).

**Inferred.** The server must send a 3935 snapshot with slot 2 (`characterId`
and `playerConnected`) and a type 1628 first presence id that is the same 16
bytes as the slot-1 card `GdeRef`. The first 8 bytes of every `GdeRef` must be
unique and not all zero: that qword is the offline-cache key. Confidence is
high for the OnEnable gate, the 3935 slot bits, and the cache key. Confidence
is medium that `+0xe8` itself reads the 3935 children: the recovered `+0xe8`
is a type-id compare, not a field getter.

## 1. OnEnable and `+0xe8`

**Observed.** `FUN_1468773a0` calls `(*param_1 + 0xe8)(param_1)` three times.

**Observed.** First call:

- False: `FUN_1417db510(entity + 0x2e0)` then `FUN_1416accc0(entity + 0x2e0, 3, 0)`.
  No `PlayerRegistry +0x10`.
- True: `FUN_1417db510(entity + 0x2b8)` must be non-null, then HUD bind, then
  `PlayerRegistry +0x10` with the entity from `FUN_1406c6ba0` (entity at
  facet `+8`). Registry EBus is `FUN_14100a940`, RTTI `0x14a13d790`.

**Observed.** A later call is `PlayerRegistry +0x20` with the same entity. That
call is not the InGame poll. The poll is `+0x30`.

**Observed.** `FUN_146876fb0` reads `PlayerRegistry +0x30` and then
`PlayerRegistry +0x58(entity + 0x38)` as a boolean, and writes that boolean
through another object's `+0xe8`. So `+0x58` is "is this entity the local
player" after the registry holds one.

**Observed.** Methods used to recover the facet `+0xe8` body:

1. `VtOwner` / `FindVt` on `FUN_1468773a0`: no MSVC COL in 512 entries.
2. `DumpVtSlot` treating `0x148536bb8` (the OnEnable slot) as a vtable start:
   slot `+0xe8` is `FUN_146914930`, a setter (wrong base). Treating
   `0x148536a80` as start: `+0xe8` is `FUN_1468bff80`, which returns the
   string `PlayerComponentClientFacet` (GetTypeName).
3. Destructor `FUN_146752e00` writes vtable `0x148536a98`. From that start,
   `+0xe8` is `FUN_1468b6360` (UUID compare, two arguments). `FindXrefs` of
   `0x148536a98` lists only destructors. A live object may use a derived
   table that was not recovered.

**Unknown.** Whether the live facet uses `0x148536a98` or a derived vtable,
and what `FUN_14174d0c0` returns when `OnEnable` calls `+0xe8` with one
argument. Residual after those three methods.

## 2. Local player metadata (`m_localPlayersServerGDERef`)

**Observed.** `FUN_141718de0` logs `Received new local player metadata, updating
m_localPlayersServerGDERef.` The branch needs `interestId == 1` and GDE object
`+0x850` virtual `+0x20` non-zero.

**Observed.** GdeRef vtable `PTR_FUN_148042278` sits at GDE `+0x850`. The
16-byte value sits at `+0x860`. Virtual `+0x20` is `FUN_14171ae40`: true if
`+0x58` (`FUN_141720d30`: `*id != -1`) or `+0x08` (`FUN_1417209c0`).

**Observed.** `FUN_14178ea40(handler, GDE+0x860, …)` stores the GdeRef into
handler tables at `+0xb8` (hash `FUN_14087cdc0`).

**Observed.** GdeRef reader `FUN_1417b4a50` stores the 16-byte id at
object `+0x20`/`+0x28` and copies the first 8 bytes through `FUN_140870df0`
into `+0x30`. Portrayal `FUN_14178ea40` hashes `*(param_2+0x20)` with
`FUN_14087cdc0`. That qword is the first 8 bytes of the GdeRef.

**Observed.** Capture counts, 89 open-world sessions, type-10 bodies only:
slot-1 player cards 1,326; GdeRef present on all of them; first 8 bytes all
zero: 0; last 8 bytes all zero: 0; all 16 bytes zero: 0. Other cards 348,440;
first 8 all zero: 0; last 8 all zero: 27,761; all 16 zero: 0. Other AssetId
values: 244,423 present, 0 all-zero, every sub id 2.

**Inferred.** A leading 8 zero bytes in this emulator's `raw_id_of_entity`
collides every entity onto one cache key. That is the merge crash. The
metadata is-set check is presence (`*id != -1` or flag at `+0x60`), not
"nonzero bytes", so a present GdeRef with a zero first half can still be
"set" and still collide.

## 3. Type 1628 `PlayerManagerSelfIdentificationMsg`

**Observed.** Current-build handler `FUN_146455ec0` (old `FUN_146454c00`). No
direct callers (vtable dispatch). Unmarshal `FUN_141516e40`:

- Presence `FUN_14087ac50` (flag bit 0, then 16 raw bytes) into the message.
- Three `FUN_1415ad690` blocks (`u32` + 16 + 16).
- `FUN_1415bad60`.
- Second presence.

**Observed.** The handler copies identity with `FUN_145aa0b40` (36 bytes to
actor `+0xaf8`), `FUN_145aa0b60` (string + 16 + flag to `+0xb20`),
`FUN_145aa0bb0` (36 bytes to `+0xb84`), sets `param_1-0x7f0 = 1`, and ends in
`FUN_145a88140` (`ConnectionSuccess`). Debug flag `param_6+10` must be 0.

**Observed.** This emulator's 1628 body used a zero first presence id. Retail
bodies are 12,702 bytes; the first 16-byte id is not zero (bootstrap bodies
note: two id values on 43 contexts).

**Observed.** Type 164's 16-byte id is not the 1628 first id (bootstrap bodies
note, all 43 contexts). This emulator still puts the entity number in 164 and
in the card GdeRef.

**Inferred.** The local check can use the 1628 first presence as `characterId`.
The emulator now writes the same 16 bytes in 1628, 3935 `characterId`, 164, and
the card GdeRef: the entity raw id. Retail keeps 164 different from 1628; that
difference is not required for the recovered OnEnable path.

## 4. Type 3935 `PlayerComponentReplicatedState`

**Observed.** Constructor `FUN_146713160`. Base ctor plus two `FUN_1416789e0`
add-slot calls: three slots, one slot byte. Slot 1 (`param_1[0x1e7]`) is packet
`group0` (22 children, `loginMatchId` first). Slot 2 (`param_1[0x1e8]`) is
packet `group1` (9 children):

0. `characterId`: vtable `PTR_FUN_1481e3e38`, reader `FUN_1434cb580`
   (`FUN_14087ac50` presence: flag + 16 bytes)
1. `characterName`
2. `homeWorldId`: same presence vtable as `characterId`
3. `playerConnected`: vtable `PTR_FUN_148040208`, reader `FUN_1417b4c20`
   (one byte, 0 or 1)
4. `lookingThroughLoadout`
5. `playerType`
6. `isInStore`
7. `platformAccountId`
8. `platformType`

**Observed.** Capture first slot-1 3935 on `NH-MSQ-10.2-Quest`: first body 87
bytes, slot byte 6. Across 89 sessions, the first 3935 per (session,
interest) is slot byte 4 (slot 2 only) 1,588 times, slot byte 2 (slot 1
only) 50 times, slot byte 6 (slots 1 and 2) 32 times. Typical first-body
length is 54 to 62 bytes.

**Inferred.** A first body of `0x00` leaves `characterId` unset and
`playerConnected` false. If `+0xe8` reads either child, it returns false and
OnEnable never calls `PlayerRegistry +0x10`.

**Observed.** The smallest body that sets those two children: slot byte `0x04`
(slot 2 only), child byte `0x09` (children 0 and 3), presence 17 bytes, then
one boolean. Total 20 bytes after the member header.

## 5. Deferred actions

**Observed.** `FUN_1416ae900` logs `Exceeded max recursion for deferred
actions`. CVar `javelin.deferred-actions-max-depth`, default 1. One leftover
action (stride `0x50`) is dropped. Only caller: `FUN_141734dd0` (tick).

**Observed.** `FUN_1416accc0` is the enable-reason bus, not the drop queue.
`FUN_1417db510` looks up the deferred target entity.

**Inferred.** The live line `{ "numDeferredActions": 1 }` matches one dropped
action. OnEnable's false path posts reason 3 on entity `+0x2e0`. That can be
the dropped action. It does not register the local player. Fixing `+0xe8` so
OnEnable takes the true path is the way past this, not raising the CVar.

## 6. Cache key and mobs (Step C)

**Observed.** `FUN_1416beb20` hashes 64-bit `param_2` (`FUN_14087cdc0`) and
compares pointer equality `puVar3[2] == param_2`. Caller `FUN_141790730`
trees by component index: null insert `+0x30`, else merge `+0x10`
(`FUN_14172dce0`).

**Observed.** Prior merge crash: incoming ALC (`idRel` at `+0x10`) into a
generic stored state, component 5. Player component 5 is PlayerTime (generic).
Mob ALC used component 5.

**Inferred.** While `GDE not present`, the handler stores into one offline
cache. Two entities that share the first 8 bytes of GdeRef then collide, so
a mob ALC at component 5 merges into the player's generic component 5.

**Inferred.** Unique first-8 GdeRefs split the cache even before portrayal.
Do not send other entities until the client's own player has spawned
(`Vitals.spawned`, first movement) as a second guard. Then the GDE is present
and the cache is per entity. `NW_POSITIONS_ONLY` is not required. Mob cards
use the entity raw id as GdeRef and as AssetId GUID so an all-zero key is
not shared. The real mob prefab GUID is still Unknown. Player ALC stays at
component 16. Mob ALC stays at 5 (the most common non-player value).

## 7. Values this server must send

| Field | Value |
|---|---|
| Slot-1 card GdeRef | `raw_id_of_entity(player)`: number in both 8-byte halves |
| Type 164 entry | the same 16 bytes |
| Type 1628 first presence | the same 16 bytes |
| 3935 component 9 | slot 2: `characterId` = those 16 bytes, `playerConnected` = true |
| Other 55 player classes | empty `0x00` (still required so unmarshal runs) |
| Own ALC | component 16, `pos10` body |
| Other entities | hold until `Vitals.spawned`; each GdeRef unique in its first 8 bytes |

## 8. After InGame (static)

Sources: `local-data/nwdb.info` (start quests, FTUE items), level paks, current sim spawn.

Still missing for a playable spawn, once `InGame` holds:

- A start position that matches the FTUE / first-light beach, not only a finite sim cell.
- Gear and FTUE items on the player (nwdb start-quest item lists). The 56 empty
  component bodies carry no paperdoll, inventory, or vitals snapshot.
- A real mob prefab GUID from `assetcatalog.catalog`. Mob cards now use a unique
  derived GUID so they do not share an all-zero key; the client still cannot
  load a slice from that GUID.
- Client input 1589 is already answered with the own ALC each tick. Fall and
  snap-back are Unknown until a live InGame session.

## Files read

- `local-data/handoff-world-entry-complete.md`
- `local-data/handoff-player-components-re.md` (index)
- `local-data/handoff-local-player-check-re.md`
- `docs/re/2026-09-10-local-player-merge-crash.md`
- `docs/re/2026-09-10-player-component-slots.md`
- `docs/re/2026-09-08-world-entry-live.md`
- `docs/re/2026-09-04-bootstrap-message-bodies.md`
- `local-data/azoth-catalog/packets/3935.json`
- `crates/nw-messages/src/{self_ident.rs,player_components.rs,gde.rs,vitals.rs}`
- Ghidra current-build copies under `<re-scratch>/`; outputs
  `<re-scratch>/out-world-entry/`
