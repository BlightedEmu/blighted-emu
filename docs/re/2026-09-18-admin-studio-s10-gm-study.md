# Admin Studio S10: in-game GM study (2026-09-18)

Step S10 of the admin studio spec: how the client asks for GM rights and sends GM commands.
Sources: the client decompile (`<re-scratch>/re-sweep`: `network.md`, `names.tsv`, the C text),
the facet code of this repo. No capture holds a GM message. Labels: **Observed** (seen in code or
data), **Inferred** (a reading of it), **Unknown**.

## GameMasterComponent messages (Observed: type indexes and names)

A ServerFacet message goes client to server; a ClientFacet message goes server to client.

| Type index | Message | Direction |
|---|---|---|
| 26 | `GameMasterComponentServerFacet_TeleportToPlayerByCharacterId` | client to server |
| 6765 | `GameMasterComponentServerFacet_TeleportPlayerToMeByCharacterId` | client to server |
| 2551 | `GameMasterComponentServerFacet_TeleportPlayerToCurrentLocation` | client to server |
| 4815 | `GameMasterComponentServerFacet_GiveGoldToPlayer` | client to server |
| 3246 | `GameMasterComponentServerFacet_ResetClaimOwnership` | client to server |
| 5413, 5419 | `ResetDailyWarCountForAllPlayers`, `ResetDailyWarCountForPlayer` | client to server |
| 3248 | `GameMasterComponentServerFacet_ResetPlaytestBroker` | client to server |
| 2668 | `GameMasterComponentClientFacet_TeleportToPlayerFailed` | server to client |
| 3253 | `GameMasterComponentClientFacet_OnWarDataReceived` | server to client |

- The component's type id is `{3D8AF697-3BB6-43A9-BAEC-0688D4AE9DFD}` (`FUN_1432695c0`). The
  `TeleportToPlayerFailed` handler looks the component up with it (Inferred from that handler).
- The client sender of `TeleportToPlayerByCharacterId` (`FUN_143225fc0`, Inferred from the
  factory next to the type registration) writes a string, 16 bytes and one flag byte (Inferred:
  a character id string, 16 bytes of Unknown meaning, a flag). Its only check is a non-empty
  string: **the client makes no permission check before it sends** (Observed).
- The `TeleportToPlayerFailed` handler (`FUN_14320cc70`) shows the minor notification "Teleport
  To Player Failed" (Observed).

## RequestGmPermissions and the GM flag

- `PlayerComponentServerFacet_RequestGmPermissions` is type 3956 (Observed). What sends it is
  **Unknown**: one candidate sender (`FUN_146686690`) writes two bytes; the link to 3956 is weak.
- No client facet message answers it (no `PlayerComponentClientFacet_*Gm*`) (Observed). The
  client shows "GM Client Enabled" in its `r_DisplayInfo` overlay when a flag of an object is set
  (`FUN_1417b8d60`, Observed); who sets that flag is **Unknown**.
- `isGameMasterClientMsg` is a field of the chat message (next to `senderName`, `isOwn`): it marks
  a chat line from a GM (Inferred), it is not a permission gate.
- No `/gm` console or chat command was found (Observed). Debug paths exist:
  `nw_teleportPlayerWithYaw`, `javelin.teleport-to-position`,
  `PlayerComponentServerFacet_DebugRequestTeleport` (3957),
  `DebugConsolePlayerComponentServerFacet_TeleportToPlayer` (6226). Which UI calls the GM senders
  is **Unknown**.

## Does the player entity need the component?

- Yes (Inferred): the client drops a facet call to a target it does not know, and the 2668 handler
  looks the component up on the entity. The server sends no component list: the client builds the
  player's components from its own `player.dynamicslice` (Observed). Whether that prefab holds a
  GameMasterComponent is **Unknown** (the prefab is not extracted).

## Server today

- The sim decodes facet messages with `ClientFacetMsg::decode` (`crates/nw-messages/src/facet.rs`)
  and logs every other type as "ignored client message" with its body (Observed). No GM message
  is decoded, and the facet code has no GameMasterComponent target constant.
- The sim can already teleport a player (`SimWorld::teleport_player`, used by the admin GM
  teleport) (Observed).

## Path to a GM teleport from the client (not built: blocked by Unknowns)

1. In a live session, find what UI sends type 26 (Unknown), then read its target and body in the
   sim's "ignored client message" log.
2. The logged target minus the player's facet entity gives the GameMasterComponent `Id` (or read
   it from `player.dynamicslice`).
3. Decode type 26 (and 6765, 2551) in `ClientFacetMsg`; the handler checks the sender's admin
   rank on the server (the client checks nothing), finds the character, and calls the teleport.
4. On a refusal, send 2668 to the sender (body: probably the facet header only; Unknown).

The admin site works without this path (spec section 8.5).
