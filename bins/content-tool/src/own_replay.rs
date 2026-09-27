// SPDX-License-Identifier: AGPL-3.0-or-later
//! Replays the client input of retail tutorial captures through the sim's own movement of the
//! player ([`nw_sim_core::TrackedBody`], with the sim's reading of each input record:
//! [`sim::camera`]) over the level's ground, and compares the body with retail's own place.
//!
//! Retail's server runs the client's own character controller, so its own place is where the
//! client stands. The client corrects its own prediction toward the server's own place when the
//! two differ by more than its correction threshold, and snaps past the full-correction distance
//! (`scripts/patch-client.py`: 0.5 m and 2.0 m in the patched client). So the share of compared
//! places past those distances is how often our server pulls the client: the rubber banding.
//!
//! Open-loop: a server move of retail (a step of more than [`SERVER_MOVE_M`] between two own
//! places) puts the body at retail's new place, as the sim does with its own moves.

use std::path::{Path, PathBuf};

use nw_capture::{CLIENT_TO_SERVER, Decoder, ledger_records};
use nw_content::client_build::load_client_build;
use nw_messages::{BodyCodec, ClientInputMsg, InputRecord, RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX};
use nw_sim_core::{Ground, LevelGround, Position, TrackedBody};
use nw_world::WorldFile;
use nw_world::collision::{LevelCollision, collision_file_path};
use nw_world::collision_index::CollisionIndex;
use nw_world::water::{WaterMap, water_file_path};
use nw_world::world_file::world_file_path;

use crate::ai_trace::{PlayerSample, trace_ledger};
use crate::ftue_entities::read_ledger;

/// The client's correction threshold, m (patched client).
pub const CORRECTION_M: f32 = 0.5;
/// The client's full-correction distance, m (patched client).
pub const FULL_CORRECTION_M: f32 = 2.0;
/// A step of retail's own place longer than this between two states is a server move, m (the
/// fastest own movement, a mount's sprint, covers well under it in one state).
pub const SERVER_MOVE_M: f32 = 8.0;
/// The longest gap from an input to the next retail own place that the replay compares, ms (an
/// own state goes out every tick, 33 ms; a longer gap is a load or a cutscene).
pub const MAX_ANSWER_MS: u64 = 100;
/// Half of the input counter's range: a counter newer than the last read lies within it.
const COUNTER_HALF_RANGE: u16 = 0x8000;

/// One compared place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Compared {
    /// Ms from the first ledger record.
    pub time_ms: u64,
    /// Retail's own place.
    pub retail: Position,
    /// The replayed body.
    pub body: Position,
    /// The record's camera gave the body ([`nw_sim_core::Tracked::consistent`]).
    pub camera: bool,
}

impl Compared {
    /// The distance on the ground plane, m.
    pub fn horizontal_m(&self) -> f32 {
        self.body.horizontal_distance(self.retail)
    }

    /// The height difference, m (positive: the body is higher).
    pub fn height_m(&self) -> f32 {
        self.body.z - self.retail.z
    }
}

/// The summary of a replay.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplaySummary {
    /// Places compared.
    pub compared: usize,
    /// Server moves of retail.
    pub server_moves: usize,
    /// Horizontal error, m: median, 90th and 99th percentile.
    pub horizontal_m: [f32; 3],
    /// Height error (absolute), m: median, 90th and 99th percentile.
    pub height_m: [f32; 3],
    /// Share of places whose distance (3D) passes [`CORRECTION_M`].
    pub corrected_share: f32,
    /// Share of places whose distance (3D) passes [`FULL_CORRECTION_M`].
    pub snapped_share: f32,
    /// Share of places whose record's camera gave nothing.
    pub no_camera_share: f32,
}

/// The client input records of the ledger `bytes`, oldest first, each read once, with the time
/// of the message that carried it (ms from the first ledger record).
pub fn input_records(bytes: &[u8]) -> Vec<(u64, InputRecord, sim::camera::RecordView)> {
    let mut decoder = Decoder::new();
    let mut first_ms = None;
    let mut last_counter: Option<u16> = None;
    let mut out = Vec::new();
    for record in ledger_records(bytes) {
        let Ok(record) = record else {
            break;
        };
        let time_ms = record.time_ms - *first_ms.get_or_insert(record.time_ms);
        let output = decoder.record(&record);
        if record.direction != CLIENT_TO_SERVER {
            continue;
        }
        for delivered in &output.envelopes {
            if delivered.envelope.type_index() != Some(RECEIVE_INPUT_FROM_CLIENT_TYPE_INDEX) {
                continue;
            }
            let Ok(message) = ClientInputMsg::decode(&delivered.envelope.body) else {
                continue;
            };
            let views = sim::camera::record_views(&message.records);
            for (input, view) in message.records.iter().zip(views).rev() {
                let counter = input.u16_after_mask;
                let newer = last_counter.is_none_or(|last| {
                    let ahead = counter.wrapping_sub(last);
                    ahead != 0 && ahead < COUNTER_HALF_RANGE
                });
                if newer {
                    last_counter = Some(counter);
                    out.push((time_ms, input.clone(), view));
                }
            }
        }
    }
    out
}

/// Replays `records` over `ground` with the sea at `sea_level_m` against retail's own places
/// `retail` (in time order); gives every compared place.
pub fn replay(
    records: &[(u64, InputRecord, sim::camera::RecordView)],
    retail: &[PlayerSample],
    ground: &(impl Ground + ?Sized),
    sea_level_m: f32,
) -> (Vec<Compared>, usize) {
    let place = |sample: &PlayerSample| Position {
        x: sample.position[0],
        y: sample.position[1],
        z: sample.position[2],
    };
    let Some(first) = retail.first() else {
        return (Vec::new(), 0);
    };
    let mut body = TrackedBody::new(place(first));
    let mut next = 0;
    let mut last_retail = place(first);
    let mut compared = Vec::new();
    let mut server_moves = 0;
    for (time_ms, record, view) in records {
        // Retail's places up to this input: a server move puts the body there.
        while next < retail.len() && retail[next].time_ms <= *time_ms {
            let at = place(&retail[next]);
            if at.horizontal_distance(last_retail) > SERVER_MOVE_M {
                body.stand_at(at);
                server_moves += 1;
            }
            last_retail = at;
            next += 1;
        }
        let Some(input) = sim::camera::track_input(record, *view) else {
            continue;
        };
        let tracked = body.record(input, ground, sea_level_m);
        let answer = retail.get(next).filter(|sample| {
            sample.time_ms.saturating_sub(*time_ms) <= MAX_ANSWER_MS
                && place(sample).horizontal_distance(last_retail) <= SERVER_MOVE_M
        });
        if let Some(answer) = answer {
            compared.push(Compared {
                time_ms: answer.time_ms,
                retail: place(answer),
                body: tracked.position,
                camera: tracked.consistent,
            });
        }
    }
    (compared, server_moves)
}

/// The summary of `compared`.
pub fn summarize(compared: &[Compared], server_moves: usize) -> ReplaySummary {
    let percentiles = |mut values: Vec<f32>| {
        values.sort_by(f32::total_cmp);
        let at = |share: f32| {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "an index into tens of thousands of places"
            )]
            let index = ((values.len().saturating_sub(1)) as f32 * share) as usize;
            values.get(index).copied().unwrap_or(f32::NAN)
        };
        [at(0.5), at(0.9), at(0.99)]
    };
    let distance = |place: &Compared| place.horizontal_m().hypot(place.height_m());
    #[allow(clippy::cast_precision_loss, reason = "counts of tens of thousands")]
    let share = |limit: f32| {
        compared
            .iter()
            .filter(|place| distance(place) > limit)
            .count() as f32
            / compared.len().max(1) as f32
    };
    ReplaySummary {
        compared: compared.len(),
        server_moves,
        horizontal_m: percentiles(compared.iter().map(Compared::horizontal_m).collect()),
        height_m: percentiles(
            compared
                .iter()
                .map(|place| place.height_m().abs())
                .collect(),
        ),
        corrected_share: share(CORRECTION_M),
        snapped_share: share(FULL_CORRECTION_M),
        no_camera_share: compared.iter().filter(|place| !place.camera).count() as f32
            / compared.len().max(1) as f32,
    }
}

/// The ground of `level` of the content in `content` for client build config `build_config`:
/// the terrain, the level collision and the water, as the sim builds it; and the sea level.
pub fn level_ground(
    content: &Path,
    build_config: &Path,
    level: &str,
) -> (LevelGround<std::sync::Arc<WorldFile>>, f32) {
    use std::sync::Arc;
    let build = load_client_build(build_config).expect("client build");
    let world =
        Arc::new(WorldFile::load(&world_file_path(content, &build, level), &build).expect("world"));
    let collision = LevelCollision::load(&collision_file_path(content, &build, level), &build)
        .expect("run `just collision` first");
    let collision = Arc::new(CollisionIndex::new(LevelCollision {
        deferred: Vec::new(),
        ..collision
    }));
    let sea = world.level.ocean_level;
    let ground = LevelGround::new(Arc::clone(&world), collision);
    let ground = match WaterMap::load(&water_file_path(content, &build, level), &build) {
        Ok(water) => ground.with_water(Arc::new(water)),
        Err(_) => ground,
    };
    (ground, sea)
}

/// The repository root.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
    const TUTORIAL_LEVEL: &str = "ftue_v2";
    /// The report's time windows, ms.
    const WINDOW_MS: u64 = 10_000;
    const MS_PER_SECOND: u64 = 1_000;
    const PERCENT: f32 = 100.0;
    /// The tutorial ship's deck lies above this height, m (the deck at 89.3 m, the sea at 20 m).
    const SHIP_DECK_ABOVE_M: f32 = 80.0;
    /// The tutorial level lies west of this x, m; the open world (the start beach at 9,600 m) east.
    const TUTORIAL_WEST_OF_M: f32 = 2_000.0;
    /// Whether a retail place lies in a part of the run.
    type InPart = fn(Position) -> bool;
    /// The parts of a tutorial run the report sums apart.
    const PARTS: [(&str, InPart); 3] = [
        ("ship", |at| {
            at.x < TUTORIAL_WEST_OF_M && at.z > SHIP_DECK_ABOVE_M
        }),
        ("sea and beach", |at| {
            at.x < TUTORIAL_WEST_OF_M && at.z <= SHIP_DECK_ABOVE_M
        }),
        ("open world", |at| at.x >= TUTORIAL_WEST_OF_M),
    ];
    /// The retail captures of the tutorial (a new character, ship to beach), each with the most
    /// share of its places the replay may pull the client in each of [`PARTS`] (the result of
    /// 2026-09-24 plus 0.02; `None`: the run has no such part). Lower one when a change brings
    /// the share down; a change that passes one is a step back.
    const LEDGERS: [(&str, [Option<f32>; 3]); 2] = [
        (
            "local-data/captures/own/newchar-retail-2026-09-18/ledger.bin",
            [Some(0.93), Some(0.47), Some(0.38)],
        ),
        (
            "local-data/captures/own/newchar-linux-2026-09-12/ledger.bin",
            [Some(0.41), Some(0.54), None],
        ),
    ];

    #[test]
    fn the_tutorial_own_movement_replays_against_retail() {
        let (ground, sea) = level_ground(
            &repo().join("local-data/content"),
            &repo().join(CLIENT_BUILD_CONFIG),
            TUTORIAL_LEVEL,
        );
        for (ledger, most_corrected) in LEDGERS {
            let bytes = read_ledger(&repo().join(ledger)).expect("ledger");
            let retail = trace_ledger(&bytes, &[]).player;
            let records = input_records(&bytes);
            let (compared, moves) = replay(&records, &retail, &ground, sea);
            let summary = summarize(&compared, moves);
            println!("{ledger}: {summary:?}");
            for window in compared.chunk_by(|a, b| a.time_ms / WINDOW_MS == b.time_ms / WINDOW_MS) {
                let window_summary = summarize(window, 0);
                let first = window[0];
                println!(
                    "  {:>4} s at ({:.0}, {:.0}, {:.1}): h {:.2}/{:.2} m, z {:.2} m, corrected {:.0}%",
                    first.time_ms / MS_PER_SECOND,
                    first.retail.x,
                    first.retail.y,
                    first.retail.z,
                    window_summary.horizontal_m[0],
                    window_summary.horizontal_m[1],
                    window_summary.height_m[0],
                    window_summary.corrected_share * PERCENT
                );
            }
            for ((part, bounds), most) in PARTS.into_iter().zip(most_corrected) {
                let inside: Vec<Compared> = compared
                    .iter()
                    .filter(|place| bounds(place.retail))
                    .copied()
                    .collect();
                let part_summary = summarize(&inside, 0);
                println!("  {part}: {part_summary:?}");
                if let Some(most) = most {
                    assert!(
                        part_summary.corrected_share <= most,
                        "{ledger}, {part}: {} of the places pull the client (at most {most})",
                        part_summary.corrected_share
                    );
                }
            }
            let mut worst: Vec<&Compared> = compared.iter().collect();
            worst.sort_by(|a, b| {
                b.horizontal_m()
                    .hypot(b.height_m())
                    .total_cmp(&a.horizontal_m().hypot(a.height_m()))
            });
            for place in worst.iter().take(10) {
                println!("  worst {place:?}");
            }
            assert!(summary.compared > 0, "{ledger}: nothing compared");
        }
    }
}
