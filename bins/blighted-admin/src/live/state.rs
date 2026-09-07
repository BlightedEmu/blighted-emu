// SPDX-License-Identifier: AGPL-3.0-or-later
//! The live state of the running servers (admin tools spec section 5.2): sims from their
//! heartbeats, the cell table, the newest complete snapshot of each sim, the metrics of every
//! metrics page, CPU per program, the bot total and the event log. [`LiveState::view`] is what
//! the page gets.

use std::collections::{BTreeMap, VecDeque};

use nw_bus::{BusId, CellTable, MOB_STATES, SimHeartbeat, SimSnapshot, SnapMob, SnapPlayer};
use nw_common::prometheus::Scrape;
use rep_gateway::server::{
    METRIC_BYTES_IN, METRIC_BYTES_OUT, METRIC_DATAGRAMS_IN, METRIC_DATAGRAMS_OUT,
};
use serde::Serialize;
use sim::server::{METRIC_BUNDLE_MEMBERS, METRIC_TICK_SECONDS, ONE_TICK_SECONDS};

/// A sim without a heartbeat for longer than this has left.
pub const SIM_SILENT_MS: u64 = 3_000;
/// Events the log keeps.
pub const EVENTS_KEPT: usize = 200;
/// The tick quantile of the p99 tile.
const P99: f64 = 0.99;
const MS_PER_SECOND: f64 = 1_000.0;
/// The position of the asleep state in the mob counts.
const ASLEEP: usize = 0;
const SUM_SUFFIX: &str = "_sum";
const COUNT_SUFFIX: &str = "_count";

/// The tiles row (each `None` is shown as "Unknown").
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Tiles {
    /// Sims with a recent heartbeat.
    pub sims: u32,
    /// Players of those sims, from their heartbeats.
    pub players: u32,
    /// Bots of the running batches.
    pub bots: u32,
    /// CPU per program, in % of one core.
    pub cpu: Vec<(String, Option<f64>)>,
    /// Average tick time over the last scrape interval, every page together.
    pub tick_avg_ms: Option<f64>,
    /// 99th percentile tick time (a bucket bound) over the last scrape interval.
    pub tick_p99_ms: Option<f64>,
    /// Gateway datagrams received per second.
    pub datagrams_in_s: Option<f64>,
    /// Gateway datagrams sent per second.
    pub datagrams_out_s: Option<f64>,
    /// Gateway bytes received per second.
    pub bytes_in_s: Option<f64>,
    /// Gateway bytes sent per second.
    pub bytes_out_s: Option<f64>,
    /// Bundle members per second, every sim together.
    pub members_s: Option<f64>,
    /// Awake mobs of the shown snapshots.
    pub mobs_awake: Option<u64>,
    /// Asleep mobs of the shown snapshots.
    pub mobs_asleep: Option<u64>,
}

/// One sim with a recent heartbeat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SimView {
    /// The sim.
    pub id: String,
    /// Its players.
    pub players: u32,
    /// Players it takes at most.
    pub max_players: u32,
}

/// One metrics page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricsView {
    /// The page.
    pub url: String,
    /// The last scrape worked.
    pub up: bool,
    /// Its average tick time over the last interval.
    pub tick_avg_ms: Option<f64>,
    /// Its 99th percentile tick time over the last interval.
    pub tick_p99_ms: Option<f64>,
}

/// One player of a shown snapshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayerView {
    /// The entity number.
    pub entity: u64,
    /// The character's name.
    pub name: String,
    /// The name starts with a bot prefix.
    pub bot: bool,
    /// Its sim.
    pub sim: String,
    /// East.
    pub x: f32,
    /// North.
    pub y: f32,
    /// Height.
    pub z: f32,
    /// Dead.
    pub dead: bool,
    /// Frozen for a handoff.
    pub frozen: bool,
}

/// One awake mob of a shown snapshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MobView {
    /// The entity number.
    pub entity: u64,
    /// Its sim.
    pub sim: String,
    /// East.
    pub x: f32,
    /// North.
    pub y: f32,
    /// State, in mob state order.
    pub state: u8,
    /// Current health.
    pub health: u32,
    /// Max health.
    pub max_health: u32,
}

/// One event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventView {
    /// Unix milliseconds.
    pub time_ms: u64,
    /// What happened.
    pub text: String,
}

/// What the page gets.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LiveView {
    /// The tiles row.
    pub tiles: Tiles,
    /// The sims.
    pub sims: Vec<SimView>,
    /// The metrics pages.
    pub metrics: Vec<MetricsView>,
    /// The newest cell table.
    pub table: Option<CellTable>,
    /// Players of the shown snapshots.
    pub players: Vec<PlayerView>,
    /// Awake mobs of the shown snapshots.
    pub mobs: Vec<MobView>,
    /// The newest events, oldest first.
    pub events: Vec<EventView>,
    /// One sim tick at 30 Hz: a tick p99 above it is a slow tick.
    pub tick_budget_ms: f64,
}

#[derive(Debug, Clone)]
struct SimEntry {
    started_at_ms: u64,
    players: u32,
    max_players: u32,
    last_seen_ms: u64,
}

/// The newest complete snapshot of one sim, and the parts of a newer one.
#[derive(Debug, Clone, Default)]
struct Snapshots {
    shown_ms: Option<u64>,
    players: Vec<SnapPlayer>,
    mobs: Vec<SnapMob>,
    counts: Option<[u32; MOB_STATES]>,
    pending_ms: Option<u64>,
    pending: Vec<Option<SimSnapshot>>,
}

/// The last scrape of one page and its change from the scrape before.
#[derive(Debug, Clone, Default)]
struct Page {
    up: bool,
    last: Option<(u64, Scrape)>,
    /// Seconds between the two scrapes, and the change.
    delta: Option<(f64, Scrape)>,
}

/// The live state.
#[derive(Debug, Default)]
pub struct LiveState {
    sims: BTreeMap<BusId, SimEntry>,
    table: Option<CellTable>,
    snapshots: BTreeMap<BusId, Snapshots>,
    pages: BTreeMap<String, Page>,
    cpu: Vec<(String, Option<f64>)>,
    bots: u32,
    events: VecDeque<EventView>,
    bot_prefixes: Vec<String>,
}

impl LiveState {
    /// An empty state that marks players whose names start with one of `bot_prefixes` as bots.
    pub fn new(bot_prefixes: Vec<String>) -> Self {
        Self {
            bot_prefixes,
            ..Self::default()
        }
    }

    fn is_bot(&self, name: &str) -> bool {
        self.bot_prefixes
            .iter()
            .any(|prefix| name.starts_with(prefix.as_str()))
    }

    /// A sim heartbeat at `now_ms`; a new sim or a new start time is an event.
    pub fn heartbeat(&mut self, beat: SimHeartbeat, now_ms: u64) {
        let entry = SimEntry {
            started_at_ms: beat.started_at_ms,
            players: beat.players,
            max_players: beat.max_players,
            last_seen_ms: now_ms,
        };
        match self.sims.insert(beat.id.clone(), entry) {
            None => self.event(format!("sim {} joined", beat.id), now_ms),
            Some(old) if old.started_at_ms != beat.started_at_ms => {
                self.snapshots.remove(&beat.id);
                self.event(format!("sim {} restarted", beat.id), now_ms);
            }
            Some(_) => {}
        }
    }

    /// The director's cell table; a new version is an event.
    pub fn table(&mut self, table: CellTable, now_ms: u64) {
        let version = table.version;
        let changed = self.table.as_ref().is_none_or(|old| old.version != version);
        self.table = Some(table);
        if changed {
            self.event(format!("cell table version {version}"), now_ms);
        }
    }

    /// One part of a sim snapshot. A snapshot is shown when all its parts arrived; a part older
    /// than the shown snapshot or than the parts waiting is dropped.
    pub fn snapshot_part(&mut self, part: SimSnapshot) {
        let entry = self.snapshots.entry(part.sim.clone()).or_default();
        let time_ms = part.time_ms;
        if entry.shown_ms.is_some_and(|shown| time_ms <= shown)
            || entry.pending_ms.is_some_and(|pending| time_ms < pending)
        {
            return;
        }
        if entry.pending_ms != Some(time_ms) {
            entry.pending_ms = Some(time_ms);
            entry.pending = vec![None; usize::from(part.parts.max(1))];
        }
        let index = usize::from(part.part);
        let Some(slot) = entry.pending.get_mut(index) else {
            return;
        };
        *slot = Some(part);
        if entry.pending.iter().all(Option::is_some) {
            let parts: Vec<SimSnapshot> = entry.pending.drain(..).flatten().collect();
            entry.pending_ms = None;
            entry.shown_ms = Some(time_ms);
            entry.counts = parts.iter().find_map(|part| part.mob_counts);
            entry.players = parts.iter().flat_map(|part| part.players.clone()).collect();
            entry.mobs = parts.into_iter().flat_map(|part| part.awake_mobs).collect();
        }
    }

    /// One scrape of `url` at `now_ms`, or why it failed.
    pub fn scrape(&mut self, url: &str, result: Result<Scrape, String>, now_ms: u64) {
        let page = self.pages.entry(url.to_owned()).or_default();
        match result {
            Ok(scrape) => {
                page.delta = page.last.as_ref().and_then(|(at, last)| {
                    let seconds = now_ms.saturating_sub(*at) as f64 / MS_PER_SECOND;
                    (seconds > 0.0).then(|| (seconds, scrape.delta(last)))
                });
                page.last = Some((now_ms, scrape));
                page.up = true;
            }
            Err(_) => {
                page.up = false;
                page.delta = None;
            }
        }
    }

    /// CPU per program from the newest sample.
    pub fn cpu(&mut self, per_name: Vec<(String, Option<f64>)>) {
        self.cpu = per_name;
    }

    /// Adds an event; the log keeps the newest [`EVENTS_KEPT`].
    pub fn event(&mut self, text: String, now_ms: u64) {
        if self.events.len() == EVENTS_KEPT {
            self.events.pop_front();
        }
        self.events.push_back(EventView {
            time_ms: now_ms,
            text,
        });
    }

    /// The bots of the running batches.
    pub fn bots(&mut self, running: u32) {
        self.bots = running;
    }

    /// Sims silent for longer than [`SIM_SILENT_MS`] leave, with one event each.
    pub fn expire(&mut self, now_ms: u64) {
        let silent: Vec<BusId> = self
            .sims
            .iter()
            .filter(|(_, sim)| now_ms.saturating_sub(sim.last_seen_ms) > SIM_SILENT_MS)
            .map(|(id, _)| id.clone())
            .collect();
        for id in silent {
            self.sims.remove(&id);
            self.snapshots.remove(&id);
            self.event(format!("sim {id} left"), now_ms);
        }
    }

    /// The view for the page.
    pub fn view(&self) -> LiveView {
        let deltas: Vec<&(f64, Scrape)> = self
            .pages
            .values()
            .filter(|page| page.up)
            .filter_map(|page| page.delta.as_ref())
            .collect();
        let rate = |metric: &str| {
            (!deltas.is_empty()).then(|| {
                deltas
                    .iter()
                    .map(|(seconds, delta)| delta.total(metric, None) / seconds)
                    .sum()
            })
        };
        let mut merged = Scrape::default();
        for (_, delta) in &deltas {
            merged.add(delta);
        }
        let (tick_avg_ms, tick_p99_ms) = ticks(&merged);
        let counts = self
            .snapshots
            .values()
            .filter_map(|snapshots| snapshots.counts)
            .fold(None, |sum: Option<[u64; MOB_STATES]>, counts| {
                let mut sum = sum.unwrap_or_default();
                for (total, count) in sum.iter_mut().zip(counts) {
                    *total += u64::from(count);
                }
                Some(sum)
            });
        let tiles = Tiles {
            sims: u32::try_from(self.sims.len()).unwrap_or(u32::MAX),
            players: self.sims.values().map(|sim| sim.players).sum(),
            bots: self.bots,
            cpu: self.cpu.clone(),
            tick_avg_ms,
            tick_p99_ms,
            datagrams_in_s: rate(METRIC_DATAGRAMS_IN),
            datagrams_out_s: rate(METRIC_DATAGRAMS_OUT),
            bytes_in_s: rate(METRIC_BYTES_IN),
            bytes_out_s: rate(METRIC_BYTES_OUT),
            members_s: rate(METRIC_BUNDLE_MEMBERS),
            mobs_awake: counts.map(|counts| counts.iter().skip(ASLEEP + 1).sum()),
            mobs_asleep: counts.map(|counts| counts[ASLEEP]),
        };
        LiveView {
            tiles,
            sims: self
                .sims
                .iter()
                .map(|(id, sim)| SimView {
                    id: id.to_string(),
                    players: sim.players,
                    max_players: sim.max_players,
                })
                .collect(),
            metrics: self
                .pages
                .iter()
                .map(|(url, page)| {
                    let (tick_avg_ms, tick_p99_ms) = page
                        .delta
                        .as_ref()
                        .filter(|_| page.up)
                        .map_or((None, None), |(_, delta)| ticks(delta));
                    MetricsView {
                        url: url.clone(),
                        up: page.up,
                        tick_avg_ms,
                        tick_p99_ms,
                    }
                })
                .collect(),
            table: self.table.clone(),
            players: self
                .snapshots
                .iter()
                .flat_map(|(sim, snapshots)| {
                    snapshots.players.iter().map(move |player| PlayerView {
                        entity: player.entity,
                        name: player.name.clone(),
                        bot: self.is_bot(&player.name),
                        sim: sim.to_string(),
                        x: player.x,
                        y: player.y,
                        z: player.z,
                        dead: player.dead,
                        frozen: player.frozen,
                    })
                })
                .collect(),
            mobs: self
                .snapshots
                .iter()
                .flat_map(|(sim, snapshots)| {
                    snapshots.mobs.iter().map(move |mob| MobView {
                        entity: mob.entity,
                        sim: sim.to_string(),
                        x: mob.x,
                        y: mob.y,
                        state: mob.state,
                        health: mob.health,
                        max_health: mob.max_health,
                    })
                })
                .collect(),
            events: self.events.iter().cloned().collect(),
            tick_budget_ms: ONE_TICK_SECONDS * MS_PER_SECOND,
        }
    }
}

/// The average and the 99th percentile tick time of a scrape change, in milliseconds; `None`
/// without ticks (an infinite bucket bound has no value either).
fn ticks(delta: &Scrape) -> (Option<f64>, Option<f64>) {
    let count = delta.total(&format!("{METRIC_TICK_SECONDS}{COUNT_SUFFIX}"), None);
    if count <= 0.0 {
        return (None, None);
    }
    let sum = delta.total(&format!("{METRIC_TICK_SECONDS}{SUM_SUFFIX}"), None);
    let p99 = delta
        .quantile(METRIC_TICK_SECONDS, P99)
        .filter(|bound| bound.is_finite())
        .map(|bound| bound * MS_PER_SECOND);
    (Some(sum / count * MS_PER_SECOND), p99)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000;
    const URL: &str = "http://127.0.0.1:9102/metrics";
    const OTHER_URL: &str = "http://127.0.0.1:9104/metrics";
    const SECOND_MS: u64 = 1_000;

    fn sim(name: &str) -> BusId {
        BusId::try_from(name.to_owned()).expect("id")
    }

    fn beat(name: &str, started_at_ms: u64, players: u32) -> SimHeartbeat {
        SimHeartbeat {
            id: sim(name),
            started_at_ms,
            players,
            max_players: 1000,
        }
    }

    fn player(entity: u64) -> SnapPlayer {
        SnapPlayer {
            entity,
            name: format!("Player{entity}"),
            x: 1.0,
            y: 2.0,
            z: 3.0,
            dead: false,
            frozen: false,
        }
    }

    fn mob(entity: u64) -> SnapMob {
        SnapMob {
            entity,
            x: 1.0,
            y: 2.0,
            state: 2,
            health: 10,
            max_health: 20,
        }
    }

    fn part(time_ms: u64, part: u16, parts: u16, players: &[u64]) -> SimSnapshot {
        SimSnapshot {
            sim: sim("sim-a"),
            time_ms,
            part,
            parts,
            players: players.iter().copied().map(player).collect(),
            awake_mobs: vec![mob(100 + u64::from(part))],
            mob_counts: (part == 0).then_some([7, 1, 1, 0, 0, 0]),
        }
    }

    fn texts(view: &LiveView) -> Vec<&str> {
        view.events
            .iter()
            .map(|event| event.text.as_str())
            .collect()
    }

    #[test]
    fn a_snapshot_shows_when_all_parts_arrived() {
        let mut state = LiveState::default();
        state.snapshot_part(part(NOW, 0, 2, &[1]));
        assert!(state.view().players.is_empty(), "one part of two");
        state.snapshot_part(part(NOW, 1, 2, &[2]));
        let view = state.view();
        assert_eq!(view.players.len(), 2);
        assert_eq!(view.mobs.len(), 2);
        assert_eq!(view.tiles.mobs_asleep, Some(7));
        assert_eq!(view.tiles.mobs_awake, Some(2));
    }

    #[test]
    fn an_old_part_after_a_new_snapshot_is_dropped() {
        let mut state = LiveState::default();
        state.snapshot_part(part(NOW, 0, 1, &[1]));
        state.snapshot_part(part(NOW + 100, 0, 2, &[2]));
        state.snapshot_part(part(NOW, 1, 2, &[3]));
        let shown: Vec<u64> = state.view().players.iter().map(|p| p.entity).collect();
        assert_eq!(
            shown,
            vec![1],
            "the older part did not complete the newer one"
        );
        state.snapshot_part(part(NOW - 100, 0, 1, &[4]));
        let shown: Vec<u64> = state.view().players.iter().map(|p| p.entity).collect();
        assert_eq!(
            shown,
            vec![1],
            "an older snapshot never replaces a newer one"
        );
    }

    #[test]
    fn the_view_gives_the_tick_budget() {
        const BUDGET_MS: f64 = 33.0;
        const TOLERANCE_MS: f64 = 1e-9;
        assert!((LiveState::default().view().tick_budget_ms - BUDGET_MS).abs() < TOLERANCE_MS);
    }

    #[test]
    fn players_with_a_bot_prefix_are_bots() {
        let mut state = LiveState::new(vec!["Adminbot".to_owned(), "Swarm".to_owned()]);
        let mut snapshot = part(NOW, 0, 1, &[1, 2, 3]);
        snapshot.players[0].name = "Adminbotab".to_owned();
        snapshot.players[1].name = "Swarmxyz".to_owned();
        snapshot.players[2].name = "Ada".to_owned();
        state.snapshot_part(snapshot);
        let view = state.view();
        let marks: Vec<(&str, bool)> = view
            .players
            .iter()
            .map(|player| (player.name.as_str(), player.bot))
            .collect();
        assert_eq!(
            marks,
            vec![("Adminbotab", true), ("Swarmxyz", true), ("Ada", false)]
        );
    }

    #[test]
    fn a_silent_sim_leaves_with_one_event() {
        let mut state = LiveState::default();
        state.heartbeat(beat("sim-a", 1, 5), NOW);
        state.snapshot_part(part(NOW, 0, 1, &[1]));
        state.expire(NOW + SIM_SILENT_MS);
        assert_eq!(state.view().sims.len(), 1, "3 s is not yet silent");
        state.expire(NOW + SIM_SILENT_MS + 1);
        state.expire(NOW + SIM_SILENT_MS + 2);
        let view = state.view();
        assert!(view.sims.is_empty() && view.players.is_empty());
        let left = texts(&view)
            .iter()
            .filter(|text| text.contains("left"))
            .count();
        assert_eq!(left, 1);
    }

    #[test]
    fn a_restarted_sim_is_one_sim() {
        let mut state = LiveState::default();
        state.heartbeat(beat("sim-a", 1, 5), NOW);
        state.heartbeat(beat("sim-a", 2, 0), NOW + SECOND_MS);
        let view = state.view();
        assert_eq!(view.sims.len(), 1);
        assert_eq!(view.tiles.sims, 1);
        assert_eq!(view.tiles.players, 0);
        assert!(texts(&view).iter().any(|text| text.contains("restarted")));
    }

    fn tick_text(sum: f64, count: u64, fast: u64) -> String {
        format!(
            "sim_tick_seconds_bucket{{le=\"0.005\"}} {fast}\n\
             sim_tick_seconds_bucket{{le=\"0.033\"}} {count}\n\
             sim_tick_seconds_bucket{{le=\"+Inf\"}} {count}\n\
             sim_tick_seconds_sum {sum}\n\
             sim_tick_seconds_count {count}\n"
        )
    }

    #[test]
    fn tick_tiles_come_from_histogram_deltas() {
        let mut state = LiveState::default();
        state.scrape(URL, Ok(Scrape::parse(&tick_text(10.0, 1000, 990))), NOW);
        assert_eq!(
            state.view().tiles.tick_avg_ms,
            None,
            "one scrape has no change"
        );
        state.scrape(
            URL,
            Ok(Scrape::parse(&tick_text(10.2, 1100, 1000))),
            NOW + SECOND_MS,
        );
        let tiles = state.view().tiles;
        let avg = tiles.tick_avg_ms.expect("avg");
        assert!((avg - 2.0).abs() < 1e-6, "0.2 s over 100 ticks: {avg}");
        assert_eq!(tiles.tick_p99_ms, Some(33.0));
    }

    #[test]
    fn rates_come_from_counter_deltas() {
        let counters = |packets: u64, bytes: u64, members: u64| {
            format!(
                "rep_gateway_datagrams_in_total {packets}\nrep_gateway_datagrams_out_total {packets}\n\
                 rep_gateway_bytes_in_total {bytes}\nrep_gateway_bytes_out_total {bytes}\n\
                 sim_bundle_members_total {members}\n"
            )
        };
        let mut state = LiveState::default();
        state.scrape(URL, Ok(Scrape::parse(&counters(100, 1000, 10))), NOW);
        state.scrape(
            URL,
            Ok(Scrape::parse(&counters(300, 5000, 70))),
            NOW + 2 * SECOND_MS,
        );
        let tiles = state.view().tiles;
        assert_eq!(tiles.datagrams_in_s, Some(100.0));
        assert_eq!(tiles.datagrams_out_s, Some(100.0));
        assert_eq!(tiles.bytes_in_s, Some(2000.0));
        assert_eq!(tiles.bytes_out_s, Some(2000.0));
        assert_eq!(tiles.members_s, Some(30.0));
    }

    #[test]
    fn a_down_metrics_port_gives_unknown_tiles() {
        let mut state = LiveState::default();
        let text = tick_text(10.0, 1000, 990);
        state.scrape(URL, Ok(Scrape::parse(&text)), NOW);
        state.scrape(OTHER_URL, Ok(Scrape::parse(&text)), NOW);
        state.scrape(URL, Err("connection refused".to_owned()), NOW + SECOND_MS);
        state.scrape(
            OTHER_URL,
            Ok(Scrape::parse(&tick_text(10.2, 1100, 1000))),
            NOW + SECOND_MS,
        );
        let view = state.view();
        let down = view.metrics.iter().find(|m| m.url == URL).expect("down");
        assert!(!down.up);
        assert_eq!((down.tick_avg_ms, down.tick_p99_ms), (None, None));
        let up = view
            .metrics
            .iter()
            .find(|m| m.url == OTHER_URL)
            .expect("up");
        assert!(up.up && up.tick_avg_ms.is_some());
        assert!(
            view.tiles.tick_avg_ms.is_some(),
            "the other page still counts"
        );
        let mut all_down = LiveState::default();
        all_down.scrape(URL, Err("refused".to_owned()), NOW);
        assert_eq!(all_down.view().tiles.tick_avg_ms, None);
        assert_eq!(all_down.view().tiles.bytes_in_s, None);
    }

    #[test]
    fn the_bot_tile_is_the_running_batch_total() {
        let mut state = LiveState::default();
        assert_eq!(state.view().tiles.bots, 0);
        state.bots(110);
        assert_eq!(state.view().tiles.bots, 110);
    }

    #[test]
    fn a_table_change_is_an_event_and_is_shown() {
        let mut state = LiveState::default();
        let table = CellTable {
            version: 3,
            cell_size_m: 1024.0,
            columns: 2,
            rows: 1,
            sims: Vec::new(),
            owners: vec![None, None],
        };
        state.table(table.clone(), NOW);
        state.table(table, NOW);
        let view = state.view();
        assert_eq!(view.table.as_ref().map(|t| t.version), Some(3));
        let changes = texts(&view)
            .iter()
            .filter(|text| text.contains("cell table"))
            .count();
        assert_eq!(changes, 1, "the same version again is not an event");
    }

    #[test]
    fn the_event_log_keeps_the_newest() {
        let mut state = LiveState::default();
        for n in 0..EVENTS_KEPT + 5 {
            state.event(format!("event {n}"), NOW);
        }
        let view = state.view();
        assert_eq!(view.events.len(), EVENTS_KEPT);
        assert_eq!(
            view.events.last().map(|e| e.text.as_str()),
            Some(format!("event {}", EVENTS_KEPT + 4).as_str())
        );
    }
}
