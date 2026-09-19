# Admin Studio S5: saved items and the records page (2026-09-18)

Step S5 (saved items, item editor and item creator) of the admin studio spec. Commits
97533de..f6eb760 on main.

## What was built

- World database: `character_item` rows and `player_character.items_saved_at`
  (`world/0004_character_items.sql`). A save carries the time its list was taken; an older list
  loses.
- The director loads the saved items at a join and sends them in `Admit.items`; a tutorial join
  gets none, so the tutorial starts from the starter kit as before.
- The sim saves a player's items in the 10 s save round when they changed, and every item at a
  leave or a shutdown. The tutorial sim saves none. An item list still waiting to be saved wins
  over the database at an admit on the same sim.
- The records page (`/records`): every table kind of the retail bundle, searched by id or English
  name; an override file with only the changed fields; a new record from a base record.

## Tests

| Test | Result |
|---|---|
| `nw-db` (infra): items none until saved, place order, older list loses, empty bag stays empty | pass |
| `sim`: a change is taken for a save once; a leave saves all items; the tutorial saves none | pass |
| `sim` (infra): a leave stores an empty bag as saved, not as never saved | pass |
| `world-director`: saved items keep their places | pass |
| admin: record search and read with design files; records page served | pass |
| page tests: changed fields, override and add files | pass |

On test world 2 (port 9280) with the real retail bundle: the first search reads the bundle in
1.6 s; item names come from the English text (for example "Worn Longsword" for
`1hSwordT2_FTUE`).

## Not tested live

- **Items over logout with a bot.** The test bots make characters without an archetype pick, so
  they get no starter kit and no bag ("the player has no bag"). Items over logout need a game
  client: log in, pick up or get an item (GM give item), log out, log in.
- **Q1 (does the client show a server-side item change): Unknown.** The sim sends the gear score
  of the item definition (`gear_score_override`, else `min_gear_score`) in every gear state, so an
  override of `gear_score_override` reaches the client in the gear state. Whether the client
  shows that number, or its own datasheet value, needs the game client: change the gear score of
  the worn starter sword on `/records`, Apply, log in, and look at the sword's gear score. A new
  item (`add` file) is not in the client's datasheets; whether the client shows it at all is also
  Unknown.

## Notes

- Two sim infra tests (`a_forwarded_input_after_a_direct_one_is_dropped`,
  `a_killed_player_gets_a_4299_and_respawns_on_request`) already fail at 42aa8c4, before S4 and
  S5.
- The world database of both test worlds was migrated (`edge-http migrate`); servers built
  before this step refuse the newer schema.
