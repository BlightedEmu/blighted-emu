# Level, XP, weapon mastery and the client GM mode (2026-09-18)

Two studies for the GM commands: what retail sends for the character level, experience and weapon
mastery, and how the client turns on its GM mode. Labels: **Observed** (seen in bytes, code or
data), **Inferred** (a reading of it), **Unknown**.

Sources: retail captures `local-data/captures/own/newchar-retail-2026-09-18/` (N: a new
character, tutorial, greatsword and blunderbuss) and
`local-data/captures/open-world/NH-MSQ-2.1-3acc9ebbecdfc753/` (M: a level 67 character); the
tables of `local-data/content/bundle-<build>.bin`; the client decompile `<re-scratch>/re-sweep`;
field names from the Azoth catalog `packets/{911,899,3681,129}.json` (facts only). Scratch tools and
the full GM sender list: `local-data/gm-research/` (git-ignored).

## 1. List grammar (Observed)

As `items::FullList` and `achievements.rs`. Full form: `wide(0) 01 wide(version) wide(count)`
and the elements. Delta form: `wide(n > 0)`, then groups of up to 8 entries, each group a `u8`
mask, each entry `wide(index) 01 wide(version) value`. Only the first entry of a child carries the
real version; the others carry version `00`. The children of one update share one version; the
login version is 3 (`FIRST_VERSION`). `index` is the list position; values are big-endian.

## 2. Type 911 CategoricalProgressionReplicatedState, component 65 (Observed)

One slot; slot 0 holds three parallel lists:

| Child | Name | Element |
|---|---|---|
| 0 | progressionIds | `u32`: CRC-32 (IEEE) of the lowercase `CategoricalProgressionId` |
| 1 | ranks | `u16`, zero-based |
| 2 | points | `u64`, experience inside the current rank |

- Not a map: a delta names the list position. A new entry goes at index = the old length, on
  all three children. The order is free (retail sent a full state in another order at the world
  change).
- Login (N): five entries (Cooking, Leatherworking, Fishing, SeasonPass, Camping), slot header
  `01 07`, each child `00 01 03 05 ...`.
- First kill (N): greatsword rank 1 with 10 points and blunderbuss rank 0 with 4 points were
  added at the end. Experience alone changes child byte `04`; a rank up `06`. Experience above
  the rank's cap carries over (34 + 20 - 50 = 4). Rank r shows as r + 1; at the top rank (19,
  shown as 20) the points stay 0 (M).
- Weapon rows of `javelindata_categoricalprogression`: `MaxLevel` 19, `RankTableId`
  `WeaponMastery`, `AutoRankUp` true, `AllowOverflow` false. Ids: `SwordAbilityTable`,
  `RapierAbilityTable`, `HatchetAbilityTable`, `SpearAbilityTable`, `GreatAxeAbilityTable`,
  `WarHammerAbilityTable`, `GreatSwordAbilityTable`, `FlailAbilityTable`, `DaggerAbilityTable`,
  `BowAbilityTable`, `MusketAbilityTable`, `BlunderbussAbilityTable`, `FireMagicAbilityTable`,
  `LifeMagicAbilityTable`, `IceMagicAbilityTable`, `VoidGauntletAbilityTable`.
- A weapon's category: `WeaponItemDefinitions.WeaponMasteryCategoryId`.
- Rank table `javelindata_weaponmastery`: `MaximumInfluence` is the experience of a rank before
  the next: 10, 50, 70, 105, 155, 225, 320, 450, 620, 830, 1100, 1450, 1870, 2370, 2950, 3560,
  4200, 4830, 5410, 8000 (ranks 0 to 19).

## 3. Type 899 ProgressionComponentReplicatedState, the level (Observed)

- Table `javelindata_xpamountsbylevel` (`ExperienceData`): `LevelNumber` 0 to 69, `XPToLevel`
  the total experience at which the level starts (0, 150, 480, 1080, 1970, ...; 2,718,521 for
  69). Level = the highest n with `XPToLevel[n] <= xp`; retail matches it at all five level ups.
- The experience in the message is the total, not the part inside the level.
- Login at level 0: `02 03 <xp> <rested>` (no level). Login of the level 67 character:
  `03 01 <level u32> 03 <xp> <rested>`.
- Level up: `03 01 <level> 01 <xp>` (no rested). One gain from level 0 to 2 sent only the new
  level. Experience gain alone: `02 01 <xp>`.
- At each level up retail also sent 6426
  `GameModeParticipantComponentClientFacet_OnGameModeSchedulerRefreshed` 8 to 18 ms before, and a
  type 129 attribute change (unspent points; `AttributePoints` of the level table: 5 per level
  low, 10 high). No level-up facet message exists; the client shows "LEVEL n" from the 899 change
  (Inferred).

## 4. Type 3681 ProgressionPointReplicatedState, component 67 (Observed)

One slot, four parallel lists: poolIds (`u32` pool CRC; a weapon pool has the 911 id), poolCounts
(`u16`, unspent points), poolCategories (`u8`: 1 weapon, 2 territory), pointEntries (10 bytes:
pool CRC `u32`, `ProgressionPointId` CRC `u32`, two bytes of Unknown meaning). Login (empty):
`01 0f` and four times `00 01 03 00`. Each weapon rank up raises the pool count by 1.

## 5. The client GM mode (Observed)

- The switch is in `RegistrationResponseMsg` (type 3), which rep-gateway sends. Its handler
  (`FUN_146b707d0`, "Client connection is authorized") copies four flag bytes into the ClientSDK:
  byte index 1 (message +0x59) to +0x6f1, which `IsGmClient` (ClientSDK vtable slot 0xb8,
  `FUN_146b6f520`) reads; byte index 2 is a second flag of Unknown name that also makes
  `IsGmClient` true. Retail sends `[1,0,0,1]` in all 43 captures: no retail capture has a GM
  client.
- `IsGmClient` turns on: "GM Client Enabled" in the `r_DisplayInfo` overlay; 3956
  `PlayerComponentServerFacet_RequestGmPermissions` (no body) from the local player's
  `OnEnable`; the spectator camera (4224 `RequestDevPermissions`, answered by 4221); a chat
  feature (Unknown); the cvar `javelin.gm.set-faction-on-start`.
- The login, session and character-list JSON carry no GM field.
- The client checks nothing before it sends a GM or debug message: the server must check the
  sender's rank.
- Senders that exist (type, name, object fields of the sender; the wire order is Unknown until a
  live body is logged): 3342 `ProgressionComponentServerFacet_Debug_SetPlayerLevel` (u32 level);
  4032 `CategoricalProgressionComponentServerFacet_DebugModifyProgression` (u8, u64, u32); 43
  `DebugSetProgressionRank` (u16, u32); 3783 `CurrencyComponentServerFacet_GmSetCurrency` (u64);
  4815 `GameMasterComponentServerFacet_GiveGoldToPlayer`; 2698 `DebugCreateItems`; 26, 6765, 2551
  GameMaster teleports by character id; 3941, 1033 `DebugTeleportPlayer(GM)` (16 bytes); 1623,
  4121 chat GM permissions. 167 GM and debug types have a sender in all. 2855 `Debug_ModifyExp`
  and 4034 `DebugMaxWeaponMastery` have none in this build.
- Correction to the S10 study: `FUN_143225fc0` sends 5419 `ResetDailyWarCountForPlayer`; the
  type 26 sender is `FUN_143226a20`.

## 6. Attribute and weapon points (2026-09-20)

Sources: newchar N (4 x 1502, 8 x 1112, 8 x 2857, each with the server answer 30 to 50 ms
later), NH-MSQ-2.1 M (logins), the tables, the decompile. Scan tools:
`local-data/scratch-work/quests/pts` (client calls and the answers in a time window, or every
body that holds a byte pattern) and `local-data/scratch-work/quests/ptrows` (the tables).

**Client calls (Observed, N).** Each body is the 16-byte facet header, then:
- 1502 `AttributeComponentServerFacet_RequestSpendAttributePoints`: a count and `u32`
  attribute ids, a count and `u32` points (parallel). N sent Strength 15, 5, then
  Constitution 5 with Strength 0, then Strength 5.
- 1112 `ProgressionPointComponentServerFacet_RequestAssignPoolPoints`: `u32` pool id, `u32`
  `ProgressionPointId` CRC, `u32` points (1 in all 8).
- 2857 `AbilityComponentServerFacet_RequestApplyAbilityPoints`: no arguments; N sent it in the
  same millisecond as each 1112.

**Attribute ids (Observed).** Client enum `CharacterAttributeType` (`FUN_142797540`):
Constitution 0, Focus 1, Strength 2, Dexterity 3, Intelligence 4. Each names an
`AttributeDefinition` table; its `Level` rows run 5 to 500, and every attribute of a new
character is 5.

**Type 129 `AttributeComponentReplicatedState`, component 27 (Observed).** Slot 0, four
children (`FUN_14276d1f0`): `attributes` (count, then `u32` id and `u32` value; retail order
4, 3, 2, 1, 0; value = 5 + spent), `attributeBonuses` and `placingBonuses` (replicated lists:
empty, and five 0), `persistentAttributeData` (`FUN_142797fc0`: `m_unspentAttributePoints`
`u32`, `m_ignoreBackstoryPoints` and `m_hasRespec` one byte each, `m_spentPoints` count and
(`u32` id, `u32` points) pairs in first-spent order, `m_chosenMagnifyBehavior` `u32`). Login:
all four, lists at version 3, unspent 0, flags 0, magnify 5. Level up: children 1 to 3 with a new
version; unspent = the `AttributePoints` of every level up to the new one less the spent points
(N: 10 at level 2, then 15, 5, 5, 5; M at level 66: 480 spent = the sum, 0 unspent). A spend:
all four. M has `m_ignoreBackstoryPoints` 1 and magnify 5.

**Type 3681 answer to a 1112 (Observed).** The pool's count change (pool count = weapon rank
less the assigned points) and the whole point list. The 2-byte end of a point entry is the
point's level: it rose by the 1112 points; territory entries of M hold up to 14, refunded
weapon entries 0.

**Type 185 `AbilityComponentReplicatedState`, component 46 (Observed).** Slot 0, five children
(`FUN_1466eba40`); only child 0 `persistentAbilityData` is built here. Writer
`FUN_146856460`, reader `FUN_14669f600`/`FUN_14669fb50`: a count, then per ability table its id
(CRC of the `AbilityData` table id; all 29 tables) and up to 3 mapped slots (`u32` ability CRC,
`u32` key), then a map (`FUN_143025640`) of the 16 weapon pools to their
`ProgressionPoolData.VersionNumber`. The slot keys are 402, 115, 139 in all 29 tables of N and
M (meaning Unknown). The answer to 2857: N's `Ability_Greatsword_Combo` and
`Ability_Blunderbuss_AzothShrapnelBlast` (`IsActiveAbility` true) went into the first slot of
their table; upgrades and passives (`IsActiveAbility` false) changed nothing. M's tables hold
two or three mapped abilities.

**Rules used (Observed in the tables).** Weapon points: `ProgressionPointData` rows with
`IsAbility` 1 of a pool with a `VersionNumber`; `MaxLevel`; `RequiredProgressionPointId` and
`RequiredProgressionPointLevel`. No weapon row sets `RequiredCategoricalProgression*`,
`RequiredCharacterLevel` or `DoNotSpendPoint`.

**Unknown.** The meaning of the slot keys, of `m_chosenMagnifyBehavior` 5 and of the placing
bonuses; whether retail adds a 0-point entry to `m_spentPoints` (N's 0-point Strength already
had points); the retail order of the 29 tables (the same in N and M; this server uses the
content's table order) and of the version map; the tree-tier rules of the client UI (points
spent per row), which no server table holds; what retail answers to a refused call.

## Unknown

The last two bytes of a point entry; the 899 bonus level; how retail splits mastery experience
between two weapons of one kill (greatsword +20, blunderbuss +4); the mastery category of weapons
with no `WeaponMasteryCategoryId`; why a new character starts at rank 50 in three trade skills;
the vitals change at level up; the wire order of the GM message bodies; the retail answer to 3956;
the meaning of registration flag byte 2; which client UI calls the GM senders.
