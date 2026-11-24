// SPDX-License-Identifier: AGPL-3.0-or-later
//! A mob's way around solids: a search over the level's floors when its straight way is closed.
//!
//! The server has no navigation mesh (no file of the client holds the retail server's), so the
//! search walks a grid of [`CELL_M`] cells around the mob on the floors the mob may stand on: each
//! step between two cells is a [`floor_line`] (the slope, the step height, the walls at knee
//! height, the edges, and [`nw_sim_core::Ground::no_stand`]: the solids, the level designers'
//! navigation marks such as a town gate, deep water). A cell has one place per floor: a bridge
//! and the ground under it are two places. The search is A* with the straight distance as its
//! estimate, held to [`SEARCH_RADIUS_M`] around the mob and [`MAX_EXPANDED`] cells (our
//! engineering limits: a sim runs hundreds of mobs).
//!
//! A goal it cannot reach gives the way to the place nearest the goal that it can reach (a
//! partial way): the mob goes there and stands, and its tree decides when to give up (the
//! trees' `Time_Since_Target_Reachable` against their `TargetUnreachableResetTime`).
//!
//! The live sim runs the searches off the tick ([`PathPool`]): a mob asks, stands, and walks the
//! way from the tick its answer comes in.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::atomic::{self, AtomicU64};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use nw_sim_core::{Ground, Position};

use crate::mob_ai::floor_line;

/// The side of a search cell, m (Inferred: a mob's body is about 1 m wide, the default capsule's
/// radius 0.5 m; a gap narrower than that holds no mob).
pub const CELL_M: f32 = 1.0;
/// How far from the mob the search goes, m (our engineering limit).
pub const SEARCH_RADIUS_M: f32 = 48.0;
/// The most cells one search takes from its queue (our engineering limit: a search of a few
/// milliseconds).
pub const MAX_EXPANDED: usize = 2_000;
/// The height of one floor band of a cell, m (our engineering limit: two floors of one cell are
/// at least a body's height apart).
const FLOOR_BAND_M: f32 = 1.5;
/// A goal this near (ground plane) ends the search, with a straight floor line to it.
pub const GOAL_REACH_M: f32 = CELL_M * 1.5;
/// A goal this far above or below a place is on another floor, m.
const GOAL_HEIGHT_M: f32 = FLOOR_BAND_M;
/// The furthest a way is pulled straight in one look, in points (our engineering limit).
const PULL_LOOKAHEAD: usize = 12;
/// The eight neighbours of a cell.
const NEIGHBOURS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// A way a search found.
#[derive(Debug, Clone, PartialEq)]
pub struct PathPlan {
    /// The goal it was searched for.
    pub goal: Position,
    /// Its places after the start, the last at the goal (or nearest to it).
    pub points: Vec<Position>,
    /// The next place to walk to.
    pub next: usize,
    /// It ends at the goal (`false`: at the place nearest to it).
    pub complete: bool,
}

impl PathPlan {
    /// The next place to walk to; `None` once the way is walked.
    pub fn next_point(&self) -> Option<Position> {
        self.points.get(self.next).copied()
    }

    /// The next place is reached.
    pub const fn advance(&mut self) {
        self.next += 1;
    }

    /// The way is walked.
    pub fn walked(&self) -> bool {
        self.next >= self.points.len()
    }
}

/// A cell of the search: its column, row and floor band.
type CellKey = (i32, i32, i32);

/// A place the search reached.
#[derive(Debug, Clone, Copy)]
struct Node {
    at: Position,
    cost: f32,
    parent: Option<usize>,
}

/// A node in the queue by its estimated total.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Queued {
    estimate: f32,
    node: usize,
}

impl Eq for Queued {}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> Ordering {
        other.estimate.total_cmp(&self.estimate)
    }
}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The way over the floors of `ground` from `from` to `to` for a mob of the slope limit
/// `max_slope_deg`; a partial way to the reachable place nearest `to` when `to` cannot be
/// reached. `None` when no step from `from` gets nearer to `to`.
pub fn find_path(
    ground: &dyn Ground,
    from: Position,
    to: Position,
    max_slope_deg: Option<f32>,
) -> Option<PathPlan> {
    // Cells of a search of tens of metres.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "cells of a search of tens of metres"
    )]
    let key_of = |at: Position| -> CellKey {
        (
            ((at.x - from.x) / CELL_M).round() as i32,
            ((at.y - from.y) / CELL_M).round() as i32,
            ((at.z - from.z) / FLOOR_BAND_M).round() as i32,
        )
    };
    // Cells of a search of tens of metres.
    #[allow(
        clippy::cast_precision_loss,
        reason = "cells of a search of tens of metres"
    )]
    let centre_of = |column: i32, row: i32, z: f32| Position {
        x: from.x + column as f32 * CELL_M,
        y: from.y + row as f32 * CELL_M,
        z,
    };
    let estimate = |at: Position| at.horizontal_distance(to);
    let goal_closed_by_map = ground.closed_by_walk_map(to);
    let mut nodes = vec![Node {
        at: from,
        cost: 0.0,
        parent: None,
    }];
    let mut best_at: HashMap<CellKey, usize> = HashMap::from([(key_of(from), 0)]);
    let mut queue = BinaryHeap::from([Queued {
        estimate: estimate(from),
        node: 0,
    }]);
    let mut nearest = 0;
    let mut expanded = 0;
    while let Some(Queued { node, .. }) = queue.pop() {
        if expanded >= MAX_EXPANDED {
            break;
        }
        expanded += 1;
        let here = nodes[node];
        if estimate(here.at) < estimate(nodes[nearest].at) {
            nearest = node;
        }
        // A goal that only the walk map closes (a player beside a wall: a mob's body does not
        // fit there) is reached at the place of the map nearest to it: the mob's attacks reach
        // from there (live sim test 2026-11-24: the beach officer dropped a player who stood
        // 10 m away on such a place, the way ended 1 m short of it).
        let near_goal = here.at.horizontal_distance(to) <= GOAL_REACH_M
            && (here.at.z - to.z).abs() <= GOAL_HEIGHT_M;
        if near_goal && goal_closed_by_map {
            return Some(plan(ground, &nodes, node, to, true, max_slope_deg));
        }
        if near_goal && let Some(end) = floor_line(ground, here.at, to, max_slope_deg) {
            nodes.push(Node {
                at: end,
                cost: here.cost + here.at.horizontal_distance(end),
                parent: Some(node),
            });
            return Some(plan(
                ground,
                &nodes,
                nodes.len() - 1,
                to,
                true,
                max_slope_deg,
            ));
        }
        let (column, row, _) = key_of(here.at);
        for (step_column, step_row) in NEIGHBOURS {
            let cell = centre_of(column + step_column, row + step_row, here.at.z);
            if cell.horizontal_distance(from) > SEARCH_RADIUS_M {
                continue;
            }
            let Some(end) = floor_line(ground, here.at, cell, max_slope_deg) else {
                continue;
            };
            let cost = here.cost + here.at.horizontal_distance(end) + (end.z - here.at.z).abs();
            let key = key_of(end);
            if best_at
                .get(&key)
                .is_some_and(|&known| nodes[known].cost <= cost)
            {
                continue;
            }
            nodes.push(Node {
                at: end,
                cost,
                parent: Some(node),
            });
            let index = nodes.len() - 1;
            best_at.insert(key, index);
            queue.push(Queued {
                estimate: cost + estimate(end),
                node: index,
            });
        }
    }
    (estimate(nodes[nearest].at) + CELL_M < estimate(from))
        .then(|| plan(ground, &nodes, nearest, to, false, max_slope_deg))
}

/// The way to node `last`, pulled straight where a floor line allows it.
fn plan(
    ground: &dyn Ground,
    nodes: &[Node],
    last: usize,
    goal: Position,
    complete: bool,
    max_slope_deg: Option<f32>,
) -> PathPlan {
    let mut chain = Vec::new();
    let mut next = Some(last);
    while let Some(index) = next {
        chain.push(nodes[index].at);
        next = nodes[index].parent;
    }
    chain.reverse();
    let mut points = Vec::new();
    let mut at = 0;
    while at + 1 < chain.len() {
        let furthest = (at + 2..chain.len().min(at + 1 + PULL_LOOKAHEAD))
            .rev()
            .find(|&ahead| floor_line(ground, chain[at], chain[ahead], max_slope_deg).is_some())
            .unwrap_or(at + 1);
        points.push(chain[furthest]);
        at = furthest;
    }
    PathPlan {
        goal,
        points,
        next: 0,
        complete,
    }
}

/// The name of a thread of a [`PathPool`].
const PATH_THREAD_NAME: &str = "path-search";

/// A way search a mob asked off the tick.
#[derive(Debug, Clone, Copy)]
struct PathJob {
    mob: u64,
    id: u64,
    from: Position,
    to: Position,
    max_slope_deg: Option<f32>,
    asked: Instant,
}

/// The answer to ask `id` of mob `mob`: the way [`find_path`] found.
#[derive(Debug, Clone, PartialEq)]
pub struct PathAnswer {
    /// The mob's entity.
    pub mob: u64,
    /// The ask ([`PathPool::ask`]).
    pub id: u64,
    /// The way.
    pub plan: Option<PathPlan>,
}

/// Way searches off the tick, on threads of their own over the mobs' ground. A search that finds
/// no way walks all its [`MAX_EXPANDED`] cells; on the tick, one such search held every mob of
/// the sim up to 325 ms (open world, load run of 110 players, 2026-11-15). An ask older than the
/// pool's life is dropped unsearched: its mob gave it up and asks again.
pub struct PathPool {
    jobs: mpsc::Sender<PathJob>,
    answers: Mutex<mpsc::Receiver<PathAnswer>>,
    next_id: AtomicU64,
}

impl PathPool {
    /// A pool of `threads` threads (at least one) over `ground`, its asks living `life`.
    ///
    /// # Errors
    ///
    /// The system refused a thread.
    pub fn new(
        ground: Arc<dyn Ground + Send + Sync>,
        threads: usize,
        life: Duration,
    ) -> std::io::Result<Self> {
        let (jobs, queue) = mpsc::channel();
        let (done, answers) = mpsc::channel();
        let queue = Arc::new(Mutex::new(queue));
        for _ in 0..threads.max(1) {
            let (queue, done, ground) = (Arc::clone(&queue), done.clone(), Arc::clone(&ground));
            std::thread::Builder::new()
                .name(PATH_THREAD_NAME.to_owned())
                .spawn(move || search_jobs(&queue, &done, &*ground, life))?;
        }
        Ok(Self {
            jobs,
            answers: Mutex::new(answers),
            next_id: AtomicU64::new(0),
        })
    }

    /// Asks the way of mob `mob` from `from` to `to` for its slope limit `max_slope_deg`; the
    /// ask's id, which its answer carries ([`Self::answers`]).
    pub fn ask(&self, mob: u64, from: Position, to: Position, max_slope_deg: Option<f32>) -> u64 {
        let id = self.next_id.fetch_add(1, atomic::Ordering::Relaxed);
        let job = PathJob {
            mob,
            id,
            from,
            to,
            max_slope_deg,
            asked: Instant::now(),
        };
        if self.jobs.send(job).is_err() {
            // Every thread ended (a search panicked): the mob gets no answer and asks again.
            tracing::warn!(mob, "no path search thread is left");
        }
        id
    }

    /// The answers that came in since the last call.
    pub fn answers(&self) -> Vec<PathAnswer> {
        self.answers
            .lock()
            .map(|answers| answers.try_iter().collect())
            .unwrap_or_default()
    }
}

/// A thread of a [`PathPool`]: it searches the asks of `queue` over `ground` and sends their
/// answers to `done`, until the pool is gone.
fn search_jobs(
    queue: &Mutex<mpsc::Receiver<PathJob>>,
    done: &mpsc::Sender<PathAnswer>,
    ground: &dyn Ground,
    life: Duration,
) {
    loop {
        let job = match queue.lock() {
            Ok(queue) => queue.recv(),
            Err(_) => return,
        };
        let Ok(job) = job else {
            return;
        };
        if job.asked.elapsed() > life {
            continue;
        }
        let answer = PathAnswer {
            mob: job.mob,
            id: job.id,
            plan: find_path(ground, job.from, job.to, job.max_slope_deg),
        };
        if done.send(answer).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUND_Z: f32 = 10.0;

    fn at(x: f32, y: f32) -> Position {
        Position { x, y, z: GROUND_Z }
    }

    /// Flat ground with walls: a mob may not stand inside any of `walls` (x and y ranges).
    struct Walls(Vec<([f32; 2], [f32; 2])>);

    impl Ground for Walls {
        fn height_at(&self, _: f32, _: f32) -> Option<f32> {
            Some(GROUND_Z)
        }

        fn no_stand(&self, feet: Position) -> bool {
            self.0
                .iter()
                .any(|(x, y)| (x[0]..=x[1]).contains(&feet.x) && (y[0]..=y[1]).contains(&feet.y))
        }
    }

    /// Two walls across the straight way, with gaps at opposite ends: no one turn passes.
    fn two_walls() -> Walls {
        Walls(vec![
            ([4.0, 5.0], [-20.0, 6.0]),
            ([9.0, 10.0], [-6.0, 20.0]),
        ])
    }

    fn walk_is_free(ground: &dyn Ground, from: Position, plan: &PathPlan) -> bool {
        let mut last = from;
        plan.points.iter().all(|point| {
            let free = floor_line(ground, last, *point, None).is_some();
            last = *point;
            free
        })
    }

    #[test]
    fn a_way_goes_around_two_walls_that_no_one_turn_passes() {
        let ground = two_walls();
        let (from, to) = (at(0.0, 0.0), at(14.0, 0.0));
        assert!(
            floor_line(&ground, from, to, None).is_none(),
            "the straight way is closed"
        );
        let plan = find_path(&ground, from, to, None).expect("a way");
        assert!(plan.complete);
        let end = *plan.points.last().expect("points");
        assert!(end.horizontal_distance(to) < 0.1, "{end:?}");
        assert!(walk_is_free(&ground, from, &plan));
        assert!(
            plan.points.len() < 12,
            "pulled straight: {}",
            plan.points.len()
        );
    }

    #[test]
    fn a_closed_goal_gives_the_way_to_the_place_nearest_it() {
        // A box of walls around the goal; its west wall is the nearest to the goal.
        let ground = Walls(vec![
            ([18.0, 19.0], [-6.0, 6.0]),
            ([25.0, 26.0], [-6.0, 6.0]),
            ([18.0, 26.0], [5.0, 6.0]),
            ([18.0, 26.0], [-6.0, -5.0]),
        ]);
        let (from, to) = (at(0.0, 0.0), at(22.0, 0.0));
        let plan = find_path(&ground, from, to, None).expect("a partial way");
        assert!(!plan.complete);
        let end = *plan.points.last().expect("points");
        assert!(end.x < 18.0 && end.x > 15.5, "stands at the wall: {end:?}");
        assert!(walk_is_free(&ground, from, &plan));
    }

    #[test]
    fn no_step_nearer_gives_no_way_and_a_walked_way_ends() {
        let ground = Walls(vec![([-2.0, 2.0], [-2.0, 2.0])]);
        // The mob stands in a closed place it may walk out of: the goal is behind it.
        let shut = Walls(vec![([0.5, 100.0], [-100.0, 100.0])]);
        assert!(find_path(&shut, at(0.0, 0.0), at(10.0, 0.0), None).is_none());
        let mut plan = find_path(&ground, at(-6.0, 0.0), at(6.0, 0.0), None).expect("a way");
        while plan.next_point().is_some() {
            plan.advance();
        }
        assert!(plan.walked());
    }

    /// How long a test waits for the answers of a pool, at most.
    const ANSWER_WAIT: Duration = Duration::from_secs(10);
    /// How often a test looks for the answers of a pool.
    const ANSWER_POLL: Duration = Duration::from_millis(5);

    /// The first `count` answers of `pool`, waited for up to [`ANSWER_WAIT`].
    fn answers_of(pool: &PathPool, count: usize) -> Vec<PathAnswer> {
        let until = Instant::now() + ANSWER_WAIT;
        let mut answers = Vec::new();
        while answers.len() < count && Instant::now() < until {
            answers.extend(pool.answers());
            std::thread::sleep(ANSWER_POLL);
        }
        answers
    }

    #[test]
    fn a_pool_gives_each_ask_the_way_of_a_search_on_the_tick() {
        const MOB: u64 = 9;
        const THREADS: usize = 2;
        let pool = PathPool::new(Arc::new(two_walls()), THREADS, ANSWER_WAIT).expect("a pool");
        let asks = [(at(0.0, 0.0), at(14.0, 0.0)), (at(-6.0, 0.0), at(6.0, 0.0))];
        let ids: Vec<u64> = asks
            .iter()
            .map(|&(from, to)| pool.ask(MOB, from, to, None))
            .collect();
        let mut answers = answers_of(&pool, asks.len());
        answers.sort_by_key(|answer| answer.id);
        assert_eq!(answers.len(), asks.len(), "every ask is answered");
        for ((answer, id), (from, to)) in answers.iter().zip(ids).zip(asks) {
            assert_eq!((answer.mob, answer.id), (MOB, id));
            assert_eq!(answer.plan, find_path(&two_walls(), from, to, None));
        }
    }

    #[test]
    fn a_pool_drops_an_ask_older_than_its_life() {
        let pool = PathPool::new(Arc::new(two_walls()), 1, Duration::ZERO).expect("a pool");
        pool.ask(1, at(0.0, 0.0), at(14.0, 0.0), None);
        std::thread::sleep(ANSWER_POLL * 20);
        assert_eq!(pool.answers(), Vec::new(), "no answer to an old ask");
    }
}
