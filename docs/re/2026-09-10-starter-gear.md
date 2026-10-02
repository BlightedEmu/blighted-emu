# Starter gear: archetype kits and the gear states of the own spawn card

Date: 2026-09-10. Build: Steam `25810808`. Labels: **Observed**, **Inferred**, **Unknown**.

Sources: content bundle `25810808` (`ArchetypeData`, `BackstoryDefinition`,
`MasterItemDefinitions`), the retail capture
`local-data/captures/open-world/01-starting-zone-*/12-1_getoffbeach-Soul-Survivor.zip` (a new
wanderer character), and client reader functions on the old build `22469132` (scratch Ghidra
project in `<re-scratch>`). The wire grammar was checked on 41 retail ledgers.

## 1. The archetype pick

**Observed.** `CreateCharacter` stores `CharacterCreationParams`. Its `visualsBlob` ObjectStream
root has a string child `backstory` (key: lowercase CRC-32 of `backstory`) with the backstory id,
for example `Archetype_Mystic`. Code: `nw_login::look::saved_backstory`.

| ArchetypeData id | backstory id | item prefix |
|---|---|---|
| mercenary | `Archetype_Soldier` | Soldier |
| warmonger | `Archetype_Destroyer` | Destroyer |
| tracker | `Archetype_Ranger` | Ranger |
| musketeer | `Archetype_Musketeer` | Musketeer |
| occultist | `Archetype_Occultist` | Occultist |
| theurgist | `Archetype_Mystic` | Mystic |
| wanderer | `Archetype_Swordsman` | Swordsman |

The `equipment` list in the same blob holds look ids (CRC-32 of `ArmorAppearanceDefinitions`
names), not item ids.

## 2. The kit

**Observed.** `BackstoryDefinition.InventoryItem` lists `item:quantity` pairs, for example
`MysticArchetypeHead:0,MysticArchetypeChest:1,...,1hSwordT2_FTUE:1,MysticArchetype_1hGauntletVoid:0,...`.
In the wanderer capture the quantity-1 items (chest, hands, legs, feet, `1hSwordT2_FTUE`) are
equipped in the first login bundle. The quantity-0 items (head and the two archetype weapons)
arrive about 5 minutes later from the tutorial captain's chest (client `TakeAllItems`, loot table
`FTUE_ArchetypesLoot1`). The bag is empty at login.

**Observed** (bundle data). Archetype armor: gear score 75, durability 375, bind on pickup.
`1hSwordT2_FTUE`: gear score 60, durability 1000.

## 3. Item index

**Observed** (client `ItemDataManager`, old build). The wire names an item by a 20-bit index. The
client walks the `MasterItemDefinitions` sheets in key order, the rows of each in file order,
skips `ItemType` `HousingItem`, and numbers the others from 1.

**Inferred.** The key is the sheet's unique id in byte order. With build `25810808` data this rule
gives 5 more than every capture index (for example `1hSwordT2_FTUE` 1228, capture 1223;
`SwordsmanArchetypeChest` 40231, capture 40226). The capture likely came from a build with 5 fewer
rows in the first sheets. **Unknown** until a live check: the server uses the rule on the
`25810808` data.

## 4. Gear states (paperdoll-inventory study)

Full first states, version 3, in the own player's group of the first bundle (retail order puts
them with the full own ALC):

- **3183 `PaperdollComponentReplicatedState`** (component 62). Slot 1 (owner): visible durability
  list, visible item list, empty hidden item, slot and durability lists, `bonusEquipLoad` 0,
  `isLocalPlayer` 1, `isLoadoutPanelOpen` 0, one loadout `@lo_name 0` with (paperdoll slot,
  owned-item key) pairs. Slot 2 (public): sheathe map (slots 9 and 24 to 29), attachment map
  (empty), active map (24 and 27), visible slot ids, item visuals.
- **1755 `ContainerComponentReplicatedState`**: component 61 (bag) with 5 empty slots, bonus 0,
  "was emptied" 1; component 76 with bonus 0 and "was emptied" 0 (its use is **Unknown**).
- **5437 `ItemManagementComponentReplicatedState`** (component 2): owned items with keys 0 to 4,
  sync reason 17.

Item record: a packed big-endian `u64`
`(((quantity*64 + syncReason)*502 + containerSlot)*2^20 + itemIndex)*8001 + durability`
(container slot 501 = none), eight variable-length numbers (max durability, a per-item counter,
gear score, five perks), 4 dye bytes, 16-byte instance id, flags (bit 0 bound), transmog `u32` and
4 dye bytes. Retail servers send the transmog bytes on every item.

Paperdoll slot ids: head 0, chest 1, hands 2, legs 3, feet 4, main hand 24, off hand 27.

Code: `nw_messages::{items, paperdoll, containers, gear}`, `nw_content::starter`, and
`bins/sim/src/world/gear.rs`. The encoder output equals the retail first paperdoll body up to the
loadout pairs (instance ids set to zero; the unknown ammo child is not sent).

## 5. What the server sends now

Each login and handoff takes the backstory from the saved character and sends the login kit of
its archetype in the own spawn card. Instance ids are a name-based UUID of the character and the
item id, so they stay the same. There is no item store: every login wears the kit.

**Later finding.** There is an item store now. `bins/sim/src/saves.rs` saves the item stacks,
storage chests and loadouts of each character (`ItemSave`).

## 6. Unknown

- The live check of the item index rule (section 3).
- The type 3935 `playerBackstory` child (retail sends the backstory CRC-32 there); its child index
  is not known here, so it is not sent.
- The meaning of the per-item counter, the loadout flag and extra pairs, the ammo child, and the
  first version value 3.
- The captain's chest and the beach bags (the quantity-0 items) need the tutorial interactions.
