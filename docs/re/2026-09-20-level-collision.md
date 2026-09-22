# Level collision: what the client collides with, and the server's copy

Date: 2026-09-20
Build: Steam build `25810808` (`config/client-build.toml`).
Method: static reading of the user's own install (`<game-dir>/assets/*.pak`), the client
decompile (`<re-sweep>`), and replays of the retail capture ledgers. Format knowledge from
nw-buddy's ImHex patterns (MIT) was used as a hint only; no code was copied.

Status labels: **Observed** (measured, with counts), **Inferred** (concluded, not proved),
**Unknown**.

## 1. Summary

- The client's character collides with the **RnR physics assets** (`.rnr`, the client's own
  physics engine, `RnRSystemAllocator`), not with render meshes. Every `.cgf` that has a
  `MeshPhysicsData` chunk (`0x1018`) has a `.rnr` next to it (112 of 112 sampled meshes:
  74 with both, 38 with neither). (Observed)
- A level's solids are the entities with a `GameRigidBodyComponent` in the slices that the
  level's capitals and chunk lists place, whose collision filter blocks the `Player` filter.
  (Observed for the data; the runtime rule is Inferred and checked against retail places, 6.)
- The server now builds this collision into a file per level (`content-tool collision`) and
  stands players and mobs on it. The tutorial deck is at the retail own place: the retail
  deck places of two captures are within 0.061 m of the collision floor, 837 of 837 within
  0.1 m. (Observed)

## 2. Where the geometry is

### 2.1 Placement (Observed)

1. `sharedassets/coatlicue/<level>/regions/r_+XX_+YY/capitals/<layer>/<layer>.capitals.json`:
   `worldPosition`, `rotation` (quaternion), `scale`, `sliceAssetId` (`{GUID}:subIdHex`).
   `<PLOT>` entries are empty; their content is in the chunk lists.
2. Chunk lists: `region.chunks` (open world, the union of the capitals' `.chunks`), else each
   capital's `.chunks`: `assetId` {`guid`, `subId`} and `worldPosition`, no rotation.
3. The `.dynamicslice` of a placement (asset id resolved by `assetcatalog.catalog`): its
   entities' `GameTransformComponent` `m_worldTM` is the entity's place **in the slice's space**.
   In the build's slices `m_localTM` equals `m_worldTM` in every entity, and a child holds about
   its parent's values: the parent chain must **not** be applied again (the Thorpe arena floor
   was 2 m too high with it, at the retail own place without it). (Observed)
4. A stored `Transform` is 12 big-endian floats: the 3 x 3 part **by columns** (basis x, y, z),
   then the translation. Check: open-world retail own places over the terrain by 0.3 m or more
   that a collision floor matches within 0.1 m: 1,384 of 2,319 with columns, 212 with rows.
   (Observed)
5. A `PrefabSpawnerComponent` with `m_spawnOnInitialize` places its `m_sliceAsset` at its
   entity (the tutorial galleon's `ftue_v2_galleon_ship_base.dynamicslice`). (Observed)

### 2.2 The solid entities (Observed)

`GameRigidBodyComponent` > `m_configuration` (`GameRigidBodyConfig`):

| Field | Use |
|---|---|
| `m_strFilter` | The collision filter (`Structure`, `Static`, `Static_NotTraversable`, `PlayerOnlyCollision`, `Empty`, ...). |
| `m_rnrAsset` | A physics asset (type `d52cccdd-9701-45d9-b261-8ea6881a6312`). When null: |
| `m_collisionShape` | The body's own shape: `QueryShapeSphere` (`m_radius`), `QueryShapeBox` (`m_box`, half its size: the client builds one from bounds as `(max - min) * 0.5`, `FUN_143534110`, constant 0.5 at `0x147f41308`), `QueryShapeAabb` (`m_aabb`, half its size, the same box method), `QueryShapeCylinder` (`m_radius`, `m_height` up from the origin, `FUN_14169ea40`), `QueryShapeCapsule` (`m_radius`, `m_height`, `m_axis`; along x or y it lies on the origin, else it stands on it, `FUN_1416987a0`). When empty: |
| the entity's `MeshComponent` mesh | Its physics asset: the `.rnr` at the mesh's path under the mesh's GUID (`galleon_body.cgf` sub 0 and `galleon_body.rnr` sub 100; a slice's baked `handcart_group_a_settlement__28236438930.cgf` and its `.rnr` at other sub ids). All 10,513 meshes with a `.rnr` of their path have it under their own GUID; sub 100 holds only for sub-0 meshes (8,847), not for the 1,666 baked ones. |
| `m_center` | Offset of the own shape. |

Collision filters (`sharedassets/physics/default.collisionfilters`, AZCS ObjectStream): per
filter a name, a description, base filters (name CRC `a1efe138`), the categories it is in
(`70978a1a`) and the categories it collides with (`e6a3571c`). The field names are not in the
install; the meaning is Inferred from the content (`Player` has base `Character`; `Character`
collides with `Terrain`, `Structure`, `Character`, `CharacterOnly`, `Static`; `Structure` has
base `Static`; `Empty` is "Filter to use when disabling collision"). A body blocks the player
when the categories of its filter (with its bases) meet the categories the player collides with.
A pair test in both directions would make `Structure` (empty collides-with list) solid for no
one, yet the player stands on the `Structure` deck. (Observed)

The player (`slices/player.dynamicslice`): capsule shape component with filter `Player`,
`m_radius` 0.41, `m_maxSlope` 70; character controller `m_enableStepUp` true,
`m_stepUpMinDetectionHeight` 0.25, `m_stepUpMaxDetectionHeight` 1.3, `m_maxSlopeAngle` 75.
(Observed)

## 3. The `.rnr` format (Observed: all 10,621 files read to their last byte)

Little-endian. Loader `FUN_140cc66a0`, shape reader `FUN_1410f7ac0`.

| Part | Layout |
|---|---|
| Header | `u32` magic `0x1234abcd`, `u32` version (6: 8,474 files, 7: 2,147) |
| Material lists (version > 1) | `u32` count; each: `u32` length + `.mtl` path, `u32` n + n x `u16` (one id per triangle) |
| Block (version > 2) | 16 bytes the server does not use |
| Flags (version >= 5) | `u8` (with id list: `i16` n + n x `u16` when set); version >= 7: one more `u8` |
| Shape count (version >= 4) | `u8` (1 in every file) |
| Shape | `u32` kind, the kind's data, then `u32` length + bytes: a 48-byte pose (3 rows of rotation row + translation) or nothing |

Shape kinds (data):

| Kind | Name | Data | Files' shapes |
|---|---|---|---:|
| 1 | box | half extents (3 f32), margin | 6,846 |
| 2 | sphere | radius | 21 |
| 3 | convex hull | `u32` points (low 31 bits), `u32` planes, points, planes, margin, `u8` flag (+ two `u16` lists) | 175 (all with 0 planes) |
| 4, 6, 8, 0x11, 0x14 | (no file uses them) | 3 f32; 2 f32; 10 f32; 10 f32; height field | 0 |
| 5 | cylinder | from, to, radius, margin | 410 |
| 7 | capsule | from, to, radius | 1 |
| 10 | triangle mesh | `u32` (skipped), `u32` points, `u32` triangles, points, 3 x `u16` per triangle, `u8` flag (+ 3 x `u16` neighbours per triangle), `u32` n + search-tree block | 16,900 |
| 11 | compound | `u32` count; each: 12 f32 place + shape | 1,689 |
| 12 | transformed | 12 f32 place + shape | 0 |
| 0x12, 0x13 | scaled | `u32`, scale (3 f32), shape | 0 |

Names of kinds 1, 2, 5 and 7 from the engine primitives (`primitive_cube.rnr`: box half 1 posed
1 m up, the `.cgf` spans z 0 to 2 m; `primitive_capsule.rnr`, `primitive_cylinder.rnr`,
`primitive_sphere.rnr`); kind 3 and 11 from the loader's strings ("RnR ConvexHullShape copy!",
"Compound Shape children"). Every margin in the build is 0. Triangle winding: counter-clockwise
seen from outside (the galleon deck's top faces up, its underside down).

Reader: `crates/nw-formats/src/rnr.rs`; its local-data test is in `crates/nw-oodle`.

## 4. The collision file

`content-tool collision [levels]` (`just collision`) writes
`local-data/content/collision-<build>-<level>.bin`: the shared container header with magic
`NWCO`, schema 2, then a postcard `LevelCollision` (`crates/nw-world/src/collision.rs`):
shapes once (`Mesh`, `Box`, `Sphere`, `Capsule`, `Cylinder`, `Convex` with planes from the hull
points), the bodies of the level as it loads (shape index + 3 x 4 place), and the **deferred
slices**: the slices that a spawner places later (an encounter's spawn group, an event's;
spawners without `m_spawnOnInitialize`, whose `m_sliceAsset` is in their server part), each
with its slice id, its spawner's place and its bodies. (Observed)

### 4.1 Variant capitals (Observed)

A capital with a `variantName` (41,005 in the open world: `master_bush` 8,263,
`master_minerals` 6,834, `master_tree` 6,667, `master_alchemy` 6,663, ...) places its master
slice as the variant shows it:

- The master slice's `Javelin::VariationDataComponent` (`{4A2D9C0A-B147-4186-A9E0-5805B610D3CC}`,
  `FUN_1442dbfb0`) names a variation table (`Gatherable_Trees`; field CRC `095a1d04`) and a
  list (`aedddde9`) of bindings: an entity (`m_entity`), an action (`m_action`: `Change Mesh`,
  `Change Material`, ...) and a column (`a77a4396`: `StartMesh`). The field names of the three
  CRCs are not in the install; the meaning is from the content.
- The table is the `VariationData` datasheet with that id
  (`javelindata_variations_gatherables_trees.datasheet`, id `Gatherable_Trees`); the variant's
  row is the one whose `VariantID` is the capital's `variantName`. 51 open-world variant
  capitals name no row of their table; they are placed as their slice is.
- `Change Mesh` gives the entity the row's mesh path (`Objects/Nature/...cgf`, made lowercase
  with forward slashes); its physics asset is the `.rnr` at that path. `Set Collision Asset
  Override`, `Set Should Have Static Collision`, `Change Transform Scale Multiplier X/Y/Z` and
  `Change Transform Translation X/Y/Z` (the quest damageables) set the shape, hide the body
  unless the flag is `1`, scale its axes and set its place. (The flag and transform semantics
  are Inferred from the action names.)
- The master gatherables hold a `Healthy` and a `Depleted` entity under the root; the tree's
  stump, the ore's fragments and the plants' picked meshes are in `Depleted`. The depleted
  state is left out (Inferred: it shows once the node is gathered).
- The solid filters: a tree's and a bush's start mesh has filter `Tree`, an ore's, a stone's
  and a log's `Static`, an alchemy plant's `Static_NotTraversable`; `Tree` and `Static` are in
  category `Static`, which the player collides with. Bushes and plants whose mesh has no
  physics asset add no body.

### 4.2 Counts

`CollisionIndex` (`crates/nw-world/src/collision_index.rs`) builds at load: a 32 m grid of the
bodies' ground-plane bounds and a bounding-volume tree per mesh. Queries take the line into the
body's own space. `floor_at(x, y, z)`: the highest surface at most 1.3 m (step-up detection)
above the feet that faces up within 70 degrees (`m_maxSlope`). `raycast(from, to)`: the first
surface either side. `CollisionIndex::with_bodies` makes an index of other bodies on the same
shapes (the shown deferred slices, 5.1).

| Level | Placements | Slices | Solid entities | Bodies | Deferred slices (bodies) | Shapes | Placed triangles | File | Index in memory | Build |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `ftue_v2` | 262 | 172 | 2,446 | 2,383 | 2 (3) | 175 | 1,282,903 | 0.83 MB | 2.6 MB | 1.2 s |
| `newworld_vitaeeterna` | 365,307 (41,005 variant capitals) | 217,573 | 1,401,538 | 1,417,073 | 8,756 (39,593) | 15,864 | 299,758,128 | 102.7 MB | 213.6 MB | 7.6 s |

Build times with the paks in the page cache, 28 threads.

Solid entities that add no body (open world; Observed counts, reasons as stated):

| Count | What | Why no body |
|---:|---|---|
| 11,268 | the mesh has no physics geometry: no `MeshPhysicsData` chunk, no physics-data id in its mesh chunk, no `.rnr` (`crates/nw-formats/src/cgf.rs`) | the client's physics loads `.rnr` assets only (`FUN_140cc66a0`); these meshes have none (Inferred: no physics shape) |
| 8,941 | no shape named: no `m_rnrAsset`, no `m_collisionShape`, no mesh, and no mesh from a variant. 2,077 are in slices without a variant (their `m_shapeEntity` is invalid, `ffffffff`); the rest are variant entities whose default mesh is null and whose variant leaves the column empty (an ore's `AddonMesh`: 12 of the 164 mineral rows set it) | nothing to build a shape from (Inferred) |
| 866 | the mesh has physics geometry but no `.rnr` (Observed: all the bush meshes like `jav_fst_burkwoodbush_b.cgf`) | as above: no physics asset (Inferred) |
| 465 | the asset is not in the build's catalog (`m_rnrAsset` `179d231f...` sub 3 in 207 bodies; 201 meshes) | the client cannot load it either (Inferred) |
| 12 | `m_rnrAsset` names a sub id the catalog does not hold (`primitive_box` sub 28627, `primitive_cube` sub 25716, `jav_lm_rock_chaos_flat_b` sub 59896), while the GUID has its `.rnr` at sub 100 | **Unknown** whether the client falls back to the GUID's `.rnr`; left out |

Query speed (one thread, 1,000,000 random queries near bodies, phase-1 file): open world
`floor_at` 4.5 us, a 5.8 m `raycast` 10.9 us; tutorial `floor_at` 2.3 us, `raycast` 3.3 us.
(Observed)

## 5. The server

- `sim` loads the collision file of its level (it does not start without it) and stands
  players and mobs on `LevelGround` (`crates/nw-sim-core/src/level_ground.rs`): the higher of
  the terrain and the structure floor; `Ground::blocked` is the terrain's or a solid on a line,
  too steep to stand on.
- `crates/nw-sim-core/src/tracking.rs`: the height is the level's floor; the camera's height
  only picks which floor at the body's place (the highest one at most 1.3 m above the camera's
  body), and is the height only where the level has neither height data nor a structure. In a
  jump the body keeps its own height and the camera's pull-back moves it on the ground plane
  (before this, a pull-back in a jump was lost: over the Fury Bay ledge gap the body flew on
  0.58 m beside retail's place and fell into the gap).
- `crates/nw-sim-core/src/character.rs`: a body walks off edges and falls on any floor; no stop
  at walls (a stop at the level's solids a step above the feet erred by 0.73 m against 0.22 m:
  the client's capsule slides along and over them). The speed is along the slope: on the
  terrain by the floor's rise in the record; on a structure by the rise across the player's
  capsule (`PLAYER_RADIUS_M` 0.41 ahead and behind) and only uphill (6.1).
- The tutorial's asset entities (5.1).
- `bins/sim/src/world/walked_floor.rs`: the walked floors are no longer a ground. Pushes and
  pulls go only over walked floor and stop at a wall of the level and of the shown asset
  solids. The walked floor stays because the collision file does not hold every solid the
  client stands on (section 4.2 and 7). It cannot go anyway: `bins/sim/src/world/attack.rs`
  (out of this work's files) records respawn places in it.

### 5.1 Dynamic solids: the tutorial's encounter walls (Observed)

`bins/sim/src/asset_solids.rs`: each asset entity of the tutorial table (`ftue-entities`) that
names a deferred slice of the collision file, spawned at its place (within 0.5 m), has that
slice's bodies. While the server shows the entity to a player (level assets show, change,
despawn), its bodies stand in that player's ground (`Player::solids`, a
`CollisionIndex::with_bodies` on the level's shapes) for the player's moves and for pushes.

| Asset entity | Deferred slice | Retail place | Slice origin |
|---|---|---|---|
| boat-wreck encounter | `AI_Wall_Collision` box | 484.350, 848.598, 91.366 | 484.350, 848.598, 91.383 |
| Thorpe arena fog wall | `ftue_v2_enc_thorpe_gate_fogwall.dynamicslice`, two gate meshes | 474.000, 238.000, 22.985 | 474.000, 238.000, 23.000 |

Mobs stand on the static ground only (they are shared by all players; the walls are each
player's).

The ship's rocking and sinking: no solid change is needed for the retail places. The galleon's
own motion (`libs/timeline/ftuev2/galleon_rotate_loop.timeline` on its `AnchorTransform`) does
not show in the own places: all 413 distinct retail deck places of the newchar tutorial are
within 0.042 m of the static deck (median 0.006 m), and the body replayed over the deck is
within 0.049 m of retail (1,066 records). The sinking states (`galleon_destructible*`) come
after the server moves the player into the sea (retail server move at record 3064); retail
never stands on the sinking ship. Run 14's own places after record 1272 (82 to 84 m inside the
hull) are our old server's, which went through the deck, not retail's.

## 6. Checks

### 6.1 Replays (retail keys and cameras against retail's own places)

"Before" is the code at the start of this work, rebuilt from its sources in a scratch crate;
the 09:24 replay binary of `run14/all.sh` was an older state (tutorial 0.292 m / 1,486).
"After" is the final code with the final collision files.

| Row | h median | h p90 | z median | z p90 | pull-backs |
|---|---:|---:|---:|---:|---:|
| tutorial (newchar retail 2026-09-18), before | 0.336 | 1.431 | 0.124 | 0.654 | 1,366 |
| tutorial, after | 0.226 | 1.321 | 0.022 | 0.117 | 1,314 |
| run 14 to record 1272 (the retail part), before | 0.000 | 0.010 | 0.011 | 0.035 | 83 |
| run 14 to record 1272, after | 0.000 | 0.010 | 0.004 | 0.006 | 82 |
| open world `ow2`, before | 0.156 | 1.448 | 0.061 | 0.413 | 1,610 |
| open world `ow2`, after | 0.153 | 1.448 | 0.055 | 0.186 | 1,579 |
| open world `ow`, before | 0.173 | 1.645 | 0.063 | 0.551 | 1,490 |
| open world `ow`, after | 0.157 | 1.614 | 0.057 | 0.211 | 1,460 |
| newchar linux 2026-09-12 (tutorial grid), before | 0.215 | 3.957 | 0.054 | 1.092 | 2,319 |
| newchar linux, after | 0.215 | 3.775 | 0.012 | 0.594 | 2,250 |

Every row is equal or better in h median, h p90, z median, z p90 and pull-backs. The whole run
14 (with our old server's places after record 1272): pull-backs 151 before, 149 after.

What closed the air pull-backs of the first collision build (tutorial 1,385, `ow2` 1,687,
`ow` 1,571): a pull-back in a jump was lost (the jump kept the keys' place), 1,370 / 1,611 /
1,495 with it kept; then the structure slope rule (5): 1,314 / 1,580 / 1,460. With the climb and
the descent across the capsule: 1,283 / 1,592 / 1,466 but medians up to 0.026 m worse (linux
0.241); across 0.2 m uphill: 1,295 / 1,594 / 1,461 and `ow` median 0.176. (Observed)

Tutorial deck: 413 distinct retail deck places, floor within 0.042 m of the own height (median
0.006 m); replayed body 89.38 to 89.50 m against retail 89.37 to 89.50 (median error 0.019 m,
before 0.213 m); linux run median 0.004 m (before 0.096 m).

### 6.2 Retail own places in the captures (Observed)

720 ledgers (`captures/`, the capture folder in the user's documents, the 2026-09-18 scratch
archive), one process each under a 4 GB cap; own places (ALC of interest 1) kept when they
moved 0.05 m; the level from the last `LevelInfoChangedMsg` (2,978,334 places in ledgers that
never name it are left out).

| Level | Places | On the terrain (0.1 m) | Over the terrain by 0.3 m+ (not swimming) | of those on a collision floor within 0.1 m | within 0.25 m | no collision floor under them |
|---|---:|---:|---:|---:|---:|---:|
| `ftue_v2` (4 ledgers) | 18,598 | 1,525 | 13,487 | 10,993 (81.5 %) | 12,938 (95.9 %) | 140 |
| `newworld_vitaeeterna` (265 ledgers), first build | 1,828,904 | 1,063,879 | 579,639 | 370,505 (63.9 %) | 450,730 (77.8 %) | 51,906 |
| `newworld_vitaeeterna`, meshes by path | 1,828,904 | 1,063,879 | 579,639 | 373,050 (64.4 %) | 453,538 (78.2 %) | 51,122 |
| `newworld_vitaeeterna`, final (with variants) | 1,828,904 | 1,063,879 | 579,639 | 372,950 (64.3 %) | 453,566 (78.3 %) | 50,907 |

The rest of the open-world places over the terrain: 74,243 have a collision floor more than
0.25 m under them (jumps, falls, or a structure the file lacks). The variants add 42,820
bodies; 100 places that matched a floor within 0.1 m before now have a higher floor within
1.3 m above their feet (a branch or a rock top of a variant's mesh over them).

## 7. Open (with where we looked)

- Solids the file does not hold: player housing plots and their furniture (placed at run
  time), vegetation from `region.vegetation` (`VegetationPhysicsComponent`, not read), the 12
  bodies with a stale `m_rnrAsset` sub id (4.2), and the open world's deferred slices, which
  no sim shows yet (the tutorial's are shown, 5.1).
- The 2,077 open-world solid entities outside variants that name no shape: their
  `m_shapeEntity` is invalid in every one; the client's use of such a body is Unknown (no
  shape found in the loader path read).
- Variant semantics Inferred from names: the depleted state hidden at load, the
  static-collision flag, the transform actions (4.1).
- How the client picks the physics source when several are set, and the meaning of a second
  shape slot (count 2): Unknown (no file has two shapes).
- The role of the 48-byte shape trailer for nested shapes: every compound child has none.
- The flags of the header block and the per-triangle material ids: not used.
- Walls for the moving body: the server does not stop the body at walls (6.1); the camera
  check holds the body to the client.
- The live stack: `local-data/tools/live/real-client-stack-card.sh` runs `target/release/sim`
  with `config/sim.toml` (`content_dir = "local-data/content"`), where the collision files are;
  the release `sim` must be rebuilt from this code first (the files are schema 2).
