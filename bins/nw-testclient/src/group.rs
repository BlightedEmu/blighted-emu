// SPDX-License-Identifier: AGPL-3.0-or-later
//! `nw-testclient expedition`: the bots of game master accounts form a group and enter an
//! expedition together, as a group of players does, then walk its start area.
//!
//! An expedition opens only for a group of at least its game mode's `MinGroupSize` (the world
//! director refuses a smaller one with `NotEnoughMembers`). The first bot leads: once each other
//! bot is in the world it invites them (3492, `nw_messages::social::groups`), each other bot
//! accepts the invite its own groups state shows (3495; a bot whose groups state already names
//! a group, kept from an earlier run, takes none), and once every one of them is in the group
//! the leader enters from afar ([`crate::activity::trial_steps`]: 5039, 5037, 946). The
//! others ask to enter once the leader is in the instance: the director adds a member of the
//! group to the group's open instance (a request before the instance is ready waits).
//!
//! In the instance each bot asks the sim where its body stands (`!where`), walks each leg of
//! [`WALK_LEGS`] from where it came in, and asks again after each leg. The report sets each
//! place the server gave against the level's floor ([`floor_lines`]: `floor_at` of the level's
//! collision file with the solids of its standing gates, as the sim's ground has them).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use nw_messages::MessageError;
use nw_messages::social::SocialFacet;
use nw_messages::social::groups::{GroupCall, REPLY_ACCEPT};
use nw_messages::wire::RAW_ID_LEN;
use nw_world::collision_index::CollisionIndex;

use crate::activity::{Act, Proof, Step, trial_steps};
use crate::pair::RecordWalk;

/// The seat of the bot that leads the group.
pub const LEADER: usize = 0;
/// The legs each bot walks in the instance, metres east and north of where it came in: out and
/// back on both axes, a short walk over the floor of the start area.
pub const WALK_LEGS: [[f32; 2]; 5] = [[6.0, 0.0], [6.0, 6.0], [0.0, 6.0], [-6.0, 0.0], [0.0, 0.0]];
/// How long the bots wait for each other to be in the world and in the group: a new
/// character first leaves the tutorial (about 30 s) and takes its game master setup.
const GROUP_WAIT: Duration = Duration::from_secs(180);
/// How long a member waits for the leader to be in the instance: the leader's three entry
/// steps, with a director that starts an instance sim first (up to 60 s).
const LEADER_WAIT: Duration = Duration::from_secs(150);
/// How long one leg may take: [`WALK_LEGS`] at the bots' walking speed, with room.
const LEG_WAIT: Duration = Duration::from_secs(20);
/// How long a playing member waits per seat before its quests: each bot's first `!questplan`
/// holds the sim's tick about a second (live 2026-11-23 02:43:54-02:43:58Z: five plans of
/// `1689_EXP_Coins_Objective` 0.92 s apart, ticks ran late, then "gateway silent; its players
/// leave"), so the bots plan one after another. Inferred: a second of plan and room.
const MEMBER_PLAY_GAP: Duration = Duration::from_secs(4);
/// How long the answer to `!where` may take.
const WHERE_WAIT: Duration = Duration::from_secs(10);
/// The game master line that answers with the sender's server place.
const WHERE: &str = "!where";
/// The last byte of an invite: 0 in every retail invite.
const INVITE_FLAG: u8 = 0;
/// A place differs from the floor by more than this, metres: the step-up height of the level
/// collision (a floor up to it above the feet is still the floor; level-collision note 4.2).
pub const OFF_FLOOR_M: f32 = 0.25;

/// What one bot of the group has done, for the others.
#[derive(Debug, Clone, Default, PartialEq)]
struct SeatState {
    /// Its character id (UUID text), from its login token.
    character: Option<String>,
    /// It is in the world and waits for its invite.
    ready: bool,
    /// Its groups state names its group.
    joined: bool,
    /// The places the server gave it in the instance, in order.
    places: Vec<[f32; 3]>,
}

/// What the bots of one group tell each other while they run.
#[derive(Debug, Default)]
pub struct GroupBoard {
    seats: Mutex<Vec<SeatState>>,
    leader_inside: AtomicBool,
    leader_done: AtomicBool,
}

/// One bot's seat on a [`GroupBoard`].
#[derive(Debug, Clone)]
pub struct GroupSeat {
    board: Arc<GroupBoard>,
    seat: usize,
}

impl GroupBoard {
    /// The seats of a group of `bots` bots; the first leads.
    pub fn seats(bots: usize) -> Vec<GroupSeat> {
        let board = Arc::new(Self {
            seats: Mutex::new(vec![SeatState::default(); bots]),
            leader_inside: AtomicBool::new(false),
            leader_done: AtomicBool::new(false),
        });
        (0..bots)
            .map(|seat| GroupSeat {
                board: Arc::clone(&board),
                seat,
            })
            .collect()
    }
}

impl GroupSeat {
    /// Runs `change` on the seats; a panic of another bot's thread leaves them readable.
    fn with<T>(&self, change: impl FnOnce(&mut Vec<SeatState>) -> T) -> T {
        let mut seats = self
            .board
            .seats
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        change(&mut seats)
    }

    /// Whether this bot leads the group.
    pub const fn leads(&self) -> bool {
        self.seat == LEADER
    }

    /// This bot's character id (UUID text) is `character`.
    pub fn set_character(&self, character: Option<String>) {
        self.with(|seats| seats[self.seat].character = character);
    }

    /// This bot is in the world and waits for its invite.
    pub fn set_ready(&self) {
        self.with(|seats| seats[self.seat].ready = true);
    }

    /// The characters of the other bots, once each of them is in the world; `None` before.
    pub fn members_ready(&self) -> Option<Vec<String>> {
        self.with(|seats| {
            seats
                .iter()
                .enumerate()
                .filter(|(seat, _)| *seat != self.seat)
                .map(|(_, state)| state.character.clone().filter(|_| state.ready))
                .collect()
        })
    }

    /// This bot's groups state names its group.
    pub fn set_joined(&self) {
        self.with(|seats| seats[self.seat].joined = true);
    }

    /// Whether every other bot is in the group.
    pub fn others_joined(&self) -> bool {
        self.with(|seats| {
            seats
                .iter()
                .enumerate()
                .all(|(seat, state)| seat == self.seat || state.joined)
        })
    }

    /// The leader is in the instance.
    pub fn set_leader_inside(&self) {
        self.board.leader_inside.store(true, Ordering::SeqCst);
    }

    /// Whether the leader is in the instance.
    pub fn leader_inside(&self) -> bool {
        self.board.leader_inside.load(Ordering::SeqCst)
    }

    /// The leader ended its play.
    pub fn set_leader_done(&self) {
        self.board.leader_done.store(true, Ordering::SeqCst);
    }

    /// Whether the leader ended its play.
    pub fn leader_done(&self) -> bool {
        self.board.leader_done.load(Ordering::SeqCst)
    }

    /// The server gave this bot the place `place`.
    pub fn record_place(&self, place: [f32; 3]) {
        self.with(|seats| seats[self.seat].places.push(place));
    }

    /// The first place the server gave this bot in the instance: where it came in.
    pub fn first_place(&self) -> Option<[f32; 3]> {
        self.with(|seats| seats[self.seat].places.first().copied())
    }

    /// The places the server gave this bot in the instance, in order.
    pub fn places(&self) -> Vec<[f32; 3]> {
        self.with(|seats| seats[self.seat].places.clone())
    }

    /// The seat's number: 0 for the leader.
    pub const fn number(&self) -> usize {
        self.seat
    }
}

/// The invite (3492) of the character `character` (UUID text) by the bot `own`: type index and
/// body.
///
/// # Errors
///
/// Returns [`MessageError`] when the call does not encode.
pub fn invite_call(character: String, own: u64) -> Result<(u64, Vec<u8>), MessageError> {
    let call = GroupCall::Invite {
        character,
        extra: [0; RAW_ID_LEN],
        flag: INVITE_FLAG,
    };
    Ok((
        call.type_index(),
        call.encode(SocialFacet::Groups.header(own))?,
    ))
}

/// The accept (3495) of the invite `invite` by the bot `own`: type index and body.
///
/// # Errors
///
/// Returns [`MessageError`] when the call does not encode.
pub fn accept_call(invite: [u8; RAW_ID_LEN], own: u64) -> Result<(u64, Vec<u8>), MessageError> {
    let call = GroupCall::Reply {
        invite,
        answer: REPLY_ACCEPT,
    };
    Ok((
        call.type_index(),
        call.encode(SocialFacet::Groups.header(own))?,
    ))
}

/// The place of an answer to `!where`: three numbers, east, north and height.
pub fn where_place(line: &str) -> Option<[f32; 3]> {
    let mut numbers = line.split_whitespace().map(str::parse::<f32>);
    let place = [
        numbers.next()?.ok()?,
        numbers.next()?.ok()?,
        numbers.next()?.ok()?,
    ];
    numbers.next().is_none().then_some(place)
}

/// A step that asks for the bot's server place and keeps it on its seat.
fn where_step(seat: &GroupSeat, what: String) -> Step {
    let keeper = seat.clone();
    Step {
        what,
        acts: vec![Act::Gm(WHERE.to_owned())],
        proof: Proof::Line(
            Box::new(move |line| {
                where_place(line)
                    .inspect(|place| keeper.record_place(*place))
                    .is_some()
            }),
            "a server place".to_owned(),
        ),
        wait: WHERE_WAIT,
    }
}

/// The leader's invite of the other bots, proved once each of them is in the group.
fn invite_step(seat: &GroupSeat) -> Step {
    Step {
        what: "invite the other bots into a group".to_owned(),
        acts: vec![Act::InviteGroup(seat.clone())],
        proof: Proof::GroupFull(seat.clone()),
        wait: GROUP_WAIT,
    }
}

/// A member's accept of the leader's invite, proved by its own groups state.
fn accept_step(seat: &GroupSeat) -> Step {
    Step {
        what: "accept the leader's invite".to_owned(),
        acts: vec![Act::AcceptGroupInvite(seat.clone())],
        proof: Proof::InGroup(seat.clone()),
        wait: GROUP_WAIT,
    }
}

/// The steps of the bot of `seat` in a group whose leader plays quests that need a group (an
/// expedition opens only for a group of its `MinGroupSize`): the leader invites the others and
/// then plays its quests (`crate::play::PlayData::activities_first`); a member accepts and stays
/// in the group, in the open world, until the leader's play ends or `hold` passes. With
/// `members_play` a member accepts, waits its turn ([`MEMBER_PLAY_GAP`] per seat) and then plays
/// the same quests: its entry of their
/// expedition joins the group's instance (the director adds a member of the group to the group's
/// open instance, or holds it until that instance is ready), so the whole group fights there.
pub fn quest_group_steps(seat: &GroupSeat, hold: Duration, members_play: bool) -> Vec<Step> {
    if seat.leads() {
        vec![invite_step(seat)]
    } else if members_play {
        vec![
            accept_step(seat),
            Step {
                what: "wait for the bots before it to plan".to_owned(),
                acts: Vec::new(),
                proof: Proof::Paused,
                wait: MEMBER_PLAY_GAP * u32::try_from(seat.number()).unwrap_or(u32::MAX),
            },
        ]
    } else {
        vec![
            accept_step(seat),
            Step {
                what: "stay in the group while the leader plays".to_owned(),
                acts: Vec::new(),
                proof: Proof::LeaderDone(seat.clone()),
                wait: hold,
            },
        ]
    }
}

/// The steps of the bot of `seat` in the group that enters the expedition of game mode `mode`
/// (the CRC-32 of its lowercase id `crc`) and walks with `records`: the group
/// (the leader invites, a member accepts and waits for the leader in the instance), the entry,
/// and the walk of [`WALK_LEGS`] with a server place before it and after each leg.
pub fn expedition_steps(
    seat: &GroupSeat,
    mode: &str,
    crc: u32,
    mutation: Option<u8>,
    records: &RecordWalk,
) -> Vec<Step> {
    let mut steps = if seat.leads() {
        vec![invite_step(seat)]
    } else {
        vec![
            accept_step(seat),
            Step {
                what: "wait for the leader in the instance".to_owned(),
                acts: Vec::new(),
                proof: Proof::LeaderInside(seat.clone()),
                wait: LEADER_WAIT,
            },
        ]
    };
    steps.extend(trial_steps(mode, crc, mutation));
    if seat.leads() {
        steps.push(Step {
            what: "tell the group the leader is in".to_owned(),
            acts: vec![Act::LeaderInside(seat.clone())],
            proof: Proof::Sent,
            wait: WHERE_WAIT,
        });
    }
    steps.push(where_step(
        seat,
        "the server place where it came in".to_owned(),
    ));
    for [east_m, north_m] in WALK_LEGS {
        steps.push(Step {
            what: format!("walk to {east_m} m east, {north_m} m north of the entry"),
            acts: vec![Act::Walk {
                seat: seat.clone(),
                east_m,
                north_m,
                records: records.clone(),
            }],
            proof: Proof::Sent,
            wait: LEG_WAIT,
        });
        steps.push(where_step(
            seat,
            format!("the server place at {east_m} m east, {north_m} m north"),
        ));
    }
    steps
}

/// One line per place of `places`: the place, the floor of `ground` under it and the height
/// of the place over that floor; and a last line with how many stand on the floor (within
/// [`OFF_FLOOR_M`]) and the largest height off it.
pub fn floor_lines(places: &[[f32; 3]], ground: &CollisionIndex) -> Vec<String> {
    let mut lines = Vec::new();
    let mut on_floor = 0;
    let mut largest: Option<f32> = None;
    for [x, y, z] in places {
        match ground.floor_at(*x, *y, *z) {
            Some(floor) => {
                let over = z - floor;
                if over.abs() <= OFF_FLOOR_M {
                    on_floor += 1;
                }
                largest = Some(largest.map_or(over.abs(), |most| most.max(over.abs())));
                lines.push(format!(
                    "- ({x:.2}, {y:.2}, {z:.2}): floor {floor:.2}, over it {over:+.3} m"
                ));
            }
            None => lines.push(format!("- ({x:.2}, {y:.2}, {z:.2}): no floor under it")),
        }
    }
    lines.push(format!(
        "- {on_floor} of {} places on the floor within {OFF_FLOOR_M} m; largest height off it: {}",
        places.len(),
        largest.map_or_else(|| "none".to_owned(), |most| format!("{most:.3} m"))
    ));
    lines
}

#[cfg(test)]
mod tests {
    use nw_world::collision::{Body, Geometry, IDENTITY_AFFINE, LevelCollision};

    use super::*;

    #[test]
    fn the_invite_and_the_accept_read_back_as_the_retail_calls() {
        use nw_messages::social::SocialCall;
        const OWN: u64 = 7;
        const CHARACTER: &str = "0b6f2a4e-1c3d-4e5f-8a9b-0c1d2e3f4a5b";
        const INVITE: [u8; RAW_ID_LEN] = [3; RAW_ID_LEN];
        let read = |(type_index, body): (u64, Vec<u8>)| {
            SocialCall::decode(type_index, &body)
                .expect("a social call")
                .expect("reads")
                .1
        };
        assert_eq!(
            read(invite_call(CHARACTER.to_owned(), OWN).expect("invite")),
            SocialCall::Group(GroupCall::Invite {
                character: CHARACTER.to_owned(),
                extra: [0; RAW_ID_LEN],
                flag: INVITE_FLAG,
            })
        );
        assert_eq!(
            read(accept_call(INVITE, OWN).expect("accept")),
            SocialCall::Group(GroupCall::Reply {
                invite: INVITE,
                answer: REPLY_ACCEPT,
            })
        );
    }

    #[test]
    fn a_where_answer_is_three_numbers() {
        assert_eq!(
            where_place("812.5 676.0 133.1"),
            Some([812.5, 676.0, 133.1])
        );
        assert_eq!(where_place("812.5 676.0"), None);
        assert_eq!(where_place("812.5 676.0 133.1 4"), None);
        assert_eq!(where_place("state level 65"), None);
    }

    #[test]
    fn the_leader_invites_once_every_other_bot_is_ready_and_then_waits_for_them() {
        let seats = GroupBoard::seats(3);
        let (leader, second, third) = (&seats[0], &seats[1], &seats[2]);
        assert!(leader.leads() && !second.leads());
        second.set_character(Some("b".to_owned()));
        second.set_ready();
        assert_eq!(
            leader.members_ready(),
            None,
            "the third bot is not in the world"
        );
        third.set_character(Some("c".to_owned()));
        assert_eq!(
            leader.members_ready(),
            None,
            "the third bot has no invite to wait for"
        );
        third.set_ready();
        assert_eq!(
            leader.members_ready(),
            Some(vec!["b".to_owned(), "c".to_owned()])
        );
        second.set_joined();
        assert!(!leader.others_joined());
        third.set_joined();
        assert!(leader.others_joined());
        assert!(!second.leader_inside());
        leader.set_leader_inside();
        assert!(third.leader_inside());
    }

    #[test]
    fn a_where_step_keeps_the_place_on_its_seat() {
        let seats = GroupBoard::seats(1);
        let step = where_step(&seats[0], "where".to_owned());
        let Proof::Line(takes, _) = &step.proof else {
            panic!("a line proof");
        };
        assert!(!takes("Done."));
        assert!(takes("1.0 2.0 3.0"));
        assert_eq!(seats[0].places(), [[1.0, 2.0, 3.0]]);
    }

    /// A keyboard walk over flat ground at 10 m.
    fn flat_walk() -> RecordWalk {
        RecordWalk {
            device: crate::record_walk::InputDevice::Keys,
            ground: Arc::new(nw_sim_core::test_support::FlatGround(10.0)),
            sea_level_m: 0.0,
        }
    }

    #[test]
    fn the_leader_enters_first_and_each_bot_walks_every_leg() {
        let seats = GroupBoard::seats(2);
        let what = |seat: &GroupSeat| -> Vec<String> {
            expedition_steps(seat, "DungeonTest", 1, None, &flat_walk())
                .into_iter()
                .map(|step| step.what)
                .collect()
        };
        let leader = what(&seats[0]);
        let member = what(&seats[1]);
        assert_eq!(leader[0], "invite the other bots into a group");
        assert_eq!(
            member[..2],
            [
                "accept the leader's invite",
                "wait for the leader in the instance"
            ]
        );
        assert!(leader.contains(&"tell the group the leader is in".to_owned()));
        let walks = |steps: &[String]| steps.iter().filter(|what| what.starts_with("walk")).count();
        assert_eq!(
            (walks(&leader), walks(&member)),
            (WALK_LEGS.len(), WALK_LEGS.len())
        );
        assert_eq!(
            leader
                .iter()
                .filter(|what| what.starts_with("the server place"))
                .count(),
            WALK_LEGS.len() + 1
        );
    }

    #[test]
    fn a_quest_group_leader_invites_and_a_member_stays_until_the_leader_is_done() {
        const HOLD: Duration = Duration::from_secs(600);
        let seats = GroupBoard::seats(3);
        let leader = quest_group_steps(&seats[0], HOLD, false);
        assert_eq!(leader.len(), 1);
        assert_eq!(leader[0].what, "invite the other bots into a group");
        let playing = quest_group_steps(&seats[2], HOLD, true);
        assert_eq!(playing[0].what, "accept the leader's invite");
        assert!(matches!(playing[1].proof, Proof::Paused));
        assert_eq!(
            playing[1].wait,
            MEMBER_PLAY_GAP * 2,
            "the third seat waits for two"
        );
        let member = quest_group_steps(&seats[2], HOLD, false);
        assert_eq!(member[0].what, "accept the leader's invite");
        assert!(matches!(&member[1].proof, Proof::LeaderDone(_)));
        assert_eq!(member[1].wait, HOLD);
        assert!(!seats[1].leader_done());
        seats[0].set_leader_done();
        assert!(seats[1].leader_done(), "every seat of the board sees it");
    }

    #[test]
    fn a_place_is_set_against_the_floor_under_it() {
        let collision = LevelCollision {
            geometries: vec![Geometry::Box {
                half_extents: [5.0, 5.0, 0.5],
            }],
            bodies: vec![Body {
                geometry: 0,
                place: IDENTITY_AFFINE,
            }],
            ..LevelCollision::default()
        };
        let ground = CollisionIndex::new(collision);
        let lines = floor_lines(
            &[[0.0, 0.0, 0.6], [1.0, 1.0, 1.5], [20.0, 0.0, 1.0]],
            &ground,
        );
        assert_eq!(
            lines[0],
            "- (0.00, 0.00, 0.60): floor 0.50, over it +0.100 m"
        );
        assert_eq!(
            lines[1],
            "- (1.00, 1.00, 1.50): floor 0.50, over it +1.000 m"
        );
        assert_eq!(lines[2], "- (20.00, 0.00, 1.00): no floor under it");
        assert_eq!(
            lines[3],
            "- 1 of 3 places on the floor within 0.25 m; largest height off it: 1.000 m"
        );
    }
}
