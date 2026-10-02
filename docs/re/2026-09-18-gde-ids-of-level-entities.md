# GDE ids of level entities (capitals and slice-spawned children)

Date: 2026-09-18.

Build: `NewWorld.exe` build 25810808 (decompile sweep `local-data/re-sweep`). Captures: every
`ledger.bin` under `local-data/captures` and `local-data/open world captures` smaller than 1.5 GB
(721 ledgers, 36,816 first sightings of zero-suffix spawn cards).

Labels: **Observed**, **Inferred**, **Unknown**.

## 1. The id on the wire

**Observed.** A level entity's spawn card (`GdeMetadataReplicatedState`, child `GdeRef`) is 16
bytes: 8 id bytes, then 8 zero bytes in the open world (the tutorial puts its phase id there).
The 8 id bytes are a little-endian `u64`. The facet entity of the entity is the same `u64`
(3743 answers name the station as `4d805a8c13e721ff`, the bytes `ff21e7138c5a804d` reversed).

**Observed.** The low 3 bits are a tag. Counts of distinct ids in the captures: tag 1: 3,216,
tag 3: 7,584, tag 7: 1,424.

## 2. Tag 1: offline region objects

**Observed.** `FUN_1469d6380` (offline id version 2): for each object of a region, with a running
counter `n` and the region cell `(rx, ry)`:
`id = ((((ry & 0x1f) | (n & 0x7ffff) << 5) << 5) | (rx & 0x1f)) << 3 | 1`.
`FUN_146167060` treats an id whose low 3 bits are 1 as a managed offline GDE (cvar
`javelin.offline-id-version` = 2). Not used by the work below.

## 3. Tag 3: capitals

**Observed.** `FUN_1469db810` and `FUN_1469dd810` (version 2), called from the region loader
`FUN_1469dd820`: `id = (hi ^ lo) & ~4 | 3`, where `hi` and `lo` are the two halves of the
capital's GUID.

**Observed.** With `hi` and `lo` read big-endian from the GUID as written in `*.capitals.json`
(`f6593b66-abc3-7460-...` gives `hi = 0xf6593b66abc37460`), the rule gives all 7,584 distinct
tag-3 ids of the captures from the world file's capitals (0 misses). Example: the MB watchtower
capital `f6593b66-abc3-7460-7a4d-5afaeb8b2620` is `0x8c14619c40485243`, card bytes
`435248409c61148c` (seen in 5 ledgers).

## 4. Tag 7: entities spawned by a slice's spawners

**Observed.** No client function builds a tag-7 id: the sweep has no `| 7`, `& ~4 | 3` or
`<< 3 | 7` form for them, and the client does not hash. The ids are the same in every session
(MB campfire `ff21e7138c5a804d` in 4 ledgers of different days, WW campfire `a761c93f095ab84d` in
2). They are a packed position, not a hash, and not derived from bits of the parent id:

| Bits | Width | Value |
|---|---:|---|
| 0-2 | 3 | 7 |
| 3-20 | 18 | `round(x * 8)` (world metres) |
| 21-38 | 18 | `round(y * 8)` |
| 39-50 | 12 | `round(z * 4)` |
| 51-54 | 4 | `floor(yaw / 23°)`, yaw in degrees in `[0, 360)` (rounded to 0.001° first) |
| 55-63 | 9 | first byte of the spawned slice's asset GUID, plus bit 0 of its second byte as bit 8 |

The spawned position and yaw: the capital's transform (position, rotation quaternion, scale)
times the spawner entry's `worldTM` (12 big-endian floats: 3 basis columns, then the
translation), recursively for spawners of spawned slices. The spawner entries come from
`region.slicedata` (one per region, in the paks): `sliceMetaDataMap` → pair (`value1`:
`SliceName`, `VariantName`; `value2`: `SliceMetaData`) → `spawners` → `SliceMetaDataSpawnerEntry`
(`worldTM`, `sliceAssetId`, `sliceName`, `variationName`, `prefabPersists`,
`maxRotationAngle`; reflection `FUN_14176ecc0`, `FUN_14176f9a0`). The pair is matched to a
capital by its `sliceName` and `variantName` (lowercase).

**Observed.** Over all 31,145 spawned entities of `newworld_vitaeeterna` (reader
`nw_world::slice_data`), the rule gives 1,387 of the 1,424 distinct tag-7 ids of the captures
exactly. Of the other 37:

- 6 match a spawned entity in position, height and asset, and differ by one yaw sector. Their
  yaw lies within 0.05° of a sector edge (for example 322.0° gives sector 14 here and 13 on
  retail, while 115.0°, 184.0° and 276.0° give the upper sector on both). The float order of the
  server's transform math is Unknown, so these stay Unknown.
- 31 have no spawned entity within 7 m. They lie in clusters of 3 to 9 ids within 1 m of each
  other (assets `0xcf`, `0x1c8`) near teleporters, loot chests and NPC swaps. Inferred: entities
  that the server spawns at run time (not from `region.slicedata`); Unknown.

**Observed.** 99 of the 31,145 spawned entities share an id with another (same slice, same
1/8 m cell, height step and sector).

**Observed.** A yaw sector of 23° (16 sectors cover 368°, so sector 15 is 345° to 360°) fits all
1,387; a 22.5° or 0.4 rad sector does not.

**Observed.** The campfire: MB capital `f6593b66-…` at (7306.0454, 3091.8823, 44.9871), 8°;
spawner 1 of its slice (`slices/pois/interactions/int_watchtowercampt1`, asset
`9b5ec8d1-ae80-5776-…`, `worldTM` translation (-2.1387, 0.3450, 0.2532)) → world
(7303.879, 3091.926, 45.240), yaw 0.1396 rad → `0x4d805a8c13e721ff`. WW capital
`163e4676-6b75-e4f2-0907-edb533f40928` → (9606.499, 2367.781, 44.941), yaw 2.9147 →
`0x4db85a093fc961a7`. Both equal the retail cards.

**Inferred.** The retail server makes these ids (the client only looks them up: card handler
`FUN_141718de0`, lookup `FUN_1417b5ed0`; "Failed to find GDE for state replication" when
absent). Two spawns of the same slice at the same 1/8 m cell, 1/4 m height and 23° sector
would get the same id.

**Unknown.** How the client gives entity ids to the entities inside a GDE's slice (the lock-on
question). This note covers only the GDE id.
**Later finding.** The lock-on did not depend on these entity ids. Commit `110ccc0` solved it with
the mob ALC `dataBits` (`awake_data_bits` in `crates/nw-messages/src/alc.rs`). How the client
gives entity ids inside a slice is still **Unknown**.

## 5. Dynamic entities

Sources: the same 721 ledgers (690 gave cards), the decompile sweep, the game's defaults function
`FUN_14616d7b0`. Scanner: `local-data/scratch-work/lock4/scan` (`idtags`, `respawns`).

### 5.1 The id form

**Observed.** A dynamic spawn card names an asset (mask `03`). Distinct ids by kind:

| Kind | Ids | Tag 0 | Tag 2 | Tag 4 | Tag 6 | Odd tags |
|---|---:|---:|---:|---:|---:|---:|
| Mob (asset in the mob prefab table) | 39,430 | 9,995 | 9,923 | 9,590 | 9,922 | 0 |
| Player (`player.dynamicslice`) | 403 | 90 | 96 | 111 | 106 | 0 |
| Other dynamic entity | 51,899 | 12,907 | 13,005 | 13,066 | 12,921 | 0 |
| Level card (no asset) | 47,980 | 0 | 0 | 0 | 0 | 47,980 (tags 1, 3, 7) |

**Observed.** Over the mob ids, every one of the 128 bits except bit 0 of byte 0 is set in 45 to
55 % of the ids; bit 0 of byte 0 is set in none. The same holds for the other dynamic ids. The
player ids show bits between 42 and 56 % (403 ids: within the noise). No mob or player id has
8 zero bytes in either half; all first halves and all second halves are distinct.

**Observed.** The same spawn has another id in each session: the tutorial's first sailor was
`aa10154d8edbbee5…` in one capture and `fcb2f3e2a7412885…` on 2026-09-18.

**Inferred.** The retail server gives each dynamic spawn 16 random bytes, per spawn and per
session, with bit 0 of byte 0 cleared. Nothing in the id is packed or derived from the prefab.

### 5.2 How the client reads the low bits

**Observed.** `FUN_146167060` (is a managed offline GDE) reads `javelin.offline-id-version`.
Version 1: offline when the high 32 bits of the little-endian first half are 0. Version 2: when
`javelin.enable-offline-capitals` (`FUN_14615f470`) is on, offline when bit 0 is set; else
offline when the tag is 1. The defaults (`FUN_14616d7b0`): `offline-id-version` 2,
`enable-offline-capitals` true, `use-static-ActorID-for-offline-objects` true. The retail remote
config layers set none of them.

**Inferred.** With the defaults, an odd first byte is a level object (the client looks it up and
does not spawn the prefab), and an even first byte is a dynamic spawn. Tags 0, 2, 4 and 6 are
the same to the client. The callers (`FUN_141718de0`, `FUN_14178ea40`, `FUN_14178fbc0`,
`FUN_14172fdf0`, `FUN_1417340d0`, `FUN_1416b8250`, `FUN_1440886e0`) only choose between the
offline cache or level lookup and the prefab spawn.

**Observed.** `FUN_14615ee90` makes the 16-byte actor id of a GDE: a copy of the GDE id for an
offline id, else 16 random bytes. `FUN_146161f90` makes random 64 bits with the low 3 bits set
to 7; `FUN_1469ef840` (`javelin.enable-register-dynamic-gde`) uses it. So the client does build
tag-7 ids for dynamic GDEs it registers itself; the level tag-7 ids of section 4 still come
from the server.

**Observed.** A search of the GDE code, the camera-lock code (`0x142c6…` to `0x142cb…`) and
the id code (`0x14615…` to `0x14616…`) found no other test of the low 3 bits.

### 5.3 Respawns

**Observed.** Per server stream, a mob's death is a vitals state with health 0 in the slot of
its card. Of 2,900 deaths, 654 were followed by a card of the same asset within 3 m of the dead
mob's first card, after the death, with another `GdeRef`; 273 of them at the identical position
(the spawn point). Gaps from the death: median 187 s, 90 % within 486 s.

**Observed.** A dead mob's id came back in 114 later cards, all with health 0 or no vitals state
(a corpse coming back into view, median 4 s after the death); never alive.

**Inferred.** A respawn is a new spawn: a new `GdeRef` and a new card. The old id ends with the
dead mob.

### 5.4 This server

- `crates/nw-messages/src/entity_ids.rs`: each player, mob and other dynamic entity gets 16
  random bytes with byte 0 bit 0 cleared and no zero half (`raw_id_of_entity`). A process-wide
  registry maps id, facet entity (the first half, little-endian) and entity number. Each sim
  world and each test-client view holds the entries it uses (`IdHolder`); an entry goes with its
  last holder, so the table holds only entities that are in a sim or a view now.
- A sim holds its players from their spawn to their departure (a leave, or a handoff that the
  taker accepted), and its mobs while it shows them; a mob that leaves (removed, or gone with its
  cell) releases its id.
- A handoff (`HandoffOffer::gde_ref`) and the ghost batches (`Ghosts::gde_refs`) carry the
  owner's ids; the taker or the receiving sim adopts them. A ghost's id is released when no sim
  sends the ghost any more.
- A respawned mob gets a new id (`renew_raw_id`); every client that had its card stops the slot
  and gets a new card with the new id and a new animation timeline. A ghost whose owner sends a
  new id is carded again the same way.
- Asset entities and spell casts keep their name-based UUIDs with bit 0 cleared
  (`as_dynamic_raw_id`). Retail's 26 tutorial asset entities all have an even first byte.

**Unknown.** Whether retail's new card after a respawn comes in the old slot or a new one; this
server may give the stopped slot again once its stop has gone out.

## 6. Crafting stations

**Observed.** A station is a spawned slice whose `.dynamicslice` has the crafting station
component (class `520015fd-2674-49e1-b862-e7371ae60043`, `m_craftingStationReference`
`m_craftingStationEntry`, `Camp1` for the campfire). Its group's members are the card and one
state per replicated component of the slice, at the component's `m_replicationIndex`:
buildable controller (`359f8468-…`, `m_initialState`, `m_structureName` = the
`BlueprintItemDefinitions` row whose `StructureCost_*` give the committed resources), ownership
(`2a333c23-…`, `m_initialPermissionRule`), placement obstruction (`fecc8785-…`), territory
interface (`e1d78b60-…`), buildable grid (`f25b009b-…`, `m_gridHeightsData` sides) and delayed
event (`d353f7e8-…`). The 295 stations of `newworld_vitaeeterna` are in the world file
(`nw_world::stations`).

**Observed.** Three town stations of `MSQ13-lv56` (blacksmith, carpentry, outfitting) send only
the card, the buildable controller (state 5, an empty resource list) and the placement
obstruction (0); their slices have no ownership, territory, grid or delayed event component.
The campfires send the obstruction as 1; their slice has a detector whose `m_strFilter` is
`CompletionObstructionCast`, the town slices none.

**Observed.** The MB campfire group has an interact member (child 1 `HasInteractors` 0); the WW
group and the town groups have none. This server sends none at spawn.

**Observed.** The list version of the resource and grid lists differs per server (`a507` = 485
MB, `ac10` = 1068 WW, `22`, `1c`, `6c` on the town stations): the retail server's counter. This
server sends `FIRST_VERSION` (3), the version of its other first states.
