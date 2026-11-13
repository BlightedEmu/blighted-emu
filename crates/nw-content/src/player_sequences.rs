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
//! What the client leaves to the server (Observed, attack sequence study 2026-11-11 on the 103
//! ledgers whose item indexes resolve on the pinned build; `blighted-emu-tools`
//! `scratch-work/capdecode/src/bin/attackseq.rs`): a press of the attack key from no attack
//! starts the stance's press start in the press record ([`AttackPress`]: a melee weapon's charge
//! start in the first slot, a staff's or gauntlet's cast in the second); a release starts what
//! follows the attack that runs then ([`AttackStep`]); a hold starts what follows it after its
//! records ([`AttackHold`]). A sequence names an attack, not a stance: one sequence per stance
//! did not fit retail. A dodge, a jump, a weapon draw, the block and the aim (right-click) and
//! every movement state are the server's too. The client starts the later attacks of a combo
//! itself (input row 1), and the server follows some of them ([`FollowUp`]). Unknown (no retail
//! data on the pinned build): the dagger, and the releases of the bow, the musket and the
//! blunderbuss.
//!
//! The client sends no slayer state for a weapon ability either (input row 1 is empty at every
//! ability key press): the server starts the ability's animation in the own state after the
//! press, 31 to 42 ms later ([`AbilityPlay`]; ability animation study 2026-11-11, 17,488 answered
//! presses of 95 abilities in 1,195 ledgers: `blighted-emu-tools`
//! `scratch-work/capdecode/src/bin/abilityslayer.rs` and `abilityplays.py`). Without it the
//! client shows the ability's effects but no animation (live 2026-11-11: Flail Trip, the life
//! staff's abilities).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use nw_formats::objectstream::lowercase_crc32;

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
    /// What an attack key press starts when no attack or ability runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press: Option<AttackPress>,
    /// What a release of the attack key starts, by the attack that runs then.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub releases: Vec<AttackStep>,
    /// What a hold of the attack key starts, by the attack that runs then.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holds: Vec<AttackHold>,
    /// Input records from the last start of a second-slot cast to its end (`16=45 18=0 48=0`):
    /// the median of the retail casts that no new press followed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast_end_records: Option<u16>,
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
    /// What a release of the attack key starts while the attack `running` plays.
    pub fn release_after(&self, running: RunningAttack) -> Option<RunningAttack> {
        self.releases
            .iter()
            .find(|step| step.after == running)
            .map(|step| step.start)
    }

    /// What a hold of the attack key starts while the attack `running` plays, and after how
    /// many input records of the hold.
    pub fn hold_after(&self, running: RunningAttack) -> Option<(RunningAttack, u16)> {
        self.holds
            .iter()
            .find(|hold| hold.after == running)
            .map(|hold| (hold.start, hold.records))
    }
}

/// The slayer slot an attack plays in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttackSlot {
    /// The first slot (children 13 to 15): the melee weapons.
    First,
    /// The second slot (children 16 to 18): the staffs, the gauntlets, the bow and the guns.
    Second,
}

/// An attack that plays: its slot and its sequence. The sequence names the attack: the same
/// attack has the same number for every player (Observed, attack sequence study 2026-11-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningAttack {
    /// Its slot.
    pub slot: AttackSlot,
    /// Its sequence (child 15 or 18).
    pub sequence: u64,
}

/// The start the retail server sent in the record of an attack key press from no attack: a
/// melee weapon's charge start in the first slot (the flail's `13=10 14=<record> 15=1319 48=1`),
/// a staff's or gauntlet's cast in the second (the life staff's `16=33 17=<record> 18=934
/// 48=3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackPress {
    /// The attack it starts.
    #[serde(flatten)]
    pub start: RunningAttack,
    /// The slot's state (child 13 or 16, wire value).
    pub state: u64,
    /// Child 48.
    pub marker: u64,
}

/// What the retail server started at a release of the attack key while `after` played (the
/// flail's light `461` after its press start `1319`; a combo step the client named: `481` after
/// `5046`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackStep {
    /// The attack that plays at the release.
    pub after: RunningAttack,
    /// The attack the release starts.
    pub start: RunningAttack,
}

/// What the retail server started when the attack key stayed down while `after` played (the
/// flail's charged attack `583`, 14 records after the press; most stances 19 records).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackHold {
    /// The attack that plays while the key is held.
    pub after: RunningAttack,
    /// The attack the hold starts.
    pub start: RunningAttack,
    /// Input records from the start of `after` to the start (the median of the retail holds).
    pub records: u16,
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

/// The first-slot state of an ability's animation (children 13 and 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirstPlay {
    /// `slayerStateId` (child 13): 8 for the full-body abilities.
    pub state: u64,
    /// `slayerSequenceId` (child 15).
    pub sequence: u64,
}

/// The animation the server starts for one weapon ability, in the own state after its key's
/// press (each slot it sets starts in the press's state): a full-body ability plays in the
/// first slot (the flail's Trip: `13=08 15=2344 17=<press> 48=01`), an upper-body one in the
/// slots past it (the life staff's Orb of Protection: `16=33 17=<press> 18=3039 48=03`; an aimed
/// one also in the third: Sacred Ground's `16=36 18=142 19=46 21=449 48=33`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbilityPlay {
    /// The ability id (its `CooldownData` and `AbilityData` id, as retail's cooldown names it).
    pub ability: String,
    /// The first slot; `None` for an upper-body ability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first: Option<FirstPlay>,
    /// The slots past the first and the marker.
    #[serde(default)]
    pub slots: SlotsChange,
    /// What an aimed ability's cast sends (Sacred Ground's `16=33 18=993 19=00 21=00 48=03`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast: Option<SlotsChange>,
    /// How many presses showed the start.
    pub seen: u32,
}

/// The file form of the table.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
struct TableFile {
    stances: Vec<StanceSequences>,
    follow_ups: Vec<FollowUp>,
    #[serde(default)]
    abilities: Vec<AbilityPlay>,
    #[serde(default)]
    reactions: Vec<ReactionPlay>,
}

/// The sequence of one hit reaction entry: the server starts it in the reaction state
/// (`13=04`) of the own state after the hit. The client numbers the sequence by the entry it
/// plays, not by the stance (Observed: `2960` in the great sword, the spear and no weapon), so
/// the table keys it by the entry's animations
/// ([`crate::player_attacks::ReactionEntry::animations`]). Mined from the own reaction starts
/// of the capture ledgers against the side of the hit and the power level of the creature's
/// attack row (`blighted-emu-tools` `scratch-work/combat/hitreact`, `reactlabel` and
/// `reacttable.py`, 2026-11-12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReactionPlay {
    /// The entry's animations, joined by `+`.
    pub animations: String,
    /// `slayerSequenceId` (child 15).
    pub sequence: u64,
}

/// The table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerSequences {
    by_key: HashMap<u32, StanceSequences>,
    follow_ups: HashMap<u64, FollowUp>,
    /// The ability animations, by CRC-32 of the lowercase ability id.
    abilities: HashMap<u32, AbilityPlay>,
    /// The reaction sequences, by the entry's animations ([`ReactionPlay`]).
    reactions: HashMap<String, u64>,
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
        Ok(Self::of_parts(file.stances, file.follow_ups)
            .with_abilities(file.abilities)
            .with_reactions(file.reactions))
    }

    /// The same table with the reaction sequences `reactions`.
    #[must_use]
    pub fn with_reactions(mut self, reactions: Vec<ReactionPlay>) -> Self {
        self.reactions = reactions
            .into_iter()
            .map(|play| (play.animations, play.sequence))
            .collect();
        self
    }

    /// The sequence of the reaction entry that plays `animations`.
    pub fn reaction(&self, animations: &str) -> Option<u64> {
        self.reactions.get(animations).copied()
    }

    /// The same table with the ability animations `abilities`.
    #[must_use]
    pub fn with_abilities(mut self, abilities: Vec<AbilityPlay>) -> Self {
        self.abilities = abilities
            .into_iter()
            .map(|play| (lowercase_crc32(&play.ability), play))
            .collect();
        self
    }

    /// The animation of the ability whose lowercase id has CRC-32 `ability`.
    pub fn ability(&self, ability: u32) -> Option<&AbilityPlay> {
        self.abilities.get(&ability)
    }

    /// The table of `stances` and `follow_ups`.
    pub fn of_parts(stances: Vec<StanceSequences>, follow_ups: Vec<FollowUp>) -> Self {
        Self {
            by_key: stances
                .into_iter()
                .map(|stance| (stance_key(&stance.tags), stance))
                .collect(),
            follow_ups: follow_ups
                .into_iter()
                .map(|follow_up| (follow_up.client, follow_up))
                .collect(),
            abilities: HashMap::new(),
            reactions: HashMap::new(),
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

    /// Abilities in the table ([`AbilityPlay`]).
    pub fn abilities_len(&self) -> usize {
        self.abilities.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two stances and a follow-up as the table file holds them (values of the mined table).
    const TABLE: &str = r#"{
        "stances": [
            {"tags": "2H_Melee,GreatSword",
             "press": {"slot": "first", "sequence": 4120, "state": 10, "marker": 1},
             "releases": [{"after": {"slot": "first", "sequence": 4120},
                           "start": {"slot": "first", "sequence": 4510}}],
             "holds": [{"after": {"slot": "first", "sequence": 4120},
                        "start": {"slot": "first", "sequence": 57}, "records": 19}],
             "alt_start": {"second_sequence": 2351},
             "alt_moves": {"moving": {"state": 27, "sequence": 1251},
                           "idle": {"state": 31, "sequence": 1673}},
             "dodge": {"forward": {"state": 7, "sequence": 4007},
                       "back": {"state": 7, "sequence": 2100}}},
            {"tags": "2H_Ranged,Blunderbuss",
             "alt_start": {"second_sequence": 4248, "third_state": 46, "third_sequence": 407,
                           "marker": 40},
             "alt_end": {"third_state": 0, "third_sequence": 0, "marker": 0},
             "alt_fire": {"second_sequence": 94, "marker": 41},
             "reload": {"second_state": 36, "second_sequence": 4225}}
        ],
        "follow_ups": [{"client": 5051, "server": 484, "at_release": true, "seen": 9}],
        "abilities": [
            {"ability": "Ability_Flail_Trip", "first": {"state": 8, "sequence": 2344},
             "slots": {"marker": 1}, "seen": 15},
            {"ability": "Ability_Lifestaff_SacredGround",
             "slots": {"second_state": 36, "second_sequence": 142, "third_state": 46,
                       "third_sequence": 449, "marker": 33},
             "cast": {"second_state": 33, "second_sequence": 993, "third_state": 0,
                      "third_sequence": 0, "marker": 3}, "seen": 417}
        ]
    }"#;

    #[test]
    fn an_ability_animation_is_found_by_the_crc_of_its_lowercase_id() {
        let table = PlayerSequences::parse(TABLE).expect("table");
        let trip = table
            .ability(lowercase_crc32("ability_flail_trip"))
            .expect("trip");
        assert_eq!(
            trip.first,
            Some(FirstPlay {
                state: 8,
                sequence: 2344
            })
        );
        assert_eq!(trip.slots.marker, Some(1));
        let ground = table
            .ability(lowercase_crc32("Ability_Lifestaff_SacredGround"))
            .expect("sacred ground");
        assert_eq!(ground.first, None, "an upper-body ability");
        assert_eq!(
            ground.cast.map(|cast| cast.second_sequence),
            Some(Some(993))
        );
        assert_eq!(table.ability(lowercase_crc32("Ability_Unknown")), None);
        assert_eq!(table.abilities_len(), 2);
        let old = PlayerSequences::parse(r#"{"stances": [], "follow_ups": []}"#)
            .expect("a table without abilities");
        assert_eq!(old.ability(lowercase_crc32("Ability_Flail_Trip")), None);
        assert_eq!(old.abilities_len(), 0);
    }

    #[test]
    fn the_table_reads_by_stance_key() {
        let table = PlayerSequences::parse(TABLE).expect("table");
        assert_eq!(table.len(), 2);
        let sword = table
            .stance(stance_key("2H_Melee,GreatSword"))
            .expect("greatsword");
        let first = |sequence| RunningAttack {
            slot: AttackSlot::First,
            sequence,
        };
        let press = sword.press.expect("the press start");
        assert_eq!(
            (press.start, press.state, press.marker),
            (first(4120), 10, 1)
        );
        assert_eq!(sword.release_after(press.start), Some(first(4510)));
        assert_eq!(sword.hold_after(press.start), Some((first(57), 19)));
        assert_eq!(
            sword.release_after(first(4510)),
            None,
            "no step after the light"
        );
        assert_eq!(
            sword.alt_start.map(|change| change.second_sequence),
            Some(Some(2351))
        );
        let gun = table
            .stance(stance_key("2H_Ranged,Blunderbuss"))
            .expect("blunderbuss");
        assert_eq!(gun.press, None, "no mined press start");
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

    #[test]
    fn a_reaction_entry_has_its_retail_sequence() {
        // The great sword's front stagger: `2960`.
        let text = r#"{"stances": [], "follow_ups": [], "reactions": [
            {"animations": "unarmed_r2_f+grtswrd_r2_f", "sequence": 2960}]}"#;
        let table = PlayerSequences::parse(text).expect("table");
        assert_eq!(table.reaction("unarmed_r2_f+grtswrd_r2_f"), Some(2960));
        assert_eq!(table.reaction("unarmed_r2_f"), None);
    }
}
