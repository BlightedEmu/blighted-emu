# Tutorial intro: achievements, facet ids, phasing and cutscenes

Date: 2026-09-10. Build: Steam `25810808`. Labels: **Observed**, **Inferred**, **Unknown**.

Sources: the retail capture
`local-data/captures/open-world/01-starting-zone-*/12-1_getoffbeach-Soul-Survivor.zip` (a new
character from login to the end of the tutorial), the client `Game.log` of our live run, client
slices and datasheets (`ftue_intro2_cinematic_trigger.dynamicslice`,
`javelindata_cinematicvideoregistry`, `javelindata_achievements`), and Ghidra on build `25810808`
(`<re-scratch>`).

## 1. Symptom

The real client loaded `FTUE_V2` (the burning ship) but played no intro video; movement and
Escape did nothing while the camera still turned. It never sent 6606 (`NotifyCinematicStarted`),
5181 (`OnAckLevelInfoChanged`) or 6745 (`ClientAckPhasingUpdate`).

## 2. What starts the intro

**Observed (slice).** The client-local trigger `FTUE_INTRO2_Cinematic_Trigger` stands 1.3 m from
the tutorial start. Its trigger volume turns on only when
`FTUE_Intro2_Cinematic_Unlock && !FTUE_Intro2_Cinematic_Watched` holds; it then plays the video
`FTUE_Intro2` (171 s) and the client sends 6606.

**Observed (capture).** The own player's `AchievementComponentReplicatedState` (type 3210,
component 66) in the first bundle is a bitset: bit `AchievementIndex`, byte `index / 8`. In the
retail first state bit 14804 (`FTUE_Intro2_Cinematic_Unlock`) is set and 14805
(`..._Watched`) is clear. Our server sent that component empty, so the trigger never turned on.

**Observed (client code).** On a cutscene start the client turns off the input maps `player`,
`movement`, `ping` and `ui_radial` and locks the menus; the camera maps stay on. That matches the
symptom. **Unknown:** which lock held our client without a cutscene start.

Achievement indexes (`AchievementData`, same in the capture build and `25810808`):
`FTUE_Intro2_Cinematic_Unlock` 14804, `_Watched` 14805, `FTUE_PCCM_Cinematic_Unlock` 15198,
`_Watched` 15199 (also Intro1 14802/3, Intro3 14806/7, Rez 14808/9).

## 3. Facet call ids

**Observed.** A facet call body starts with two big-endian `u64`: a target id and the entity. The
entity is the first 8 bytes of the `ClientAddEntryMsg` entry id in reverse byte order. The target
id minus the entity is a constant per component; for `ActionList` it is the same in three
separate retail sessions. The client logs "RMIDispatch: The facet target ID couldn't be found" and
drops a call with an unknown target: our `SyncTimeChange` (4246) and `SyncDayPhase` (4247), which
had random ids.

| Component | constant |
|---|---|
| `TimeComponent` | `d41eb32c0f154004` |
| `PlayerComponent` | `df0ce8c98d27619d` |
| `PlayerCutsceneComponent` | `d49931510e1dd23f` |
| `GridTrackingComponent` | `3946bc0c0a4083da` |
| `ActionListComponent` | `3c591a4ea3c2ad75` |

**Inferred:** the constant is the component's id in the player prefab; it is the same for every
character. **Unknown:** the constants of the combat facets (2071, 3601, 4299), which still carry
other ids.

## 4. Retail order

| t (s) | Direction | Message |
|---|---|---|
| 0.32 | S>C | 1635 `FTUE_V2` (client context 1) |
| 0.36 | S>C | first bundle: 3210 with the intro unlock |
| 5.04 | C>S | 6606 `ftue_intro2` (CRC-32 of the lowercase name), 2617 `playcinematicvideo` |
| 5.33 | S>C | 1635 again, same bytes |
| 5.34 | C>S | 5181: the client context of that 1635 |
| 5.36 | S>C | 6310: count, the 1635 phasing entries, a byte (3 tutorial, 2 open world; meaning **Unknown**), sequence 1 |
| 5.39 | C>S | 6745: the sequence |
| 7.09 | S>C | 3210 change: bit 15198 (`FTUE_PCCM_Cinematic_Unlock`) |
| 178.1 | C>S | 6542 `ftue_intro2` ended; S>C 3210 change: bit 14805 |
| 178.2 | C>S | 6703 `ftue_01_solesurvivor_confrontingcrewmember_igc`; S>C 6707 with the same id; C>S 6706 |
| 193.9 | C>S | 6705; S>C 6709 (header only); 3210 change: bit 15199 |
| 715 | C>S | 502 `ReturnToMainWorld`; S>C 3951 (flag 1), 6310 with no entries (byte 0), then the open-world level change |

A 3210 change is the list delta form: a count, a mask byte per group of 8 entries, and per entry
the byte index (wide), a version flag and version (wide, rising), and the new byte.

## 5. What the server sends now

Code: `nw_messages::{facet, achievements}`, `nw_content::achievements`,
`bins/sim/src/world/level_flow.rs`, `bins/sim/src/login.rs`.

- Facet ids of the time messages and of every new call: entity plus component constant.
- Tutorial players: the full achievement bitset with bit 14804 in the first card.
- 5 s after each 1635, the same 1635 again; on 5181 of that client context, 6310 with the 1635
  entries (byte 3 in the tutorial, 2 in the open world). In the tutorial, 5181 also sets bit 15198
  (retail sets it 1.8 s later; the trigger of that change is **Unknown**).
- 6542 and 6705 set the "watched" bit of the video or cutscene (Intro1 to 3, Rez, crew member).
- 6703 gets 6707 with the asked id; 6705 gets 6709.
- 502 in the tutorial: 3951 and an empty 6310, then the hand-off to the open world (the same path
  as the tutorial exit point). The phasing sequence goes on across the level change.

## 6. Unknown

- The 6310 byte's meaning; the 3210 first version value 3.
- The names of the other retail cutscene ids (`c723c42d`, `fc37fb55`, `af4c01c8`) and their
  achievement bits; this server grants them but sets no bit for them.
- What sets `Expansion_2023_Unlock` at 2.8 s, and the trigger behind client message 560.
- The live result: not yet seen with the real client.
