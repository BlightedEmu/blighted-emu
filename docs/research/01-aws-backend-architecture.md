# 01 - New World: AWS Backend Architecture (Public-Source Research)

Research date: 2026-08-29
Scope: public sources only. No client or server binaries were examined for this document.
Convention: each claim has a source link. **Unknown** = no public source found. "Unconfirmed" = seen only in a search-result snippet, a secondary source, or a low-authority source, and not verified against a primary page.

---

## 1. Summary

- The main primary source for the backend is the AWS for Games blog post "The Unique Architecture behind Amazon Games' Seamless MMO New World" (06 Apr 2022). It names the parts of the system and gives scale numbers. [S1]
- Topology per world: clients connect to one of **4 Remote Entry Points (REPs)** (EC2 `c5.2xlarge`, the only public IPs). Behind the REPs are **7 compute units** (EC2 `c5.9xlarge`) that together form a **"hub"**. The world map has a **grid overlay**; each grid cell is a physical simulation volume, and each hub owns **two non-consecutive grid subdivisions**. Player state is **handed off** between hubs as players move. Instanced content (expeditions, quest sessions) runs on a **separate shared pool of hubs**. [S1]
- Simulation runs at **30 updates per second** (compared with about 5 for a "traditional MMO", as the blog says). [S1]
- One world: **2,500 players**, more than **7,000 AI entities**, hundreds of thousands of objects. [S1] The launch cap was reported as **2,000** per world, later raised in steps of 250 (unconfirmed for the final value per world). [S9][S10][S11]
- Persistence: **Amazon DynamoDB** holds game state. About **800,000 writes every 30 seconds**. It scaled from single test servers to more than 500 servers in production. [S1]
- Microservices: **AWS Lambda** behind **Amazon API Gateway**. Global services: achievements, channels (chat), globally unique naming. Local (per-region or per-world) services: queues (login queue), contracts (very probably the trading post order book; unconfirmed), sessions, guilds (companies), characters. [S1]
- Telemetry: **Kinesis Data Streams -> Kinesis Data Firehose -> S3 -> Athena** (about 23 million events per minute). [S1]
- Werner Vogels' re:Invent 2021 keynote also named **ElastiCache, Elasticsearch (now OpenSearch), and Redshift**. Their exact roles in New World are **Unknown**. [S3]
- Game servers are written in **C++** (Amazon job postings for "SDE Game Server"). [S16] The engine is Amazon's in-house "Azoth" engine, derived from Lumberyard (the Azoth name is from an Amazon job post; the Lumberyard lineage is from secondary sources). [S17]
- Official shutdown: **31 January 2027**, all platforms. Announced **15 January 2026** in the official post "The Future of New World: What to Expect". In-game purchases ended **20 July 2026**. The post says nothing about private servers, offline mode, or server-file release. [S4][S5]

---

## 2. Findings by Sub-Topic

### 2.1 Primary sources about the backend

| ID | Source | Type | Notes |
|----|--------|------|-------|
| S1 | https://aws.amazon.com/blogs/gametech/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world | AWS for Games blog, 06 Apr 2022, authors "AWS for Games Content Team and Nicholas Walsh" | Main technical source. A Japanese translation exists: https://aws.amazon.com/jp/blogs/news/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world |
| S2 | https://amer.resources.awscloud.com/blog-web-assets-cloudfront/aws-re-invent-2021-keynote-dr-werner-vogels | AWS re:Invent 2021 keynote, Dr. Werner Vogels | The keynote ends with a New World segment. A full transcript was **not** found. |
| S3 | https://www.rackspace.com/blog/dr-werner-vogels-aws-reinvent-2021-keynote | Third-party recap of S2 | Gives the list of AWS services named in the keynote. |
| S18 | https://aws.amazon.com/jp/video/watch/c686143e6de | AWS Japan video "AGTC Session 5 - Building a MMO in the Cloud" | Panel about the New World backend (microservices, serverless). Date and speakers are not shown on the page. Content of the video was **not** reviewed. |

No GDC talk specific to the New World backend was found. The only GDC Vault hit was a 2017 Lumberyard talk ("Building a Scalable AAA Game Engine", https://gdcvault.com/play/1024276/Building-a-Scalable-AAA-Game), which is not specific to New World.

### 2.2 AWS services and their use

From S1 unless noted:

| AWS service | Use in New World | Source |
|---|---|---|
| Amazon EC2 `c5.2xlarge` | Remote Entry Points (REPs): 4 per world, the only public IP addresses; "resilience and security" | S1 |
| Amazon EC2 `c5.9xlarge` | Hub compute units: 7 per REP group, together form a "hub"; run the world simulation | S1 |
| Amazon EC2 (shared pool) | Session-based content (quests, expeditions): instances come from a shared hub pool and go back to it after the session | S1 |
| Amazon DynamoDB | Game state persistence; about 800,000 writes per 30 s; same code from 1 test server to more than 500 production servers | S1 |
| AWS Lambda | Microservices. Global: achievements, channels, globally unique naming. Local: queues, contracts, sessions, guilds, characters. More than 150 million invocations per minute | S1 |
| Amazon API Gateway | "Front door" for external applications; about 200 million calls per minute | S1 |
| Amazon Kinesis Data Streams | Telemetry from each world and player; hundreds of thousands of sources; sub-second latency | S1 |
| Amazon Kinesis Data Firehose | Delivers about 23 million events per minute to S3 | S1 |
| Amazon S3 | Event (telemetry) storage. Other S3 uses (for example patch or asset delivery) are **Unknown** | S1 |
| Amazon Athena | SQL queries on telemetry in S3 (for example, which monsters players fight most) | S1 |
| Amazon ElastiCache | Named in the re:Invent 2021 keynote. Role **Unknown** | S3 |
| Amazon Elasticsearch (OpenSearch) | Named in the re:Invent 2021 keynote. Role **Unknown** | S3 |
| Amazon Redshift | Named in the re:Invent 2021 keynote. Role **Unknown** (probably analytics; unconfirmed) | S3 |
| Amazon GameLift | **No** public source says New World used GameLift. The hub pool in S1 looks custom. Unconfirmed either way | search, no hit |
| Amazon Cognito | **No** public source. Login uses the "Amazon Games iD" with Steam / console account linking | S19, S20 |
| AWS Global Accelerator | **No** public source | search, no hit |
| SQS / SNS | **No** public source | search, no hit |
| CloudFront | **No** public source | search, no hit |

S1 also says hubs are "hosted globally in multiple AWS Regions". The exact AWS region for each game region is **Unknown**.

### 2.3 Server topology

**Per-world layout** [S1]
- Client -> one of 4 REPs (public IP) -> hub (7 compute units).
- The world is divided by a grid. Each grid cell is a physical simulation volume.
- Each hub maps to two non-consecutive grid subdivisions. (Possible reason: spread the load of adjacent hot areas; this reason is an inference, not stated in S1.)
- Player state hands off between hub instances when a player crosses the map.
- Tick: 30 state updates per second.
- Capacity per world (S1, 2022): 2,500 players, more than 7,000 AI entities, hundreds of thousands of objects, "millions of state changes per second".
- What is **Unknown**: the exact protocol between REP and hub, the grid cell size, whether the 7 units are 7 processes or 7 machines with several processes, how the hand-off is done (state transfer, ghosting, or replication across borders), and how a grid cell maps to a process.

**Server authority**
- Amazon said New World is **not** client-authoritative; the simulation is server-based. This was a reply on the official forums in October 2021, reported secondhand. The primary forum post was not retrieved (unconfirmed wording).
  - https://labs.invenglobal.com/articles/15527/it-consultant-responds-new-world-code-injection-exploit
  - https://www.gamestar.de/artikel/ist-new-world-client-authorative,3374817.html (403 on fetch; summary from search snippet only)
- The October 2021 "invincibility" exploit (drag the game window in windowed mode) made players think the game was client-authoritative. It was fixed in patch 1.0.3 / 1.0.4. [https://techraptor.net/gaming/news/game-breaking-new-world-invincibility-exploit-discovered] [https://pcgamesn.com/new-world/invincibility-exploit] [https://pcgamer.com/new-world-invincibility-exploit]

**Population cap and login queue**
- Launch cap reported as 2,000 players per world; one report says 2,200. [S9] https://massivelyop.com/?p=355633 ; https://www.wepc.com/gaming/new-world/new-world-server-queues-new-servers/
- Login queues reached about 25,000; after that size, new players got an error (Massively OP observation). [S9]
- The cap was raised twice by 250 to 2,500 (secondary guide; unconfirmed per world). [S10] https://newworldguide.de/posts/new-world-megaservers-are-impossible ; tests above 2,000 on some servers: https://mein-mmo.de/new-world-testet-server-kapazitaten/
- 2,500 matches the S1 number (April 2022). [S1]
- The login queue is a "local" Lambda microservice ("queues"). [S1]

**World sets and regions**
- Amazon said it would "open one new world in a new world set" for US-East, South America and Central Europe (launch 2021). A "world set" seems to be a group of worlds in a region. Amazon did not publish an exact definition (**Unknown**). [S9]
- Player-facing regions: US East, US West, Central Europe, South America, AP Southeast (Australia). https://www.pcgamer.com/uk/new-world-server-list/ ; https://www.newworld.com/en-us/news/articles/road-to-new-world-aeternum-servers
- Region-to-AWS-region mapping: **Unknown**.

**Splitters (2024)**
- Patch 5.0.2 (June 2024) added "splitters": separate copies of the game world to spread population (a form of layering/channels). Technical details **Unknown**. https://mein-mmo.de/new-world-patch-5-0-2-splitter-teaser-summer-game-fest-ankuendigung-konsole/

**Instanced content**
- Session content uses a separate shared hub pool. [S1]

**Cross-world and global services** [S1]
- Global microservices: achievements, channels, unique naming.
- Local microservices: queues, contracts, sessions, guilds, characters.
- Whether the trading post was per-world or per-region: per world in 2021 (player observation; no technical source). Its backend storage: **Unknown** (probably the "contracts" service plus DynamoDB; unconfirmed).

**Character transfer, merges, worlds**
- Character server transfers came with patch 1.0.3 (October 2021) and were paused after the "Character_Persist_Failure" bug (see 2.4). https://www.pcgamesn.com/new-world/gold-duping-bug
- Fresh Start worlds and transfer tokens; 11-server merge announcement (2023). https://massivelyop.com/?p=436391
- Later rounds, for example 18 PC servers merged into 10 (2024-2025, secondary sources). https://massivelyop.com/?p=531234
- New World: Aeternum (October 2024): existing worlds became "Legacy"; new "Standard" worlds; crossplay PC/PS5/Xbox Series X|S; characters locked to their platform; region transfers disabled; rules for transfers between Standard and Legacy. https://www.newworld.com/en-us/news/articles/road-to-new-world-aeternum-servers
- Seasonal worlds ("Season of Conquerors", 21 January 2025, always-on PvP from level 15; secondary source). https://icy-veins.com/forums/topic/83158-future-seasonal-worlds-in-new-world-aeternum
- Final year: "There will be no new content or further server merges for the remainder of the game's life." [S4]

### 2.4 Persistence model and incidents

**Known facts**
- DynamoDB stores game state; about 800,000 writes per 30 seconds (about 26,700 writes/s across the production fleet at the time of S1). [S1]
- An exact character save interval is **Unknown**. The "per 30 seconds" in S1 is a rate figure. It is not stated as a save interval.

**Incident: Character_Persist_Failure gold dupe (October 2021)**
- After patch 1.0.3 added server transfers, some characters got the error `Character_Persist_Failure`. Amazon described "an invalid data state that prevents their character from properly saving" (quoted by secondary sources). If such a player gave gold to another character and logged out, only the receiver's change was saved, so the sender kept the gold. Some normal players also lost progress on re-login. Transfers were paused.
  - https://www.pcgamesn.com/new-world/gold-duping-bug (21 Oct 2021)
  - https://massivelyop.com/?p=359153 (403 on fetch; summary from search snippet)
  - https://www.nme.com/news/new-world-gold-cheat-fix-causes-another-gold-cheat-3086260
- **Backend implication:** characters were saved as separate units. A trade between two characters was **not** one atomic transaction over both character records. A failed save of one side caused a dupe. An emulator must make value transfers atomic (one database transaction across both parties).

**Incident: wealth transfer shutdown (November 2021)**
- All wealth transfer was disabled on 1 November 2021 (trading post, player trade, company treasury, currency send). A second dupe used town upgrades: start a project, the project does not start, log out and in, the cost returns to the company treasury.
  - https://www.wepc.com/news/new-world-wealth-transfers-disabled-gold-dupe-exploit/
  - https://techraptor.net/gaming/news/new-world-gold-dupe-exploit-new-world-trading-disabled
  - https://www.newworld.com/en-gb/news/articles/new-world-update-1-0-5
- More than 1,200 bans. The first clean-up pass (2 November) removed more than 80% of duplicated value; 98% of the rest removed by 15 November. https://gamespot.com/articles/amazon-has-banned-more-than-1200-new-world-cheaters-following-economy-shutdowns/1100-6498082/
- **Backend implication:** Amazon could trace duplicated currency and items after the event. This suggests per-item or per-transaction logs (probably the Kinesis telemetry; unconfirmed).

**Incident: Central EU rollback (25 November 2021)**
- A "database change" meant to refund taxes lost in earlier outages credited up to 300,000 gold to EU players. Amazon took EU servers offline and **restored characters, companies and the trade post to 12:45 CET**, before the change. Players who transferred between 12:45 and 15:00 CET got `character_restore_failure`.
  - https://www.actugaming.net/?p=459154 (developer statement, French)
  - https://www.kitguru.net/gaming/matthew-wilson/amazon-rolls-back-new-world-servers-after-giving-out-too-much-free-gold/
- **Backend implication:** Amazon could restore a region's persistent data (characters, companies, trade post) to a point in time. DynamoDB has point-in-time recovery, but which method Amazon used is **Unknown**. Characters, companies and the trade post are separate data domains.
- House loss in earlier outages; houses re-enabled December 2021. https://massivelyop.com/2021/12/09/new-world-re-enables-buying-houses-lost-in-rollbacks-tweaks-winter-event-on-ptr/

**Incident: chat HTML injection (October 2021)**
- Chat text was rendered as HTML and was not sanitized. Players could post images, crash other clients, and (per one report) script a quest for gold. Amazon fixed it about 29 October 2021.
  - https://www.invenglobal.com/articles/15523/new-world-players-discover-potentially-game-breaking-code-injection-exploits
- **Backend implication:** the chat ("channels") service relayed raw text; the client UI renders HTML (the in-game UI is web-based; unconfirmed in this research). An emulator chat service must sanitize or escape text.

**Other**
- Server crash from cosmetic skins (February 2022). https://gamespot.com/articles/new-world-valentines-day-cosmetics-removed-following-server-crashing-bug/1100-6500486/
- Long maintenance for update 1.0.1 (6 October 2021). https://videogameschronicle.com/news/new-world-is-offline-due-to-technical-difficulties-rolling-out-its-1-01-update

### 2.5 Scale numbers

| Metric | Value | Source |
|---|---|---|
| Peak concurrent (first weekend) | more than 900,000 (fifth highest on Steam at that time) | S1 |
| Steam concurrent shortly after launch | more than 700,000 | https://massivelyop.com/?p=355633 |
| Players per world | 2,000 at launch -> 2,500 | S1, S9, S10 |
| AI entities per world | more than 7,000 | S1 |
| Objects per world | hundreds of thousands | S1 |
| Tick rate | 30 Hz | S1 |
| Production servers (DynamoDB context) | more than 500 | S1 |
| DynamoDB writes | about 800,000 per 30 s | S1 |
| Lambda invocations | more than 150 million per minute | S1 |
| API Gateway calls | about 200 million per minute | S1 |
| Telemetry to S3 | about 23 million events per minute | S1 |
| REP instance | `c5.2xlarge` (8 vCPU, 16 GiB) x 4 per world | S1; instance specs from AWS EC2 docs |
| Hub instance | `c5.9xlarge` (36 vCPU, 72 GiB) x 7 per hub | S1; instance specs from AWS EC2 docs |
| Network bandwidth per player | **Unknown** | - |
| Number of worlds at peak | **Unknown** exactly ("hundreds of servers" in press) | S9 |

Compute per world (from S1): 4 x 8 + 7 x 36 = 284 vCPUs for 2,500 players. This is arithmetic on S1 numbers. It assumes one hub per world; S1 does not say clearly whether a world has one or more hubs (**Unknown**).

### 2.6 Client network facts

- Official port list (Amazon support page, retrieved only as a search snippet; the page redirects to help.newworld.com, which returned 403): TCP 80/443 in and out; UDP 27000-27050 inbound; UDP 20000-59999 outbound. Unconfirmed by direct fetch.
  - https://www.amazongames.com/en-us/support/new-world/articles/port-forwarding
  - https://help.newworld.com/hc/en-us/articles/36341884420507-Port-Forwarding
- An EAC troubleshooting snippet mentions ports 18887-18903. Unconfirmed. https://help.newworld.com/hc/en-us/articles/36413457011355-Troubleshooting-EAC-Issues
- Anti-cheat: Easy Anti-Cheat (Epic). https://windowscentral.com/how-fix-new-world-unable-launch-due-eac-connection-error
- Account: Amazon Games iD (id.amazongames.com) linked to Steam / console accounts. [S19] https://www.amazongames.com/en-ca/support/new-world/articles/link-or-unlink-your-amazon-and-steam-accounts ; [S20] https://amazongames.com/en-ca/support/new-world/articles/game-progress-lost

### 2.7 Official shutdown

- October 2025: Amazon ended active development after company layoffs; Season 10 / Nighthaven was the last content; servers planned to stay online through 2026; at least six months notice promised before access changes.
  - https://www.videogameschronicle.com/news/new-world-is-officially-ending-development-following-huge-amazon-layoffs
  - https://www.notebookcheck.net/Amazon-ends-active-development-on-New-World-while-promising-servers-stay-live-till-2026.1152044.0.html
- 15 January 2026: official post. Quotes: "New World: Aeternum will officially be taken offline from all platforms on January 31, 2027." Delisted 15 January 2026. "Beginning July 20, 2026, players will no longer be able to purchase Marks of Fortune or anything else in game." No refunds for Marks of Fortune. No more merges. [S4] https://newworld.com/en-us/news/articles/the-future-of-new-world-aeternum-what-to-expect
- Press: [S5] https://www.videogameschronicle.com/news/amazon-games-confirms-mmo-new-world-has-a-year-left-before-it-goes-offline-permanently
- Private servers / emulators / server-file release / offline mode: the official post does **not** mention them. No Amazon statement found. One report says no offline mode is planned (secondary). A Change.org petition asks Amazon to release server files or license community servers (https://www.change.org/p/save-new-world). Facepunch's COO publicly offered USD 25 million to buy the game; no outcome reported (https://insider-gaming.com/rust-director-offers-25-million-to-keep-new-world-alive/).
- Legal position of an emulator: **Unknown**; outside the scope of this research.

---

## 3. Implications for a Non-AWS Emulator

Target: one normal dedicated server (or a small cluster), no AWS dependency. The emulator only has to reproduce what the **client** sees. The internal AWS layout is a guide for the service boundaries, not a requirement.

### 3.1 Service replacement map

| AWS part | Role in New World | Replacement on a dedicated server | Notes |
|---|---|---|---|
| EC2 REPs (4 per world) | Public client entry, connection fan-in | One gateway process (or one listener in the world server) | The client sees only the entry endpoint(s). The emulator must match the client protocol (transport, encryption, handshake). These details are **Unknown** from public sources and need client analysis. |
| EC2 hubs (7 x c5.9xlarge) + grid hand-off | Spatial world simulation, 30 Hz | One world-server process with in-process spatial partitioning (grid cells as data, not processes) | A preservation server with tens to a few hundred players does not need multi-process hand-off. Keep a cell abstraction so that more processes can be added later if needed. |
| Shared hub pool (sessions) | Instanced expeditions / quests | Instance manager in the world server (threads or child processes) | Start and stop instances on demand. |
| DynamoDB | Game state persistence | PostgreSQL (first choice: transactions, JSONB, point-in-time recovery with WAL archiving). ScyllaDB / Cassandra only if DynamoDB-style key-value at large scale is needed; it is not needed at this scale | Use one DB transaction for each value transfer (trade, trading post, treasury). This prevents the 2021 Character_Persist_Failure dupe class. |
| Lambda + API Gateway | Microservices (queue, contracts, sessions, guilds, characters, achievements, channels, naming) | One backend service process (modular monolith, HTTP/gRPC or in-process modules) | Keep the S1 service names as module boundaries. If the client calls HTTPS endpoints directly, the emulator must serve them at the host names the client expects (DNS / hosts-file redirect, local TLS certificate). Which client calls go over HTTPS is **Unknown**. |
| ElastiCache | Role **Unknown** (probably session/presence cache) | Valkey or Redis, or in-process cache | Optional at small scale. |
| Kinesis Streams + Firehose | Telemetry ingestion | Not required for game play. Optional: append-only log table in PostgreSQL, or NATS / Redpanda + file sink | The client may send telemetry; the emulator can accept and discard it. |
| S3 | Telemetry storage (other uses **Unknown**) | Local file system, or MinIO / Garage (S3-compatible) | Use MinIO only if the client or tools expect an S3-style API. |
| Athena / Redshift | Analytics | DuckDB or PostgreSQL queries | Not required for game play. |
| Elasticsearch / OpenSearch | Role **Unknown** | OpenSearch (self-hosted) or PostgreSQL full-text search | Only if a client-visible feature needs search (for example trading post search; unconfirmed). |
| Amazon Games iD / Steam / console auth | Account login | Local account service (username + password hash, or token). Optional Steam ticket validation through the Steam Web API | Console clients are probably out of scope. Login over Amazon's identity service cannot work after shutdown; the client login path must be redirected. |
| Easy Anti-Cheat | Client integrity | Not available after shutdown | The client may refuse to start or connect without EAC. This needs client analysis (**Unknown**). |
| Global region fleet | Multi-region worlds | One region, one or a few worlds | Not needed. |

### 3.2 Design lessons from the 2021 incidents

1. Make each economic action atomic across all parties (trade, trading post buy/sell, company treasury, territory projects). [2.4]
2. Validate all state changes on the server; do not trust client input (the server-authoritative model, as Amazon stated). [2.3]
3. Escape or sanitize all chat text before relaying it. [2.4]
4. Support point-in-time restore of all persistent data (PostgreSQL WAL archiving / base backups). [2.4]
5. Keep an audit log of currency and item creation and transfer, so that duplicated value can be traced. [2.4]

### 3.3 Capacity estimate

- The official world used about 284 vCPUs for 2,500 players at 30 Hz (arithmetic in 2.5). The server code was written for that fleet. A rewrite for one dedicated server must budget CPU for 7,000+ AI entities per full world map. For a small population, simulate AI only in cells near players (sleep idle cells).

---

## 4. Open Questions

1. Client-to-server transport and protocol: UDP or TCP, encryption, message format, handshake with the REP. Not public. Needs client analysis.
2. Which client features call HTTPS / Lambda microservices directly, and at which host names?
3. Exact login flow (Amazon Games iD token, Steam ticket, world list, queue).
4. Grid cell size and how the client is told about cell / hub changes (does the client reconnect on hand-off, or is this hidden behind the REP?).
5. Does a world have one hub (7 units) or more?
6. Character save interval and the DynamoDB data model (tables, keys, item storage).
7. Trading post design: per world or per region; is it the "contracts" service?
8. Roles of ElastiCache, Elasticsearch/OpenSearch and Redshift.
9. AWS region for each game region.
10. Exact definition of "world set".
11. Splitter (2024) technical design.
12. Can the client run without Easy Anti-Cheat?
13. Content of the AWS Japan "AGTC Session 5 - Building a MMO in the Cloud" panel (video not reviewed) and the New World part of the re:Invent 2021 keynote video (no transcript found).
14. Any Amazon statement on private servers or server-file release (none found as of 2026-08-29).

---

## 5. Source List

Primary (Amazon / AWS):
- [S1] AWS for Games blog, "The Unique Architecture behind Amazon Games' Seamless MMO New World", 06 Apr 2022: https://aws.amazon.com/blogs/gametech/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world
- S1 Japanese version: https://aws.amazon.com/jp/blogs/news/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world
- [S2] AWS re:Invent 2021 keynote, Dr. Werner Vogels: https://amer.resources.awscloud.com/blog-web-assets-cloudfront/aws-re-invent-2021-keynote-dr-werner-vogels
- [S18] AWS Japan video, "AGTC Session 5 - Building a MMO in the Cloud": https://aws.amazon.com/jp/video/watch/c686143e6de
- [S4] Official post, "The Future of New World: What to Expect", 15 Jan 2026: https://newworld.com/en-us/news/articles/the-future-of-new-world-aeternum-what-to-expect
- Official post, "Road to New World: Aeternum - Servers": https://www.newworld.com/en-us/news/articles/road-to-new-world-aeternum-servers
- Official patch notes, Update 1.0.5: https://www.newworld.com/en-gb/news/articles/new-world-update-1-0-5
- [S19] Amazon Games support, link/unlink Steam: https://www.amazongames.com/en-ca/support/new-world/articles/link-or-unlink-your-amazon-and-steam-accounts
- [S20] Amazon Games support, game progress lost: https://amazongames.com/en-ca/support/new-world/articles/game-progress-lost
- Amazon Games support, port forwarding (search snippet only): https://www.amazongames.com/en-us/support/new-world/articles/port-forwarding ; https://help.newworld.com/hc/en-us/articles/36341884420507-Port-Forwarding
- EAC troubleshooting (search snippet only): https://help.newworld.com/hc/en-us/articles/36413457011355-Troubleshooting-EAC-Issues
- [S16] Amazon job post, "SDE Game Server": https://www.amazon.jobs/en/jobs/2602197/sde-game-server
- [S17] Amazon job post, "Software Development Engineer, AZoth Game Engine": https://www.amazon.jobs/jobs/2577815

Secondary (press, recaps, guides):
- [S3] Rackspace recap of re:Invent 2021 keynote: https://www.rackspace.com/blog/dr-werner-vogels-aws-reinvent-2021-keynote
- [S5] VGC, shutdown confirmed: https://www.videogameschronicle.com/news/amazon-games-confirms-mmo-new-world-has-a-year-left-before-it-goes-offline-permanently
- VGC, development ended (Oct 2025): https://www.videogameschronicle.com/news/new-world-is-officially-ending-development-following-huge-amazon-layoffs
- Notebookcheck (Oct 2025): https://www.notebookcheck.net/Amazon-ends-active-development-on-New-World-while-promising-servers-stay-live-till-2026.1152044.0.html
- Inven Global, shutdown 2027: https://www.invenglobal.com/articles/20100/amazons-mmorpg-new-world-to-shut-down-in-2027-service-end-date-confirmed
- [S9] Massively OP, launch queues and cap: https://massivelyop.com/?p=355633
- WePC, new servers for queues: https://www.wepc.com/gaming/new-world/new-world-server-queues-new-servers/
- [S10] newworldguide.de, cap increases: https://newworldguide.de/posts/new-world-megaservers-are-impossible
- [S11] Mein-MMO, capacity tests: https://mein-mmo.de/new-world-testet-server-kapazitaten/
- Mein-MMO, splitters (patch 5.0.2): https://mein-mmo.de/new-world-patch-5-0-2-splitter-teaser-summer-game-fest-ankuendigung-konsole/
- PC Gamer, server list: https://www.pcgamer.com/uk/new-world-server-list/
- Massively OP, 11-server merge / Fresh Start transfers: https://massivelyop.com/?p=436391
- Massively OP, later merge round: https://massivelyop.com/?p=531234
- Icy Veins, seasonal worlds: https://icy-veins.com/forums/topic/83158-future-seasonal-worlds-in-new-world-aeternum
- PCGamesN, Character_Persist_Failure / transfers paused: https://www.pcgamesn.com/new-world/gold-duping-bug
- Massively OP, transfers halted: https://massivelyop.com/?p=359153
- NME, gold fix causes another dupe: https://www.nme.com/news/new-world-gold-cheat-fix-causes-another-gold-cheat-3086260
- WePC, wealth transfers disabled: https://www.wepc.com/news/new-world-wealth-transfers-disabled-gold-dupe-exploit/
- TechRaptor, gold dupe / trading disabled: https://techraptor.net/gaming/news/new-world-gold-dupe-exploit-new-world-trading-disabled
- GameSpot, 1,200 bans: https://gamespot.com/articles/amazon-has-banned-more-than-1200-new-world-cheaters-following-economy-shutdowns/1100-6498082/
- GameSpot, cosmetic server crash: https://gamespot.com/articles/new-world-valentines-day-cosmetics-removed-following-server-crashing-bug/1100-6500486/
- ActuGaming, EU rollback and database change: https://www.actugaming.net/?p=459154
- KitGuru, EU rollback: https://www.kitguru.net/gaming/matthew-wilson/amazon-rolls-back-new-world-servers-after-giving-out-too-much-free-gold/
- Massively OP, houses after rollbacks: https://massivelyop.com/2021/12/09/new-world-re-enables-buying-houses-lost-in-rollbacks-tweaks-winter-event-on-ptr/
- VGC, 1.0.1 maintenance: https://videogameschronicle.com/news/new-world-is-offline-due-to-technical-difficulties-rolling-out-its-1-01-update
- Inven Global, chat code injection: https://www.invenglobal.com/articles/15523/new-world-players-discover-potentially-game-breaking-code-injection-exploits
- Inven Global, "not client authoritative" update: https://labs.invenglobal.com/articles/15527/it-consultant-responds-new-world-code-injection-exploit
- GameStar, client authority (403 on fetch): https://www.gamestar.de/artikel/ist-new-world-client-authorative,3374817.html
- TechRaptor, invincibility exploit: https://techraptor.net/gaming/news/game-breaking-new-world-invincibility-exploit-discovered
- PCGamesN, invincibility fix: https://pcgamesn.com/new-world/invincibility-exploit
- PC Gamer, invincibility exploit: https://pcgamer.com/new-world-invincibility-exploit
- Windows Central, EAC: https://windowscentral.com/how-fix-new-world-unable-launch-due-eac-connection-error
- Change.org petition: https://www.change.org/p/save-new-world
- Insider Gaming, Facepunch offer: https://insider-gaming.com/rust-director-offers-25-million-to-keep-new-world-alive/
- GDC Vault, Lumberyard 2017 (not New World specific): https://gdcvault.com/play/1024276/Building-a-Scalable-AAA-Game
