# Death and respawn

Date: 2026-09-04.

Build: Steam build `22469132`, `Bin64/NewWorld.exe`, image base `0x140000000`.

This note decodes `PlayerComponentClientFacet_OnPlayerDeath` (3950), `PlayerComponentServerFacet_RequestRespawn` (1938), `VitalsComponentClientFacet_ClientSyncDeathRecap` (4299), and what a mob death sends.

The game was not run. Easy Anti-Cheat was not analyzed. No body bytes, names, ids, positions, tokens, or IP addresses are in this note.

Labels: **Observed**, **Inferred**, **Unknown**.

## Method

Same Ghidra settings as the mob-spawn note: heap `16G`, `-max-cpu 10`, ten decompiler threads, existing project, analysis off. REA `open_binary` was not called. No REA Evidence ID.

Capture checks: one catacombs session and one player-death session, with the reliable-frame join. The catalog name for type 4299 is `ClientSyncDeathRecap`. The task name matches that class.

## 1. Type 3950

**Observed.** Unmarshal `FUN_14658d070` (`0x14658d070`) calls `FUN_1425a1950` (`0x1425a1950`). The body is two `u64` big-endian (`FUN_1415095d0`) and one swapped `u32` (`FUN_14087a270`). That is 20 bytes when the `u32` is present.

**Observed.** Type 3950 was absent from the catacombs session and from the player-death session. Those sessions did contain type 4299 and type 1938.

**Observed.** Across all 695 ledgers the count of type 3950 is 0. There is no body to check to an exact end. The class name is a player message, and these captures do not contain it.

## 2. Type 1938

**Observed.** Unmarshal `FUN_1465ace90` (`0x1465ace90`) calls `FUN_146577f90` (`0x146577f90`). The body is two `u64` big-endian, one byte that must be 0 or 1, then `FUN_14669cc70` (`0x14669cc70`): a prefix count below 33, then that many bytes.

**Observed.** Five bodies. The prefix length is 1 byte. The count equals the number of bytes after it, so the reader ends on the last byte.

| Session | Bodies | Count value | Bytes after the count |
|---|---:|---:|---:|
| Catacombs | 2 | 22 | 22 |
| Catacombs | 1 | 23 | 23 |
| Player-death session | 2 | 22 | 22 |

Body length is 40 when the count is 22, and 41 when the count is 23. 16 + 1 + 1 + count = the length.

**Inferred.** This is the client asking to respawn. The count bytes are the request payload. Their meaning is not named in the reader. They were not copied.

**Observed.** Each type 4299 in the four fight sessions was paired with the next type 1938 on the same connection context. Four pairs. Lowest gap 2.423 s, median 2.545 s, highest 2.646 s. Five type 1938 bodies were decoded to the last byte, as in the table above.

## 3. Type 4299

**Observed.** Unmarshal `FUN_14656e940` (`0x14656e940`) calls `FUN_1465b38c0` (`0x1465b38c0`). After the two `u64` values it reads two strings (`FUN_14087a3d0`: a prefix length below `0x2ffff`, then that many bytes) and then `FUN_14669f120` (`0x14669f120`).

**Observed.** `FUN_14669f120` reads one flag byte that must be 0 or 1, one `u32` big-endian, two swapped `u32` values, one string, one `u16` big-endian (`FUN_14087a1c0`), a counted block (`FUN_1415bc800`), another `u16`, another counted block, and `FUN_14669cf60`.

**Observed.** Body sizes in the counted sessions: catacombs two bodies of 147 bytes and two of 116 bytes. The death session has one body of 152 bytes. The size changes with the strings, which matches two length-prefixed strings.

**Observed.** `FUN_145ce8b20` (`0x145ce8b20`) reads one `u32` big-endian and one `u16` big-endian, then calls `FUN_145b4f440` (`0x145b4f440`). That function reads one `u32`, one flag byte that must be 0 or 1, two more `u32` values, and two more flag bytes. It then calls `FUN_145b51330` (`0x145b51330`).

**Observed.** `FUN_145b51330` calls `FUN_142db7920` (`0x142db7920`) twice. Each call reads a prefix count below 6 and that many `u32` values. It then reads one flag byte and a prefix count below 4, then that many `u32` values. `FUN_14669cf60` reads one more `u32` after that call returns. The string bytes were not copied.

**Observed.** This reader was applied to all 280 type 4299 bodies in the 695 ledgers. 280 end on the last byte. Failures: 0.

**Observed.** The numeric words from this reader were compared with crc32 of local datasheet ids (ability id 702, weapon tag 32, creature id 8,673, vitals id 8,673, vitals category 14,111, item id 37,559, status-effect id 4,521). The `u32` in `FUN_145ce8b20` matched an item id on 89 of 280 bodies. The last `u32` of `FUN_14669cf60` matched a status-effect id on 81 of 280 bodies. Every other numeric field in this reader matched 0 bodies in every one of those sets. The ids were not printed.

## 4. What a mob death sends

**Observed.** A mob is a non-player entity. Its health is type 15 (`HealthAmount` in the vitals note). Its removal is one of the messages in the spawn note (types 566, 1517, or 1791). Type 3950 is a player class. Type 4299 is a vitals client message, so a mob can send it, and this pass did not show that a mob body is different from a player body.

**Inferred.** A player death on these captures is type 4299, then type 1938 about 2.5 s later. Type 3950 is not part of that pair here. Round 3 withdraws the claim that a mob does not send `HealthAmount`. Slot 0 child 0 is that field.

**Observed.** Gaps between two type 10 members that share a replication key, in the four fight sessions, when the gap is from 30 s to 2 hours: 13 gaps. Lowest 30.3 s, median 56.9 s, highest 159.6 s. None is near the 360 s slice cooldown.

**Observed.** A player-style pair is the next message on the same connection context. Type 4299 then type 1938 is that pair for a player. A mob respawn would be the next type 10 for the same replication key. Types 566, 1517, and 1791 occur 0 times, so there is no remove message to start that clock. The next type 10 on the same connection, with no shared replication key, was measured anyway: 157 gaps, lowest 0 s, median 2.154 s, highest 171.974 s. A gap of 0 s is another entity's spawn card in the same ledger time. It is not the dead mob.

**Withdrawn in Round 3.** This paragraph used to call the death-to-respawn time Unknown, and it said the measurement needs the running client. That is not the standing result. Current health is slot 0 child 0. The clocks and the reason they are not the 360 s asset are in Round 3.

## Files read

- `docs/re/2026-09-04-m6-mob-spawn-messages.md`
- `docs/re/2026-09-04-m6-vitals-state.md`
- Ghidra program `NewWorld.exe` in `<re-scratch>/ghidra`, read-only

## Round 3

**Corrected.** Section 4 said current health is not a wire field, and that a death-to-respawn time needs the running client. The vitals note, round 3, puts current health on slot 0 child 0. The clocks below were measured offline. They do not match the 360 s asset, for the reason in the inference at the end of this section.

### Asset side

**Observed.** The spawners note already records one point spawner: `m_minRespawnCooldown` and `m_maxRespawnCooldown` are both 360.0 seconds. A directory pass over the install paks found 20,342 paths whose names contain `pointspawner` and end in `.dynamicslice`. Every one of those entries uses a compression method other than store, so the zip reader cannot inflate them. This pass did not extract those slices. The only cooldown read from a slice remains 360.0 and 360.0 on that one asset.

**Observed.** The generated datasheet rows name `RespawnCooldownMod` on experience, `MinRespawnRate` and `MaxRespawnRate` on gatherables, and `DungeonRespawnsPermitted` on game modes. `VitalsData` has no respawn-time column. Those datasheet columns are not a mob's death-to-spawn delay.

### Capture side

**Observed.** World files under `local-data/content/` hold 118,706 vitals spawn points (115,937 of them in the open world). Catacomb `nw_catacomb_00` contributes 0 vitals spawns. Dungeon points in these files are mostly absent from the vitals section.

**Later finding.** Those counts came from the build 22469132 world files. Those files are deleted. `local-data/content/` now holds build 25810808 files only.

**Observed.** Child 0 of slot 0 (`HealthAmount`) went from above 5 to at most 0.5 on 100 samples that also had a recent position. 73 of those positions were within 20 m of a vitals spawn. 27 were not.

**Observed.** Type 10 cards seen while the walk was aligned: 39,475. A position on the same replication key, in the same bundle or within 5 s, was missing for 39,475 of them. On one catacombs ledger the same check is 365 card events, 1 distinct card key, 7 distinct position keys, overlap 0. The card key and the position key are not the same key in the members this walk reaches.

**Observed.** A second join treats the first parsed position of a key as a birth. Births: 5,262. Within 20 m of a spawn: 3,066. Paired with an earlier death at that spawn, on a different key: 1. The gap is 14.869 s.

**Observed.** The same key's `HealthAmount` later rose above 5 after that drop on 5,686 of 5,931 drops. 245 drops had no later sample above 5 before the ledger ended. The gaps run from 0 s to 1,861.265 s. The median is below 1 s (the 1-second histogram's middle sample is in the 0 bucket). 5,177 of 5,686 gaps are under 30 s. 19 of 5,686 are from 300 s through 420 s.

**Inferred.** That clock is not the spawner cooldown. A return in the same second is a dip through 0.5, or a heal, on a key that never left. It is not a new life at 360 s. The spawn-point join does not supply the missing 360 s cluster: it produced one gap, 14.869 s. These captures do not contain a death-to-next-life sample set that matches the 360 s asset. The asset value stands for the one opened spawner. The capture clocks measure a different event.
