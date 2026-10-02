# Own player position: why the client ignores ours

Date: 2026-09-10. Build: Steam `25810808`. Labels: **Observed**, **Inferred**, **Unknown**.

Sources: live run 22:28Z (sim debug log of refused moves), a scratch capture scanner over all
696 ledgers (counts and child values only; no body bytes kept in the repo), Ghidra headless
read-only (`StringFuncs.java`, dumps `<re-scratch>/out-ownpos`, `out-alcnames`).

## Live symptom

**Observed.** The client's own position in its inputs (type 1589): x 2049.7, y 15.7, z -1.6.
The sim's position: 11132.5, 7305.9, 168.25. All 1,296 moves were refused as `too_fast`.
(2048, 16) is the phasing-grid origin that our type 1635 sends. The client never applies the
`pos10` we send in the own ALC (type 11, component 16).

## ALC child names

**Observed.** `FUN_142a36d50` registers the type-11 children by name, in constructor order
(this matches `SLOT0_CHILDREN` in `crates/nw-messages/src/alc.rs`):

| Child | Name | Kind |
|---|---|---|
| 0 | idRel | u8 |
| 1 | timeOffsetRel | u8 |
| 2, 4, 6, 8 | slayerSeqTimeRel | u8 |
| 3, 5, 7, 9 | slayerSeqTimeAbs | half |
| 10 | worldPosRel | pos3 |
| 11 | rotation | quat |
| 12 | lookDir | quat |
| 13-24 | slayerStateId, slayerStateIdStarted, slayerSequenceId (x4) | prefix, u16, prefix |
| 25 | idAbs | u8 |
| 26 | timeOffsetAbs | u8 |
| 27 | worldPosAbs (`pos10`) | f32, f32, u16 |
| 28-30 | scopeTimeBlob0Data0, scopeTimeBlobEx, scopeTimeBlob1Data0 | |
| 31 | teleportAndMigrationId | u8 |

Later children: scopeData, scopeInfoBlob, globalFragTags, distGround, waterDepth,
aiAngleToDesiredFacing, ... (see the dump).

**Observed.** The binary has the text "Absolute portion in delta compression wasn't set! We
can't calculate the value." near this class. The position and the id/time fields are
delta-compressed: an absolute part (`*Abs`) and a relative part (`*Rel`).

## Retail own ALC (captures)

**Observed.** 22,387,471 own-player ALC members (group interest 1). Children 0, 1 and 2 are in
almost all of them. `worldPosAbs` (27) is in 2,140,782, and then `worldPosRel` (10) is present
too (2,140,745). Those `worldPosAbs` values are a median 6.2 m from the client's own input
position.

**Observed.** Child 0 (`idRel`) changes in about half of all updates; it is not a teleport
counter. Children 1 and 2 do not echo the client input frame number (the `u16` after each
input record mask, a simple counter).

**Correction (later the same day).** The earlier reading "the first own ALC at entry has no
position" was wrong: our member walk stops at the first unknown class (3147) before it reaches
the real first own ALC, so it saw only later small updates.

**Observed.** A byte search of the first bundle after 1628 in the beach capture (Soul
Survivor) found the client's start spot (478.5, 849.7; client's first input 480.6, 850.2) inside
an own ALC member (`10 0b`: component 16, class 11) of the 13,324-byte entry bundle. It is a
FULL state: slot mask `0x03`, slot-0 presence `0x21ff3fafffffff`, slot-1 presence `0x7f`.
Over 315 retail logins the first own ALC always has both slots, `idAbs` (25), `timeOffsetAbs`
(26), `worldPosAbs` (27), `teleportAndMigrationId` (31) and fixed children 29-53;
`segmentedStamina` (46) is 4021696251 in all 315.

**Fix (live-verified 2026-09-10 23:28Z).** `AlcState::spawn` writes the most common retail first
state with our position; the sim sends it with the own spawn card only. The client then starts at
the sim position (client inputs x 11146-11154, y 7302-7312 vs sim 11132.5, 7305.9), the world
renders, and the player walks. The network-warning icon is gone (carrier fix).

## Unknown (next)

- (Solved above.) The source of the start position. Not the character list (no coordinates; it has
  `LocationId`, `LocationGroupId`, `FtueCompleted`). Not 1617 (empty). Candidates: a member of
  the first own group that our walk cannot read yet (type 13 `PositionInTheWorldReplicatedState`
  or another), or a GDE / streaming path.
- Next safe step: search the first bundles after 1628 for float pairs near the client's first
  input position, ONE ledger at a time with a small memory cap (a run that kept many bundles
  for all ledgers in one process ran the machine out of memory on 2026-09-10).
- The local-player correction settings (`g_localPlayer_*Correction*`) are registered in
  `FUN_141031800` at settings offsets `0x2f0`-`0x308`; their readers are not traced yet.
  **Later finding.** `docs/client-patch.md` (fourth change) lists eight variables at `0x2F0` to
  `0x30C`. The patch sets `CorrectionThreshold` and `ErrorDistanceForFullCorrection` to
  100,000 m, so the client does not correct its own movement.
