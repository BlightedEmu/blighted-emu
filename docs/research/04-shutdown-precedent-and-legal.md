# 04 - Shutdown, Emulator Precedent, and Legal Landscape

Research date: 2026-08-29. Every claim has a source URL. Items that we could not verify are marked **Unknown** or "unconfirmed". This document gives facts only. It is not legal advice.

---

## 1. Summary

- **Shutdown is confirmed.** Amazon Games announced on **2026-01-15** that New World: Aeternum goes offline on all platforms on **2027-01-31**. The game was delisted on 2026-01-15. Marks of Fortune (cash shop) sales stopped on **2026-07-20**. The Nighthaven season continues until shutdown. The "about January 2027" date in the task is correct: the exact date is **January 31, 2027**. [S1][S2][S3]
- **No offline mode, no server files, no source release, no private-server licence.** The official announcement does not mention these topics. [S1]
- **Earlier history.** Aeternum launched on PS5, Xbox Series X|S, and PC on **2024-10-15** [S8]. On **2025-10-28** Amazon said Season 10 (Nighthaven) is the last content update, after Amazon corporate layoffs of about 14,000 jobs [S5][S6]. An April 2024 Amazon Games layoff (over 100 people) did not cut the New World team at that time [S10][S11].
- **Community preservation exists but is early.** Three public technical projects exist: `nw-private-server/first-light` (Python, now "defunct", work moved to the "OpenWorld" Discord), `AScriver/NewWorldPreservation` (research workspace), and `Coldzer0/Aeternum-World` (AGPL-3.0 analysis tools, packet format notes). No project has reached in-world gameplay. [S20][S21][S22][S23]
- **Precedent shows the pattern that works:** (1) capture traffic and data before shutdown, (2) extract static game data from the client, (3) split the server into login/auth, world, and database services, (4) redirect the unmodified client with a launch argument, config file, or hosts file plus a local CA, (5) store state in MySQL/MariaDB/SQLite/PostgreSQL. NexusForever (WildStar) and MHServerEmu (Marvel Heroes) are the closest models for a modern client with HTTP/token-based login.
- **Legal risk is real.** The US DMCA 1201 exemption covers individual play of games whose server support is discontinued, but the 2024 rulemaking refused a broader remote-access exemption for libraries. The EU "Stop Destroying Videogames" initiative got 1,294,188 verified signatures, but on **2026-06-16** the Commission declined to propose a legal obligation. Amazon Games Terms of Use forbid reverse engineering and unauthorized emulation. Bnetd (2005), Scapegaming (2010), Nostalrius (2016), and Landmark (2017) show how publishers act. [S40]-[S55]

---

## 2. Shutdown facts (Topic A)

### 2.1 Timeline

| Date | Event | Source |
|---|---|---|
| 2021-09 | New World launches on PC (Steam app 1063730). AWS states >900,000 concurrent users in the first weekend. | [S30] |
| 2024-04-04 (memo date, year per context) | Amazon Games layoff of "over 100" people (Prime Gaming, Game Growth, San Diego studio). Christoph Hartmann said the Irvine New World team "will continue to grow". The "180 jobs" figure is **unconfirmed**. | [S10][S11] |
| 2024-03 | Commentary on a "content drought" and no 2024 expansion. | [S12] |
| 2024-06 | Aeternum revealed at Summer Game Fest, release date set. | [S9] |
| 2024-10-15 | New World: Aeternum launches on PS5, Xbox Series X\|S, and PC (Steam). Cross-play, no cross-progression. | [S8] |
| 2025-10 (early) | Season 10: Nighthaven goes live. | [S5] |
| 2025-10-28 | Official post "Update on New World": Season 10/Nighthaven is the last content update; "no longer sustainable" to continue new content; servers "operating through 2026"; at least six months notice before changes that affect play. Nighthaven and Rise of the Angry Earth made free. Coincides with Amazon layoffs of about 14,000 corporate jobs, including Amazon Games Orange County / San Diego. | [S4][S5][S6] |
| 2025-10-28 to 2025-10-31 | Developers' in-game farewell note in Nighthaven is publicised. | [S7] |
| 2026-01-15 | Official post "The Future of New World: What to Expect": delisting on that day; servers offline 2027-01-31; Nighthaven extended to the end; Marks of Fortune sales end 2026-07-20; no refunds for Marks of Fortune. Same post on Steam news. | [S1][S2] |
| 2026-01-15/16 | Delisted from Steam, Xbox Store, Microsoft Store. | [S2][S3] |
| 2026-07-20 | In-game store (Marks of Fortune and "anything else in game") closes. | [S1] |
| **2027-01-31** | **Servers taken offline on all platforms.** | [S1] |

Discrepancies found:
- `first-light` README says "~Dec 2026". This is wrong. The official date is 2027-01-31. [S20][S1]
- Checkpoint Gaming reported a June 20 currency cut-off. Official text says July 20, 2026. [S13][S1]
- One outlet said the content wind-down was announced in November 2025. Official post and most outlets say late October 2025. [S14]

### 2.2 What happens to purchases (from the official post [S1])
- Players who own the game can still download and reinstall it until shutdown, and can play on the platform of purchase (PlayStation, Xbox Series X|S, Steam).
- "refunds are not available for Marks of Fortune purchases."
- Recent buyers must contact the support team of the platform where they bought the game.
- No new content and no more server merges. Bug and performance monitoring continue. World bosses and bonus weeks continue until shutdown.
- After 2027-01-31 the game is not playable. Achievements are discontinued. [S15]

### 2.3 Offline mode, private servers, source/tool release
- The official post does **not** mention offline mode, private servers, server binaries, source code, or tools. [S1]
- **Unknown:** any later Amazon statement on these topics. We found none up to 2026-08-29.
- An unverified rumour (screenshot in a GamingOnLinux comment) says Facepunch offered Amazon money to buy the game. **Unconfirmed.** [S3]

### 2.4 Server architecture facts that matter for emulation (official AWS blog [S30])
- Each world has **four Remote Entry Points (REPs)**, each a C5.2xlarge EC2 instance; REPs are the only public IPs.
- Behind them, a **hub** of seven C5.9xlarge instances; hubs map to grid subdivisions ("each grid representing a physical simulation volume"); state hands off between hub instances as players move.
- Simulation at **30 updates per second**.
- Per world: >7,000 AI entities, hundreds of thousands of objects, 2,500 players.
- Persistence in **Amazon DynamoDB** (~800,000 writes per 30 s).
- **AWS Lambda microservices**: global (achievements, channels, unique naming) and local (queues, contracts, sessions, guilds, characters). >150 million invocations/min.
- **API Gateway** front door (~200 million calls/min).
- Telemetry: Kinesis Data Streams -> Firehose -> S3 -> Athena.

Note: the community name "Javelin REP" (first-light) matches the AWS term "REP". The meaning of "Javelin" is **Unknown** (possibly an internal network layer name; unconfirmed).

### 2.5 Player and community reaction
- Change.org "Save New World" petition asks Amazon to release dedicated server files or license vetted community operators, plus documentation and tools. Reported >30,928 signers at capture; target 50,000. Live count: **Unknown**. [S16][S17]
- A second petition asks to "Reinstate AGS' New World: Aeternum team". [S18]
- Steam forum threads ask for server files or a dedicated server option, and discuss private servers. [S19]
- Press and commenters connect the shutdown to the Stop Killing Games movement. [S3]
- A German outlet speculates that fans may build a server emulator "as was done with The Crew". [S24]

---

## 3. Existing New World preservation efforts

| Project | URL | Language / tools | Status (as read 2026-08-29) | Notes |
|---|---|---|---|---|
| first-light | https://github.com/nw-private-server/first-light | Python 3.11/3.12, Frida, pyOpenSSL, python3-dtls, pytest, Ghidra notes | Banner "THIS REPOSITORY IS DEFUNCT"; active work moved to "OpenWorld" Discord (invite in README). Status 2026-05-11: Gate 1 auth flow (HTTPS / OmniSDK / character creation) **Complete**; Gate 2 "Javelin REP - DTLS handshake + V3 registration" blocked (client re-sends V3 until session drops); Gates 3-4 (world streaming, movement/replication) not started. | Redirects "Amazon's auth hostnames" to 127.0.0.1 via hosts file; self-signed CA added to Windows ROOT store; HTTPS auth mock on 443; REP stub on **UDP 24083 over DTLS**; uses Frida to bypass client certificate pinning; needs non-EAC launch path; Steam must run and be logged in. GridMate reference doc. Licence: none detected. [S20][S21] |
| NewWorldPreservation | https://github.com/AScriver/NewWorldPreservation | PowerShell 7, Python 3.11/3.12, uv | Research workspace. Stock client 1.400.6031.6004151 (Steam build 22469132) reached a local HTTPS server on 443 (`GET /STEAM_APP_ID.1063730.json`, TLS 1.2), made five private HTTP requests, reached DTLS, then got fatal `unknown_ca` alerts. No login yet. | Principle: keep the owned stock client, replace only the backend; no memory inspection, no binary patching, no EAC changes. A cert-data-only modified client was refused by EAC ("Unrecognized game client"). Milestone 1: two accounts, one map, mutual movement. Licence not stated. [S21] |
| Aeternum-World | https://github.com/Coldzer0/Aeternum-World | Analysis tools (packet capture helper, type-index class mapper, Lua decompiler, OodlePak extractor) | Analysis only, client v1.400.6031.40375 | **AGPL-3.0.** Packet doc: GridMate carrier over **DTLS/UDP**; 4-byte datagram header (mode, format, 16-bit seq); optional **LZ4** block compression; channels 0 (reliable events/RMI), 1 (replication/state bundles), 3 (system); AZ-Reflect envelope with `type_idx` (0 = inline UUID; 8 = state bundle); LEB128 varints; mostly big-endian; presence bitmaps; compressed positions/quaternions. [S22][S23] |
| New-World-Private-Server-Source (siaginw) | https://github.com/siaginw/New-World-Private-Server-Source | Reported Rust + SQLite | Search index described an unfinished Rust server; URL returned **HTTP 404** on 2026-08-29. Current status **Unknown**. | [S25] |
| Data tools | https://github.com/Kattoor/nw-datasheet-reader ; https://github.com/MontagueM/NewWorldUnpacker ; https://github.com/Kattoor/aeternum-map | Datasheet reader, .pak unpacker, map | Client-data tools; current compatibility **Unknown**. `nw-buddy` (by "Ginie") is referenced by Aeternum-World; its repo URL was not confirmed (404 at guessed path). | [S26][S22] |
| RaGEZone thread "[New World] Emulator" | https://forum.ragezone.com/threads/new-world-emulator.1257186/ | Forum | 403 on fetch; search summary says users see Lumberyard backend as hard to emulate. **Unconfirmed.** | [S27] |
| mmotopforge "New World private servers" list | https://mmotopforge.com/new-world | Listing site | **Unverified**; treat as unaffiliated. | [S19] |

Communities: Steam discussions [S19]; Change.org petitions [S16][S18]; savenewworldaeternum.com (fetch failed, content **Unknown**); "OpenWorld" Discord (named in first-light README; we did not join it; member count and activity **Unknown**).

---

## 4. Emulator precedent

### 4.1 Precedent table

| Game (shutdown) | Project | Language / stack | Login/auth handling | Client redirection | Database | How logic was rebuilt | Licence / legal status |
|---|---|---|---|---|---|---|---|
| World of Warcraft (live) | TrinityCore | C++ | 3.3.5: `authserver` + `worldserver`; master: `bnetserver` (with LoginREST) + `worldserver` | 3.3.5: `realmlist.wtf` `set realmlist <ip>`; modern clients: Arctium WoW Client Launcher, Battle.net certificate needed for external hosts, `Config.wtf` portal | MySQL: `auth`, `characters`, `world`, `hotfixes` (newer branches) | Clean-room from packet logs and client data (DBC/DB2) | GPL-2.0 [S31][S32][S34] |
| World of Warcraft 3.3.5a | AzerothCore | C++ | authserver + worldserver (same lineage) | `realmlist.wtf` | MySQL (`data/sql`) | Fork of MaNGOS -> TrinityCore -> SunwellCore (2016); module system | GPL-2.0 [S33][S35] |
| EverQuest (live) | EQEmu | C++, Perl and Lua quest scripting | Separate `loginserver`, plus `world`, `zone`, `ucs`, `queryserv`, `shared_memory` | **Unknown** for exact method (commonly `eqhost.txt`; unconfirmed) | MySQL/MariaDB, >200 tables (ProjectEQ content) | "packet sniffing, client disassembly, and in-game testing"; "No proprietary code or server sources were ever used" | GPL-3.0 [S36] |
| WildStar (2018-11-28) | NexusForever | C# / .NET 10; RabbitMQ (Rebus) or Azure Service Bus; optional .NET Aspire | `AuthServer` + `StsServer` (emulates Carbine/NCSoft Secure Token Service) + `WorldServer`, `ChatServer`, `GroupServer`, `CharacterAPI` | ClientConnector starts `WildStar64.exe` with `/auth <host> /authNc <host> /lang <lang> /patcher <host> /SettingsKey WildStar /realmDataCenterId 9`; no patching, no injection; also a GUI launcher | MySQL/MariaDB via EF Core migrations: auth, character, world (`nexus_forever_world`) | Client tables and map extraction (`MapGenerator`, NFMAP formats); packet details **Unknown** | AGPL-3.0; no donations; no support for public hosts [S37][S38][S39] |
| Marvel Heroes (2017) | MHServerEmu | C# / .NET 8; Docker images available | Built-in web frontend on 8080 (`/AuthServer` login, dashboard); game frontend on TCP 4306 | Launch arg `-siteconfigurl=localhost/SiteConfig.xml`; Apache (mod_ssl, mod_proxy, mod_rewrite) on 443 serves SiteConfig.xml and proxies `^/AuthServer(.*)` to `localhost:8080`; self-signed cert; `ClientConfig.xml` has `SiteConfigLocation` defaults to `cdn.marvelheroes.com` / `auth.marvelheroes.com` | SQLite `Account.db` (JSON option), periodic backups | Protobuf message definitions (`proto` folder) and game data `.sip` files from client; README claims all 2017 systems restored | AGPL-3.0 [S40][S41][S42][S43] |
| Star Wars Galaxies (2011-12-15) | SWGEmu Core3 | C++; Core3 (open, AGPL) on Engine3 (proprietary); CORBA-derived ORB for distributed objects | Login server (port 44453), status 44455, ping 44462, world 44463, ORB 44419 | SWGEmu Launchpad lets user add a login server (IP:port); exact config file keys **Unknown** | MySQL/MariaDB plus Berkeley DB object store (historical) | Clean-room from packet logs and TRE client data; `conf/config.lua` | AGPL (Core3) [S44][S45][S46] |
| Star Wars Galaxies NGE | ProjectSWG (NGECore2, Holocore) | Java (JDK 8 for NGECore2); service tree, intent/event bus | **Unknown** detail | **Unknown** | PostgreSQL (NGECore2) | TRE Explorer client data extraction | NGECore2: LGPL core + closed engine, 30-minute / 5-connection limit [S47][S48] |
| Star Wars Galaxies (leaked) | SWG Legends | Reported to use leaked SOE NGE code; disputed | **Unknown** | **Unknown** | **Unknown** | Leaked source (community claim, **unconfirmed**) | Unlicensed; Aug 2026 asset-decryption dispute [S49] |
| City of Heroes (2012-11-30) | Homecoming (and Ouroboros/"Volume 2") | Original NCSoft/Paragon server code (leaked to public in 2019); component names (dbserver, mapserver, auth) **unconfirmed** | Original auth server code | Launcher with manifest (Homecoming in-house launcher is part of licence; CreamSoda for Volume 2) | **Unknown** (often reported as MS SQL Server; unconfirmed) | Not rebuilt: original code | Homecoming got official NCSoft licence January 2024 (closed source, NCSoft can revoke) [S50][S51][S52] |
| Warhammer Online (2013-12-18) | Return of Reckoning (from WarEmu) | C# (per WarEmu lineage; **unconfirmed** for current code) | **Unknown** | `RoRLauncher.exe` (.NET single-file bundle, inferred from Lutris script) | **Unknown** | Players archived network packets before shutdown; "reimplementation of the Warhammer Online protocol"; no original server code used | Run by "Return of Reckoning Ltd"; no publisher licence reported [S53][S54] |
| The Crew (2024-03-31) | The Crew Unlimited | **Unknown** | Emulated Ubisoft backend | Launcher patches the game install to connect to emulated server; offline mode = local server on player PC; online public server | **Unknown** | Team logged network traffic before shutdown; "save dumper" tool used before shutdown | Requires legitimate copy; not affiliated with Ubisoft [S56][S57] |
| Anthem (2026-01-12) | Revival (And799, "Cenotaph") | **Unknown** | **Unknown** | Proof-of-concept local server video (2026-01) | **Unknown** | Research only; ex-developer says local server code existed internally | Early stage [S58][S59] |
| Tabula Rasa (2009) | Infinite Rasa (historic) | **Unknown** | **Unknown** | **Unknown** | **Unknown** | Forum mention only (2011) | No current repo found [S60] |
| EverQuest Next Landmark (2017-02-21) | none | - | - | - | - | - | Daybreak: will "not license or authorise the operation of a Landmark emulator or a fan-operated Landmark server" [S61] |

### 4.2 Details per project

**NexusForever (WildStar) - closest structural model.**
WildStar used a separate Secure Token Service (STS) for login, then an auth server, then a world server. NexusForever re-implements each as its own .NET process (`NexusForever.AuthServer`, `NexusForever.StsServer`, `NexusForever.WorldServer`, plus Chat, Group, CharacterAPI) with a shared message broker (RabbitMQ via Rebus, or Azure Service Bus) and three EF Core database contexts (Auth, Character, World) on MariaDB/MySQL [S37][S38]. The client is redirected only by command-line arguments that the official client already accepts (`/auth`, `/authNc`, `/patcher`), so no binary patch is needed [S39]. It supports one exact client build (16042, final patch). Static game data comes from client tables and map extraction [S38]. Lesson: a cloud-era client with separate token, auth, and game endpoints can be served by small local services if the client already reads endpoint hosts from args or config.

**MHServerEmu (Marvel Heroes) - model for HTTP/web login behind TLS.**
The client fetches `SiteConfig.xml` over HTTPS, then logs in at `/AuthServer`. The emulator uses a local Apache with a self-signed cert to serve SiteConfig and reverse-proxy login to the emulator's built-in web frontend [S41][S42]. Persistence is one SQLite file with automatic backups [S43]. Lesson: a reverse proxy plus a local CA can stand in for cloud HTTPS endpoints; SQLite is enough for a small preservation server.

**TrinityCore / AzerothCore (WoW).** Two-tier split: an account/auth service and one or more world servers, all on MySQL with separate auth, characters, world (and hotfixes) databases [S34]. Old clients are redirected by a plain text config file; newer clients need the external Arctium launcher and a matching certificate [S34][S35]. Lesson: put the content database (static, versioned SQL) apart from the characters database (dynamic).

**EQEmu.** Many process types (login, world, zone per area, chat `ucs`, `queryserv`, `shared_memory` loader). Quest logic is in Perl/Lua scripts, not C++ [S36]. Lesson: a script layer lets contributors add content without core rebuilds.

**SWGEmu Core3.** Splits game logic (Core3, AGPL) from a distributed object engine (Engine3, proprietary, CORBA-like ORB). Historic Berkeley DB object store plus MySQL [S44][S45]. Lesson: a closed engine part limits community growth (same for NGECore2's 30-minute limit [S47]).

**City of Heroes Homecoming.** Not an emulator: runs original server code that leaked in 2019. It later got an official licence from NCSoft (January 2024), with conditions: closed source, own launcher, NCSoft may request shutdown at any time [S50][S51]. Lesson: a licence is possible, but it came after years and only to one team.

**Return of Reckoning.** Players archived packets before the 2013 shutdown; development started January 2014 as a protocol re-implementation with no original code [S53]. Lesson: captures before shutdown are necessary.

**The Crew Unlimited.** Team formed within days of the shutdown notice, logged traffic, and shipped a save-dumper tool before shutdown so players could keep progress [S56][S57]. Release 1.0 (September 2025) was offline-first (local server on the player PC) [S57]. Lesson: a single-process "local server" mode is a strong first deliverable.

### 4.3 Emulators of cloud-backend games: how cloud services were replaced

| Cloud dependency | How projects replaced it | Example |
|---|---|---|
| HTTPS config/bootstrap URL (CDN) | Local web server serves the same file; launch arg or config file points to it | MHServerEmu `-siteconfigurl` [S41]; NW `GET /STEAM_APP_ID.1063730.json` seen by NewWorldPreservation [S21] |
| Token service / OAuth-like login | Local stub that issues tokens it also validates | NexusForever `StsServer` [S37]; first-light `auth_mock.py` (HTTPS 443) [S20] |
| Hard-coded hostnames + TLS | Hosts file to 127.0.0.1 plus local CA in OS trust store | first-light, NewWorldPreservation [S20][S21] |
| Certificate pinning / anti-cheat | first-light used Frida to bypass pinning and skipped EAC launch chain; NewWorldPreservation refuses binary/memory changes and found EAC rejects modified files | [S20][S21] |
| Managed NoSQL (DynamoDB) | Relational or embedded DB (MySQL/MariaDB, SQLite, PostgreSQL) | All projects in 4.1 |
| Serverless microservices (Lambda + API Gateway) | In-process services or a few processes over a message broker | NexusForever RabbitMQ/Rebus [S38] |
| Distributed simulation hubs | Single process or few processes on one host | MHServerEmu single server; Crew Unlimited local server [S43][S57] |

---

## 5. Lessons for our design

1. **Target one exact client build.** Every mature project pins one build (WildStar 16042, MH 1.52, WoW 3.3.5a 12340). For New World, the community uses 1.400.6031.x [S21][S22]. Archive this build and its Steam manifest before 2027-01-31.
2. **Capture live traffic and account state now (before 2027-01-31).** RoR and The Crew Unlimited depended on pre-shutdown captures [S53][S56]. Note: New World game traffic is DTLS-encrypted [S23], so plain packet captures need key logging or decrypted capture points; HTTPS login flows also need capture. Legal limits apply (section 6).
3. **Do not patch the client if possible.** NexusForever needs no patch [S39]. For New World, EAC rejects modified files [S21]. Preferred path: hosts file + local CA + stock client. Open blocker: DTLS `unknown_ca` (likely pinned or private trust store) [S21].
4. **Service split that fits one dedicated server:**
   - `bootstrap-http`: serves `/STEAM_APP_ID.1063730.json` and other config files (replaces CDN/API Gateway).
   - `auth`: replaces the token/login HTTPS endpoints (first-light calls this "OmniSDK" flow) [S20].
   - `rep-gateway`: DTLS/UDP entry (port 24083 seen by first-light) [S20]; one REP instead of four.
   - `world`: one simulation process for one world, 30 Hz tick to match the client expectation [S30].
   - `services`: in-process modules for guilds, contracts (trading post), queues, characters, names, achievements (list from AWS blog) [S30].
   - `db`: PostgreSQL or SQLite in place of DynamoDB.
5. **Separate static content DB from player DB** (TrinityCore pattern). Load static data from client datasheets (tools: Kattoor datasheet reader, Aeternum-World OodlePak) [S22][S26].
6. **Offer an offline/single-player mode first** (The Crew Unlimited pattern) [S57], then multiplayer.
7. **Licence choice:** most emulators use GPL/AGPL. Aeternum-World is AGPL-3.0; first-light has no licence, so we must not copy its code [S21][S22]. Clean-room documentation reduces risk.
8. **Do not distribute game files.** NexusForever, MHServerEmu, Crew Unlimited all require the user's own legitimate client [S38][S41][S57].
9. **No monetization.** NexusForever "will never ask for donations" [S39]; Scapegaming (monetized) got an $88.6M default judgment [S72].

---

## 6. Legal landscape (facts only, not legal advice)

### 6.1 United States - DMCA Section 1201
- The current exemption class (37 C.F.R. 201.40) is described on the Copyright Office renewal form as: "Video games for which outside server support has been discontinued, to allow individual play by gamers and preservation of games by libraries, archives, and museums ... and discontinued video games that never required server support, for preservation by libraries, archives, and museums." [S62]
- The exemption for individual play covers **local gameplay** of games where the copyright owner stopped access to an external **authentication** server; "local gameplay" means play on a PC or console, or locally connected PCs/consoles, "not through an online service or facility" (quoted in a 2021 comment). Exact current paragraph numbers changed between cycles; verify on eCFR (fetch blocked). [S63][S64]
- Library/archive preservation of server-dependent games was added in 2018 and is limited to physical premises (per commentary). [S64][S65]
- **2024-10-25:** The Register of Copyrights refused a new exemption (Software Preservation Network, supported by Video Game History Foundation) for **remote access** to preserved games by researchers. ESA opposed it; the Office said remote access could act as an "online arcade". [S66][S67][S68]
- The **tenth triennial rulemaking (2027 cycle)** is open; a renewal petition for the video-game exemption exists on copyright.gov. [S62]
- Note: the 1201 exemptions address circumvention of access controls. They do not by themselves grant copyright licences, do not override contracts (EULA/ToU), and do not cover public multiplayer servers (the "local gameplay" definition excludes online services). [S63]

### 6.2 Contract (EULA / Terms of Use)
- Amazon Games Terms of Use (copy found on an Amazon subdomain) forbid decompiling/reverse engineering the game clients, acting as a host or matchmaking service without consent, facilitating any unauthorized connection that "emulates portions of the games", and creating derivative works. Verify the current official text. [S69]
- *Davidson & Associates v. Jung*, 422 F.3d 630 (8th Cir. 2005) (bnetd): Battle.net server emulator; court enforced the EULA/ToU ban on reverse engineering, rejected preemption and the 1201(f) interoperability defence, and found DMCA anti-circumvention and anti-trafficking violations. [S70][S71]

### 6.3 Enforcement precedents
- **Scapegaming (2010):** default judgment of $88,594,589 against a monetized WoW private server operator (copyright, unfair competition, circumvention). [S72]
- **Nostalrius (2016):** free vanilla WoW server (>800,000 accounts) closed on 2016-04-10 after a cease-and-desist from Blizzard's lawyers; code later given to Elysium. No court case. [S73]
- **Landmark (2017):** Daybreak publicly refused to license or authorize any emulator. [S61]
- **City of Heroes (2024):** NCSoft granted a revocable licence to Homecoming only. [S50][S51]
- **The Crew (2024-2025):** players sued Ubisoft; emulator released with "own a legitimate copy" rule; Ubisoft position on the emulator **Unknown**. [S56][S57]

### 6.4 European Union - "Stop Destroying Videogames" (Stop Killing Games)
- Submitted to the Commission **2026-01-26** with **1,294,188** verified statements; thresholds met in 24 Member States. [S74]
- Public hearing at European Parliament **2026-04-16**; plenary debate **2026-05-21**. [S75]
- **2026-06-16 Commission reply:** it "cannot, at this stage, propose" a legal obligation to keep games playable; points to existing consumer law; commits to start talks on an industry **code of conduct on "end of life"** of games by end of 2026 and to report on the Digital Content Directive before end of 2026. [S75][S76][S77]
- Effect on New World: none confirmed. Amazon gave 12 months notice but no end-of-life tools. [S1]

---

## 7. Open questions

1. Will Amazon publish any end-of-life tool, server binary, or licence before 2027-01-31? **Unknown.**
2. Can the stock client complete DTLS against a local REP without pin bypass (the `unknown_ca` blocker)? **Unknown.** [S21]
3. What does the login ("OmniSDK") flow require from Steam (session ticket validation)? **Unknown**; first-light says Steam must be running. [S20]
4. Does the client depend on Steam/platform APIs that also stop working after 2027-01-31? **Unknown.**
5. What does "Javelin" mean in the NW network stack? **Unknown.**
6. Activity, size, and licence of the "OpenWorld" Discord effort. **Unknown.**
7. Status of the siaginw Rust server (404). **Unknown.**
8. Exact current text and paragraph numbers of 37 C.F.R. 201.40 for video games (eCFR fetch was blocked). **Unknown.**
9. Can console (PS5/Xbox) clients ever be redirected? Likely no without jailbreak; **Unknown**.
10. Outcome of the EU code-of-conduct talks (due to start by end of 2026). **Unknown.**

---

## 8. Source list

New World shutdown and history
- [S1] Amazon Games, "The Future of New World: What to Expect" (2026-01-15): https://www.newworld.com/en-us/news/articles/the-future-of-new-world-aeternum-what-to-expect
- [S2] VGC (2026-01-15): https://www.videogameschronicle.com/news/amazon-games-confirms-mmo-new-world-has-a-year-left-before-it-goes-offline-permanently ; Steam news copy: https://store.steampowered.com/news/app/1063730/view/529868344958914242
- [S3] GamingOnLinux (2026-01-16): https://www.gamingonlinux.com/2026/01/amazon-confirm-new-world-aeternum-will-go-permanently-offline-next-year-and-is-being-delisted/
- [S4] Amazon Games, "Update on New World" (2025-10-28): https://www.newworld.com/en-us/news/articles/update-on-new-world
- [S5] RPG Site: https://www.rpgsite.net/news/18876-amazon-game-studios-announces-the-end-of-content-updates-for-new-world-aeternum ; TheSixthAxis: https://www.thesixthaxis.com/2025/10/29/new-world-aeternum-season-10-will-be-the-last-content-update-for-amazons-mmorpg
- [S6] VGC (2025-10-29): https://www.videogameschronicle.com/news/new-world-is-officially-ending-development-following-huge-amazon-layoffs
- [S7] CGMagazine: https://www.cgmagonline.com/news/new-world-says-goodbye/
- [S8] Amazon press release (2024-10): https://press.aboutamazon.com/2024/10/amazon-games-launches-new-world-aeternum-now-available-for-playstation-5-xbox-series-x-s-and-pc
- [S9] Amazon press release (2024-06): https://press.aboutamazon.com/2024/6/amazon-games-reveals-new-world-aeternum-releasing-october-15-on-playstation-5-xbox-series-x-s-and-pc
- [S10] Gulf News: https://gulfnews.com/business/markets/amazon-lays-off-about-100-employees-in-gaming-units-1.94945100
- [S11] Game World Observer: https://gameworldobserver.com/?p=18431
- [S12] Massively OP (2024-03-25): https://massivelyop.com/2024/03/25/vitae-aeternum-analyzing-new-worlds-content-drought-comms-chaos-and-lack-of-a-2024-expansion/
- [S13] Checkpoint Gaming: https://checkpointgaming.net/news/2026/01/amazons-new-world-mmo-to-shut-down-in-a-year/
- [S14] Inven Global: https://www.invenglobal.com/articles/20100/amazons-mmorpg-new-world-to-shut-down-in-2027-service-end-date-confirmed
- [S15] TrueAchievements: https://www.trueachievements.com/news/new-world-aeternum-servers ; MMORPG.com: https://www.mmorpg.com/news/new-world-aeternum-will-officially-sunset-on-january-31-2027-2000137038
- [S16] Change.org "Save New World": https://www.change.org/p/save-new-world
- [S17] MMORPG.com petition: https://www.mmorpg.com/news/new-world-aeternum-players-spun-up-a-petition-to-save-the-mmo-2000136458 ; Icy Veins: https://www.icy-veins.com/new-world/news/petition-save-new-world/
- [S18] Change.org "Reinstate AGS' New World team": https://www.change.org/p/reinstate-ags-new-world-aeternum-team
- [S19] Steam discussions: https://steamcommunity.com/app/1063730/discussions/0/652585180867015751/ ; https://steamcommunity.com/app/1063730/discussions/0/682992180338558530/ ; https://steamcommunity.com/app/1063730/discussions/0/802331141614105571/ ; listing site https://mmotopforge.com/new-world
- [S24] PC Games (DE): https://www.pcgames.de/New-World-Aeternum-Spiel-74821/News/Ab-2027-gehen-die-Server-offline-1491035/
- [S30] AWS for Games blog, New World architecture: https://aws.amazon.com/blogs/gametech/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world

New World preservation projects
- [S20] first-light: https://github.com/nw-private-server/first-light
- [S21] NewWorldPreservation: https://github.com/AScriver/NewWorldPreservation
- [S22] Aeternum-World: https://github.com/Coldzer0/Aeternum-World
- [S23] Aeternum-World packet doc: https://github.com/Coldzer0/Aeternum-World/blob/main/docs/Network/packet_system.md
- [S25] siaginw (404 on 2026-08-29): https://github.com/siaginw/New-World-Private-Server-Source
- [S26] https://github.com/Kattoor/nw-datasheet-reader ; https://github.com/MontagueM/NewWorldUnpacker ; https://github.com/Kattoor/aeternum-map
- [S27] RaGEZone: https://forum.ragezone.com/threads/new-world-emulator.1257186/

Emulator precedent
- [S31] TrinityCore: https://github.com/TrinityCore/TrinityCore
- [S32] TrinityCore setup: https://trinitycore.info/install/Server-Setup
- [S33] AzerothCore: https://github.com/azerothcore/azerothcore-wotlk
- [S34] TrinityCore wiki (DBs, bnetserver, Arctium): https://trinitycore.atlassian.net/wiki/x/bMOlB ; https://elijahketchersid.com/forums/thread-81031.html
- [S35] AzerothCore client setup: https://www.azerothcore.org/wiki/client-setup
- [S36] EQEmu: https://github.com/EQEmu/EQEmu
- [S37] NexusForever: https://github.com/NexusForever/NexusForever ; Source tree: https://github.com/NexusForever/NexusForever/tree/master/Source
- [S38] NexusForever Linux install: https://www.emulator.ws/installation/server-guide/server-installation/server-install-guide-linux.md ; docs index https://www.emulator.ws/llms.txt
- [S39] ClientConnector code: https://raw.githubusercontent.com/NexusForever/NexusForever/master/Source/NexusForever.ClientConnector/Program.cs ; guide https://www.emulator.ws/installation/client-connection-guide/nexusforever-client-connector.md ; FAQ https://www.emulator.ws/getting-started/faq.md
- [S40] MHServerEmu: https://github.com/Crypto137/MHServerEmu
- [S41] MHServerEmu ManualSetup: https://raw.githubusercontent.com/Crypto137/MHServerEmu/master/docs/Setup/ManualSetup.md
- [S42] MHServerEmu ClientConfig: https://raw.githubusercontent.com/Crypto137/MHServerEmu/master/docs/Client/ClientConfig.md
- [S43] MHServerEmu Docker: https://hub.docker.com/r/zkoesters/mhserveremu
- [S44] SWGEmu Core3 docs: https://app.assembla.com/wiki/show/swgemu/Core3_Documentation ; https://github.com/swgemu/Core3/blob/unstable/README.md
- [S45] Running SWGEmu Core3: https://virtuallyfun.com/2019/05/15/running-swgemu-core3/
- [S46] SWGEmu ports (forum): https://lr.in.psf.lt/r/SWGEmu
- [S47] NGECore2: https://github.com/projectswgcore/ngecore2
- [S48] Holocore wiki: https://bitbucket.org/projectswg/holocore.git/wiki
- [S49] Massively OP on SWG Legends (2026-08-29): https://massivelyop.com/2026/08/29/a-reddit-mod-used-ai-to-hack-a-star-wars-galaxies-emulator-over-a-14-year-old-grudge/
- [S50] Massively OP, Homecoming licence (2024-01-04): https://massivelyop.com/2024/01/04/ncsoft-has-officially-granted-a-city-of-heroes-server-license-to-the-homecoming-crew/
- [S51] Techdirt (2024-01-08): https://www.techdirt.com/2024/01/08/city-of-heroes-fan-server-will-continue-with-an-official-license-from-the-developer/
- [S52] City of Heroes leak coverage: https://www.thegamer.com/city-of-heroes-secret-server-leaks/ ; Volume 2 client setup https://wiki.ourodev.com/Volume_2_Client_setup
- [S53] Return of Reckoning wiki: https://wiki.returnofreckoning.com/Return_of_Reckoning ; history https://www.returnofreckoning.com/forum/app.php/about-history
- [S54] Lutris RoR installer: https://lutris.net/games/install/28463/view
- [S56] RacingGames.gg: https://racinggames.gg/article/how-a-team-of-modders-are-saving-the-crew
- [S57] PCGH (DE): https://www.pcgameshardware.de/The-Crew-Spiel-37320/News/Unlimited-Server-Emulator-Release-1481241/ ; VGC: https://www.videogameschronicle.com/news/devs-behind-fan-made-the-crew-resurrection-project-says-its-not-their-problem-if-it-doesnt-work-properly-with-pirated-copies
- [S58] GamingBolt (Anthem): https://gamingbolt.com/?p=635251
- [S59] Rock Paper Shotgun (Anthem): https://www.rockpapershotgun.com/anthem-private-server-experimentations-underway-as-the-shooters-subreddit-mods-rethink-retirement-plans
- [S60] DOL forum (Tabula Rasa): https://dolserver.sourceforge.net/viewtopic.php?p=131686
- [S61] PCGamesN (Landmark): https://www.pcgamesn.com/landmark/landmark-closing-date

Legal
- [S62] Copyright Office, tenth triennial renewal petition (abandoned video games): https://www.copyright.gov/1201/2027/petitions/renewal/Renewal-Pet-Abandoned-Video-Games-Dan-Elkins.pdf
- [S63] MADE 1201 comment (2021): https://www.law.berkeley.edu/wp-content/uploads/2021/07/MADE-1201-Comment.pdf ; Class 14 post-hearing: https://www.copyright.gov/1201/2021/post-hearing/letters/Class%2014%20Joint%20Creators%20and%20Copyright%20Owners%20Post-Hearing%20Response.pdf
- [S64] ESA 2018 opposition: https://www.copyright.gov/1201/2018/comments-021218/class8/Class_08_Opp'n_ESA.pdf
- [S65] IPWatchdog (2018): https://ipwatchdog.com/2018/01/27/dmca-exemption-online-video-game-preservation/id=92913/
- [S66] InfoDocket (2024-10-25): https://www.infodocket.com/2024/10/25/u-s-copyright-office-rejects-dmca-exemption-to-support-game-preservation/
- [S67] ESA statement: https://theesa.com/esa-statement-on-the-u-s-copyright-offices-triennial-rulemaking-on-section-1201-of-the-digital-millennium-copyright-act-dmca
- [S68] MP1st: https://mp1st.com/news/us-copyright-office-blocks-game-preservation-effort-saying-preserved-video-games-would-be-used-for-recreation
- [S69] Amazon Games Terms of Use (copy): https://digprjsurvey.amazon.com/csad/help/node/G201482650
- [S70] Davidson v. Jung opinion: https://law.resource.org/pub/us/case/reporter/F3/422/422.F3d.630.04-3654.html
- [S71] EFF bnetd briefs: https://www.eff.org/files/filenode/Blizzard_v_bnetd/20050112_opening_brief_of_defendants.pdf
- [S72] Game Developer (Scapegaming): https://gamedeveloper.com/game-platforms/blizzard-wins-88m-judgment-against-i-wow-i-private-server-owner ; Engadget: https://www.engadget.com/2010-08-14-private-server-company-forced-to-pay-blizzard-88-million.html
- [S73] Wikipedia, Nostalrius: https://en.wikipedia.org/wiki/Nostalrius
- [S74] ECI news (2026-01-26): https://citizens-initiative.europa.eu/news/14th-valid-initiative-stop-destroying-videogames-submitted-commission-examination-2026-01-26_en
- [S75] ECI Commission reply page: https://citizens-initiative.europa.eu/stop-destroying-videogames-commissions-reply-european-citizens-initiative_en ; news (2026-06-16): https://citizens-initiative.europa.eu/news/european-commission-replies-stop-destroying-videogames-initiative-2026-06-16_en
- [S76] Lewis Silkin (2026-06-24): https://www.lewissilkin.com/insights/2026/06/24/european-commission-stops-short-of-imposing-mandatory-requirements-following-citi-102n4yy
- [S77] Out of Games: https://outof.games/news/9185-the-european-commission-answers-to-the-stop-killing-games-movement/

