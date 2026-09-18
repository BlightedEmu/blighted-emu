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
| `mob` | `mobs/` | `VitalsData` (also its `loot_table_id`) | `VitalsID` |
| `npc` | `npcs/types/` | `NPCData` | `NPCId` |
| `objective` | `quests/` | `Objectives` | `ObjectiveID` |
| `objective_task` | `quests/tasks/` | `ObjectiveTasks` | `TaskID` |
| `spawn_point` | `spawns/<map>/` | creature spawn points of a world file | point key |
| `npc_point` | `npcs/<map>/` | NPC points of a world file | point key |

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

## Checks

- `just check` (any PC, no game needed): the file format, folders, field names and values.
- `just check-design` (needs `just bundle` and `just world`): also checks every id against the
  client data. Apply on the Studio page runs this check first and writes nothing on an error.
