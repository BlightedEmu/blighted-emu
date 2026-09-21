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

## Unknown

The last two bytes of a point entry; the 899 bonus level; how retail splits mastery experience
between two weapons of one kill (greatsword +20, blunderbuss +4); the mastery category of weapons
with no `WeaponMasteryCategoryId`; why a new character starts at rank 50 in three trade skills;
the vitals change at level up; the wire order of the GM message bodies; the retail answer to 3956;
the meaning of registration flag byte 2; which client UI calls the GM senders.
