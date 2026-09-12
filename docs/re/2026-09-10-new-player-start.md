# New-player start: tutorial level, then the main-world beach

Date: 2026-09-10. Build: Steam `25810808`. Labels: **Observed**, **Inferred**, **Unknown**.

Source: capture `local-data/captures/open-world/01-starting-zone-*/12-1_getoffbeach-Soul-Survivor.zip`
(a full login through the end of the tutorial, about 12 minutes), read with a scratch tool
(field values and lengths only). The other nine starting-zone captures (the tutorial quest
chain: skinning knife, boar feast, crewmates, survivors, pillage ship, recover items, demon
storm 1 and 2, raise alarm) start mid-session and have no login.

## Level messages (type 1635)

**Observed.** At login the server sends 1635 twice (5 s apart), 80 bytes each:

| Field | Value |
|---|---|
| level name | `FTUE_V2` |
| terrain path | `coatlicue/FTUE_V2` |
| phasing origin X, Y | 400.0, 150.0 |
| phasing bounds X, Y | 416.0, 736.0 |
| reserved (u64) | a random 64-bit value P |
| phasing count | 1 |
| entry | keyType 3, keyId 0, phaseId P (same value), extra 2 |
| flagA0, clientCtx, isMaster, flagA3 | 1, 1, 1, **1** |
| instance | a clock stamp |

**Observed.** About 12 minutes later (end of the tutorial) the server sends 1635 for
`NewWorld_VitaeEterna`: first 89 bytes (open-world grid origin 2048, 16; bounds 12272, 10250;
reserved 0; phasing count 0; flags 1, **clientCtx 2**, 1, 0; same instance), then 5 s later the
106-byte form with one phasing entry (keyType 1, keyId 0x2331, a phase id, extra 0).

## Start positions (client's first non-zero input after each 1635)

| Level | Position (x, y, z) |
|---|---|
| `FTUE_V2` (tutorial start, shipwreck beach) | 480.57, 850.16, 91.26 |
| `NewWorld_VitaeEterna` (after the tutorial) | 7278.91, 3143.20, 41.45 |

**Observed.** The first full own ALC after login carries `worldPosAbs` 478.5, 849.7 (the
tutorial start; own-player-position note). The client's first input is 0, 0, 0 before that.

## Our data

**Observed.** `local-data/content/world-25810808-ftue_v2.bin` (24,730 bytes) and
`world-25810808-climaxftue_02.bin` exist. Each sim and director runs one level
(`level` in `config/sim.toml` and `local-data/world-director.toml`).

## Unknown

- What ends the tutorial on the server side (the quest chain; we have no quest system).
- Whether `climaxftue_02` is a separate level step between `FTUE_V2` and the open world.
- The terrain heights of `FTUE_V2` in our world file (24 KB may hold no heightmap).
