# Admin Studio S7: general quests (2026-09-18)

Step S7 (general quests, quest progress, quest editor) of the admin studio spec. Commits
cceb47c..a5b59b7 on main.

## What was built

- `QuestBook`: every objective of the content (after the design merge), its tree built from one
  task index when it starts. A player runs one quest at a time (the tracker of the
  objectives-state note shows one) and keeps a list of finished objectives.
- Start: from the GM page (`/api/gm/start-quest`, game master rank; the live page's "Start
  quest" for the selected player). A player in the tutorial, or with an active quest, is refused;
  a done quest starts again only when `Repeatable`. A player that leaves the tutorial gets the
  tutorial on its done list and starts `12_GetOffBeach`, as before.
- Completion: the tracker sends the retail end states, the objective joins the done list, and
  `ItemRewardName` x `ItemRewardQty` goes into the bag.
- Saves: the world database keeps the active run and the done list (`character_quest`,
  `world/0005_character_quests.sql`); the director sends them with each join, a handoff carries
  them, and the sim saves them when they change and when the player leaves, with the time taken
  (an older state loses). A saved run that no longer fits the quest's tree is dropped with a
  warning.
- Records page: an objective shows its task tree (`/api/studio/quest-tree/{objective}`); each
  task opens its record.

## Tests

| Test | Result |
|---|---|
| `nw-db` (infra): quests round trip, older state loses | pass |
| `sim`: start, refuse a second, timer finishes, reward in the bag, done list, repeatable | pass |
| `sim`: a saved run resumes at login; a run that does not fit the tree is dropped | pass |
| `sim` (infra): a leave saves the done list | pass |
| `world-director`: saved runs read, a bad one is dropped | pass |
| admin: GM start quest goes to the player's sim; quest tree with a loop and a missing task | pass |

## Live check (test world 2, port 9280)

| Step | Observed |
|---|---|
| Bot `Instbotaaaa` joins; GM "start quest" `12_GetOffBeach` | `done` |
| The same start again | `failed: the player already runs a quest` |
| Stop the bot | row `Instbotaaaa / 12_getoffbeach / active` with its run |
| Restart the stack, the same bot logs in, start again | `failed: the player already runs a quest` (the run resumed) |
| Quest tree of `12_GetOffBeach` | the container tree from `12_getoffbeach_container` |

The test bots `Instbotaaaa` and `Instbotbaaa` keep their saved quest rows (test characters).

## Unknown and not built

- **NPC quest offer: blocked.** The sim spawns no NPC (S4), and how the client shows an NPC is
  Unknown.
- **Experience and coin rewards: Unknown.** They come from `SuccessGameEventId` (game event
  rows); their amounts and client messages are not read.
- **Client display of a quest started this way: not checked live** (needs the game client). The
  tracker states are the ones the tutorial and `12_GetOffBeach` already send. A new quest (design
  `add` file) is not in the client's data; whether the client shows it is Unknown (Q1).
- Area tasks are edited as numbers on the records page; a map editor for them is not built.
