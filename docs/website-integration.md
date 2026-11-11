# Connecting a website

The community website lives outside this repository. This page lists every request that passes
between a website, the game servers and the player's launcher, with the data formats. With it, a
site can show the world status, the leaderboards and the news. It describes the protocol only, so
any site that speaks it works.

## Who talks to whom

The site never opens a connection to the game machine, and the game machine opens no port for
the site. The game side and the launcher start every request:

| What | Who asks | Request | How often |
|---|---|---|---|
| World status | `status-reporter` | `POST <site>/api/ingest/status` | every 3 to 15 s (15 s by default) |
| Leaderboards | `status-reporter` | `POST <site>/api/ingest/leaderboards` | every 15 s or more (5 min by default) |
| Message of the day | `edge-http` | `GET <site>/api/game/cms/motd` | every 60 s |
| Where the news is | the player's launcher | `GET <login server>/launcher/news-source` | once at each start |
| Launcher news | the player's launcher | `GET <site>/api/news?target=launcher` | once at each start |

The launcher learns the site's address from the game server, not from its own settings. A player
types only the game server, and the server's `[website]` section decides which site's news the
player sees.

The site never reads a game database, a metrics page or a file on the game machine.

## Setup on the game side

There are two parts to set up: `status-reporter` for the status and the leaderboards, and the
`[website]` section of `edge-http` for the message of the day and the launcher news. They are
separate. A world can have one without the other.

### status-reporter

`status-reporter` runs once per world, on the game machine. Copy `config/status-reporter.toml` and
set:

- `world_id`: the `[[world]] id` of `config/edge.toml`. The site must know the world under the
  same id.
- `site_url`: the site, such as `https://example.org`, with no trailing `/`.
- `secret_env`: the name of an environment variable that starts with `NW_SECRET_`. Its value is
  the world's report secret, at least 32 characters (`openssl rand -hex 32`). The site keeps the
  same secret for that world.
- `edge_config`: the `edge-http` config of the world. The reporter reads the season end from its
  remote config sets (`javelin.leaderboard-season-end-datetime`), the same value the client gets.
- `[parts]`: the metrics pages of each server of the world. List every sim.

An environment variable with the prefix `NW_REPORTER_` overrides a value of the file, for
example `NW_REPORTER_SITE_URL=https://example.org`. Keep the live site address and the secret in
the environment of the game machine, not in a file you copy from another machine.

Run `content-tool leaderboards` once per client build. It writes the board table
`local-data/content/leaderboards-<build>.json` that the reporter reads.

### edge-http

Add this to the `edge-http` config for the message of the day and the launcher news:

```toml
[website]
site_url = "https://example.org"
```

`edge-http` reads its config only when it starts, so restart it after you add or change this
section. It refuses a `site_url` without `http://` or `https://`, with a trailing `/`, or on an
Amazon or CloudFront host.

Without `[website]`, the launcher shows no news and the message of the day stays the file in
`[cms]`. Nothing warns about this: the server just tells the launcher it has no news. A missing
section is the first thing to check when a post shows on the site but not in the launcher.

## Checking the link

Each check below needs only `curl` and the logs. Use your own domain in place of `example.org`.

The launcher news source:

```sh
curl https://login.example.org/launcher/news-source
```

The answer names the feed, `{"news_url":"https://example.org/api/news?target=launcher"}`. When
`news_url` is `null`, the `edge-http` config has no `[website]` section, or `edge-http` has not
been restarted since it got one. A `404` means the server is older than the news source.

The feed itself, as the launcher reads it:

```sh
curl https://example.org/api/news?target=launcher
```

A post must be in this list to show in the launcher. A post the site shows only on its own pages
is not in it.

The message of the day: the `edge-http` log says `pulling the message of the day` at start, and
`new message of the day from the website` when it gets a new document.

The reports: the `status-reporter` log says `report accepted` with `kind="status"` and
`kind="leaderboards"`. It logs that line for the first accepted report and for the first one
after a failure, not for every report, so a quiet log is normal. When sending starts to fail, it
logs `report not sent` once with the error. The site's status page shows whether reports still
arrive.

## Signed reports

Both reports are `POST` requests with `Content-Type: application/json` and three headers:

| Header | Value |
|---|---|
| `X-Blighted-World` | the world id, such as `8d0f4c2e-0000-4000-8000-000000000001` |
| `X-Blighted-Timestamp` | Unix seconds when the report was signed |
| `X-Blighted-Signature` | lowercase hex HMAC-SHA256 of `<timestamp>.<raw body>`, with the world's secret as the key |

The signed text is the timestamp in decimal, one `.`, then the body bytes exactly as sent. A test
vector: the key `key` over the text `1.x` gives
`fc4913c41a7bf793267e2a384f3261c9436ec2cd097bd745e7b0d99113f510ec`.

A site should check, in this order: the world id is known, the timestamp is within 60 s of its own
clock, the signature matches, and the same world, report kind and timestamp have not come before.
The reporter sends at most one report of each kind per second, so the replay check never refuses
a real report. Keep the game machine's clock in sync (NTP).

The reporter reads any `2xx` answer as accepted. It logs any other answer, or no answer within
`request_timeout_ms` (5 s by default), with the status and the start of the answer. It does not
send a failed report again, because the next one replaces it.

A site may limit what it takes. The reference site checks the body size and the time to send
the body while the body arrives, before the signature check, so a `408` or `413` can also answer
an unsigned request. It checks the reports per minute after the signature check. Its limits:

| Limit | Status report | Leaderboard report |
|---|---|---|
| Reports per minute and world | 20 | 4 |
| Body size | 64 KiB | 8 MiB |
| Time to send the whole body | 10 s | 10 s |

The reporter stays under the rate limits: it refuses a status interval under 3 s and a
leaderboard interval under 15 s.

The answers the reporter can get from the reference site:

| Answer | Meaning |
|---|---|
| `204` | accepted |
| `400` | the body is not valid JSON, or the site refused the leaderboard report: a board twice, a row for an unknown board, a rank outside 1 to 100 or a smaller rank number than the row before it, or a row without a name |
| `401` | unknown world, a timestamp outside the window, a wrong signature, or a replay |
| `408` | the body came too slowly |
| `413` | the body is too large |
| `429` | too many reports of this kind in the last minute |

## World status report

```json
{
  "parts": {
    "login": "up",
    "gateway": "up",
    "director": "up",
    "sims": "up",
    "social": "down"
  },
  "playersOnline": 12,
  "rttP99Ms": 87
}
```

- `parts`: one value per part, `up` or `down`. `login` is `edge-http`, `gateway` is
  `rep-gateway`, `director` is `world-director`, `sims` is every sim of the world, and `social` is
  the social server.
- A part is `up` when each of its metrics pages answers within the time limit with a `2xx`
  status and a `text/plain` (Prometheus text) page. An empty page counts as up, because a server
  that just started has counted nothing yet. No answer, an error status or another kind of page
  (such as HTML) makes the part `down`.
- `playersOnline`: the sum of `sim_players` over the sims that answered.
- `rttP99Ms`: the largest 99th percentile round trip of the gateway workers, in whole
  milliseconds. It is 0 without players or without the gateway. Bots send no ping echo, so only
  real clients move it.

A site can read it like this: the world is offline when `gateway` or `sims` is down, and has
problems when another part is down. A site that gets no report for three periods (45 s at the
default 15 s) should show that it has no signal.

## Leaderboard report

Each report holds every board of the game data, including boards without rows, and replaces all
boards of that world on the site. A board that leaves the report is gone.

```json
{
  "boards": [{
    "boardId": "WeekExpeditionClearTimeEnneadM1",
    "name": "Intermediate",
    "group": "The Ennead",
    "rotation": "week",
    "subgroup": "Expedition Clear Time",
    "valueKind": "Time",
    "lowerIsBetter": true,
    "regional": false,
    "entityKind": "group",
    "rotationId": "w178",
    "resetsAt": "2026-07-07T02:00:00Z"
  }],
  "rows": [{
    "boardId": "WeekExpeditionClearTimeEnneadM1",
    "rotation": "week",
    "rank": 1,
    "value": 1046.5,
    "names": ["Ann", "Bob"]
  }]
}
```

Board fields:

| Field | Meaning |
|---|---|
| `boardId` | the game data id (`LeaderboardId`). Together with `rotation` it names one board. |
| `name` | English board name (`DisplayName`). Many boards share a name inside one group, for example the difficulty levels of an expedition. |
| `group` | English group (`Category`), such as `War` or `The Ennead`. |
| `rotation` | `week`, `month` or `season`. |
| `subgroup` | English sub-group (`SecondLevelCategory`), such as `Expedition Max Score` or `Expedition Clear Time`. Left out when the game data has no text. Group, sub-group, name and `regional` together tell the boards apart. |
| `valueKind` | English label of the value column (`ValueString`), such as `Score`, `Time`, `Days`, `Wins`, `Ratio`, `K/D Ratio`. Left out when the game data has no text. |
| `lowerIsBetter` | `true` for boards that rank the lowest value first (clear times). |
| `regional` | `true` for boards that hold the players of every world (cross-world Outpost Rush, Capture The Flag and regional expedition boards). |
| `entityKind` | `character`, `company` or `group`: what one row ranks. |
| `rotationId` | the current rotation, as the client names it: `w<week>`, `m<month>`, `s<season>`. Left out before the first rotation starts. |
| `resetsAt` | when the current rotation ends, ISO 8601 UTC. Weeks start at the weekly reset hour of `[leaderboards]`, months at 00:00 UTC on the first day, and the season ends at the configured season end. Left out for a season without a valid end. |

Row fields:

| Field | Meaning |
|---|---|
| `boardId`, `rotation` | the board of the row. |
| `rank` | the place the game client shows, 1 to 100. Equal values share a rank and the next rank skips (1, 1, 3). |
| `value` | the value as the game client gets it, a number with decimals. `Time` values are seconds. Ratios keep their decimals. |
| `names` | at least one name. A character or company row has its name. A group row has the names of its members that this world knows, in member order. |

Rules the reporter follows, which a site can check:

- Rows come best first, so ranks never go down along a board. Each board has at most 100 rows.
- A row whose entity has no name in the world database is left out: a deleted character, a
  faction, or a player of another world on a regional board. Its place stays empty, so ranks can
  have gaps. A group row is left out only when none of its members has a name. The reporter never
  makes up a name.
- A value that is not a finite number is left out.
- When a database fails, the reporter sends no leaderboard report at all, because a partial one
  would empty the site's boards.

In build 25810808 the game data has 422 boards that need only a world and a rotation. The two
influence race boards also need a territory and a race id, so they are not in the report.

## Message of the day

The client shows the message of the day on the character select screen and asks the server for
it once a minute. With `[website]` set, `edge-http` asks the site just as often:

```
GET <site>/api/game/cms/motd
User-Agent: blighted-edge-http/<version>
If-None-Match: <the ETag of the last 200 answer>
```

The site answers `200` with the JSON document and an `ETag` header, or `304 Not Modified` when the
document has not changed. `edge-http` sends back the exact ETag text it got, quotes included.

The document has the shape of the retail document. The three top-level members must be objects:

```json
{
  "worlds": {
    "8d0f4c2e-0000-4000-8000-000000000001": {
      "announcement": "Servers restart at 04:00 UTC",
      "warning": "",
      "de-DE-announcement": "",
      "de-DE-warning": ""
    }
  },
  "globalAnnouncement": {
    "title": "Welcome",
    "description": "First line\nSecond line",
    "de-DE-title": "",
    "de-DE-description": ""
  },
  "worldSets": {
    "live": { "setName": "Blighted" }
  }
}
```

- `worlds` holds one object per world id. The ids are the `[[world]] id` values of the `edge-http`
  config.
- Each text has localized copies with the prefixes `es-ES-`, `es-MX-`, `fr-FR-`, `de-DE-`,
  `pl-PL-`, `it-IT-` and `pt-BR-` (the example shows only `de-DE-`).
- The client shows these texts as plain text. Its text fields for the message of the day and the
  global announcement have markup turned off, so `<` and `>` show as they are and HTML is not
  read. Use `\n` for a new line.
- The document must not name an image, because the message of the day has none.

`edge-http` keeps the last good copy (at start, the file in `[cms]`) when the site does not
answer within 10 s, answers an error, sends more than 1 MiB, or sends a document without the
three members. It logs when the pull starts and stops failing. It serves the copy to the client
with its own ETag.

News stories (`newsStories`) and menu tiles (`tileData`) still come only from the files in
`[cms]`. The client loads their pictures as DDS files from its image host, so it cannot use a
site's JPEG pictures there yet.

## Launcher news

A player types the game server, such as `example.org:8443`. The launcher puts `login.` in front
of the host and talks to `https://login.example.org:8443`. After it connects, it asks that server
where the news is:

```
GET <login server>/launcher/news-source
```

```json
{ "news_url": "https://example.org/api/news?target=launcher" }
```

`news_url` is `null` when the server has no `[website]`. An older server answers `404`. In both
cases the launcher shows no news and deletes its cached feed and pictures, so old news from
another source does not come back. `/launcher/info` does not change, so older launchers keep
working.

The launcher then reads the feed from the site:

```json
{
  "posts": [{
    "id": 41,
    "kind": "downtime",
    "title": "Maintenance on Tuesday",
    "bodyMarkdown": "The servers are **down** from 04:00 to 05:00 UTC.",
    "bodyHtml": "<p>The servers are <strong>down</strong> from 04:00 to 05:00 UTC.</p>",
    "pictureUrl": "https://example.org/media/41.jpg",
    "createdAt": "2026-11-09T12:00:00Z"
  }]
}
```

- `posts` come newest first. `kind` is `news`, `patch_notes`, `dev_update` or `downtime`. The
  launcher shows any other kind as a plain post.
- The launcher reads `bodyHtml` and ignores `bodyMarkdown`. It renders only the tags `p`,
  `strong`, `em`, `a`, `ul`, `ol`, `li`, `code` and `br`, and shows any other tag as text. It opens
  only `http`, `https` and `mailto` links. A site should send only those tags and link schemes.
- `pictureUrl` is an absolute address of a JPEG, or `null`. The launcher takes at most 4 MiB. It
  reads the picture only over `https` (or `http` on `localhost` and `127.0.0.1`), and never from
  an Amazon or CloudFront host. The same rules hold for the feed address.
- A picture may be at most 4096 px on each side. The launcher scales it to at most 960 px wide
  before it keeps it.
- `createdAt` is an RFC 3339 time.
- The feed may be at most 1 MiB.
- The launcher follows no redirect for the news source, the feed or a picture. Serve each of
  them at its own address. A redirect counts as offline.

The launcher shows the 5 newest posts. A `downtime` post younger than 24 hours is pinned at the
top. The launcher asks for the news once each time it starts, when it first connects to a server,
and asks again only when the player connects to another server. There is no timer, so a post
written while the launcher is open shows at its next start. Each request has a 5 s time limit.

The launcher keeps the last feed and the scaled pictures in its settings folder (`news.json`,
`news-pictures/<id>.jpg`) and shows them at the next start before it asks. Offline, it shows the
cached posts with the date of that copy, or "No news" without a copy. A feed that is not valid
JSON counts as offline. Login and Play never wait for the news.
