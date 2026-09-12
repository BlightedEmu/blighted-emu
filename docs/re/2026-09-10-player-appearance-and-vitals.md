# Player look, vitals, and the child-byte order

Date: 2026-09-10. Client build `25810808` (Ghidra project `<re-scratch>/ghidra-25810808`,
headless, `-readOnly -noanalysis`). Old build `22469132` for constructor matches only.

Offline only. The game was not run. Easy Anti-Cheat was not analyzed. No body bytes, names,
ids, tokens, or IP addresses from the captures are in this note. Capture data is used only as
counts, sizes, child sets, and value ranges.

Labels: **Observed**, **Inferred**, **Unknown**.

Problem (live, 2026-09-10): the real client is in game, but the player is a naked default mesh,
the health bar reads 00/00, and `Game.log` repeats `VitalsComponent: Could not find stat
GetStatRegenRate Mana`. The own spawn card sent every player component except 3935 as an empty
body (`00`). Follow-up of `2026-09-10-local-player-check.md` section 8.

## 1. Child bytes and child data alternate

**Observed.** The child reader of a slot (`FUN_1417b5330`, current build) reads one child byte,
then the data of the children that byte sets (up to 7), then, when bit 7 is set, the next child
byte. The child bytes are not all in front of the data.

**Observed.** With that order a scratch decoder ends every type-15 body: 2,232,966 of 2,232,966
in the 89 open-world `fragments.jsonl` files, and 2,120 of 2,120 in the 10 starting-zone
captures. 525,732 of them set a child of index 7 or higher. With the old order (all child bytes
first), 99 of the 2,120 starting-zone bodies fail.

**Inferred.** This explains the 89 "Round 5" failures of `2026-09-04-m6-vitals-state.md`. That
round read the child bytes of `FUN_1417b43c0` as one block.

**Fix.** `crates/nw-messages/src/slots.rs` `read_children` / `write_children` take a per-child
reader or writer and keep the retail order. Every state that uses them (vitals, stamina, combat
status, delayed event, spawn card, 3935, appearance, mana) now reads and writes through them.
Bodies with only children 0 to 6 have the same bytes as before.

## 2. `MB::PlayerAppearanceComponentReplicatedState` (type 1195, component 12)

**Observed.** Constructor `FUN_146711510` (old `FUN_146710240`). One slot, 14 children, in this
order: `playerGender`, `playerRace`, `playerSkinTone`, `playerHairstyle`, `playerFacialHair`,
`playerHairColor`, `playerFacialHairColor`, `playerEyeColor`, `playerFaceMark`, `playerScar`,
`playerTattoo`, `playerTattooColor` (one `u8` each, reader `FUN_14279f1b0`), `iconData`
(`FUN_1469527b0` -> `FUN_145ce86a0`), `appearanceChangeFlag` (one `u8`, same reader as the look).

**Observed.** `iconData` is `PlayerIconData`: 7 `u8` (gender, race, skin tone, hair, hair colour,
facial hair, facial-hair colour; hair colour comes before facial hair here) and a big-endian
`u16` background colour. 9 bytes.

**Observed.** Captures: all 6,749 type-1195 bodies of 111 files end with this grammar. Every
first local-player body after a spawn card (1,150) sets all 14 children: 25 bytes, child bytes
`ff` and `7f`. `appearanceChangeFlag` is 0 on 6,718 of 6,726 full bodies. The icon bytes equal
the look bytes, except race on 574 female bodies (+1, +2, or +4; reason **Unknown**). The
background colour is 0 to 27, and 0 on 5,291 bodies; its table is **Unknown**.

### Value of each look byte

**Observed.** The look lists are in `assets/SharedDataStrm-part7.pak`, folder
`sharedassets/genericassets/character/charactercreationdatabase/` (AZCS + zlib, ObjectStream v3).
Gender selects `male.charcreatedb` or `female.charcreatedb`; each race row names its skin-tone,
hair, facial-hair, eye, face-mark, scar, and tattoo lists.

| field | rows | position of a saved name |
|---|---:|---|
| gender | 2 | `Male` 0, `Female` 1 |
| race (male) | 31 | face01-03 at 0-2, `Chroma` 3, face04-10 at 4-10, `ChromaCutout` 11, face11-29 at 12-30 |
| race (female) | 30 | faceNN at NN-1 |
| skin tone | 10 | skinNN at NN-1 |
| hairstyle | 72 | `None` 0, hairstyleNNm at NN |
| facial hair | 69 | `None` 0, facialhairNNm at NN |
| hair / facial-hair colour | 30 | haircolorN at N-1 |
| eye colour | 20 | eyecolorN at N-1 |
| face mark | 7 | `none` 0, facemarkNN at NN |
| scar | 11 | `none` 0, scarN at N |
| tattoo | 62 | `none` 0, tattooN at N |
| tattoo colour | 15 | tattoocolorN at N-1 |

**Observed.** In the captures the largest value of each field is its row count minus 1. Male race
never uses 3 or 11.

**Inferred.** The wire byte is the 0-based row of the list, `none` and Chroma rows included. The
client code that indexes the list was not decompiled (**Unknown**).

### Saved look

**Observed.** `CreateCharacter` stores `CharacterCreationParams`: base64 of zlib data. The data is
JSON written byte by byte: control bytes as `\u00XX`, bytes above `0x7f` raw (not UTF-8). Its
`visualsBlob` string is a binary ObjectStream v3. The root has one string child per look choice,
keyed by the lowercase CRC-32 of `gender`, `race`, `skintone`, `hairstyle`, `facialhair`,
`haircolor`, `facialhaircolor`, `eyecolor`, `facemark`, `scar`, `tattoo`, `tattoocolor`. The
root also has an `equipment` list (`gloves`, `shirt`, `hat`, `boots`, `pants`, each with a 4-byte
item key).

**Observed.** The live test character decodes to `[0, 5, 0, 65, 44, 4, 4, 11, 0, 6, 26, 14]`
(male, face05, skin01, hairstyle65m, facialhair44m, haircolor5 twice, eyecolor12, no face mark,
scar6, tattoo26, tattoocolor15).

## 3. Vitals, stamina, and mana

**Observed.** First local-player type-15 bodies: slot 0 children `0x21ff` on 1,033 of 1,313
(children 0 to 8 and 13), slot 1 `0x1f` on 1,122. Children 9 (`vitalsCategoryId`) and 10
(`vitalsLevel`) are never set in 322,444 local-player bodies. `vitalsId` is `0x98197a65`, the
lowercase CRC-32 of `player`, on all 1,333 that carry it. At level 1 health amount, `HealthMax`,
and `maxHealth` are 856 (5 of 6). Stamina and mana are 100 of 100; health and mana tick rates are
0; stamina tick rate is `0x5100` on all 1,333.

**Observed.** First type-4297 (stamina) bodies: 26 bytes, all 6 children: 100, 100, 0, 0, 1, 1.
First type-1652 (`MB::ManaComponentReplicatedState`, constructor `FUN_1467088c0`, one slot, four
big-endian `f32`): 18 bytes: 100, 100, `regenDelay` 0, `regenRate` 1. The names of the first two
children are **Unknown**.

**Observed.** The "Could not find stat" log comes from `FUN_14696d700`, called by the
`GetStat*` functions (stat 0 health, 1 stamina, 2 mana). The health bar reads a map that the
facet apply (`FUN_14683f940` -> `FUN_1467a8830`) fills only when a stat's amount, max, or tick
rate arrives. `GetStatRegenRate` reads a second map from `VitalsDataManager`, keyed by the
`vitalsId` that the apply copies.

**Inferred.** The empty bodies gave no stat entries (bar 00/00) and no `vitalsId` (no mana regen
entry, so the log line).

**Observed.** Content bundle `25810808`, VitalsBaseData row `Player`: health 856, mana 100,
stamina 100.

## 4. What the server sends now

The own spawn card (`bins/sim/src/world/members.rs`, `nw_messages::player_members`) sends:

- 1195: the saved look, its icon on background 0, change flag 0 (25 bytes, as retail).
- 15: health amount and `HealthMax` from the sim health, stamina 100 of 100, tick rates (0,
  `0x5100`), `vitalsId` `0x98197a65`, `maxHealth`; slot 1 mana 100 of 100, tick rate 0.
  `vitalsData`, `healthChangeFlags`, and the affliction lists are not sent (meaning or empty form
  **Unknown**).
- 4297: 100, 100, 0, 0, 1, 1. 1652: 100, 100, 0, 1.
- 3935: unchanged (connected, login `characterId`).

The director decodes the look from the stored appearance (`nw_login::look::saved_look`) and sends
it in `Admit`; a handoff carries it on. A character without creation settings (bots) keeps the
empty 1195 body.

## 5. Not done

- Gear. The paperdoll (type 3183, component 62, 3 slots) has retail first bodies of about 1.7 KB;
  its layout is not known here. The saved creation `equipment` list gives the starter items. Until
  the paperdoll is sent, the player keeps the default clothes mesh.
- Other players. A remote player's card still has an empty look and vitals (sims share no
  look in ghosts yet).

## Files read

- `docs/re/2026-09-10-player-component-slots.md`
- `docs/re/2026-09-10-local-player-check.md`
- `docs/re/2026-09-04-m6-vitals-state.md`
- `local-data/azoth-catalog/packets/1195.json`
- `crates/nw-messages/src/{slots,vitals,components,player_components,gde}.rs`
- Session notes `appearance-1195-re.md`, `player-vitals-re.md` (side-session notes folder)
