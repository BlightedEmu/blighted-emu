# Design files

This folder holds the team's changes to the game world: one TOML file per record. The game data
itself is never stored here; it comes from your own New World install. `just design` merges these
files over the client data and writes the files the servers read. The Studio page of the admin
site (`/studio`) does the same with its **Apply** button.

## A file

```toml
schema = 1
kind = "loot_table"        # what the file changes (table below)
action = "override"        # override | add | disable
id = "LT_EXAMPLE"          # the record id; also the file name: LT_EXAMPLE.toml
max_roll = 50              # the fields to set
```

- `override` sets the named fields of the client record with this id. Other fields keep their
  client values.
- `add` makes a new record. Start from a client record with `base = "<id>"` and name only the
  fields you change. Without `base`, the file must hold every required field of the row.
- `disable` removes the client record. It takes no fields.

Field names are the snake_case names of the row structs in `crates/nw-content/src/generated/`.
Do not copy client values into a file; name only what you change.

## Kinds and folders

| `kind` | Folder | Client data | Id |
|---|---|---|---|
| `item` | `items/` | `MasterItemDefinitions` | `ItemID` |
| `weapon` | `items/weapons/` | `WeaponItemDefinitions` | `WeaponID` |
| `armor` | `items/armor/` | `ArmorItemDefinitions` | `WeaponID` |
| `consumable` | `items/consumables/` | `ConsumableItemDefinitions` | `ConsumableID` |
| `perk` | `perks/` | `PerkData` | `PerkID` |
| `loot_table` | `loot/tables/` | `LootTablesData` | `LootTableID` |
| `mob` | `mobs/` | `VitalsBaseData` (also its `loot_table_id`, `loot_drop_chance`, `loot_tags`) | `VitalsID` |
| `npc` | `npcs/types/` | `NPCData` | `NPCId` |
| `objective` | `quests/` | `Objectives` | `ObjectiveID` |
| `objective_task` | `quests/tasks/` | `ObjectiveTasks` | `TaskID` |
| `spawn_point` | `spawns/<map>/` | creature spawn points of a world file | point key |
| `npc_point` | `npcs/<map>/` | NPC points of a world file | point key |
| `ai_profile` | `ai/` | (own) a behaviour and brain numbers | profile id |
| `path` | `paths/<map>/` | (own) a patrol path of one level | path id |
| `spawn_group` | `spawns/<map>/` | (own) new mob spawns of one level | group id |

## Spawn and NPC points

A retail point has no id of its own, so it gets a **point key**: `<type id>_<x>_<y>_<z>`, with the
position rounded to 0.1 m (and `_2`, `_3`, ... when two points share a key). To find the key of
a mob spawn, turn on "Mob spawns" on the admin Map page, zoom in, and click the spawn: the status
line shows its design key. NPC points are not on the map yet. A point file also names its level
with `map`:

```toml
schema = 1
kind = "spawn_point"
action = "override"
id = "123456789_100.0_200.0_30.5"
map = "newworld_vitaeeterna"
level = 30
```

Point fields: `spawn_point` takes `vitals_id`, `category_id` (text ids), `level` (1 to 65) and
`position` (`[x, y, z]` in metres); `npc_point` takes `npc_id`, `faction_type` (0 to 255) and
`position`. Points can be changed or disabled, not added.

## AI profiles, paths and spawn groups

The last three kinds have no client record: they are only ever `add`.

```toml
# ai/gate_guard.toml: every number is optional and keeps the sim's [combat] value when missing.
schema = 1
kind = "ai_profile"
action = "add"
id = "gate_guard"
behaviour = "guard"        # wander (default) | guard | patrol | idle
sight_m = 25.0
speed_mps = 4.0
respawn_ms = 60000
```

- `wander`: walks near its spawn, attacks a player in sight.
- `guard`: stands at its spawn, attacks a player in sight, then goes back.
- `patrol`: walks its `path`, attacks a player in sight, then goes back to where it left the path.
- `idle`: stands at its spawn and fights only a player who hits it.

Other numbers: `wake_radius_m`, `wander_radius_m`, `wander_pause_ms`, `leash_radius_m`,
`attack_reach_m`, `attack_interval_ms`.

```toml
# paths/newworld_vitaeeterna/gate_walk.toml
schema = 1
kind = "path"
action = "add"
id = "gate_walk"
map = "newworld_vitaeeterna"
points = [[100.0, 200.0, 30.0], [140.0, 200.0, 30.0]]
waits_ms = [3000, 0]       # optional: the wait at each point
mode = "back_and_forth"    # loop (default) | back_and_forth | once

# spawns/newworld_vitaeeterna/gate_wolves.toml
schema = 1
kind = "spawn_group"
action = "add"
id = "gate_wolves"
map = "newworld_vitaeeterna"
vitals = "Wolf_Alpha"      # a VitalsBaseData id
level = 20
points = [[100.0, 205.0, 30.0], [102.0, 207.0, 30.0]]
ai_profile = "gate_guard"  # optional
path = "gate_walk"         # optional: a path of the same level
```

A `spawn_point` file may also set `ai_profile` and `path`. A spawn group's spawns get the keys
`<group id>_1`, `<group id>_2`, ...

## Checks

- `just check` (any PC, no game needed): the file format, folders, field names and values.
- `just check-design` (needs `just bundle` and `just world`): also checks every id against the
  client data. Apply on the Studio page runs this check first and writes nothing on an error.
