# 03 - Engine and Client Game Data

Research date: 2026-08-29. All facts come from public sources. Each claim has a link.
Labels:

- **Verified (source)**: a public page or public source code shows the fact.
- **Verified (code)**: we read the public source code of a community tool. The tool shows the fact.
- **Inferred**: a logical result of verified facts. It is not a direct statement in a source.
- **Unknown** or **unconfirmed**: we found no reliable public source.

---

## 1. Summary

1. New World uses an Amazon in-house engine named "AZoth" / "Azoth". Wikipedia lists the engine as "Amazon Lumberyard" and "Azoth". Lumberyard is a fork of CryEngine. Amazon licensed CryEngine from Crytek in 2015. Amazon job posts describe AZoth as a proprietary engine for New World. The exact Lumberyard version that Azoth started from is **Unknown**.
2. The Lumberyard engine code (AzCore, AzFramework, GridMate, CryEngine parts) is public in the archived `aws/lumberyard` repository. It is **not** open source. The AWS terms limit its use. O3DE is the open-source successor (Apache-2.0 or MIT, your choice). The O3DE AzCore `ObjectStream` binary format and the "AZCS" compressed stream header are the same formats that community tools parse in New World files.
3. The client data is in about 125 `.pak` files (about 1.18 million files, in one sample from nw-buddy docs). A `.pak` file is a standard ZIP archive. Entries use the ZIP methods 0 (store), 8 (deflate/zlib), or a custom method `0x0F` (15), which is Oodle. Community tools open the paks with a standard ZIP reader. No tool that we read decrypts the paks.
4. Main data formats are documented in community code: `.datasheet` (binary tables), AZ ObjectStream (`.slice`, `.dynamicslice`, `.slicedata`, timelines, `*db` files, often wrapped in AZCS zlib), `.loc.xml` localization, `.luac` Lua bytecode (with a 2-byte prefix), JSON world/level files (`tracts.json`, `terrain.json`, `mapsettings.json`, `*.capitals.json`), `region.heightmap` (16-bit TIFF), `region.distribution`, `region.chunks`, and `.actionlist` files.
5. A large amount of game logic **data** ships in the client: loot tables, loot buckets, vitals (mob stats), damage tables, spell tables, abilities, status effects, perks, crafting and refining recipes, objectives/quests, game modes, and NPC data. Nw-buddy lists about 2,250 converted data tables. The client slices also hold `...ServerFacet` component data (for example, AI vitals table IDs and levels). AI `.actionlist` files for mobs and bosses ship in the client too.
6. Server **code** (the hub simulation, AI runtime, server-side component logic, persistence, and AWS microservices) does not ship in any public form. We found no verified public leak of a New World server binary. Community type tables, which are partly built by scanning `NewWorld.exe` for type UUID strings, list about 360 `...ServerFacet` component type names. This shows the client binary knows these types (reflection data). It does **not** show that the client binary holds the server logic. That is **Unknown**.
7. Legal: O3DE code is safe to reuse under Apache-2.0 or MIT. Lumberyard code has AWS-specific terms (no "third-party compute, storage or database services" for forks; non-AWS "Alternate Web Service" limits). The Amazon Games Terms of Use (as published on 2026-08-29) prohibit reverse engineering of the Game Clients, data mining, "act[ing] as a host ... without our consent", and "unauthorized connection that emulates portions of the Games". A US DMCA 1201 exemption exists for games whose server support has stopped, but its scope for persistent MMOs is limited. Get legal advice before release.

---

## 2. Findings by sub-topic

### 2.1 Engine lineage

| Claim | Status | Source |
|---|---|---|
| New World's engine is listed as "Amazon Lumberyard" and "Azoth". | Verified (source) | https://en.wikipedia.org/wiki/New_World_(video_game) |
| Lumberyard is based on CryEngine, licensed from Crytek in 2015. Lumberyard launched on 2016-02-09. Last version: Beta 1.28 (2021-05-19). | Verified (source) | https://en.wikipedia.org/wiki/Amazon_Lumberyard |
| Amazon hired for an "AZoth Engine Team" (Irvine, Seattle). Job posts describe a proprietary engine for New World and future games. | Verified (source, job aggregators, posts expired) | https://gamejobs.co/Sr-Software-Development-Engineer-AZoth-Engine-Team-at-Amazon , https://gamejobs.co/Software-Development-Engineer-AZoth-Engine-Team-at-Amazon , https://www.showbizjobs.com/jobs/amazon-games-studios-sr-software-development-engineer-azoth-engine-team-in-irvine/jid-g3bpk5 (page returned HTTP 403 to our fetch; seen only in search results) |
| The exact Lumberyard version or branch that Azoth forked from. | **Unknown** | No public source found. |
| The `aws/lumberyard` repo is archived (2022-10-04). It has branches `1.10` to `1.28` and `master`. README: "Amazon Lumberyard is no longer offered." It recommends O3DE. | Verified (source) | https://github.com/aws/lumberyard |
| The Lumberyard source tree has `dev/Code/Framework/{AzCore, AzFramework, GridMate, ...}`, `dev/Code/CryEngine/{CryNetwork, CryPhysics, CrySystem, Cry3DEngine, RenderDll, ...}`, and `dev/Code/LauncherUnified`. | Verified (source, GitHub API listing) | https://github.com/aws/lumberyard/tree/master/dev/Code |
| In July 2021, Amazon and the Linux Foundation announced O3DE. It is "partially based on Lumberyard" with many parts rewritten. | Verified (source) | https://en.wikipedia.org/wiki/Amazon_Lumberyard |
| O3DE license: "The default license for Open 3D Engine is the Apache License, Version 2.0 ...; you may elect at your option to use ... under the MIT License". Third-party parts (for example Qt, LGPL-3) have their own licenses. | Verified (source) | https://github.com/o3de/o3de/blob/development/LICENSE.txt |
| O3DE removed GridMate on 2022-04-20 (PR #9026): "Unused and unmaintained networking solution. Replaced by AzNetworking and MultiplayerGem". | Verified (source) | https://github.com/o3de/o3de/blob/development/RETIRED_CODE.md |
| The AWS architecture blog for New World links to O3DE but does not name Lumberyard or Azoth. | Verified (source) | https://aws.amazon.com/blogs/gametech/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world |
| Which network layer New World uses (GridMate, CryNetwork, a custom layer, or something else). | **Unknown** | No public source found. |

#### 2.1.1 Public O3DE parts that are useful to study

All are in https://github.com/o3de/o3de (branch `development`).

- **AZ::Component / AZ::Entity, SerializeContext, EditContext**: `Code/Framework/AzCore/AzCore/Component/` and `Code/Framework/AzCore/AzCore/Serialization/`. Folder listing: https://github.com/o3de/o3de/tree/development/Code/Framework/AzCore/AzCore/Serialization
- **ObjectStream (binary, XML and JSON streams)**: https://github.com/o3de/o3de/blob/development/Code/Framework/AzCore/AzCore/Serialization/ObjectStream.cpp
  - `s_objectStreamVersion = 3`, `s_binaryStreamTag = 0`, `s_xmlStreamTag = '<'`, `s_jsonStreamTag = '{'`.
  - Binary element flags: `ST_BINARYFLAG_MASK = 0xF8`, `ST_BINARY_VALUE_SIZE_MASK = 0x7`, `ELEMENT_HEADER = 1<<3`, `HAS_VALUE = 1<<4`, `EXTRA_SIZE_FIELD = 1<<5`, `HAS_NAME = 1<<6`, `HAS_VERSION = 1<<7`, `ELEMENT_END = 0`.
  - The new-world-tools Go parser uses the same flag values (see 2.3.3). So New World object streams use the AzCore ObjectStream binary format. **Verified (code)** on both sides.
- **AZCS compressed stream header**: https://github.com/o3de/o3de/blob/development/Code/Framework/AzCore/AzCore/IO/Compressor.h . The struct `CompressorHeader` has `char m_azcs[4]` ("AZCS AmaZon Compressed Stream") and `AZ::u32 m_compressorId`.
- **AzNetworking** (low-level UDP/TCP, packets, auto-packets from XML, serializers, DTLS for UDP): https://docs.o3de.org/docs/user-guide/networking/aznetworking/ and the code at `Code/Framework/AzNetworking`.
- **Multiplayer Gem** (server-authoritative, net roles Client / Autonomous / Authority / Server, delta replication with ACKs, rollback, auto-components from XML): https://docs.o3de.org/docs/user-guide/networking/multiplayer/overview/ . Child pages: `/autocomponents/`, `/code_separation/`, `/hierarchy/`, `/spawning/`.
- **Dedicated server launcher**: `Code/LauncherUnified/Server.cpp` and `launcher_server_files.cmake` in https://github.com/o3de/o3de/tree/development/Code/LauncherUnified . Lumberyard has `dev/Code/LauncherUnified` with the same role.
- **Prefabs (O3DE) vs slices (Lumberyard)**: O3DE has a `Gems/Prefab` gem. New World data uses Lumberyard-style `.slice` and `.dynamicslice` files (see 2.2). The O3DE prefab docs URL we tried (`https://docs.o3de.org/docs/user-guide/prefabs/`) returned HTTP 404. Lumberyard slice docs on docs.aws.amazon.com also returned 404. Older Lumberyard slice code is in the `aws/lumberyard` archive (`AzCore/Slice`).
- **Script Canvas, Script Events, Lua**: `Gems/ScriptCanvas`, `Gems/ScriptEvents` in O3DE. Whether New World uses Script Canvas is **Unknown**. Lua is used (see 2.2.6).
- **Recast navigation**: `Gems/RecastNavigation` in O3DE. Whether New World uses Recast is **Unknown**. The client type tables list `NavMeshConstraintComponent`, `NavigationObstacleMeshComponent`, `NavigationSettings`, and `AffectNavmesh` (source: https://github.com/giniedp/nw-buddy/tree/main/tools/nwbt/rtti/nwt).

### 2.2 Client file layout and formats

#### 2.2.1 Pak archives

| Claim | Status | Source |
|---|---|---|
| Paks are found by walking the game `assets` folder for `*.pak`. | Verified (code) | https://github.com/new-world-tools/new-world-tools/blob/master/pak/pak.go |
| Paks are opened with Go's standard `archive/zip`. So they are standard ZIP containers. | Verified (code) | https://github.com/new-world-tools/new-world-tools/blob/master/pak/parser.go |
| Supported entry methods: `zip.Store` (0), `zip.Deflate` (8, with a zlib-header check), and `0x0f` (15) = Oodle, decompressed by `oodle.Decompress(data, UncompressedSize64)`. | Verified (code) | https://github.com/new-world-tools/new-world-tools/blob/master/pak/parser.go , https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/formats/pak/entry.go |
| A 2021 QuickBMS script for New World maps ZIP method 15 to Oodle. | Verified (source) | https://gist.github.com/jessy-lua/d080b7bf499d72d2911f9bb15d9f2fc5 |
| Pak encryption. | No tool we read decrypts paks. The standard Go ZIP reader works. **Inferred**: current paks are not encrypted. | Same code links as above. |
| Sample size: "125 paks 1178744 files". | Verified (source, nw-buddy docs log output; date of sample unknown) | https://github.com/giniedp/nw-buddy/blob/main/docs/level-server.md |
| nw-buddy skips `shadercachestartup.pak` and `shadersbin_d3d11.pak` when it loads the archive. | Verified (code) | https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/nwfs/pak_archive.go |
| The new-world-tools README shows an example input path `...\New World\assets\server\server.pak`. | Verified (source). The contents of that pak and whether it still ships are **Unknown**. | https://github.com/new-world-tools/new-world-tools/blob/master/README.md |

#### 2.2.2 Oodle

| Claim | Status | Source |
|---|---|---|
| nw-buddy loads `oo2core_9_win64.dll` or `oo2core_8_win64.dll` and calls `OodleLZ_Decompress`. Its note: "not available for standalone download, obtain from an existing game installation". | Verified (code) | https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/utils/oodle.go |
| Kattoor's reader ships `oo2core_8_win64.dll` in its repo. | Verified (source) | https://github.com/Kattoor/nw-datasheet-reader |
| nw-extract does not ship the DLL. The user must supply it. | Verified (source) | https://github.com/giniedp/nw-extract |
| new-world-tools uses `github.com/zelenin/go-oodle-lz` (Go, MIT). Code comment: "works on linux, bugged on windows" for the streaming reader. | Verified (code) | https://github.com/new-world-tools/new-world-tools/blob/master/pak/parser.go , https://github.com/zelenin/go-oodle-lz |
| Open-source Oodle decoder `ooz` (Kraken / Mermaid / Selkie / Leviathan / LZNA / Bitknit decompressor), C++, GPL-3.0 in source headers, last push 2019. | Verified (source) | https://github.com/powzix/ooz |
| Which Oodle codec New World uses (Kraken, Leviathan, other). | **Unknown**. nw-buddy text says "Kraken/Leviathan" in general terms only. | https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/utils/oodle.go |

#### 2.2.3 Datasheets (`.datasheet`)

- Location: `sharedassets/springboardentitites/datatables/**.datasheet` (note the spelling "entitites" in the real path). **Verified (code)**: https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/commands/pull/cmd.go , https://github.com/new-world-tools/new-world-tools/blob/master/README.md
- Layout from new-world-tools (little-endian): **Verified (code)**: https://github.com/new-world-tools/new-world-tools/blob/master/datasheet/parser.go
  - Header: 4-byte signature, then `UniqueIdCrc32`, `UniqueIdOffset`, `TypeCrc32`, `TypeOffset`, ..., `BodyLength`, ..., `BodyOffset` (all `uint32`).
  - Meta: `Field2`, `Field3`, `ColumnCount (u32)`, `RowCount (u32)`, 4 more 4-byte fields.
  - Column index: per column 12 bytes = `Crc32 (u32)`, `Offset (u32)` of the name, `ColumnType (u32)`.
  - Rows: per cell 8 bytes = `Offset (u32)` + 4 bytes of value data.
  - Strings: null-terminated, at `headerSize + BodyOffset + offset`.
- Column types from the ZenHAX thread: 1 = string, 2 = float, 3 = int or bool (thread does not agree). Status: **unconfirmed** (community forum): https://zenhax.com/viewtopic.php@t=15506.html
- Name hashes: new-world-tools names the field `Crc32`. AZCS compressor IDs in the same project are `crc32(lower("ZLib"))`. So CRC32 of the lowercase name is likely. **Inferred** for datasheet columns.
- The datasheet-converter can resolve localization keys (`-localization`) and output CSV, JSON or YAML. **Verified (source)**: https://github.com/new-world-tools/new-world-tools/blob/master/README.md
- nw-buddy generates TypeScript types for about 226 interfaces and lists about 2,250 table JSON files (count from `libs/nw-data/generated/datatables.ts`). **Verified (code)**: https://github.com/giniedp/nw-buddy/tree/main/libs/nw-data/generated

#### 2.2.4 Object streams, slices, AZCS

| Claim | Status | Source |
|---|---|---|
| AZCS header: `"AZCS"` (4 bytes), `compressorId` (u32 big-endian), `uncompressedSize` (u64 big-endian). `0x73887d3a` = crc32("zlib") is supported. `0x72fd505e` = crc32("zstd") is "not implemented" in the tool. Zlib form: `numSeekPoints` (u32 BE), zlib data, then 16-byte seek points at the end. | Verified (code) | https://github.com/new-world-tools/new-world-tools/blob/master/reader/azcs/reader.go |
| Object stream: first byte `0x00` (binary tag), u32 BE version. Elements: flags byte, optional name CRC (u32 BE), optional version (u8), 16-byte type UUID; for stream version 2 a second 16-byte "specialized type" UUID; then size and value. | Verified (code) | https://github.com/new-world-tools/new-world-tools/blob/master/azcs/parser.go |
| `object-stream-converter` "Converts object streams (slices, timelines, various .*db) to json". | Verified (source) | https://github.com/new-world-tools/new-world-tools/blob/master/README.md |
| Type UUID → name tables are needed to decode streams. new-world-tools ships about 2,660 entries in `azcs/typeData.go`. nw-buddy builds tables from: UUID/name strings in `Bin64/NewWorld.exe`, Lumberyard `.xml`/`.slice`/`.uicanvas` files, level files, and the new-world-tools table. | Verified (code) | https://github.com/new-world-tools/new-world-tools/blob/master/azcs/typeData.go , https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/commands/types/scan_executable.go , https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/commands/types/scan_lumberyard.go , https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/commands/types/cmd.go |
| nw-buddy handles `.slice` by looking up the matching `.dynamicslice`. | Verified (code) | https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/game/slices.go |
| ImHex pattern files exist for `azcs`, `catalog`, `cgf`, `distribution`, `rnr`, `terrain`. | Verified (source) | https://github.com/giniedp/nw-buddy/tree/main/docs/imhex |
| Asset catalog: `assetcatalog.catalog` maps asset IDs (GUID + subId) to paths. `asset-catalog-parser` exports it to CSV. | Verified (source/code) | https://github.com/new-world-tools/new-world-tools/blob/master/README.md , https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/formats/catalog/assetid.go |
| `.meta` files. | We found no public documentation of a New World `.meta` format. **Unknown**. The nw-buddy level docs name a `[LAYER].metadata` file as "either a build artifact or actual metadata". | https://github.com/giniedp/nw-buddy/blob/main/docs/level-format.md |

#### 2.2.5 World, terrain, regions

Source for this whole table: https://github.com/giniedp/nw-buddy/blob/main/docs/level-format.md (Verified, community notes) and the parsers in https://github.com/giniedp/nw-buddy/tree/main/tools/nwbt/formats .

| Path | Content (per nw-buddy) |
|---|---|
| `levels/[LVL]/levelinfo.xml`, `mission_mission0.xml`, `mission0.entities_xml`, `resourcelist.txt` | CryEngine-style level files. Notes say "mostly abandoned for new world, except for main menu". |
| `sharedassets/coatlicue/[LVL]/tracts.json` | `tractmapCellSize`, `heightmapCellSize`, `regionSize` (2048), `territoryMasterSlicePath`, world size in regions, tracts, regions. |
| `sharedassets/coatlicue/[LVL]/terrain.json` | Ocean level, mountain height, world material path. |
| `.../playable.json`, `world.json`, `offlineoptions.json` | Playable regions, world settings, impostor cell size (128). |
| `.../regions/r_+XX_+YY/mapsettings.json` | `cellResolution`, `regionSize`, `regionType`. |
| `.../regions/r_.../region.heightmap` | 16-bit single-channel TIFF. Region is 2048 m. |
| `.../regions/r_.../region.tractmap.tif`, `localmappings.json` | Color-coded tract map and its color table. |
| `.../regions/r_.../region.distribution` | Gatherables and mass object placement (slices, variants, indices, positions). |
| `.../regions/r_.../region.chunks`, `capitals/**/[NAME].capitals.json` | Placed dynamic slices (id, world position, rotation, scale, footprint, slice path, slice asset ID, variant). |
| `.../regions/r_.../region.slicedata` | Slice lookup data (object stream). Example key: `coatgen\...\pointspawner_risen_eat_3512824316.dynamicslice` (source: https://github.com/giniedp/nw-buddy/blob/main/docs/slice-notes/slice-notes-ai-variant.txt). |
| `.../regions/r_.../region.waterqt`, `region.vegetation` | Water quadtree; vegetation (not parsed by nw-buddy yet). |
| `.../regions/r_.../impostors/*.cgf` | Low-detail CryEngine geometry (16x16 cells of 128 m). |

- Open world level name example: `newworld_vitaeeterna`. Dungeon example: `nw_dungeon_edengrove_00`. **Verified (code)**: https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/game/level/dir_coatlicue.go , https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/formats/capitals/format.go
- **Navmesh data in the client**: **Unknown**. Navigation types exist in the type tables, but we found no public note of a navmesh file in the paks.

#### 2.2.6 Localization, Lua, other files

| Claim | Status | Source |
|---|---|---|
| Localization: `*.loc.xml`, `<resources><string key="...">value</string></resources>`. Folder example `localization/en-us`. | Verified (code/source) | https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/formats/loc/format.go , https://github.com/new-world-tools/new-world-tools/blob/master/README.md |
| Lua bytecode `.luac` ships in the paks. Files start with `04 00 1B 4C 75 61` (2-byte prefix + standard `\x1BLua`). Tools strip 2 bytes, then decompile. | Verified (code) | https://github.com/new-world-tools/new-world-tools/blob/master/cmd/pak-extracter/main.go , https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/commands/unpack/process_lua.go |
| nw-buddy uses cLuaDecompiler (Free Pascal, AGPL-3.0, Lua 5.1-5.5). | Verified (source) | https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/utils/unluac.go , https://github.com/Coldzer0/LuaDecompiler |
| The Lua version used by New World. | **Unknown** | - |
| `.actionlist` files under `sharedassets/springboardentitites/`. Names include `actionlists/characters/ancientguardian/ancientguardian_combat.actionlist` and `actionlists/playerdata/cage/playeractions_ability_dagger.actionlist` (about 750 lines of entries). | Verified (code) | https://github.com/giniedp/nw-buddy/blob/main/libs/nw-data/generated/actionlists.ts , https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/commands/pull/pull_actionlists.go |
| `sharedassets/genericassets/playerbaseattributes.pbadb` (player base attributes). | Verified (code) | https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/commands/pull/pull_constants.go |
| Mannequin animation DB (`.adb` XML), `.bspace`, `.cdf`, `.cgf`, `.chrparams`, `.mtl`, `.dds` textures. | Verified (code) | https://github.com/giniedp/nw-buddy/tree/main/tools/nwbt/formats |

### 2.3 Community tools

GitHub metadata was read from the GitHub REST API on 2026-08-29.

| Tool | URL | Language | License | Last push | Status / role |
|---|---|---|---|---|---|
| nw-buddy | https://github.com/giniedp/nw-buddy | TypeScript + Go (`tools/nwbt`) | MIT | 2026 | Active. Web/Electron app plus `nwbt` CLI: pak access, Oodle, datasheets, object streams, RTTI type generation, levels, heightmaps, models to glTF, Lua decompile, local file server. Most complete public toolset. |
| new-world-tools | https://github.com/new-world-tools/new-world-tools | Go | MIT | 2025-11-05 | `pak-extracter`, `datasheet-converter` (csv/json/yaml), `object-stream-converter`, `asset-catalog-parser`. |
| go-oodle | https://github.com/new-world-tools/go-oodle | Go | MIT | 2025-02-01 | Go wrapper for Oodle. |
| datasheets-json / -csv / -yaml / localization | https://github.com/new-world-tools/datasheets-json , https://github.com/new-world-tools/datasheets-csv , https://github.com/new-world-tools/datasheets-yaml , https://github.com/new-world-tools/localization | data | none detected | json: 2023-09-23; csv, yaml, localization: 2026-03-26 | Pre-converted dumps of Amazon data. Note the copyright point in 2.6. |
| nw-extract | https://github.com/giniedp/nw-extract | TypeScript | MIT | 2023-03-18 | Archived (read-only). Extract + convert (`json:**/*.datasheet`, `json:**/*.loc.xml`, `png:**/*.dds`). |
| nw-datasheet-reader | https://github.com/Kattoor/nw-datasheet-reader | JavaScript | none detected | 2021-09-12 | Old. Ships Oodle DLL. |
| nw-datasheet-analyzer | https://github.com/mdreier/nw-datasheet-analyzer | TypeScript | none detected by API (npm page says Apache-2.0) | 2021-11-21 | Old. Analyzes converted sheets. https://www.jsdelivr.com/package/npm/nw-datasheet-analyzer |
| nw-datasheets-csv | https://github.com/meissnereric/nw-datasheets-csv | data | none | 2021-12-12 | Old dump. |
| NewWorldDatasheetDecoder | https://github.com/Json-x-ly/NewWorldDatasheetDecoder | Python | GPL-3.0 | 2021-08-23 | Old. |
| go-oodle-lz | https://github.com/zelenin/go-oodle-lz | Go | MIT | 2025-02-26 | Dependency of new-world-tools. |
| ooz | https://github.com/powzix/ooz | C++ | GPL-3.0 (file headers) | 2019-02-13 | Open-source Oodle decompressor. Useful on Linux without the Windows DLL. Codec coverage for New World is **unconfirmed**. |
| cLuaDecompiler | https://github.com/Coldzer0/LuaDecompiler | Free Pascal | AGPL-3.0 | 2026-05-18 | Lua bytecode decompiler used by nw-buddy. |
| QuickBMS script | https://gist.github.com/jessy-lua/d080b7bf499d72d2911f9bb15d9f2fc5 | BMS | - | 2021-10-15 | ZIP + Oodle (method 15). |
| ZenHAX datasheet thread | https://zenhax.com/viewtopic.php@t=15506.html | forum | - | 2021 | Early format notes; C, Rust, TS, Kaitai sketches. |
| nwdb.info | https://www.patreon.com/nwdb/about | web | proprietary | - | Database site. Its extraction method is not public (**Unknown**). |

### 2.4 Client-side data vs server-only logic

#### 2.4.1 What ships in client data (verified)

Source for table names: https://github.com/giniedp/nw-buddy/blob/main/libs/nw-data/generated/datatables.ts and https://github.com/giniedp/nw-buddy/blob/main/libs/nw-data/generated/types.ts . Each item is **Verified (code)** as present in the client data that nw-buddy extracts.

- **Items**: `javelindata_itemdefinitions_master_*` (common, loot, quest, crafting, AI, artifacts, ...), weapons, armor, consumables, ammo, perks, affixes, perk buckets.
- **Loot**: `javelindata_loottables*.json` (main, omega, salvage, seasons, fishing, playtest), `javelindata_lootbuckets*.json`, `javelindata_lootlimits.json`, `javelindata_loottagpresets.json`, `javelindata_divertedloot.json`. Fields include `'AND/OR'`, `Conditions`, `ChanceToExceedIndex`, `GearScoreRange1..N`.
- **Mobs**: `VitalsData`, `VitalsBaseData`, `VitalsLevelData`, `VitalsCategoryData`, `VitalsModifierData`, many `javelindata_damagetable_*` files, `javelindata_aitargeting.json`, `javelindata_aileveldisparity.json`, `charactertables/*`.
- **Combat**: `javelindata_spelltable_*` (per weapon and for AI), abilities, status effects, damage types, attack types, mana costs, cooldowns, diminishing returns.
- **Crafting and economy**: `javelindata_crafting_*` (fields like `BonusItemChance`, `BaseGearScore`, `CraftingFee`, `CooldownSeconds`), refining recipes, generator recipes, crafting categories, currency exchange rates, trade skill XP.
- **Quests and content**: `javelindata_objectives.json`, `javelindata_objectivetasks.json`, NPCs, conversation state/topics, game events, game modes, game mode maps, mutators, arenas, invasion, faction control, territory data, housing, achievements.
- **Placement**: slices and region files place spawners, gatherables, POIs, NPCs (see 2.2.5).
- **Server facet configuration in slices**: a client `.dynamicslice` holds `FacetedComponent` data with `m_serverfacetptr` of type `AIVariantProviderComponentServerFacet` with `m_vitalstableid`, `m_vitalstablerowid`, `m_vitalscategorytableid`, `m_vitalslevel`. Source: https://github.com/giniedp/nw-buddy/blob/main/docs/slice-notes/slice-notes-ai-variant.txt
- nw-buddy reads many component types from slices, including `AreaSpawnerComponentServerFacet`, `SpawnerComponentServerFacet`, `EncounterComponent`, `EncounterManagerComponent`, `POIManagerComponentServerFacet`, `ProjectileComponentServerFacet`, `StatusEffectsComponentServerFacet`, `LuckConstraintComponentServerFacet`, `GatherableControllerComponent`, `NpcComponent`, `TerritoryDataProviderComponent`. Source: https://github.com/giniedp/nw-buddy/tree/main/tools/nwbt/game/scanner
- **AI action lists**: per-creature combat, reaction, navigation, and death `.actionlist` files. See 2.2.6.
- **Lua bytecode**: present. What the scripts do (UI only or also game logic) is **Unknown**.

#### 2.4.2 What does not ship or is unknown

| Item | Status | Notes / source |
|---|---|---|
| Server binary / hub process | No public release found | Architecture: REPs (EC2 C5.2xlarge), hubs of seven C5.9xlarge per group, grid cells, 30 Hz state updates. https://aws.amazon.com/blogs/gametech/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world |
| Backend microservices (queues, contracts, sessions, guilds, characters, achievements, channels, naming) | Not in client | Lambda + API Gateway + DynamoDB per the AWS blog above. Must be rebuilt. |
| Persistence schema (DynamoDB) | Not in client | Same source. |
| Server-side component code (damage resolution, loot rolls, AI decision runtime, quest state machine, trading post) | **Unknown** if in client binary | Type names exist in RTTI tables (about 360 `*ServerFacet` names in https://github.com/new-world-tools/new-world-tools/blob/master/azcs/typeData.go). Names alone do not prove that code is present. |
| Behavior tree assets | **Unknown** | Type names `BehaviorTreeComponent`, `BehaviorTreeComponentServerFacet`, `BehaviorTreeAssetRef`, `BlackboardComponentServerFacet` exist in type tables (https://github.com/giniedp/nw-buddy/tree/main/tools/nwbt/rtti/nwt). No public note on the asset files. |
| Navmesh data | **Unknown** | See 2.2.5. |
| Network protocol | **Unknown** | No public documentation found. |
| Authority model | Amazon said New World is not client authoritative (reported in 2021). | Report: https://www.gamestar.de/artikel/ist-new-world-client-authorative,3374817.html ; exploit context: https://techraptor.net/gaming/news/game-breaking-new-world-invincibility-exploit-discovered |

### 2.5 Leaks, server code paths, emulators

- We found **no verified public leak** of a New World server binary or server source. Searches of the web and GitHub (2026-08-29) found no such project.
- A Steam forum post (as quoted by another user) claims a small team made a "single player" version of about 98 GB on GitHub at version 0.1. No name and no link were given. Another user disputes the claim. Status: **unconfirmed**. https://steamcommunity.com/app/1063730/discussions/0/682992180338558530/
- GitHub repository searches on 2026-08-29 for "new world emulator", "newworld server emulator", "new world aeternum server", "new world private server" returned no relevant project. Some later searches hit rate limits, so coverage is not complete.
- Two recent repositories use the name "Azoth engine" but are not related to Amazon as far as their descriptions show: https://github.com/WizWorksInc/azoth-rhi , https://github.com/themixednuts/az-rs . Relation to New World: **Unknown** (probably none).
- Client server-code paths: see 2.4.2. The client knows server-facet **types** (Verified, code). Whether a listen-server or server code path exists in `NewWorld.exe` is **Unknown**.
- The client uses Easy Anti-Cheat. Source: https://windowscentral.com/how-fix-new-world-unable-launch-due-eac-connection-error . How EAC and the Amazon/Steam login will act against a custom server is **Unknown**. Bypassing anti-cheat may have legal effects (see 2.6).
- Shutdown: service ends 2027-01-31. Delisted 2026-01-15. No offline mode is planned. Sources: https://www.videogameschronicle.com/news/amazon-games-confirms-mmo-new-world-has-a-year-left-before-it-goes-offline-permanently , https://fullcleared.com/news/new-world-aeternum-meets-its-end-on-january-31-2027/ , https://en.wikipedia.org/wiki/New_World_(video_game)

### 2.6 Licensing and legal notes (not legal advice)

| Topic | Fact | Source |
|---|---|---|
| O3DE | Apache-2.0 by default, or MIT at your option. Keep notices. Third-party parts have separate licenses (Qt is LGPL-3). | https://github.com/o3de/o3de/blob/development/LICENSE.txt |
| Lumberyard | "Your use of Lumberyard is governed by the AWS Customer Agreement"; "Your fork may not enable the use of third-party compute, storage or database services." Service Terms section 57. | https://github.com/aws/lumberyard , https://aws.amazon.com/serviceterms/#57._Amazon_Lumberyard_Engine |
| Lumberyard (2016 reporting) | Without consent, Lumberyard Materials may run only on your own hardware or AWS, not on an "Alternate Web Service". Current wording is **unconfirmed**. | https://www.theregister.com/2016/02/10/amazon_launches_lumberyard_beta_free_cloud_gaming_engine/ , https://www.thesixthaxis.com/2016/02/11/amazons-lumberyard-game-engine-has-a-very-specific-clause/ |
| Lumberyard (Wikipedia) | Users "may not publicly release the Lumberyard engine source code or use it to release their own game engine." | https://en.wikipedia.org/wiki/Amazon_Lumberyard |
| Amazon Games Terms of Use (page says "Last Updated: September 21, 2026"; a search snippet showed September 23, 2025, so the date is **unconfirmed**) | §2.5(i) do not "reverse engineer, decompile or disassemble, or otherwise tamper with, the Game Clients and associated Game Content"; §2.5(ii) do not "act as a host or matchmaking service for the Games without our consent" or "facilitate any unauthorized connection that emulates portions of the Games"; §2.5(iii) no derivative works; §2.5(iv) no "scraping, mining, or reading data from the Games"; §2.4 license is "limited, non-exclusive, non-transferable, revocable", personal and non-commercial. | https://digprjsurvey.amazon.com/csad/help/node/G201482650 (hosted on an amazon.com subdomain; the official amazongamestudios.com URL we tried returned 404) |
| US DMCA 1201 | An exemption exists for video games whose outside server support has stopped, for individual play and for preservation by libraries, archives and museums. The 2015 request excluded persistent-world MMOs. A 2027-cycle renewal petition exists. Exact current text of 37 C.F.R. § 201.40: check https://www.copyright.gov/1201/ . | https://www.eff.org/deeplinks/2015/04/videogame-publishers-no-preserving-abandoned-games-even-museums-and-archives , https://www.copyright.gov/1201/2027/petitions/renewal/Renewal-Pet-Abandoned-Video-Games-Dan-Elkins.pdf |
| Oodle | Proprietary RAD/Epic library. nw-buddy says it is not available for standalone download. Do not redistribute the DLL. `ooz` is GPL-3.0; GPL obligations apply if linked into a distributed program. | https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/utils/oodle.go , https://github.com/powzix/ooz |
| Data dumps | Public repos that redistribute converted Amazon data (datasheets-json etc.) exist. Their legal status is **Unknown**. Amazon game data is Amazon's copyright. | Repos in 2.3 |

---

## 3. Implications for the emulator

### 3.1 What we can reuse from O3DE (Apache-2.0 / MIT)

1. **ObjectStream reader**: port or reuse the O3DE `ObjectStream` binary parser logic and the AZCS `CompressorHeader` logic. The flags and tags match the community parsers for New World files. Use O3DE as the license-clean reference. Do not copy Lumberyard code.
2. **AZ type system ideas**: UUID-based type IDs, CRC32 name hashes, SerializeContext versioning. Use them to build our own type registry from the community UUID/name tables.
3. **Networking reference**: AzNetworking (UDP, reliability, packet fragmenting, DTLS) and the Multiplayer Gem (net roles, delta replication, rollback) are good design references for a 30 Hz server-authoritative loop. They do **not** match the New World wire protocol (that protocol is **Unknown**). Treat them as design material only.
4. **Server launcher pattern**: `Code/LauncherUnified/Server.cpp` shows a headless server build. Useful only if we choose to build on O3DE. A plain C++/Rust/Go server is also possible, because we need data formats, not the O3DE runtime.
5. **Math / shapes / spline / terrain gems**: useful as a reference for collision shapes (box, capsule, cylinder, polygon prism, compound) that appear in slice data.

### 3.2 What we can extract from client data

1. **Asset access**: ZIP reader + Oodle (method 15) + deflate. On Linux use `ooz` (GPL-3.0) or load the game DLL in a Windows tool step. Build a one-time extraction step that converts paks to an internal store.
2. **All datasheets** to typed records (items, loot, vitals, damage, spells, abilities, status effects, perks, recipes, objectives, NPCs, game modes, territories, housing, achievements).
3. **World placement**: regions, heightmaps (for server-side ground height and spawn placement), tract maps, capitals/chunks/slicedata (all spawners, POIs, gatherables, NPCs, encounters), distribution (gatherable nodes).
4. **Server-facet configuration** inside slices: spawner settings, AI variants, vitals levels, luck constraints, POI managers, encounter data.
5. **AI action lists** for mobs and bosses, plus player ability action lists.
6. **Lua scripts** (after decompile) for any logic they contain (scope **Unknown**).
7. **Localization** for names and text (the server sends keys; the client resolves text).
8. **Collision geometry**: CGF meshes and impostors are available; whether server-side collision meshes exist separately is **Unknown**.

### 3.3 What must be rebuilt by hand

1. **Network protocol** (login, world entry, entity replication, RPCs, chat). **Unknown** in public sources. It needs packet capture from the live game before 2027-01-31 and client analysis. Capture now while servers run. See the ToS notes in 2.6.
2. **Authentication and character services** (Amazon/Steam login, EAC interaction, character list, world select). The AWS blog lists these as Lambda/API Gateway services.
3. **Persistence** (characters, inventory, storage, houses, companies, territories, trading post). Replace DynamoDB with a local database (for example PostgreSQL or SQLite).
4. **Simulation logic**: damage formula, gear score, perk and status effect execution, loot roll algorithm (the tables exist; the roll code does not), crafting result rules, AI runtime (behavior tree / action list interpreter), pathfinding (navmesh source **Unknown**; may need generation from geometry, for example with Recast), quest state machine, wars/invasions, faction control, economy.
5. **Hub and grid partitioning**: not needed at small scale. One process can host one world for a small player count.
6. **Validation data**: numeric behavior (damage numbers, drop rates) must be checked against live captures, community wikis, and nw-buddy calculators before shutdown.

### 3.4 Recommended first steps

1. Pin a copy of the final client build (all paks, `Bin64/NewWorld.exe`, Oodle DLL) for local, personal use.
2. Run nw-buddy `nwbt` and new-world-tools against it. Archive the outputs (datasheets JSON, object stream JSON, type tables) with the game build number.
3. Write our own clean-room readers (pak, Oodle via ooz or DLL, datasheet, AZCS, ObjectStream) with tests based on the documented layouts.
4. Start live packet capture and documentation now (2026-08 to 2027-01-31). Coordinate with the network research document.

---

## 4. Open questions

1. Which Lumberyard version did Azoth fork from? Does the networking layer derive from GridMate or CryNetwork, or is it custom?
2. Does `NewWorld.exe` hold server-side logic for any `*ServerFacet` component, or only reflection stubs?
3. Does `assets/server/server.pak` still ship? What is in it?
4. What do the `.luac` scripts do (UI only, or game rules)? Which Lua version?
5. What is the binary format of `.actionlist`? Is it an ObjectStream? How does the AI runtime execute it?
6. Are behavior tree assets and navmeshes in the paks? Under which paths?
7. Which Oodle codecs do the paks use? Does `ooz` decode every entry?
8. What is the exact datasheet column type enum (int vs bool for type 3)? What are the unknown header fields?
9. What is the wire protocol (transport, encryption, compression, message IDs)?
10. How does the client authenticate (Amazon account, Steam ticket, EAC)? Can it connect to a custom endpoint without binary patches?
11. What is the current, exact wording of the AWS Service Terms §57 and of 37 C.F.R. § 201.40 for this use case? Does any jurisdiction-specific law (EU Software Directive interoperability exceptions) apply? Legal advice is needed.

---

## 5. Source list

Engine and lineage
- https://en.wikipedia.org/wiki/New_World_(video_game)
- https://en.wikipedia.org/wiki/Amazon_Lumberyard
- https://gamejobs.co/Sr-Software-Development-Engineer-AZoth-Engine-Team-at-Amazon
- https://gamejobs.co/Software-Development-Engineer-AZoth-Engine-Team-at-Amazon
- https://www.showbizjobs.com/jobs/amazon-games-studios-sr-software-development-engineer-azoth-engine-team-in-irvine/jid-g3bpk5
- https://github.com/aws/lumberyard
- https://github.com/aws/lumberyard/tree/master/dev/Code
- https://aws.amazon.com/serviceterms/#57._Amazon_Lumberyard_Engine
- https://aws.amazon.com/blogs/gametech/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world

O3DE
- https://github.com/o3de/o3de
- https://github.com/o3de/o3de/blob/development/LICENSE.txt
- https://github.com/o3de/o3de/blob/development/RETIRED_CODE.md
- https://github.com/o3de/o3de/blob/development/Code/Framework/AzCore/AzCore/Serialization/ObjectStream.cpp
- https://github.com/o3de/o3de/blob/development/Code/Framework/AzCore/AzCore/IO/Compressor.h
- https://github.com/o3de/o3de/tree/development/Code/LauncherUnified
- https://docs.o3de.org/docs/user-guide/networking/multiplayer/
- https://docs.o3de.org/docs/user-guide/networking/multiplayer/overview/
- https://docs.o3de.org/docs/user-guide/networking/aznetworking/

Community tools and format notes
- https://github.com/giniedp/nw-buddy
- https://github.com/giniedp/nw-buddy/blob/main/docs/level-format.md
- https://github.com/giniedp/nw-buddy/blob/main/docs/level-server.md
- https://github.com/giniedp/nw-buddy/tree/main/docs/imhex
- https://github.com/giniedp/nw-buddy/blob/main/docs/slice-notes/slice-notes-ai-variant.txt
- https://github.com/giniedp/nw-buddy/tree/main/tools/nwbt
- https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/utils/oodle.go
- https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/utils/unluac.go
- https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/formats/pak/entry.go
- https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/nwfs/pak_archive.go
- https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/commands/types/scan_executable.go
- https://github.com/giniedp/nw-buddy/blob/main/tools/nwbt/commands/types/scan_lumberyard.go
- https://github.com/giniedp/nw-buddy/tree/main/tools/nwbt/rtti/nwt
- https://github.com/giniedp/nw-buddy/tree/main/libs/nw-data/generated
- https://github.com/new-world-tools/new-world-tools
- https://github.com/new-world-tools/new-world-tools/blob/master/README.md
- https://github.com/new-world-tools/new-world-tools/blob/master/pak/parser.go
- https://github.com/new-world-tools/new-world-tools/blob/master/reader/azcs/reader.go
- https://github.com/new-world-tools/new-world-tools/blob/master/azcs/parser.go
- https://github.com/new-world-tools/new-world-tools/blob/master/azcs/typeData.go
- https://github.com/new-world-tools/new-world-tools/blob/master/datasheet/parser.go
- https://github.com/new-world-tools/new-world-tools/blob/master/cmd/pak-extracter/main.go
- https://github.com/new-world-tools/go-oodle
- https://github.com/new-world-tools/datasheets-json
- https://github.com/new-world-tools/datasheets-csv
- https://github.com/new-world-tools/datasheets-yaml
- https://github.com/new-world-tools/localization
- https://github.com/giniedp/nw-extract
- https://github.com/Kattoor/nw-datasheet-reader
- https://github.com/mdreier/nw-datasheet-analyzer
- https://www.jsdelivr.com/package/npm/nw-datasheet-analyzer
- https://github.com/meissnereric/nw-datasheets-csv
- https://github.com/Json-x-ly/NewWorldDatasheetDecoder
- https://github.com/zelenin/go-oodle-lz
- https://github.com/powzix/ooz
- https://github.com/Coldzer0/LuaDecompiler
- https://gist.github.com/jessy-lua/d080b7bf499d72d2911f9bb15d9f2fc5
- https://zenhax.com/viewtopic.php@t=15506.html
- https://www.patreon.com/nwdb/about

Server, exploits, anti-cheat, shutdown
- https://steamcommunity.com/app/1063730/discussions/0/682992180338558530/
- https://www.gamestar.de/artikel/ist-new-world-client-authorative,3374817.html
- https://techraptor.net/gaming/news/game-breaking-new-world-invincibility-exploit-discovered
- https://windowscentral.com/how-fix-new-world-unable-launch-due-eac-connection-error
- https://www.videogameschronicle.com/news/amazon-games-confirms-mmo-new-world-has-a-year-left-before-it-goes-offline-permanently
- https://fullcleared.com/news/new-world-aeternum-meets-its-end-on-january-31-2027/
- https://github.com/WizWorksInc/azoth-rhi
- https://github.com/themixednuts/az-rs

Legal
- https://digprjsurvey.amazon.com/csad/help/node/G201482650
- https://www.theregister.com/2016/02/10/amazon_launches_lumberyard_beta_free_cloud_gaming_engine/
- https://www.thesixthaxis.com/2016/02/11/amazons-lumberyard-game-engine-has-a-very-specific-clause/
- https://www.eff.org/deeplinks/2015/04/videogame-publishers-no-preserving-abandoned-games-even-museums-and-archives
- https://www.copyright.gov/1201/2027/petitions/renewal/Renewal-Pet-Abandoned-Video-Games-Dan-Elkins.pdf
- https://www.copyright.gov/1201/
