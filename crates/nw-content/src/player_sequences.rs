// SPDX-License-Identifier: AGPL-3.0-or-later
//! The player slayer sequence table (`player-sequences-<build>.json`; run-15 lock and aim study,
//! 2026-09-22): which slayer states and sequences the server starts for a player, by weapon
//! stance, the way the retail server did in the capture ledgers of the pinned client build.
//!
//! The client numbers its slayer sequences at run time (no pak file holds the table, and neither
//! the fragment order of the animation databases nor the order of the player's animations gives
//! the numbers), so the table holds the numbers the retail server sent: mined from the own ALC
//! states (children 13 to 24 and 48) against the own inputs and the own weapon of 740 ledgers
//! (`local-data/scratch-work/run15/lockaim2/seqmine.py` and `gentable.py`; the aimed shot and the
//! reload of the gun and bow stances: `lockaim3/aimtable.py`). Each value is the most common one
//! of its case. Match rates on the October ledgers (the attack numbers moved between
//! builds; the right-click, dodge, jump, draw and movement numbers held): light starts 62.6 %
//! (228 of 364), charge starts 97.8 % (572 of 585), follow-ups 82.2 % (2,353 of 2,861).
//!
//! What the client leaves to the server (Observed, retail newchar capture 2026-09-18): a tap of
//! the attack key from no attack starts the light attack at the release record; a hold of
//! [`PlayerSequences::hold_records`] records starts the charge; a dodge, a jump, a weapon draw,
//! the block and the aim (right-click) and every movement state are the server's. The client
//! starts the attacks of a combo and the heavy attack itself (input row 1), and the server
//! follows some of them ([`FollowUp`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::player_attacks::stance_key;

const FILE_PREFIX: &str = "player-sequences-";
const FILE_SUFFIX: &str = ".json";

/// The path of the table of `client_build` in `dir`.
pub fn player_sequences_path(dir: &Path, client_build: &str) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{client_build}{FILE_SUFFIX}"))
}

/// A bad table file.
#[derive(Debug, thiserror::Error)]
pub enum PlayerSequencesError {
    /// The file cannot be read.
    #[error("player sequence table {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not the table's JSON.
    #[error("player sequence table: {0}")]
    Json(#[from] serde_json::Error),
}

/// A slayer state and the sequence it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatePlay {
    /// `slayerStateId`.
    pub state: u64,
    /// `slayerSequenceId`.
    pub sequence: u64,
}

/// The slayer children past the first slot that one server reaction sends: the second slot
/// (children 16 and 18; the state as on the wire, one more than the client's), the third (19 and
/// 21), the fourth (22 and 24) and the slayer marker (child 48). `None`: not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SlotsChange {
    /// Child 16.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_state: Option<u64>,
    /// Child 18.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_sequence: Option<u64>,
    /// Child 19.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third_state: Option<u64>,
    /// Child 21.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third_sequence: Option<u64>,
    /// Child 22.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fourth_state: Option<u64>,
    /// Child 24.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fourth_sequence: Option<u64>,
    /// Child 48.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<u64>,
}

impl SlotsChange {
    /// Whether it sends no child.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The first-slot movement states while the right-click is held (block or aim walk).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AltMoves {
    /// While a movement key is held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moving: Option<StatePlay>,
    /// Standing still.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle: Option<StatePlay>,
}

/// The first-slot movement sequences of the weapon drawn (the sheathed ones are the sim's own,
/// from the newchar capture). A missing one keeps the sheathed sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DrawnMoves {
    /// A move from standing still, forward or to the side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<StatePlay>,
    /// A move from standing still, back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back_start: Option<StatePlay>,
    /// The keys let go.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<u64>,
    /// A key again while stopping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<u64>,
    /// Stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle: Option<u64>,
}

/// Where the movement keys point when a dodge or a jump starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Forward (with or without a side key).
    Forward,
    /// Back.
    Back,
    /// Left alone.
    Left,
    /// Right alone.
    Right,
    /// No movement key.
    None,
}

impl Direction {
    /// The direction of the held keys.
    pub const fn of_keys(forward: bool, back: bool, left: bool, right: bool) -> Self {
        if back {
            Self::Back
        } else if forward {
            Self::Forward
        } else if left {
            Self::Left
        } else if right {
            Self::Right
        } else {
            Self::None
        }
    }
}

/// A first-slot state the server starts for a key, by direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Directional {
    /// Forward.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward: Option<StatePlay>,
    /// Back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back: Option<StatePlay>,
    /// Left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<StatePlay>,
    /// Right.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<StatePlay>,
    /// No key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub none: Option<StatePlay>,
}

impl Directional {
    /// The state of `direction`, else of no key, else of forward (best guess: a direction the
    /// ledgers did not show plays the stance's default one).
    pub fn of(&self, direction: Direction) -> Option<StatePlay> {
        let exact = match direction {
            Direction::Forward => self.forward,
            Direction::Back => self.back,
            Direction::Left => self.left,
            Direction::Right => self.right,
            Direction::None => self.none,
        };
        exact.or(self.none).or(self.forward)
    }
}

/// The server's sequences of one weapon stance.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StanceSequences {
    /// The weapon's `MannequinTag` cell (empty: no weapon).
    pub tags: String,
    /// The light attack the server starts at the release of a tap, standing still.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<u64>,
    /// The same while a movement key is held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_moving: Option<u64>,
    /// The charge the server starts when the key is held [`PlayerSequences::hold_records`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charge: Option<u64>,
    /// What a right-click press starts (block for a melee weapon, aim for a ranged one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_start: Option<SlotsChange>,
    /// What its release sends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_end: Option<SlotsChange>,
    /// The movement states while it is held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_moves: Option<AltMoves>,
    /// The movement sequences of the weapon drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawn_moves: Option<DrawnMoves>,
    /// The dodge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dodge: Option<Directional>,
    /// The jump.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump: Option<Directional>,
    /// The draw: the second slot's state (wire value) and sequence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw: Option<StatePlay>,
    /// What an attack key press with the right-click held sends while a shot is loaded: the
    /// aimed shot (retail blunderbuss `18=5e 48=29`, 30 to 80 ms after the press).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_fire: Option<SlotsChange>,
    /// What the first attack key press sends after the weapon emptied and was loaded again: the
    /// reload (retail blunderbuss `16=24 17=<record> 18=8142`, 1.5 to 1.9 s after the last shot;
    /// that press fires nothing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reload: Option<SlotsChange>,
}

impl StanceSequences {
    /// The light attack, moving or standing; one of them when the other is not known.
    pub fn light_of(&self, moving: bool) -> Option<u64> {
        if moving {
            self.light_moving.or(self.light)
        } else {
            self.light.or(self.light_moving)
        }
    }
}

/// A server follow-up of an action the client started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowUp {
    /// The client's sequence (input row 1).
    pub client: u64,
    /// The sequence the server starts after it.
    pub server: u64,
    /// The server starts it at the attack key's next release (else one own state after).
    pub at_release: bool,
    /// How many times the ledgers showed the pair.
    pub seen: u32,
}

/// The file form of the table.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
struct TableFile {
    hold_records: u16,
    stances: Vec<StanceSequences>,
    follow_ups: Vec<FollowUp>,
}

/// The table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerSequences {
    /// Input records the attack key is held before the server starts the charge (Observed:
    /// every retail charge start came 19 records after the press, 572 of 585 with the table's
    /// sequence).
    pub hold_records: u16,
    by_key: HashMap<u32, StanceSequences>,
    follow_ups: HashMap<u64, FollowUp>,
}

impl PlayerSequences {
    /// Reads the table of `client_build` in `dir`.
    ///
    /// # Errors
    ///
    /// Returns [`PlayerSequencesError`] when the file cannot be read or is not the table.
    pub fn load(dir: &Path, client_build: &str) -> Result<Self, PlayerSequencesError> {
        let path = player_sequences_path(dir, client_build);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| PlayerSequencesError::Io { path, source })?;
        Self::parse(&text)
    }

    /// The table in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`PlayerSequencesError::Json`] when the text is not the table.
    pub fn parse(text: &str) -> Result<Self, PlayerSequencesError> {
        let file: TableFile = serde_json::from_str(text)?;
        Ok(Self::of_parts(
            file.hold_records,
            file.stances,
            file.follow_ups,
        ))
    }

    /// The table of `stances` and `follow_ups`, with the charge after `hold_records`.
    pub fn of_parts(
        hold_records: u16,
        stances: Vec<StanceSequences>,
        follow_ups: Vec<FollowUp>,
    ) -> Self {
        Self {
            hold_records,
            by_key: stances
                .into_iter()
                .map(|stance| (stance_key(&stance.tags), stance))
                .collect(),
            follow_ups: follow_ups
                .into_iter()
                .map(|follow_up| (follow_up.client, follow_up))
                .collect(),
        }
    }

    /// The sequences of the stance of key `key` ([`stance_key`]).
    pub fn stance(&self, key: u32) -> Option<&StanceSequences> {
        self.by_key.get(&key)
    }

    /// The server's follow-up of the client's sequence `client`.
    pub fn follow_up(&self, client: u64) -> Option<FollowUp> {
        self.follow_ups.get(&client).copied()
    }

    /// Stances in the table.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// Whether it has no stance.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two stances and a follow-up as the table file holds them (values of the mined table).
    const TABLE: &str = r#"{
        "hold_records": 19,
        "stances": [
            {"tags": "2H_Melee,GreatSword", "light": 4510, "light_moving": 4510, "charge": 57,
             "alt_start": {"second_sequence": 2351},
             "alt_moves": {"moving": {"state": 27, "sequence": 1251},
                           "idle": {"state": 31, "sequence": 1673}},
             "dodge": {"forward": {"state": 7, "sequence": 4007},
                       "back": {"state": 7, "sequence": 2100}}},
            {"tags": "2H_Ranged,Blunderbuss", "light": 461,
             "alt_start": {"second_sequence": 4248, "third_state": 46, "third_sequence": 407,
                           "marker": 40},
             "alt_end": {"third_state": 0, "third_sequence": 0, "marker": 0},
             "alt_fire": {"second_sequence": 94, "marker": 41},
             "reload": {"second_state": 36, "second_sequence": 4225}}
        ],
        "follow_ups": [{"client": 5051, "server": 484, "at_release": true, "seen": 9}]
    }"#;

    #[test]
    fn the_table_reads_by_stance_key() {
        let table = PlayerSequences::parse(TABLE).expect("table");
        assert_eq!(table.hold_records, 19);
        assert_eq!(table.len(), 2);
        let sword = table
            .stance(stance_key("2H_Melee,GreatSword"))
            .expect("greatsword");
        assert_eq!(sword.charge, Some(57));
        assert_eq!(sword.light_of(true), Some(4510));
        assert_eq!(
            sword.alt_start.map(|change| change.second_sequence),
            Some(Some(2351))
        );
        let gun = table
            .stance(stance_key("2H_Ranged,Blunderbuss"))
            .expect("blunderbuss");
        assert_eq!(
            gun.light_of(true),
            Some(461),
            "standing value when moving is unknown"
        );
        let aim = gun.alt_start.expect("aim");
        assert_eq!((aim.third_state, aim.third_sequence), (Some(46), Some(407)));
        assert!(!aim.is_empty());
        let shot = gun.alt_fire.expect("the aimed shot");
        assert_eq!((shot.second_sequence, shot.marker), (Some(94), Some(41)));
        let reload = gun.reload.expect("the reload");
        assert_eq!(
            (reload.second_state, reload.second_sequence),
            (Some(36), Some(4225))
        );
        assert_eq!(sword.alt_fire, None, "a melee weapon blocks");
        assert_eq!(
            table.follow_up(5051),
            Some(FollowUp {
                client: 5051,
                server: 484,
                at_release: true,
                seen: 9
            })
        );
        assert_eq!(table.follow_up(1), None);
    }

    #[test]
    fn a_direction_the_ledgers_did_not_show_plays_the_default() {
        let table = PlayerSequences::parse(TABLE).expect("table");
        let dodge = table
            .stance(stance_key("2H_Melee,GreatSword"))
            .and_then(|stance| stance.dodge)
            .expect("dodge");
        assert_eq!(
            dodge.of(Direction::Back).map(|play| play.sequence),
            Some(2100)
        );
        assert_eq!(
            dodge.of(Direction::Left).map(|play| play.sequence),
            Some(4007)
        );
        assert_eq!(
            Direction::of_keys(true, false, true, false),
            Direction::Forward
        );
        assert_eq!(
            Direction::of_keys(false, false, false, true),
            Direction::Right
        );
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let missing = PlayerSequences::load(Path::new("/nonexistent"), "0");
        assert!(matches!(missing, Err(PlayerSequencesError::Io { .. })));
    }
}
