# World and object stream formats (M1b preparation)

Date: 2026-08-31
Build: Steam build `22469132` (same install as `2026-08-29-client-install-findings.md`)
Method: static, offline, read-only reading of the user's own install. No game file was changed. The game was not run. The executable was not read. Throwaway tools were kept in the session scratch folder only (a Rust program that uses `nw-formats` and `nw-oodle` by path, plus Python scripts). Every decoded pak entry passed the size and ZIP CRC32 checks of `PakArchive::read`.

Status labels:

- **Observed**: measured on the install with the counts given.
- **Inferred**: a conclusion from observed data, or from community code, that we did not prove.
- **Unknown**: not determined.

All "pak" paths are relative to the install root. All byte offsets are offsets in the decoded entry.

## 1. Summary

- All 650,236 ObjectStreams that we parsed are binary version 3. Versions 1 and 2 do not occur. All parse to the end with no trailing bytes. (Observed)
- 81,063 files use the AZCS wrapper. All use zlib (`0x73887d3a`). zstd does not occur. Every AZCS file has exactly one seek point. (Observed)
- `.dynamicslice`, `region.chunks`, `region.metadata`, `region.slicedata`, `region.waterqt`, capital `.chunks` and `.metadata`, and `.slice.meta` are raw ObjectStreams (no AZCS). `.timeline` files are AZCS + zlib + ObjectStream. `.actionlist` files are plain XML. (Observed)
- `typeindex.json` does not describe ObjectStream types: 1 of 2,174 stream type UUIDs is in it. (Observed)
- The server can get spawn points, NPC points, gatherable points and ground height without a full slice decoder. `region.metadata` (and the per-capital `.metadata` in dungeons) lists vitals spawns, NPCs and gatherables with world positions and CRC32 IDs that match datasheet IDs. `region.heightmap` gives terrain height as `value / 32` metres. (Observed)

## 2. Inventory (question 1)

### 2.1 Counts and sizes per type (Observed)

Source: ZIP central directories of all 130 paks (1,446,650 entries). Sizes are bytes.

| Type | Files | Stored size | Decoded size | ZIP methods | Container |
|---|---:|---:|---:|---|---|
| `.dynamicslice` | 248,826 | 772,464,264 | 8,350,593,187 | 15: 248,826 | ObjectStream v3 |
| `.slice` (stand-alone) | 0 | - | - | - | - |
| `.slice.meta` | 317,545 | - | 277.4 MB | - | ObjectStream v3 |
| `.slicedata` (only `region.slicedata`) | 49 | 5,058,097 | 88,316,859 | 15: 49 | ObjectStream v3 |
| `.timeline` | 80,367 | 95,611,590 | 95,748,958 | 0: 75,630; 15: 4,737 | AZCS zlib + ObjectStream v3 |
| `*.capitals.json` | 1,741 | 26,645,679 | 218,928,946 | 15: 1,369; 0: 372 | JSON |
| capital `*.chunks` | 1,513 | - | - | - | ObjectStream v3 |
| capital `*.metadata` | 878 | - | - | - | ObjectStream v3 |
| `region.heightmap` | 100 | 236,518,702 | 236,518,702 | 0: 100 | TIFF |
| `region.tractmap.tif` | 100 | 4,411,236 | 4,411,236 | 0: 100 | TIFF |
| `region.distribution` | 100 | 24,441,920 | 41,904,517 | 15: 89; 0: 11 | custom binary |
| `region.chunks` | 49 | 2,722,624 | 71,734,471 | 15: 30; 0: 19 | ObjectStream v3 |
| `region.metadata` | 49 | 1,753,564 | 91,765,656 | 15: 25; 0: 24 | ObjectStream v3 |
| `region.waterqt` | 100 | 1,623,187 | 221,097,466 | 15: 65; 0: 35 | ObjectStream v3 |
| `region.vegetation` | 98 | 4,359,774,975 | 4,359,774,975 | 0: 98 | not examined |
| `region.dds*` (mips) | 26 | small | small | 15 | DDS |
| `tracts.json` | 41 | 41,473 | 332,574 | 15: 41 | JSON |
| `terrain.json` | 41 | 11,720 | 14,830 | 15: 41 | JSON |
| `mapsettings.json` | 100 | 7,732 | 7,732 | 0: 100 | JSON |
| `playable.json` / `world.json` / `offlineoptions.json` | 40 / 5 / 41 | small | small | mixed | JSON |
| `.actionlist` | 748 | 991,016 | 12,033,877 | 15: 740; 0: 8 | XML |

Where the files are (Observed):

- `region.heightmap` and `region.tractmap.tif`: all in `assets/CoatlicueStrm.pak`.
- `region.vegetation`: `assets/VegetationStrm*.pak` (5 paks).
- Other region and capital files: `assets/SharedDataStrm-part4/5/6/7.pak`.
- `.dynamicslice`: 17 paks. 233,628 files (6.9 GB decoded) are under `coatgen/<8-hex>/`. Hand-made slices are under `slices/` (largest folders: `pois` 2,814, `characters` 2,700, `worldevents` 2,391, `dungeon` 1,630, `housing` 1,311, `structures` 936).
- `.timeline`: 77,941 under `timelines/vo/` (voice), the rest under `libs/timeline`, `timelines/musicalperformances`, `timelines/igc`.
- `.slice.meta`: one per hand-made slice (example: `slices/gatherables/master_bush.slice.meta` next to `master_bush.dynamicslice` in `assets/SharedDataStrm-part8.pak`).

### 2.2 Levels and regions (Observed)

41 levels have a folder `sharedassets/coatlicue/<level>/` with `tracts.json`. Region folders are `regions/r_+XX_+YY/`.

| Level | Regions (heightmap) | `region.chunks` / `.metadata` / `.slicedata` | `*.capitals.json` |
|---|---:|---:|---:|
| `newworld_vitaeeterna` (open world) | 49 (7 x 7) | 49 each | 1,334 |
| `frontend` | 9 | 0 | 9 |
| `nw_frontend_base` | 4 | 0 | 0 |
| 38 other levels (dungeons, raids, arenas, OPR, CTF, battle royale, trials, quest instances, FTUE, `templateworld`) | 1 each | 0 | 1 to 99 each |

- Open world: `world.json` lists 49 region cells (0..6 x 0..6). `playable.json` lists 26 of them. `tracts.json` has `regionSize` 2048, `world` = Island 7 x 7, 204 tracts, 24 regions. (Observed)
- Dungeons (13 `nw_dungeon_*`) and other instanced levels have one region `r_+00_+00` and use only capitals (`.capitals.json`, `.chunks`, `.metadata`). The region-level aggregate files exist only in the open world. (Observed)
- There are also 29 legacy level paks `assets/levels/<level>/level.pak` (CryEngine files: `levelinfo.xml`, `mission0.entities_xml`, `terrain/terrain.dat`, `*.bai`). The `mnmnavmission0.bai` files are 72 bytes, so they hold no navigation mesh. No other navigation mesh file was found. (Observed for the sizes; "no navmesh in the client" is Inferred.)

## 3. AZCS wrapper (question 2)

Header (Observed on 81,063 files; all integers big-endian):

| Offset | Size | Field | Observed values |
|---|---:|---|---|
| 0 | 4 | magic `AZCS` | always |
| 4 | 4 | compressor ID | `0x73887d3a` (crc32 of "zlib") in 81,063 of 81,063 |
| 8 | 8 | decoded size | equals the zlib output size in every file |
| 16 | 4 | number of seek points | 1 in every file |
| 20 | n | zlib stream (starts `78 da`) | - |
| end - 16 x count | 16 each | seek points | see below |

- Seek point (Observed on samples): two big-endian u64 values. Example: `00000000 00000016 00000000 00000000`. The first value (22) is the file offset of the deflate data after the 2-byte zlib header (20 + 2). The second value is 0 (decoded offset). The field order "compressed offset, then decoded offset" is Inferred from the values and from new-world-tools.
- zstd (`0x72fd505e`): does not occur. (Observed)
- AZCS files by type (Observed): `.timeline` 80,367; `.aliasasset` 478; `.hairstyledb` 61; `.skintonedb` 61; `.curveconfiglist` 54; `.stimulusdurations` 8; `.paperdolldb` 5; `.virtualaudiodb` 3; `.charcreatedb` 2; 24 other `*db`-style types with 1 file each (for example `.pbadb`, `.uidb`, `.collisionfilters`, `.refreshzoneconfigs`). All decode to ObjectStream v3.
- Coverage: we scanned all `.dynamicslice`, `.timeline`, `.actionlist`, region and capital files, and 373,909 other entries (all types except textures, meshes, animation, audio, video, fonts, shaders, `.luac`, `.datasheet`, `.cfxb`, `.dat`, `.grid`, `.vegimage`, `region.vegetation`). AZCS in the excluded types is Unknown.

## 4. ObjectStream binary layout (question 3)

### 4.1 Layout (Observed on 650,236 streams, 363,644,735 elements)

Stream header: byte 0 = `0x00` (binary tag), bytes 1..4 = version (u32 big-endian). Version is 3 in every stream. Then the root element follows. A `0x00` byte ends each child list and the stream.

Element:

1. Flags (u8). `0x00` = end of the current child list.
2. Name CRC (u32 BE) if flag `0x40`.
3. Version (u8) if flag `0x80`.
4. Type UUID (16 bytes, raw order as in the text form). For version 2 streams a second 16-byte UUID follows (from new-world-tools); version 2 does not occur.
5. Value size if flag `0x10`: if flag `0x20` is set, the low 3 bits give the width of a size field (1, 2 or 4 bytes, BE) that follows; else the low 3 bits are the size (0..7).
6. Value bytes.
7. Child elements, then `0x00`.

Flag `0x08` (element header) is set on every element.

Observed flag bytes (all files): `0x08` (timeline root, no version), `0x88` (other roots), `0x48`, `0xc8`, `0x58`..`0x5f`, `0x79`, `0x7a`, `0xd8`, `0xf9`. Size-field widths: 1 byte (about 59.5 million times in dynamic slices), 2 bytes (17 times), 4 bytes (never). Maximum nesting depth: 27 in `.dynamicslice`, 15 in world and timeline files.

Value encoding (Observed): numbers are big-endian. `float` is IEEE-754 BE (example `0x46034000` = 8400.0). `Vector3` is 12 bytes (3 BE floats). `Transform` is 48 bytes (12 BE floats: 9 rotation/scale values, then translation x, y, z; the translation position is Observed, the 3 x 3 order is Inferred). `AZStd::string` is raw bytes without a terminator. `AZ::Uuid` is 16 raw bytes. `Asset` values are 56 bytes plus a path: asset GUID (16), 16 bytes that were zero in samples (sub ID and padding, Inferred), asset type UUID (16), u64 BE path length, path text.

### 4.2 Annotated example (Observed)

Entry `sharedassets/coatlicue/newworld_vitaeeterna/regions/r_+04_+04/region.chunks` in `assets/SharedDataStrm-part6.pak` (2,518,474 bytes decoded):

| Offset | Bytes | Meaning |
|---|---|---|
| 0x00 | `00` | binary tag |
| 0x01 | `00000003` | version 3 |
| 0x05 | `88` | root: header + version |
| 0x06 | `01` | class version 1 |
| 0x07 | `ac608be6...389d91` | root type UUID (name Unknown) |
| 0x17 | `48` | header + name |
| 0x18 | `c74dc4b5` | name CRC = crc32("chunks") |
| 0x2c | `c8` | header + name + version |
| 0x2d | `41405e39` | name CRC = crc32("element") |
| 0x31 | `02` | class version 2 |
| 0xcc | `0000000000000020` | value of `size` (u64) = 32 |
| 0x136 | `46034000 4600c000 441a47cd` | value of `worldPosition` = (8400.0, 8240.0, 617.12) |

### 4.3 Type UUIDs and names (Observed)

- Distinct type UUIDs in all parsed streams: 2,174 (1,825 in `.dynamicslice`; 96 in world and timeline files; 384 in the other AZCS and meta files). 
- In `typeindex.json` (7,052 UUIDs): 1 of 2,174. Byte-swapped GUID order and full reversal give 0 matches. So `typeindex.json` is not an ObjectStream type table. This supports the earlier inference that it is the network `type_idx` table. (Observed for the mismatch; the network role is still Inferred.)
- Root types: every `.dynamicslice` root is `AZ::Entity` (`75651658-8663-478d-9090-2432dfcafa44`) that holds a `SliceComponent`. Every `.slice.meta` root is type `7d314916-0502-4d0c-b457-ae485e62a156` (per-slice streaming metadata).
- Type names inside the install: XML ObjectStreams (`<Class name="..." field="..." type="{...}">`) in `mission0.entities_xml` (48 files), `materials/terrain/*` (85 files), `config/game.xml`, `libs/config`, `default.physicsmaterialset` give 298 named UUIDs. 251 of them occur in the binary streams (for example `AZ::Entity`, `SliceComponent`, `GameTransformComponent`, `AoiComponent`, `PrefabSpawnerComponent`, `Vector3`, `AssetId`). The other 1,923 UUIDs have no name in the data files. (Observed)
- Name CRCs: the name CRC is crc32 of the lower-case field name (verified: "element" = `0x41405e39`, "chunks" = `0xc74dc4b5`). 3,980 distinct name CRCs occur. Identifier strings from install XML, JSON and Lua bytecode match 949 of them, which cover 85.5 % of all name occurrences. Collisions are possible with this method, so treat each such name as a candidate. (Observed)
- We did not read the executable. nw-buddy builds full tables from the executable; that path is out of scope here.

## 5. World files (question 4)

Coordinate frame (Observed): world x and y in metres, z up. Region `r_+XX_+YY` covers x = XX x 2048 .. +2048 and y = YY x 2048 .. +2048. XX is the x index: the last column of `r_+04_+04` and the first column of `r_+05_+04` differ by a mean of 23 raw units, against thousands for the other edge pairs.

### 5.1 `region.heightmap`

- Format (Observed, 100 files): little-endian TIFF, one IFD, 16 bits per sample, 1 sample per pixel, LZW compression (5). Predictor 2 (horizontal) in 97 files. Size 2048 x 2048 in 98 files, 1024 x 1024 in 2 (FTUE levels). 59 files set Photometric = 2 (RGB) with 1 sample, which is not valid TIFF; Pillow rejects them unless the reader ignores Photometric. 29 files set SampleFormat = 1. Some files carry tool, date, user and host tags (305, 306, 315, 316); we do not copy their values.
- Orientation (Observed): image column = local x. Image row 0 = the north edge (high y): row = 2047 - local y.
- Scale (Observed for the open world): height in metres = raw / 32 (equal to raw x 2048 / 65536, and `terrain.json` `mountainHeight` is 2048). Test: for 267,846 capital positions in `newworld_vitaeeterna`, median (z - height) = 0.005 m; 50.5 % within 0.5 m; 66 % within 2 m. For 115,937 vitals spawn positions from `region.metadata`: median -0.08 m, 76 % within 1 m. Battle royale levels give the same result.
- Example: `r_+04_+04` raw minimum 13,735 and maximum 39,654, that is 429.2 m to 1,239.2 m. Open-world raw range is 11 to 41,170 (0.3 m to 1,286.6 m). `oceanLevel` is 40.0.
- Dungeon and instance levels (`mountainHeight` 256): heightmaps are flat (edengrove: all samples 8,191) and capitals do not sit on them. Ground in these levels comes from meshes, not terrain. (Observed)
- The exact sample position inside a 1 m cell (corner or centre) is Unknown.

### 5.2 `region.distribution` (Observed: all 100 files parse to the exact end)

All integers little-endian.

| Field | Type |
|---|---|
| optional magic | 4 bytes `d6 b9 01 00` (33 files: 29 open-world regions, both battle royale levels, `nw_opr_004_trench`, `nw_raid_monarchbluff_00`) |
| `count` | u16 |
| slice names | `count` x (u8 length + text) |
| variant names | `count` x (u8 length + text), same index as slice names |
| `n1` | u32 |
| index | `n1` x u16 (index into the name tables) |
| position | `n1` x (u16, u16): first = local y, second = local x; metres = value / 65535 x 2048 |
| unknown A | `n1` x 4 bytes (Unknown; maybe rotation) |
| unknown B | `n1` x u8, range 70..205, mode 100 (Inferred: scale in percent) |
| unknown C | only with magic: `n1` x u8, all 0 in all 33 files |
| `n2`, then `n2` x (u16, u16) + `n2` x u8 | block 2: 4,703 items in all files, type values 2 and 8 |
| `n3`, then `n3` x (u16, u16) + `n3` x u8 | block 3: 284,052 items, type value always 4 |

- Axis order evidence: with "first u16 = y", 98.1 % of open-world items are above `oceanLevel`; with "first u16 = x", 75.8 %. (Observed)
- Totals: 3,238,413 main items (2,728,908 in the open world). (Observed)
- Example `r_+04_+04`: magic present, `count` 1,069, `n1` 62,491, `n2` 0, `n3` 0. Item 0: raw (43263, 20063) = world (8818.98, 9543.99). Slice names are paths without `slices/` and extension (for example `gatherables/master_bush`); the file `slices/gatherables/master_bush.dynamicslice` exists. The commonest item is a bush gatherable with a variant name. Blocks 2 and 3 meaning: Unknown.
- nw-buddy's current parser does not read the extra byte of the magic variant. (Observed by comparison.)

### 5.3 `*.capitals.json` (Observed: 1,741 files, 291,657 entries)

Top level: `{"Capitals": [...]}`. Fields per entry (count of entries that have it):

| Field | Count | Form |
|---|---:|---|
| `id` | 291,657 | GUID text |
| `worldPosition` | 291,657 | `{x, y, z}`, absolute world metres |
| `rotation` | 291,657 | quaternion `{x, y, z, w}` |
| `footprint` | 291,657 | `{type, id, radius}` or `{type, id, width, height}`; types Circle 251,684, Rectangle 39,965, AxisAlignedSquare 8 |
| `sliceName` | 291,657 | slice path text, or `<PLOT>` |
| `sliceAssetId` | 291,657 | `{GUID}:subIdHex` |
| `params` | 61,889 | flags such as `HAS_VEG_COMPONENTS`, `TERRITORY_DATA_ID` |
| `scale` | 50,005 | one float (uniform) |
| `variantName` | 41,005 | text |
| `cookie` | 6,239 | terrain blend values |

- Example (Observed, `ftue_v2/regions/r_+00_+00/capitals/capitals/capitals.capitals.json`): a player spawner capital at (479.49, 849.70, 89.53), rotation z = -0.7071, w = 0.7071, circle radius 1.0.
- In the open world, 182,417 of 267,847 entries are `<PLOT>` with a zero asset ID. Their content is in the `.chunks` files (see 5.4). 36,471 open-world entries are spawner slices under `CoatGen\\<hash>\\pointspawner_*.dynamicslice` (path uses backslashes).
- Asset IDs resolve: all 89,062 non-zero `sliceAssetId` values resolve through `assetcatalog.catalog` (`assets/Engine.pak`). (Observed)

### 5.4 `region.chunks` and capital `.chunks` (Observed)

- `region.chunks` is the union of the capital `.chunks` files of the region (`r_+04_+04`: 6,524 = sum of 57 capital files). Open world total: 186,850 chunks.
- Structure: root (type `ac608be6...`) > `chunks` (container) > `element` (type `92ca42b0...`, version 2) with: `cellIndex` {`x`, `y`, `z`: u64}, `size` (u64: 32, 64, 128, 256, 512 or 1024), `spawnRadius` (float), `layer` (string, the capital name), `worldPosition` (Vector3), `chunkType` (int: 0, 1, 2), `assetId` {`guid`, `subId`}.
- Example (`r_+04_+04`, first element): cellIndex (6, 1, 19), size 32, spawnRadius 0.0, worldPosition (8400.0, 8240.0, 617.12), chunkType 0. The asset ID resolves to `coatgen/49630a4e/03gc03_mtnevil_ai_02_group_32x32_static_chunk_6_1_19.dynamicslice`. All 6,524 asset IDs of the region resolve.
- Content: the 6,524 chunk slices of `r_+04_+04` hold 99,254 entities. Components: `GameTransformComponent` on all, rigid bodies 41,108, meshes 17,639, particles, decals, lights, vegetation areas, scripts 1,264, trigger areas 947, `PrefabSpawnerComponent` 68. Chunk slices are mostly static props and effects. Entity transforms are relative to the chunk origin; the chunk `worldPosition` places them (Inferred from an identity root transform and small child offsets).

### 5.5 `region.metadata` and capital `.metadata` (Observed)

- `region.metadata` is the union of the capital `.metadata` files (`r_+04_+04`: identical section counts). Dungeons have only the capital `.metadata` files (for example `nw_dungeon_edengrove_00/.../dungeon_spawners/dungeon_spawners.metadata`).
- Root type `bda6adf9...` version 6, with 8 lists. Open-world totals: 

| Section (name CRC) | Name | Records | Fields in each record |
|---|---|---:|---|
| `1fe2c4e5` | `npcData` | 1,626 | `territoryIds` (u16 list), `npcId` (Crc32), `worldPosition`, `factionType` (u8), a string tag, 2 bools, a second Crc32 |
| `390be215` | Unknown (vitals spawns) | 115,937 | `territoryIds`, `vitalsId` (Crc32), `vitalsCategoryId` (Crc32), `vitalsLevel` (u32), `worldPosition`, 3 bools, `isEncounter` |
| `5f96b5ce` | Unknown (gatherables) | 148,069 | `territoryIds`, `gatherableId` (Crc32), a string, `worldPosition`, 3 bools, an int, `isEncounter` |
| `6e2cbf8b` | Unknown (landmarks) | 2,020 | `territoryIds`, `landmarkType` (u8), `landmarkData` (string), `worldPosition`, `Radius`, `actorId` |
| `cf91f6ef` | Unknown (encounters) | 920 | `territoryIds`, `encounterId` (string), a Crc32, `worldPosition` |
| `04172e19` | `loreData` | 1,235 | `territoryIds`, `loreId` (Crc32), `worldPosition` |
| `c496c848` | `igcData` | 73 | `igcLocation` (Vector3), `igcId` (Crc32), `Duration`, 2 bools |
| `7cc66b4d` | Unknown | 44 | a Crc32, `worldPosition`, `GDEID` {`ID` u64}, `spawnId` (Crc32) |

- Field names come from CRC matches against candidate words. "Unknown" names did not match any candidate.
- Crc32 IDs are crc32 of the lower-case datasheet ID. In `r_+04_+04`: `vitalsId` matches a datasheet string in 5,477 of 5,477 records, `npcId` 65 of 66, `gatherableId` 7,143 of 7,191, `loreId` 30 of 30. (Observed)
- Example (`r_+04_+04`, first vitals record): `vitalsId` at 0x5c52 = `ae65fdc7`, `vitalsLevel` at 0x5c9d = 46, `worldPosition` at 0x5cb8 = (8537.24, 8199.65, 584.83), `isEncounter` false.

### 5.6 `region.slicedata` and `.slice.meta` (Observed)

- `region.slicedata`: root `be64a116...` > `slicemetadatamap` > `element` pairs: `value1` {`sliceName`, `variantName`} and `value2` (type `7d314916...` version 3). Open world: 72,994 pairs.
- `value2` and every `.slice.meta` root share type `7d314916...`. Fields: `gdeSpawnRadius`, `gridRegistrationRadius`, `aoiDistance`, `slicePhysicalRadius` (floats), `gridCategory` (u16), `isStaticSlice`, `hasCollision`, `isRequiredOnServer`, `skipMidrangeImpostors` (bools), `sliceTags` (u32), a `Meshes` list of asset IDs, and other fields with unmatched names.
- Example (`r_+04_+04`, first pair): a log gatherable with a variant; gdeSpawnRadius 75.0, gridRegistrationRadius 8.05, aoiDistance 180.66, slicePhysicalRadius 2.49, gridCategory 2.
- Inferred: this is the streaming and interest-management data for each slice and variant (who must load it and at what distance). `isRequiredOnServer` is a direct server hint.

### 5.7 Other region files

- `region.waterqt` (Observed): root `23082a77...` with `regionSize` (int 2048) and a list of 9,613 records in `r_+04_+04`, each with `Height` (float), a second float and `Flags`. The value 65535.0 occurs where there is no water (Inferred).
- `mapsettings.json`: `cellResolution` 1, `regionSize` 2048, `regionType` 0. `terrain.json`: `generatorType`, `mountainHeight`, `oceanLevel`, `worldMaterialAssetPath` and 5 other keys. `offlineoptions.json`: `distributionDataEnabled`, `distributionBakeSeed`, `vegetationDataEnabled`, `streaming.impostorCellEdgeLength` 128, `streaming.regionChunksEnabled`. (Observed)
- `region.vegetation`, `region.tractmap.tif`, `localmappings.json`, `impostors.json`: not examined beyond headers. (Unknown)

### 5.8 Spawner slices (Observed)

Example `coatgen/5277d58e/pointspawner_0wp_basic_wander_1067044928.dynamicslice`: one entity with `GameTransformComponent`, `PositionInTheWorldComponent`, `AoiComponent`, a component whose server facet names the tables `javelindata_vitals` and `javelindata_vitalscategories`, and a `PrefabSpawnerComponent` with `m_aliasAsset` (path to an `.aliasasset`), `m_spawnOnInitialize`, `m_respawnAfterDeath`, `m_minRespawnCooldown` / `m_maxRespawnCooldown` (360.0 s each) and more. Many component and facet types in spawners have no name in the install (for example `8b86f1ed`, `b3cbd45c`, `d480029e`).

### 5.9 Asset catalog (Observed)

`assetcatalog.catalog` (`assets/Engine.pak`, 283,026,856 bytes, magic `RASC`, version 1) holds 1,595,260 assets in 40-byte records (little-endian) with UUID, type, directory and file-name tables, as described by nw-buddy `formats/catalog`. It maps every chunk and capital asset ID that we tested to a pak path. The directory text ends with `/`, so joined paths contain `//`; remove one slash before the pak lookup.

## 6. AI `.actionlist` files (question 5) (Observed)

- 748 files, 12,033,877 bytes decoded. Folders: `sharedassets/springboardentitites/characters` 648, `.../playerdata` 63, `.../fxscripts` 37.
- Container: plain XML text, root `<ActionList>`. Not ObjectStream, not AZCS. All 748 parse as XML.
- Root children: `Actions` + `Conditions` (538 files), `Reactions` + `Conditions` (89), `Actions` only (77), `Scripts` (37, effect scripts), `Conditions` only (7).
- Commonest elements: `Default`, `Not`, `And`, `Conditional`, `IfMannequinTag`, `Action`, `Activities`, `ActivationCondition`, `DeactivationCondition`, `IfFragmentDone`, `SetMannequinTag`, `Or`, `BehaviorTreeTask`, `IfBehaviorTreeTask`, `AnimationDrivenMotion`.
- Related: 316 `.grid` files in the same tree are also XML (root `<Grid>`).
- Behavior tree assets themselves: Unknown.

## 7. Recommended M1b scope and reading order (question 6)

### 7.1 What the server needs first

1. **Level and region frame**: `tracts.json`, `world.json`, `playable.json`, `terrain.json`, `mapsettings.json`. These are small JSON files. They give the region grid, region size and ocean level.
2. **Ground height**: `region.heightmap`. Write a small TIFF reader for exactly this profile (16-bit, one sample, LZW, predictor 1 or 2, strips, ignore Photometric). Height = raw / 32. Row = 2047 - local y. Use it for the open world only. For dungeons, keep the spawn z from the data and do not snap to terrain.
3. **ObjectStream v3 reader** in `nw-formats`: generic tree reader (flags, name CRC, version, UUID, value), big-endian value helpers, plus an AZCS zlib unwrapper. Version 2 support is not needed for this build. Use name CRCs (crc32 of the lower-case name) as keys; keep a small table of the names we use.
4. **Spawn and point lists**: per-capital `.metadata` (all levels) or `region.metadata` (open world). These give vitals spawns (vitals ID, category, level, position), NPCs, gatherables, lore, encounters and in-game cutscene points with world positions. Join the Crc32 IDs to datasheet IDs with crc32 of the lower-case ID.
5. **Placement records**: `*.capitals.json` (position, rotation, scale, slice path, asset ID, variant). Use them for spawner rotation and for the slice path when the server needs per-spawner options.
6. **Later**: `assetcatalog.catalog` reader, then spawner slice decoding (`PrefabSpawnerComponent` respawn timers, `.aliasasset` to creature slice), then `region.slicedata` / `.slice.meta` for interest-management radii, then `region.distribution` for mass gatherables (bushes, plants).

Items 1 to 4 are enough to place creatures, NPCs and gatherables on the ground in the open world. Item 5 adds rotations and per-spawner variants.

### 7.2 Risks

- About 88 % of stream type UUIDs and 76 % of name CRCs have no name inside the install. The server must work with UUIDs and CRCs as keys. Any field we use needs a CRC match or a test on known values.
- Several metadata section names and many fields are Unknown. The section meaning comes from field content (vitals ID, gatherable ID), not from a name.
- Heightmap TIFFs are slightly non-standard (Photometric = 2 with 1 sample). Generic TIFF libraries can reject them.
- Dungeon and instance ground is mesh geometry. The client has no navigation mesh (the `.bai` files are empty). Movement validation and AI pathing in dungeons need another source (mesh collision from `.cgf`, or a server-made navmesh). This is outside M1b.
- `region.metadata` vitals count (115,937) is about 3 times the number of spawner capitals (36,471). The relation between the two lists is Unknown. Test it before the server spawns from both, so that creatures do not spawn twice.
- Licence note: the format knowledge here is from our own reading plus MIT community code (new-world-tools, nw-buddy). No code was copied.

### 7.3 Open questions (Unknown)

- Root type names of `region.chunks`, `region.metadata`, `region.slicedata`, `region.waterqt`.
- Names of metadata sections `390be215`, `5f96b5ce`, `6e2cbf8b`, `cf91f6ef`, `7cc66b4d`.
- `region.distribution`: the 4-byte field, blocks 2 and 3, the extra byte of the magic variant.
- `chunkType` values 0, 1, 2 and the `cellIndex.z` meaning (Inferred: a quadtree level or node index).
- The sample position inside a heightmap cell, and the height scale of the two FTUE levels (`mountainHeight` 1024, where `raw / 32` does not match the capitals).
- The meaning of the one stream UUID that is also in `typeindex.json` (`fff40d4b-d06e-4985-91c3-ec13bd3ff665`).
