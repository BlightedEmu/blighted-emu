// SPDX-License-Identifier: AGPL-3.0-or-later
//! The capture the flag match on its instance sim: the game mode entity (2343), the players'
//! participant states (3312) and affiliations (3563), the level entities of both maps, the start
//! barriers, the two flags, the power-ups and jump pads, the score, the kills and deaths, the
//! timers, the overtime, the end and the return.
//!
//! Sources. Retail ledgers (times in seconds from each ledger's first record): `CTF01` (queue,
//! the whole match on `NW_CTF_002_Wide`, 317.353 to 1241.952), `CTF02` and `CTF03` (inside
//! `NW_CTF_002_Wide` matches), `CTF04` (`NW_CTF_003_Long` 1.591 to 770.235) and
//! `CTF_QueueAndJoin_PickUpSpeedHealthAndFlagThenDied` (written `CTF_QJ` below, `NW_CTF_003_Long`
//! 44.731 to 351.480). The client's game mode script `CaptureTheFlagData` (`FUN_146b45bd0` to
//! `FUN_146b586c0`; findings in `local-data/notes/2026-11-12-big-systems-plan.md`).
//!
//! Observed:
//!
//! - The game mode entity: a card with no asset on component 0 and the game mode state on
//!   component 1 (`CTF_QJ` 46.961): script state 3 (`PregameTimer` +180 s), the mode `930fce8f`,
//!   the map's CRC, the participants with status 3, their teams and character ids, one raid per
//!   team, the values of [`base_values`] and the timers `GameModeTimeout` (+3 h) and
//!   `PregameTimer`.
//! - Script states (`FUN_146b4b0d0`, 5 ledgers): 3 pregame, 0 playing (`TimeLimit` +600 s), 2
//!   overtime (`OvertimeTimer` +300 s), 1 postgame (`Cleanup` +60 s, the only timer).
//! - The six start barriers (`ctf_starting_barriers` on Long, `or_fort_barriers` on Wide) carry the
//!   closed door (2330 `01 01 01`) and stop 24 ms before state 0 (`CTF_QJ` 225.153, `CTF01`
//!   498.274).
//! - The flags (`FUN_146b4fb10`): status `FlagStatusCarriedByTeam0` 0, `...Team1` 1,
//!   `FlagStatusDropped` 2, `FlagStatusSafe` 3. The red flag is team 0's (it stands at team 0's
//!   spawn), the blue flag team 1's. A flag's entity value names the base or dropped flag's facet
//!   entity, or its carrier's.
//! - Take (`FUN_146b538c0`): the client's trigger interaction (kind 33) on the flag, answered
//!   with two 3743 of status 2 (the first with a zero id; 4 of 4 in `CTF01`, 1 in `CTF_QJ`); the
//!   flag's entity stops, a ghost flag stands at the base, the carried flag (a spell entity of
//!   `GM_CTF_RedFlag` or `GM_CTF_BlueFlag` on the carrier) spawns.
//! - Drop (`FUN_146b52ef0`): the carrier's death; status 2, a pickup flag at the death place
//!   whose interaction is off, `RedFlag_Timer` set and gone at once, the reset timer +20 s, the
//!   interact timer +2 s, the cannot-score value 0; the killer's team counts a flagbearer kill.
//! - Return (`FUN_146b53f50`): the owner team touches its dropped flag (its `FlagsReturned` +1),
//!   or the reset timer ends (no count; `CTF01` 982.158); the flag stands at its base again with
//!   the interact timer +2 s. 5 s before a reset the warning value turns 1 (`FUN_146b51f80`,
//!   `CTF01` 972.192 and 977.159) and stays.
//! - Capture (`FUN_146b4c340`, `FUN_146b54810`): a carrier entering its own team's dropzone (at
//!   its own flag's base) while its own flag is safe scores 1 (`team-flag-score-value`) for its
//!   team, the flag goes back to its base with the interact timer +5 s and the celebration slice
//!   stands at the dropzone for 7 s (6 of 6); with its own flag away the cannot-score value turns
//!   1 and is 0 again when it leaves (`CTF04` 569.077 to 570.475).
//! - Stats (`FUN_146b4f000`, `FUN_146b50690`): the six kill values appear at the first death; a
//!   death counts for the dead player's team and a kill for the killer's.
//! - The end (`FUN_146b4a770`): 3 points win at once (`CTF03` 494.232, 3-0); at `TimeLimit` the
//!   team with more points wins (`CTF04` 754.390, 2-1), a tie starts the overtime (`CTF02`
//!   696.776, 1-1; `CTF01` 1098.286, 1-1) whose first capture ends it (`CTF02` 724.242, `CTF01`
//!   1222.217); `WinningTeamIdx` 0, 1 or 2 (a tie after the overtime), the regular and wild flag
//!   captures, both flag entity values `ffffffff`, the carried flags stop and the flag timers end
//!   (`Cleanup` alone). The script's rule: the `TimeLimit` handler (`FUN_146b4dc10`, timer
//!   `8f0e9156` in state 0) marks the time as up and calls `FUN_146b4a770`, which starts the
//!   overtime on a tie and ends the match with the leader otherwise; the `OvertimeTimer` handler
//!   (state 2) marks the overtime as over, so a tie then ends the match with index 2; a capture
//!   after the time is up (`FUN_146b54810`, `FUN_146b56b40`) calls it again. A capture before
//!   that ends the match only when the scorer's team has `points-to-win` and leads by
//!   `points-over-enemy-team-to-win` (`FUN_146b488a0`).
//! - `CTF01` holds the same rule: its 2343 shows 1-0 at `TimeLimit` because the ledger lost the
//!   delta of team 1's capture at 744.33 s (versions 96 and 97 of the game mode state are
//!   missing). That capture is in the other records of the ledger: at 744.351 the celebration
//!   stands at team 1's dropzone (854.13, 1127.82) and the red flag stands at its base again,
//!   the red interact timer of the capture ends 5 s later (749.367), the red flag's next take
//!   (827.647) is a take at its base (no reset timer goes), and the end sends
//!   `Team1RegularFlagCaptures` 1 with team 0's 2: the score was 1-1 at 1098.286.
//! - Power-ups (`FUN_146b4c340`, the entity names `Powerup_Speed`, `_Damage`, `_Healing`): a
//!   touch takes the power-up away and gives `CTF_PowerUp_Speed`, `_Damage` or `_Health`; a new
//!   one stands there 30 s later (31 of 31 respawns of `CTF04` and `CTF02` at 29.9 to 30.0 s).
//! - Jump pads (`FUN_146b4c340`): a touch gives `CTF_JumpPad_Long`, `_High` or `_Super`, or the
//!   `CTF_Carrier_JumpPad_*` effect to a flag carrier.
//! - Participant flags `03ca61080000`; affiliations own team (kind 0) and other team (kind 4);
//!   statuses 3, `0x20` ready, `0x04` dead.
//! - The end tick gives each player its team's end event (`CaptureTheFlag_T4` to the winners,
//!   `_T2` to the losers; amounts in [`nw_content::game_mode_rewards`]), and 0.1 s later every
//!   participant's affiliations are empty (`CTF03` 494.317, `CTF04` 754.504).
//! - A carried flag names its carrier's company, faction and team raid
//!   ([`SimWorld::place_carried`]).
//! - The dropzone, power-up and jump pad triggers are the client slices' shapes ([`DROPZONE`]).
//!
//! Inferred (no ledger shows it): a player who left before the end gets no end event; a trigger
//! meets the player's capsule; a team with no participant left loses; PvP between the two teams while the match runs; the respawn at the
//! team's spot as soon as the client asks; the leave of a running match bars the queue as the
//! arena's does; a carrier who leaves gives its flag back to its base (Observed in the postgame
//! only, `CTF04` 765.287); the points over the other team that win at once
//! (`points-over-enemy-team-to-win` 1: no ledger ends at 3 points with the other team above 0).
//!
//! Unknown: `min-playtime-seconds` and `use-score-win-condition` (`FUN_146b488a0` reads them
//! before a score win; the client registers no default and no message sends them; the only
//! score win, `CTF03`, came 404 s after the start, so this sim checks no play time); the names of
//! the flag value keys (no
//! string of the client hashes to them); the wild flag (no wild flag value or entity in 5 ledgers:
//! retail runs none, so this sim runs none and sends its capture values as 0).

use bevy_ecs::entity::Entity;
use nw_bus::ArenaSeat;
use nw_content::instances::{InstanceKind, InstanceMap};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::buildables::{NO_OWNER_KIND, OwnerIdentification};
use nw_messages::client_input::{InteractionRequest, TRIGGER_KIND};
use nw_messages::components::InteractState;
use nw_messages::facet::facet_entity;
use nw_messages::game_mode_participant::PARTICIPANT_FLAGS_LEN;
use nw_messages::game_mode_state::{GameModeState, Keyed};
use nw_messages::list_delta::{ListChange, ReplicatedList};
use nw_messages::spell_state::SpellState;
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BundleMember, MemberState, OwnershipState, PositionInTheWorldReplicatedState, Rotation,
    RotationComponent, Scale, ServerMessage, WorldPosition,
};
use nw_sim_core::Position;

use super::arena_match::{
    CLEANUP_TIMER, GAME_MODE_TIMEOUT, GAME_MODE_TIMEOUT_MS, LEAVER_PENALTY_NS, OVERTIME_TIMER,
    PREGAME_TIMER, RAID_ID_BASE, READY_BIT, WINNING_TEAM,
};
use super::crafting::{Station, dynamic_ref, unowned};
use super::encounter_roots::card;
use super::game_mode_entity::{
    FIRST_LIST_VERSION, MatchEntry, MatchParticipant, PLAYING_STATUS, full, mode_facet,
    participant_lists, team_affiliations,
};
use super::hit_shape::PLAYER_CAPSULE;
use super::outpost_rush::TIME_LIMIT_TIMER;
use super::outpost_rush_doors::closed_door;
use super::queue::team_spot;
use super::spell_areas::caster_ref;
use super::{Body, Player, SimWorld, gather_answer, unix_ns};

/// The script states (Observed, `FUN_146b4b0d0`).
const RUNNING: u8 = 0;
const ENDED: u8 = 1;
const OVERTIME: u8 = 2;
const PREGAME: u8 = 3;
/// The state times (Observed: `PregameTimer` +180 s, `TimeLimit` +600 s, `OvertimeTimer` +300 s
/// and `Cleanup` +60 s in 5 ledgers).
const PREGAME_MS: u64 = 180_000;
const TIME_LIMIT_MS: u64 = 600_000;
const OVERTIME_MS: u64 = 300_000;
const CLEANUP_MS: u64 = 60_000;
/// The flag values (Observed by their values; names Unknown).
const RED_STATUS: u32 = 0xf5c0_e5d2;
const RED_ENTITY: u32 = 0xf870_d972;
const BLUE_STATUS: u32 = 0x82c7_d544;
const BLUE_ENTITY: u32 = 0x8f77_e9e4;
/// Whether each flag may be touched (Observed: 0 while its interact timer runs).
const RED_INTERACTABLE: u32 = 0xa38b_c334;
const BLUE_INTERACTABLE: u32 = 0xb4a3_a7f4;
/// The flag reset time in seconds (Observed 20, `FUN_146b4fb10` reads `flagreset-seconds`).
const FLAG_RESET_SECONDS: u32 = 0xcdc0_16ca;
/// The team scores (Observed: `FUN_146b54810` adds a capture of team 0 to `19a55d86`).
const TEAM_0_SCORE: u32 = 0x19a5_5d86;
const TEAM_1_SCORE: u32 = 0xd2f9_8e23;
/// The cannot-score values of the red flag's carrier (team 1) and the blue flag's (team 0).
const RED_CANNOT_SCORE: u32 = 0xb949_3120;
const BLUE_CANNOT_SCORE: u32 = 0x64df_e8a5;
/// The reset warnings of the red and the blue flag (`FUN_146b51f80`).
const RED_RESET_WARNING: u32 = 0x12df_05f2;
const BLUE_RESET_WARNING: u32 = 0xc53d_85aa;
/// The stats (CRC-32 of the client's names, `FUN_146b50690`).
const TEAM_0_KILLS: u32 = 0xf1af_cc7c;
const TEAM_1_KILLS: u32 = 0x3021_13bc;
const TEAM_0_DEATHS: u32 = 0x3f83_e38b;
const TEAM_1_DEATHS: u32 = 0xa426_afe4;
const TEAM_0_FLAGBEARERS_KILLED: u32 = 0xc022_4c6b;
const TEAM_1_FLAGBEARERS_KILLED: u32 = 0x2f70_fa8a;
const TEAM_0_FLAGS_RETURNED: u32 = 0x0f0e_b47e;
const TEAM_1_FLAGS_RETURNED: u32 = 0x9201_5508;
const TEAM_0_REGULAR_CAPTURES: u32 = 0x956e_0f58;
const TEAM_1_REGULAR_CAPTURES: u32 = 0xf009_341e;
const TEAM_0_WILD_CAPTURES: u32 = 0xa5b2_2c84;
const TEAM_1_WILD_CAPTURES: u32 = 0x2214_e7c7;
/// The flag timers: reset (+20 s) and interact (Observed by their times; names Unknown).
const RED_RESET_TIMER: u32 = 0x0e7b_3c84;
const BLUE_RESET_TIMER: u32 = 0xbc39_2cd4;
const RED_INTERACT_TIMER: u32 = 0xcf6b_25c1;
const BLUE_INTERACT_TIMER: u32 = 0x0379_7f3a;
/// The flag statuses (`FUN_146b4fb10`).
const STATUS_DROPPED: u64 = 2;
const STATUS_SAFE: u64 = 3;
/// A flag entity value with no entity (Observed `ffffffff`).
const NO_FLAG_ENTITY: u64 = 0xffff_ffff;
/// `WinningTeamIdx` of a tie after the overtime (`FUN_146b4a770`).
const TIE: u8 = 2;
/// The flag reset, its warning and the interact buffers (Observed).
const FLAG_RESET_MS: u64 = 20_000;
const RESET_WARNING_MS: u64 = 5_000;
const DROP_BUFFER_MS: u64 = 2_000;
const RETURN_BUFFER_MS: u64 = 2_000;
const SCORE_BUFFER_MS: u64 = 5_000;
const MS_PER_S: u64 = 1_000;
/// Points of a capture and the points that win (Observed), and the lead over the other team
/// that wins at once (`points-over-enemy-team-to-win`, Inferred: no ledger shows a 3-1 or 3-2).
const CAPTURE_POINTS: u32 = 1;
const POINTS_TO_WIN: u32 = 3;
const LEAD_TO_WIN: u32 = 1;
/// The celebration's time at the dropzone, the power-up respawn (Observed).
const CELEBRATION_MS: u64 = 7_000;
const POWER_UP_RESPAWN_MS: u64 = 30_000;
/// The end events of a player's team: a win, a loss and a tie. Observed: each ledger's end tick
/// sends one 497 entry to each player, `CaptureTheFlag_T4` to the winners (`CTF01` 1222.234 s,
/// `CTF02` 724.242 s, `CTF04` 754.390 s) and `CaptureTheFlag_T2` to the losers (`CTF03`
/// 494.251 s). Inferred: `CaptureTheFlag_T3` for a tie (no ledger ends in a tie; its rewards lie
/// between the two in the content).
const WIN_EVENT: &str = "CaptureTheFlag_T4";
const LOSS_EVENT: &str = "CaptureTheFlag_T2";
const TIE_EVENT: &str = "CaptureTheFlag_T3";
/// The trigger shapes (Observed, client slices, the `m_shape` of each trigger area with the
/// `Player` tag): `Dropzone_A` and `Dropzone_B` of `gamemode_ctf_003_long` and
/// `gamemode_ctf_002_wide` hold a `QueryShapeCylinder` (`{709B11EA-...}`, `FUN_141584ac0`) of
/// `m_height` 6 and `m_radius` 2.25 with no offset; `powerup_speed`, `_damage` and `_healing`
/// hold a sphere (`{7F2EF312-...}`, `m_radius` only) of 1.25 m 1 m up; the jump pads' spheres
/// are in [`JumpPadKind::trigger`].
const DROPZONE: TriggerShape = TriggerShape::Cylinder {
    radius_m: 2.25,
    height_m: 6.0,
};
const POWER_UP_TRIGGER: TriggerShape = TriggerShape::Sphere {
    radius_m: 1.25,
    up_m: 1.0,
};
/// The 3743 status of every retail flag interaction (Observed).
const FLAG_INTERACTION_STATUS: u8 = 2;
/// The dead bit of a participant status (Observed `07`, `27`).
const DEAD_BIT: u8 = 0x04;
/// The participant flags of the whole match (Observed).
const MATCH_FLAGS: [u8; PARTICIPANT_FLAGS_LEN] = [0x03, 0xca, 0x61, 0x08, 0x00, 0x00];
/// The teams of a match.
const TEAMS: usize = 2;
/// The components of the level entities (Observed).
const GAME_MODE_COMPONENT: u64 = 1;
const OWNERSHIP_COMPONENT: u64 = 1;
const FLAG_POSITION_COMPONENT: u64 = 2;
const PROP_POSITION_COMPONENT: u64 = 1;
const INTERACT_COMPONENT: u64 = 4;
const CARRIED_SPELL_COMPONENT: u64 = 2;
const CARRIED_OWNERSHIP_COMPONENT: u64 = 3;
/// The ownership rules (Observed): a base flag, a spawn spot, a barrier and a carried flag 4; a
/// dropped and a ghost flag 1.
const TEAM_RULE: u8 = 4;
const OPEN_RULE: u8 = 1;
const NO_RAID: u64 = 0;
/// The carried flag's area scale (Observed 1.0).
const CARRIED_RADIUS_SCALE: f32 = 1.0;
/// The potency of a power-up or jump pad effect (Inferred: plain).
const PLAIN_POTENCY: f64 = 1.0;
/// The namespaces of the `GdeRef`s ("ctfmatch", "ctflevel").
const MATCH_NAMESPACE: u64 = 0x6374_666d_6174_6368;
const LEVEL_NAMESPACE: u64 = 0x6374_666c_6576_656c;

/// The asset id of a slice from its 32 hex digits.
const fn asset(hex: &str) -> [u8; RAW_ID_LEN] {
    const fn nibble(digit: u8) -> u8 {
        match digit {
            b'0'..=b'9' => digit - b'0',
            b'a'..=b'f' => digit - b'a' + 10,
            _ => panic!("not a hex digit"),
        }
    }
    let digits = hex.as_bytes();
    let mut id = [0; RAW_ID_LEN];
    let mut index = 0;
    while index < RAW_ID_LEN {
        id[index] = nibble(digits[2 * index]) << 4 | nibble(digits[2 * index + 1]);
        index += 1;
    }
    id
}

/// The slices (Observed, the client's asset catalog).
const RED_PICKUP: [u8; RAW_ID_LEN] = asset("224cb695559250a0b2d6cf05065a841b");
const BLUE_PICKUP: [u8; RAW_ID_LEN] = asset("67019cab42325497856db759e7ab1da1");
const RED_CARRIED: [u8; RAW_ID_LEN] = asset("096ef9d6e08d5a5da73031982aa36239");
const BLUE_CARRIED: [u8; RAW_ID_LEN] = asset("f3ce2d3f2855501aba6dd01bf04406f0");
const RED_GHOST: [u8; RAW_ID_LEN] = asset("585238f486245e14a530ebae7b4cac26");
const BLUE_GHOST: [u8; RAW_ID_LEN] = asset("e52c15df262c51aeac802ab71b09862a");
const PLATFORM: [u8; RAW_ID_LEN] = asset("2a454578ba3e5c3e9e758930d5f5a860");
const CELEBRATION: [u8; RAW_ID_LEN] = asset("3d8a18ae5965584399a108152a122a00");
const SPAWN_POINTS: [u8; RAW_ID_LEN] = asset("29e2f26f23a4505b873026cff4c52ed6");
const SPEED: [u8; RAW_ID_LEN] = asset("26a2edb1416256f8b91f05c4d8d1e705");
const DAMAGE: [u8; RAW_ID_LEN] = asset("79460361555c5c73bc9ddb8dc3885d5e");
const HEALING: [u8; RAW_ID_LEN] = asset("393471c8fd795472b9c29a71696c4909");
const PAD_LONG: [u8; RAW_ID_LEN] = asset("ad454ca148f9553aa5021d8af33c9b87");
const PAD_HIGH: [u8; RAW_ID_LEN] = asset("fdafa3ae8ccb5be6bea4f8ea5fe1ead5");
const PAD_SUPER: [u8; RAW_ID_LEN] = asset("2a4831df88bf55febf2197b7ccf40e11");
const LONG_BARRIERS: [u8; RAW_ID_LEN] = asset("34b89afb057f5edba9294d3b5857ea09");
const WIDE_BARRIERS: [u8; RAW_ID_LEN] = asset("42fc3c158ef850aeac8a3981d85b1039");

/// The flag carrier effects of the red and the blue flag; the carried flag's spell is the same
/// id (Observed: spell `15bc550c` = `GM_CTF_RedFlag`, `CTF_QJ` 255.038).
const FLAG_EFFECTS: [&str; TEAMS] = ["GM_CTF_RedFlag", "GM_CTF_BlueFlag"];

/// A level entity's place: the retail x, y and height step, and its turn.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placed {
    at: WorldPosition,
    turn: Option<Rotation>,
}

impl Placed {
    fn position(&self) -> Position {
        Position {
            x: self.at.x,
            y: self.at.y,
            z: self.at.z(),
        }
    }

    /// The position state on `component`, with `scale`.
    fn member(&self, component: u64, scale: Option<Scale>) -> BundleMember {
        BundleMember {
            component,
            state: MemberState::Position(PositionInTheWorldReplicatedState {
                position: Some(self.at),
                rotation: self.turn,
                scale,
            }),
        }
    }
}

const fn placed(xy: [f32; 2], z_raw: u16, turn: Option<Rotation>) -> Placed {
    Placed {
        at: WorldPosition {
            x: xy[0],
            y: xy[1],
            z_raw,
        },
        turn,
    }
}

const fn turn(components: [RotationComponent; 3], w_negative: bool) -> Rotation {
    Rotation {
        components,
        w_negative,
    }
}

use nw_messages::RotationComponent::{One as O, Value as V, Zero as Z};

/// A power-up's kind: its slice and its effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PowerUpKind {
    Speed,
    Damage,
    Healing,
}

impl PowerUpKind {
    const fn asset(self) -> [u8; RAW_ID_LEN] {
        match self {
            Self::Speed => SPEED,
            Self::Damage => DAMAGE,
            Self::Healing => HEALING,
        }
    }

    const fn effect(self) -> &'static str {
        match self {
            Self::Speed => "CTF_PowerUp_Speed",
            Self::Damage => "CTF_PowerUp_Damage",
            Self::Healing => "CTF_PowerUp_Health",
        }
    }
}

/// A jump pad's kind: its slice and its effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JumpPadKind {
    Long,
    High,
    Super,
}

impl JumpPadKind {
    const fn asset(self) -> [u8; RAW_ID_LEN] {
        match self {
            Self::Long => PAD_LONG,
            Self::High => PAD_HIGH,
            Self::Super => PAD_SUPER,
        }
    }

    /// The trigger sphere (Observed: `ctf_jumppad_long` and `_high` 2.6 m, `ctf_jumppad_super`
    /// 2.5999756 m, each 0.2 m up).
    const fn trigger(self) -> TriggerShape {
        const UP_M: f32 = 0.2;
        const RADIUS_M: f32 = 2.6;
        const SUPER_RADIUS_M: f32 = 2.599_975_6;
        let radius_m = match self {
            Self::Long | Self::High => RADIUS_M,
            Self::Super => SUPER_RADIUS_M,
        };
        TriggerShape::Sphere {
            radius_m,
            up_m: UP_M,
        }
    }

    /// The effect for a player, or for a flag carrier when `carrier`.
    const fn effect(self, carrier: bool) -> &'static str {
        match (self, carrier) {
            (Self::Long, false) => "CTF_JumpPad_Long",
            (Self::High, false) => "CTF_JumpPad_High",
            (Self::Super, false) => "CTF_JumpPad_Super",
            (Self::Long, true) => "CTF_Carrier_JumpPad_Long",
            (Self::High, true) => "CTF_Carrier_JumpPad_High",
            (Self::Super, true) => "CTF_Carrier_JumpPad_Super",
        }
    }
}

/// The level entities of one map (Observed: the first spawn group of each entity in the
/// ledgers of the map, `CTF_QJ` and `CTF04` for Long, `CTF01` and `CTF02` for Wide).
#[derive(Debug, Clone, Copy, PartialEq)]
struct CtfMap {
    folder: &'static str,
    barrier_asset: [u8; RAW_ID_LEN],
    barrier_scale: Option<Scale>,
    barriers: &'static [Placed],
    /// The bases of the red (team 0) and the blue (team 1) flag; each is its team's dropzone
    /// (Observed: the celebration of each capture stood there).
    flag_bases: [WorldPosition; TEAMS],
    platforms: &'static [Placed],
    power_ups: &'static [(PowerUpKind, Placed)],
    jump_pads: &'static [(JumpPadKind, Placed)],
    /// The spawn spots of team 0 and team 1 (at the map's team teleports).
    spawn_points: [Placed; TEAMS],
}

/// The Long barriers' scale (Observed binary16 2, 2, 1).
const LONG_BARRIER_SCALE: Scale = Scale::PerAxis {
    tag: 2,
    values: [16384, 16384, 15360],
};

const CTF_MAPS: [CtfMap; 2] = [
    CtfMap {
        folder: "nw_ctf_003_long",
        barrier_asset: LONG_BARRIERS,
        barrier_scale: Some(LONG_BARRIER_SCALE),
        barriers: &[
            placed(
                [990.974_85, 969.299_4],
                9042,
                Some(turn([Z, Z, V(64418)], false)),
            ),
            placed(
                [992.316_04, 931.741_33],
                9042,
                Some(turn([Z, Z, V(64418)], false)),
            ),
            placed(
                [1_024.802_7, 949.758_7],
                9042,
                Some(turn([Z, Z, V(64418)], false)),
            ),
            placed(
                [1_067.379_8, 1_103.248_8],
                9024,
                Some(turn([Z, Z, V(64418)], false)),
            ),
            placed(
                [1_099.644_2, 1118.075],
                9051,
                Some(turn([Z, Z, V(64418)], false)),
            ),
            placed(
                [1_103.617_8, 1_082.183_1],
                9042,
                Some(turn([Z, Z, V(64418)], false)),
            ),
        ],
        flag_bases: [
            WorldPosition {
                x: 1079.281,
                y: 1_082.619_8,
                z_raw: 9010,
            },
            WorldPosition {
                x: 1_014.424_3,
                y: 970.193_36,
                z_raw: 9010,
            },
        ],
        platforms: &[
            placed(
                [1_014.424_3, 970.193_36],
                9010,
                Some(turn([Z, Z, V(64418)], false)),
            ),
            placed(
                [1079.281, 1_082.619_8],
                9010,
                Some(turn([Z, Z, V(24286)], false)),
            ),
        ],
        power_ups: &[
            (
                PowerUpKind::Speed,
                placed(
                    [1_030.172_6, 997.450_6],
                    9127,
                    Some(turn([Z, Z, V(62476)], false)),
                ),
            ),
            (
                PowerUpKind::Speed,
                placed(
                    [1_063.687_1, 1_055.487_8],
                    9133,
                    Some(turn([Z, Z, V(34761)], false)),
                ),
            ),
            (
                PowerUpKind::Damage,
                placed(
                    [1_007.362_85, 984.370_85],
                    8626,
                    Some(turn([Z, Z, V(62318)], false)),
                ),
            ),
            (
                PowerUpKind::Damage,
                placed(
                    [1_030.124_1, 971.626],
                    8627,
                    Some(turn([Z, Z, V(62425)], false)),
                ),
            ),
            (
                PowerUpKind::Damage,
                placed(
                    [1_063.239_1, 1_081.256_6],
                    8628,
                    Some(turn([Z, Z, V(34925)], false)),
                ),
            ),
            (
                PowerUpKind::Damage,
                placed(
                    [1_086.160_4, 1068.87],
                    8625,
                    Some(turn([Z, Z, V(34636)], false)),
                ),
            ),
            (
                PowerUpKind::Healing,
                placed(
                    [1_027.453_1, 1_037.718_4],
                    8741,
                    Some(turn([Z, Z, V(22299)], false)),
                ),
            ),
            (
                PowerUpKind::Healing,
                placed(
                    [1_066.144_4, 1_015.366_76],
                    8741,
                    Some(turn([Z, Z, V(65178)], true)),
                ),
            ),
        ],
        jump_pads: &[
            (
                JumpPadKind::Long,
                placed(
                    [1_026.108_5, 1_005.944_46],
                    8699,
                    Some(turn([V(32097), V(32380), V(61137)], true)),
                ),
            ),
            (
                JumpPadKind::Long,
                placed(
                    [1039.608, 998.150_5],
                    8699,
                    Some(turn([V(33154), V(32097), V(49146)], false)),
                ),
            ),
            (
                JumpPadKind::Long,
                placed(
                    [1040.107, 1_030.344_6],
                    8904,
                    Some(turn([Z, Z, V(4390)], false)),
                ),
            ),
            (
                JumpPadKind::Long,
                placed(
                    [1_053.852_3, 1_022.328_25],
                    8904,
                    Some(turn([Z, Z, V(49151)], false)),
                ),
            ),
            (
                JumpPadKind::Long,
                placed(
                    [1_054.143_2, 1_054.502_4],
                    8699,
                    Some(turn([V(33516), V(33199), V(61135)], true)),
                ),
            ),
            (
                JumpPadKind::Long,
                placed(
                    [1_067.642_8, 1_046.708_4],
                    8699,
                    Some(turn([V(32335), V(33516), V(49145)], false)),
                ),
            ),
            (
                JumpPadKind::High,
                placed(
                    [1_010.345_64, 989.205_5],
                    8638,
                    Some(turn([V(32340), V(31166), V(64388)], false)),
                ),
            ),
            (
                JumpPadKind::High,
                placed(
                    [1_032.861_8, 976.204_5],
                    8639,
                    Some(turn([V(32340), V(31166), V(64388)], false)),
                ),
            ),
            (
                JumpPadKind::High,
                placed(
                    [1_060.855_7, 1_076.691_3],
                    8639,
                    Some(turn([V(33254), V(34593), V(64376)], false)),
                ),
            ),
            (
                JumpPadKind::High,
                placed(
                    [1_083.372_3, 1_063.691_3],
                    8639,
                    Some(turn([V(33254), V(34593), V(64376)], false)),
                ),
            ),
        ],
        spawn_points: [
            placed(
                [1_087.669_4, 1_097.134_2],
                9077,
                Some(turn([Z, Z, V(64418)], false)),
            ),
            placed(
                [1_005.846_07, 955.411_1],
                9049,
                Some(turn([Z, Z, V(24286)], false)),
            ),
        ],
    },
    CtfMap {
        folder: "nw_ctf_002_wide",
        barrier_asset: WIDE_BARRIERS,
        barrier_scale: None,
        barriers: &[
            placed([836.007, 1_260.919_9], 5689, Some(turn([Z, Z, O], true))),
            placed(
                [836.288_27, 1114.882],
                5699,
                Some(turn([Z, Z, V(32767)], false)),
            ),
            placed([853.999_63, 1_070.000_2], 5117, Some(turn([Z, Z, O], true))),
            placed([872.449_9, 1_260.911_4], 5690, Some(turn([Z, Z, O], true))),
            placed(
                [874.366_7, 1114.882],
                5699,
                Some(turn([Z, Z, V(32767)], false)),
            ),
            placed(
                [876.999_45, 1_301.000_2],
                4774,
                Some(turn([Z, Z, V(55937)], false)),
            ),
        ],
        flag_bases: [
            WorldPosition {
                x: 854.254,
                y: 1_248.616_7,
                z_raw: 5588,
            },
            WorldPosition {
                x: 854.129_46,
                y: 1_127.817_9,
                z_raw: 5588,
            },
        ],
        platforms: &[
            placed(
                [854.129_46, 1_127.817_9],
                5588,
                Some(turn([Z, Z, V(32767)], false)),
            ),
            placed(
                [854.254, 1_248.616_7],
                5588,
                Some(turn([Z, Z, V(32767)], false)),
            ),
        ],
        power_ups: &[
            (
                PowerUpKind::Speed,
                placed(
                    [797.617_7, 1_142.171_4],
                    5665,
                    Some(turn([Z, Z, V(21570)], false)),
                ),
            ),
            (
                PowerUpKind::Speed,
                placed(
                    [797.617_8, 1_233.605_6],
                    5665,
                    Some(turn([Z, Z, V(63266)], false)),
                ),
            ),
            (
                PowerUpKind::Speed,
                placed(
                    [909.016_85, 1_142.171_4],
                    5468,
                    Some(turn([Z, Z, V(63394)], false)),
                ),
            ),
            (
                PowerUpKind::Speed,
                placed(
                    [909.016_97, 1_233.736_6],
                    5468,
                    Some(turn([Z, Z, V(59553)], false)),
                ),
            ),
            (
                PowerUpKind::Damage,
                placed(
                    [830.334, 1_187.757_2],
                    5662,
                    Some(turn([Z, Z, V(25327)], false)),
                ),
            ),
            (
                PowerUpKind::Damage,
                placed([878.907_35, 1_187.606_7], 5662, Some(turn([Z, Z, O], true))),
            ),
            (
                PowerUpKind::Healing,
                placed(
                    [803.094_6, 1_187.891_8],
                    5697,
                    Some(turn([Z, Z, V(64894)], false)),
                ),
            ),
            (
                PowerUpKind::Healing,
                placed(
                    [820.407_8, 1_187.891_8],
                    5539,
                    Some(turn([Z, Z, V(59711)], false)),
                ),
            ),
            (
                PowerUpKind::Healing,
                placed(
                    [890.724_1, 1_187.676_8],
                    5540,
                    Some(turn([Z, Z, V(59700)], false)),
                ),
            ),
            (
                PowerUpKind::Healing,
                placed(
                    [907.101_2, 1_187.676_8],
                    5351,
                    Some(turn([Z, Z, V(4825)], false)),
                ),
            ),
        ],
        jump_pads: &[
            (
                JumpPadKind::Long,
                placed(
                    [840.639_1, 1_187.920_2],
                    5527,
                    Some(turn([Z, Z, V(9597)], false)),
                ),
            ),
            (
                JumpPadKind::Long,
                placed(
                    [868.673, 1_187.920_2],
                    5527,
                    Some(turn([Z, Z, V(55937)], false)),
                ),
            ),
            (
                JumpPadKind::Super,
                placed([797.5, 1160.11], 5661, Some(turn([Z, Z, V(32767)], false))),
            ),
            (
                JumpPadKind::Super,
                placed([797.5, 1216.0], 5653, Some(turn([Z, Z, V(32767)], false))),
            ),
            (
                JumpPadKind::Super,
                placed([831.560_7, 1_225.568_4], 5527, Some(turn([Z, Z, O], false))),
            ),
            (
                JumpPadKind::Super,
                placed(
                    [831.869_57, 1_150.651_9],
                    5527,
                    Some(turn([Z, Z, V(32767)], false)),
                ),
            ),
            (
                JumpPadKind::Super,
                placed([876.849_1, 1_225.565_4], 5527, Some(turn([Z, Z, O], false))),
            ),
            (
                JumpPadKind::Super,
                placed(
                    [876.947_7, 1_150.651_9],
                    5527,
                    Some(turn([Z, Z, V(32767)], false)),
                ),
            ),
            (
                JumpPadKind::Super,
                placed(
                    [909.516_4, 1_160.483_6],
                    5341,
                    Some(turn([Z, Z, V(32767)], false)),
                ),
            ),
            (
                JumpPadKind::Super,
                placed(
                    [909.516_4, 1_214.483_6],
                    5341,
                    Some(turn([Z, Z, V(32767)], false)),
                ),
            ),
        ],
        spawn_points: [
            placed([854.152_65, 1_265.849_6], 5756, Some(turn([Z, Z, O], true))),
            placed(
                [854.424_5, 1_111.207_5],
                5752,
                Some(turn([Z, Z, V(32767)], false)),
            ),
        ],
    },
];

/// The position of a retail place.
fn position_of(at: WorldPosition) -> Position {
    Position {
        x: at.x,
        y: at.y,
        z: at.z(),
    }
}

/// A trigger area's shape about its entity's place.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TriggerShape {
    /// An upright cylinder centred on the place (Inferred: the shapes are centred, as the
    /// slices give no offset).
    Cylinder { radius_m: f32, height_m: f32 },
    /// A sphere `up_m` above the place (the slice's translation offset).
    Sphere { radius_m: f32, up_m: f32 },
}

/// Whether a player standing at `feet` touches `shape` at `place`: its body
/// ([`PLAYER_CAPSULE`], Inferred: the trigger meets the player's capsule) overlaps the shape.
fn touches(feet: Position, place: Position, shape: TriggerShape) -> bool {
    let body = PLAYER_CAPSULE;
    let apart = feet.horizontal_distance(place);
    match shape {
        TriggerShape::Cylinder { radius_m, height_m } => {
            let half = height_m / 2.0;
            apart <= radius_m + body.radius_m
                && feet.z <= place.z + half
                && feet.z + body.height_m >= place.z - half
        }
        TriggerShape::Sphere { radius_m, up_m } => {
            let centre = place.z + up_m;
            let axis_low = feet.z + body.radius_m;
            let axis_high = (feet.z + body.height_m - body.radius_m).max(axis_low);
            let rise = centre - centre.clamp(axis_low, axis_high);
            apart.hypot(rise) <= radius_m + body.radius_m
        }
    }
}

/// The ownership of a level entity under `rule` for `raid`.
fn ownership(rule: u8, raid: u64) -> BundleMember {
    BundleMember {
        component: OWNERSHIP_COMPONENT,
        state: MemberState::Ownership(OwnershipState {
            raid_id: Some(raid),
            ..unowned(rule, "")
        }),
    }
}

/// The interact state of a flag that may (`true`) or may not be touched.
fn interact_member(enabled: bool) -> BundleMember {
    BundleMember {
        component: INTERACT_COMPONENT,
        state: MemberState::Interact(InteractState {
            enabled: Some(enabled),
            has_interactors: None,
        }),
    }
}

/// Where a flag is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum FlagPlace {
    /// Safe at its base.
    Base,
    /// Carried by player `number` (`entity`) of team `team`.
    Carried {
        entity: Entity,
        number: u64,
        team: u8,
    },
    /// Dropped where its carrier died.
    Dropped,
}

/// One flag of the match: team 0's red or team 1's blue.
#[derive(Debug, Clone, PartialEq)]
struct Flag {
    place: FlagPlace,
    /// The pickup entity at the base or the drop place: slot key and raw id.
    pickup: Option<(u64, [u8; RAW_ID_LEN])>,
    /// The carried flag's slot key.
    carried: Option<u64>,
    /// The ghost flag's slot key (at the base while the flag is away).
    ghost: Option<u64>,
    /// The server ms of the reset of a dropped flag.
    reset_ms: Option<u64>,
    /// The server ms when the flag may be touched again.
    interact_ms: Option<u64>,
    /// The reset warning was sent.
    warned: bool,
    /// The cannot-score value, once sent.
    cannot_score: Option<bool>,
    /// The carrier stood in its dropzone at the last tick.
    in_zone: bool,
}

impl Flag {
    const fn new() -> Self {
        Self {
            place: FlagPlace::Base,
            pickup: None,
            carried: None,
            ghost: None,
            reset_ms: None,
            interact_ms: None,
            warned: false,
            cannot_score: None,
            in_zone: false,
        }
    }

    /// The status value.
    fn status(&self) -> u64 {
        match self.place {
            FlagPlace::Base => STATUS_SAFE,
            FlagPlace::Carried { team, .. } => u64::from(team),
            FlagPlace::Dropped => STATUS_DROPPED,
        }
    }

    /// The entity value: the carrier's facet entity or the pickup's.
    fn entity(&self) -> u64 {
        match self.place {
            FlagPlace::Carried { number, .. } => facet_entity(number),
            FlagPlace::Base | FlagPlace::Dropped => self
                .pickup
                .map_or(NO_FLAG_ENTITY, |(_, gde_ref)| mode_facet(&gde_ref)),
        }
    }

    fn carrier(&self) -> Option<Entity> {
        match self.place {
            FlagPlace::Carried { entity, .. } => Some(entity),
            FlagPlace::Base | FlagPlace::Dropped => None,
        }
    }
}

/// One power-up of the map: its row and its entity (gone until the respawn).
#[derive(Debug, Clone, Copy, PartialEq)]
struct PowerUp {
    index: usize,
    entity: Option<u64>,
    respawn_ms: Option<u64>,
}

/// The per-team counts of the match.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TeamStats {
    kills: [u32; TEAMS],
    deaths: [u32; TEAMS],
    flagbearers_killed: [u32; TEAMS],
    flags_returned: [u32; TEAMS],
    captures: [u32; TEAMS],
}

/// The running capture the flag match of this instance sim.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CaptureTheFlagMatch {
    entity: u64,
    gde_ref: [u8; RAW_ID_LEN],
    crc: u32,
    map: InstanceMap,
    layout: &'static CtfMap,
    participants: Vec<MatchParticipant>,
    state: u8,
    /// The server ms of the current state's timer.
    ends_ms: u64,
    timeout_ms: u64,
    flags: [Flag; TEAMS],
    scores: [u32; TEAMS],
    stats: TeamStats,
    /// The kill values were sent (at the first death).
    stats_shown: bool,
    raids: [u64; TEAMS],
    winner: Option<u8>,
    barriers: Vec<u64>,
    /// The static level entities (platforms, jump pads, spawn spots).
    props: Vec<u64>,
    power_ups: Vec<PowerUp>,
    /// The celebrations: slot key and server ms of the end.
    celebrations: Vec<(u64, u64)>,
    /// The jump pads each player touched at the last tick.
    on_pads: Vec<(Entity, usize)>,
    version: u64,
    list_version: u64,
    both_teams_seen: bool,
}

/// The values every state holds (Observed, `CTF_QJ` 46.961: these nine).
fn base_values(running: &CaptureTheFlagMatch) -> Vec<Keyed> {
    let [red, blue] = &running.flags;
    let interactable = |flag: &Flag| u64::from(flag.interact_ms.is_none());
    vec![
        (FLAG_RESET_SECONDS, FLAG_RESET_MS / MS_PER_S),
        (RED_INTERACTABLE, interactable(red)),
        (TEAM_1_SCORE, u64::from(running.scores[1])),
        (BLUE_ENTITY, blue.entity()),
        (TEAM_0_SCORE, u64::from(running.scores[0])),
        (BLUE_STATUS, blue.status()),
        (RED_ENTITY, red.entity()),
        (RED_STATUS, red.status()),
        (BLUE_INTERACTABLE, interactable(blue)),
    ]
}

impl CaptureTheFlagMatch {
    fn next_version(&mut self) -> u64 {
        self.version += 1;
        self.version
    }

    fn index_of(&self, entity: Entity) -> Option<usize> {
        self.participants
            .iter()
            .position(|participant| participant.entity == entity)
    }

    /// The team raid id and the team place of player `number` ([`super::raids`]).
    pub(super) fn raid_seat_of(&self, number: u64) -> Option<(u64, u8)> {
        let participant = self
            .participants
            .iter()
            .find(|participant| participant.number == number)?;
        let raid = *self.raids.get(usize::from(participant.team))?;
        Some((raid, participant.place))
    }

    fn of(&self, entity: Entity) -> Option<&MatchParticipant> {
        self.participants
            .iter()
            .find(|participant| participant.entity == entity)
    }

    fn present(&self, team: u8) -> bool {
        self.participants
            .iter()
            .any(|participant| participant.team == team)
    }

    const fn fighting(&self) -> bool {
        matches!(self.state, RUNNING | OVERTIME)
    }

    /// The values of the match now.
    fn values(&self) -> Vec<Keyed> {
        let mut values = base_values(self);
        let stats = &self.stats;
        if self.stats_shown {
            values.extend([
                (TEAM_0_KILLS, u64::from(stats.kills[0])),
                (TEAM_1_KILLS, u64::from(stats.kills[1])),
                (TEAM_0_DEATHS, u64::from(stats.deaths[0])),
                (TEAM_1_DEATHS, u64::from(stats.deaths[1])),
                (
                    TEAM_0_FLAGBEARERS_KILLED,
                    u64::from(stats.flagbearers_killed[0]),
                ),
                (
                    TEAM_1_FLAGBEARERS_KILLED,
                    u64::from(stats.flagbearers_killed[1]),
                ),
            ]);
        }
        for (key, returned) in [TEAM_0_FLAGS_RETURNED, TEAM_1_FLAGS_RETURNED]
            .into_iter()
            .zip(stats.flags_returned)
        {
            if returned > 0 {
                values.push((key, u64::from(returned)));
            }
        }
        let [red, blue] = &self.flags;
        for (key, flag) in [(RED_CANNOT_SCORE, red), (BLUE_CANNOT_SCORE, blue)] {
            if let Some(cannot) = flag.cannot_score {
                values.push((key, u64::from(cannot)));
            }
        }
        for (key, flag) in [(RED_RESET_WARNING, red), (BLUE_RESET_WARNING, blue)] {
            if flag.warned {
                values.push((key, 1));
            }
        }
        if self.state == ENDED {
            values.retain(|(key, _)| *key != RED_ENTITY && *key != BLUE_ENTITY);
            values.extend([
                (RED_ENTITY, NO_FLAG_ENTITY),
                (BLUE_ENTITY, NO_FLAG_ENTITY),
                (TEAM_0_REGULAR_CAPTURES, u64::from(stats.captures[0])),
                (TEAM_1_REGULAR_CAPTURES, u64::from(stats.captures[1])),
                (TEAM_0_WILD_CAPTURES, 0),
                (TEAM_1_WILD_CAPTURES, 0),
                (WINNING_TEAM, u64::from(self.winner.unwrap_or(TIE))),
            ]);
        }
        values
    }

    /// The timers of the match now.
    fn timers(&self) -> Vec<Keyed> {
        let mut timers = match self.state {
            PREGAME => vec![
                (GAME_MODE_TIMEOUT, self.timeout_ms),
                (PREGAME_TIMER, self.ends_ms),
            ],
            RUNNING => vec![
                (GAME_MODE_TIMEOUT, self.timeout_ms),
                (TIME_LIMIT_TIMER, self.ends_ms),
            ],
            OVERTIME => vec![
                (OVERTIME_TIMER, self.ends_ms),
                (GAME_MODE_TIMEOUT, self.timeout_ms),
            ],
            _ => vec![(CLEANUP_TIMER, self.ends_ms)],
        };
        let [red, blue] = &self.flags;
        for (reset, interact, flag) in [
            (RED_RESET_TIMER, RED_INTERACT_TIMER, red),
            (BLUE_RESET_TIMER, BLUE_INTERACT_TIMER, blue),
        ] {
            if let Some(at) = flag.reset_ms {
                timers.push((reset, at));
            }
            if let Some(at) = flag.interact_ms {
                timers.push((interact, at));
            }
        }
        timers
    }

    /// The values and timers at a new version.
    fn state_change(&mut self) -> GameModeState {
        let version = self.next_version();
        GameModeState {
            values: Some(full(version, self.values())),
            timers: Some(full(version, self.timers())),
            ..GameModeState::default()
        }
    }

    /// The change to script state `state` whose timer ends at `ends_ms`.
    fn script_change(&mut self, state: u8, ends_ms: u64) -> GameModeState {
        self.state = state;
        self.ends_ms = ends_ms;
        GameModeState {
            script_state: Some(state),
            ..self.state_change()
        }
    }

    /// The change of participant `index`'s status to `status`.
    fn status_change(&mut self, index: usize, status: u8) -> GameModeState {
        self.participants[index].status = status;
        let version = self.next_version();
        GameModeState {
            statuses: Some(ReplicatedList::Changes(vec![ListChange {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                version,
                value: status,
            }])),
            ..GameModeState::default()
        }
    }

    /// The team with more points; `None` on a tie.
    fn leader(&self) -> Option<u8> {
        match self.scores[0].cmp(&self.scores[1]) {
            std::cmp::Ordering::Greater => Some(0),
            std::cmp::Ordering::Less => Some(1),
            std::cmp::Ordering::Equal => None,
        }
    }
}

/// What a flag interaction does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Touch {
    /// The player takes the flag.
    Take,
    /// The owner team brings its dropped flag back.
    Return,
}

/// What a touch of a flag of team `owner` at `place` by a player of team `team` does; `None`
/// for the own flag at its base and a carried flag.
fn touch_of(owner: u8, place: FlagPlace, team: u8) -> Option<Touch> {
    match place {
        FlagPlace::Base if team != owner => Some(Touch::Take),
        FlagPlace::Dropped if team == owner => Some(Touch::Return),
        FlagPlace::Dropped => Some(Touch::Take),
        FlagPlace::Base | FlagPlace::Carried { .. } => None,
    }
}

/// The winner of the match by its points at a time limit (`FUN_146b4a770`): the leader, else
/// `None` (a tie) with `overtime` (the end of the overtime) or the start of the overtime
/// without.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeUp {
    Win(Option<u8>),
    Overtime,
}

const fn time_up(leader: Option<u8>, overtime: bool) -> TimeUp {
    match (leader, overtime) {
        (None, false) => TimeUp::Overtime,
        (leader, _) => TimeUp::Win(leader),
    }
}

/// The end event of a player of team `team` when `winner` won (`None`: a tie).
fn end_event(team: u8, winner: Option<u8>) -> &'static str {
    match winner {
        Some(winner) if winner == team => WIN_EVENT,
        Some(_) => LOSS_EVENT,
        None => TIE_EVENT,
    }
}

/// Whether `scores` win for team `team` at once (`FUN_146b488a0`).
fn wins(scores: [u32; TEAMS], team: u8) -> bool {
    let own = scores[usize::from(team.min(1))];
    let other = scores[usize::from(1 - team.min(1))];
    own >= POINTS_TO_WIN && own.saturating_sub(other) >= LEAD_TO_WIN
}

impl SimWorld {
    /// Whether this instance sim's game mode is capture the flag.
    pub(super) fn runs_capture_the_flag(&self) -> bool {
        self.instance_run
            .as_ref()
            .and_then(|run| self.content.instances.by_id(run.game_mode()))
            .is_some_and(|mode| mode.kind == InstanceKind::CaptureTheFlag)
    }

    /// Player `entity` arrived with its seat `seat`: it joins the match (the first one opens it)
    /// with its participant state and affiliations.
    pub(super) fn join_capture_the_flag(&mut self, entity: Entity, seat: ArenaSeat) {
        let Some((number, character, eligible)) = self.world.get::<Player>(entity).map(|player| {
            (
                player.number,
                *player.character.as_bytes(),
                player.arena.queue_eligible().to_vec(),
            )
        }) else {
            return;
        };
        if self.arena_matches.capture_the_flag.is_none() && !self.open_capture_the_flag() {
            return;
        }
        let server_ms = self.server_ms;
        let phase = self.instance_run.as_ref().map_or(0, |run| run.phase());
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return;
        };
        if running.of(entity).is_some() || running.state == ENDED {
            return;
        }
        running.participants.push(MatchParticipant {
            number,
            entity,
            character,
            team: seat.team,
            place: seat.place,
            status: PLAYING_STATUS,
        });
        running.both_teams_seen |= (0..TEAMS as u8).all(|team| running.present(team));
        let version = running.next_version();
        let lists = participant_lists(&running.participants, version, true);
        running.list_version += 1;
        let list_version = running.list_version;
        let entry = MatchEntry {
            gde_ref: running.gde_ref,
            mode: running.crc,
            map: running.map.map_crc,
            phase,
            flags: MATCH_FLAGS,
            team: seat.team,
            list_version,
            eligible,
        };
        let mode_entity = running.entity;
        self.change_game_mode_state(mode_entity, lists);
        self.put_match_joined(entity, entry, server_ms);
        self.put_affiliations(entity, list_version, team_affiliations(seat.team));
        tracing::info!(
            player = number,
            team = seat.team,
            "player joined the capture the flag match"
        );
    }

    /// Opens the match of this sim's capture the flag instance: the game mode entity in the
    /// pregame and the level entities. `false` when this sim runs no capture the flag or its
    /// map has no entity table.
    fn open_capture_the_flag(&mut self) -> bool {
        let content = std::sync::Arc::clone(&self.content);
        let Some(mode) = self
            .instance_run
            .as_ref()
            .and_then(|run| content.instances.by_id(run.game_mode()))
            .filter(|mode| mode.kind == InstanceKind::CaptureTheFlag)
        else {
            return false;
        };
        let folder = self.settings.level.folder;
        let (Some(map), Some(layout)) = (
            mode.map_of(folder).cloned(),
            CTF_MAPS.iter().find(|map| map.folder == folder),
        ) else {
            tracing::warn!(
                level = folder,
                "capture the flag map without level entities"
            );
            return false;
        };
        let server_ms = self.server_ms;
        let started = self.arena_matches.started;
        self.arena_matches.started += 1;
        let raid = RAID_ID_BASE + started * TEAMS as u64;
        let mut running = CaptureTheFlagMatch {
            entity: 0,
            gde_ref: [0; RAW_ID_LEN],
            crc: mode.crc,
            map,
            layout,
            participants: Vec::new(),
            state: PREGAME,
            ends_ms: server_ms.saturating_add(PREGAME_MS),
            timeout_ms: server_ms.saturating_add(GAME_MODE_TIMEOUT_MS),
            flags: [Flag::new(), Flag::new()],
            scores: [0; TEAMS],
            stats: TeamStats::default(),
            stats_shown: false,
            raids: [raid, raid + 1],
            winner: None,
            barriers: Vec::new(),
            props: Vec::new(),
            power_ups: Vec::new(),
            celebrations: Vec::new(),
            on_pads: Vec::new(),
            version: FIRST_LIST_VERSION,
            list_version: FIRST_LIST_VERSION,
            both_teams_seen: false,
        };
        running.barriers = layout
            .barriers
            .iter()
            .map(|barrier| {
                self.place_ctf_entity(barrier.position(), |gde_ref| {
                    vec![
                        card(layout.barrier_asset, gde_ref),
                        ownership(TEAM_RULE, NO_RAID),
                        barrier.member(FLAG_POSITION_COMPONENT, layout.barrier_scale),
                        closed_door(),
                    ]
                })
                .0
            })
            .collect();
        let mut props: Vec<u64> = layout
            .platforms
            .iter()
            .map(|platform| self.place_prop(PLATFORM, platform))
            .collect();
        props.extend(
            layout
                .jump_pads
                .iter()
                .map(|(kind, pad)| self.place_prop(kind.asset(), pad)),
        );
        for (team, spot) in layout.spawn_points.iter().enumerate() {
            let raid = running.raids[team];
            props.push(
                self.place_ctf_entity(spot.position(), |gde_ref| {
                    vec![
                        card(SPAWN_POINTS, gde_ref),
                        ownership(TEAM_RULE, raid),
                        spot.member(FLAG_POSITION_COMPONENT, None),
                    ]
                })
                .0,
            );
        }
        running.props = props;
        running.power_ups = (0..layout.power_ups.len())
            .map(|index| PowerUp {
                index,
                entity: Some(self.place_power_up(layout, index)),
                respawn_ms: None,
            })
            .collect();
        for team in 0..TEAMS {
            let other_raid = running.raids[1 - team];
            running.flags[team].pickup =
                Some(self.place_pickup(team, layout.flag_bases[team], Some(other_raid), true));
        }
        let version = running.version;
        let state = GameModeState {
            script_state: Some(PREGAME),
            spawned: Some(full(version, Vec::new())),
            mode: Some(running.crc),
            map: Some(running.map.map_crc),
            participants: Some(ReplicatedList::Full(full(version, Vec::new()))),
            statuses: Some(ReplicatedList::Full(full(version, Vec::new()))),
            teams: Some(ReplicatedList::Full(full(version, Vec::new()))),
            characters: Some(ReplicatedList::Full(full(version, Vec::new()))),
            raids: Some(ReplicatedList::Full(full(version, running.raids.to_vec()))),
            values: Some(full(version, running.values())),
            timers: Some(full(version, running.timers())),
            no_affliction: true,
            linked_mode: Some(0),
            ..GameModeState::default()
        };
        let middle = position_of(layout.flag_bases[0]);
        let (entity, gde_ref) = self.place_game_mode_entity_on(
            GAME_MODE_COMPONENT,
            MATCH_NAMESPACE,
            None,
            middle,
            state,
        );
        running.entity = entity;
        running.gde_ref = gde_ref;
        tracing::info!(level = %running.map.folder, "capture the flag match opened");
        self.arena_matches.capture_the_flag = Some(running);
        true
    }

    /// Places a level entity of the match at `position` whose members `members` makes for its
    /// raw id; returns its slot key and raw id.
    fn place_ctf_entity(
        &mut self,
        position: Position,
        members: impl FnOnce([u8; RAW_ID_LEN]) -> Vec<BundleMember>,
    ) -> (u64, [u8; RAW_ID_LEN]) {
        let namespace = LEVEL_NAMESPACE ^ self.me.generation;
        let mut raw_id = [0; RAW_ID_LEN];
        let number = self.stations.place_camp(|number| {
            // A slot key is far below `usize::MAX` on the 64-bit targets the server runs on.
            let gde_ref = dynamic_ref(namespace, number as usize);
            raw_id = gde_ref;
            Station {
                gde_ref,
                position,
                station_type: String::new(),
                name: Vec::new(),
                members: members(gde_ref),
            }
        });
        (number, raw_id)
    }

    /// Places a prop (a platform, jump pad, power-up or celebration): card and position.
    fn place_prop(&mut self, asset: [u8; RAW_ID_LEN], at: &Placed) -> u64 {
        self.place_ctf_entity(at.position(), |gde_ref| {
            vec![
                card(asset, gde_ref),
                at.member(PROP_POSITION_COMPONENT, None),
            ]
        })
        .0
    }

    /// Places power-up `index` of `layout`.
    fn place_power_up(&mut self, layout: &CtfMap, index: usize) -> u64 {
        let (kind, at) = layout.power_ups[index];
        self.place_prop(kind.asset(), &at)
    }

    /// Places the pickup flag of team `team` at `at`: at its base for the raid `raid` that may
    /// take it, else dropped for anybody; its interaction on when `enabled`.
    fn place_pickup(
        &mut self,
        team: usize,
        at: WorldPosition,
        raid: Option<u64>,
        enabled: bool,
    ) -> (u64, [u8; RAW_ID_LEN]) {
        let asset = [RED_PICKUP, BLUE_PICKUP][team];
        let owner = raid.map_or(ownership(OPEN_RULE, NO_RAID), |raid| {
            ownership(TEAM_RULE, raid)
        });
        let place = placed([at.x, at.y], at.z_raw, None);
        self.place_ctf_entity(position_of(at), |gde_ref| {
            vec![
                card(asset, gde_ref),
                owner,
                place.member(FLAG_POSITION_COMPONENT, None),
                interact_member(enabled),
            ]
        })
    }

    /// Places the ghost flag of team `team` at its base.
    fn place_ghost(&mut self, team: usize, at: WorldPosition) -> u64 {
        let asset = [RED_GHOST, BLUE_GHOST][team];
        let place = placed([at.x, at.y], at.z_raw, None);
        self.place_ctf_entity(position_of(at), |gde_ref| {
            vec![
                card(asset, gde_ref),
                ownership(OPEN_RULE, NO_RAID),
                place.member(FLAG_POSITION_COMPONENT, None),
            ]
        })
        .0
    }

    /// Places the carried flag of team `team` on carrier `entity` (`number`) of raid `raid`.
    /// `None` when the carrier has no body here.
    ///
    /// Observed (every carried flag body of the five ledgers, `CTF04` 205.526 s): the spell
    /// starts at the carrier with a turn about the up axis only; the ownership names the
    /// carrier's company (id, name and crest; all zero, empty and no crest for a carrier
    /// without one), the carrier (kind 2), rule 4, its team's raid, its faction and a PvP value
    /// that differs between carriers of one ledger. Inferred: the turn is the carrier's look
    /// and the PvP value its PvP flag; a company whose look is not known yet is left out
    /// ([`super::company_looks`]).
    fn place_carried(
        &mut self,
        team: usize,
        entity: Entity,
        number: u64,
        raid: u64,
    ) -> Option<u64> {
        let (owner, faction, pvp, yaw, at) =
            self.world.get::<Player>(entity).and_then(|player| {
                let at = self.world.get::<Body>(entity)?.motion.position();
                Some((
                    OwnerIdentification {
                        first: player.character.to_string().into_bytes(),
                        second: player.name.clone().into_bytes(),
                        kind: NO_OWNER_KIND,
                    },
                    player.faction.faction,
                    player.pvp_flagged,
                    player.view.map_or(0.0, |view| view.yaw),
                    at,
                ))
            })?;
        let look = self.company_look_of(entity);
        let half = yaw / 2.0;
        let spell = SpellState {
            spell: Some(lowercase_crc32(FLAG_EFFECTS[team])),
            start: WorldPosition::from_metres(at.x, at.y, at.z),
            start_rotation: Some(Rotation::of_quaternion([0.0, 0.0, half.sin(), half.cos()])),
            attachment: Some(caster_ref(number)),
            caster: Some(caster_ref(number)),
            radius_scale: Some(CARRIED_RADIUS_SCALE),
            ..SpellState::default()
        };
        let mut ownership = OwnershipState {
            owner: Some(owner),
            raid_id: Some(raid),
            faction_type: Some(faction),
            owner_pvp: Some(pvp),
            ..unowned(TEAM_RULE, "")
        };
        if let Some((id, look)) = look {
            ownership.guild_id = Some(id);
            ownership.guild_owner = Some(look.name);
            ownership.guild_crest = Some(look.crest);
        }
        let asset = [RED_CARRIED, BLUE_CARRIED][team];
        self.place_ctf_entity(at, |gde_ref| {
            vec![
                card(asset, gde_ref),
                BundleMember {
                    component: CARRIED_SPELL_COMPONENT,
                    state: MemberState::Spell(spell),
                },
                BundleMember {
                    component: CARRIED_OWNERSHIP_COMPONENT,
                    state: MemberState::Ownership(ownership),
                },
            ]
        })
        .0
        .into()
    }

    /// Asks for the companies of the participants ([`super::company_looks`]).
    fn ask_ctf_companies(&mut self) {
        let Some(running) = self.arena_matches.capture_the_flag.as_ref() else {
            return;
        };
        let participants: Vec<Entity> = running
            .participants
            .iter()
            .map(|participant| participant.entity)
            .collect();
        self.ask_company_looks(&participants);
    }

    /// Sends the values and timers of the match.
    fn send_ctf_state(&mut self) {
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return;
        };
        let change = running.state_change();
        let entity = running.entity;
        self.change_game_mode_state(entity, change);
    }

    /// 6824: player `number` readied up (or not) in the pregame.
    pub(super) fn capture_the_flag_ready(&mut self, number: u64, ready: bool) {
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return;
        };
        let Some(index) = running
            .participants
            .iter()
            .position(|participant| participant.number == number)
        else {
            return;
        };
        if running.state != PREGAME {
            return;
        }
        let status = running.participants[index].status;
        let status = if ready {
            status | READY_BIT
        } else {
            status & !READY_BIT
        };
        let change = running.status_change(index, status);
        let entity = running.entity;
        self.change_game_mode_state(entity, change);
    }

    /// Whether players `first` and `second` are foes in the match: participants of two teams
    /// while it runs. `None` when this sim runs no capture the flag match.
    pub(super) fn capture_the_flag_foes(&self, first: Entity, second: Entity) -> Option<bool> {
        let running = self.arena_matches.capture_the_flag.as_ref()?;
        let team = |entity: Entity| running.of(entity).map(|participant| participant.team);
        Some(
            running.fighting()
                && team(first)
                    .zip(team(second))
                    .is_some_and(|(first, second)| first != second),
        )
    }

    /// Where dead player `entity` respawns: its team's spot, with its status alive again.
    /// `None` when it plays no capture the flag match here.
    pub(super) fn capture_the_flag_respawn(&mut self, entity: Entity) -> Option<Position> {
        let running = self.arena_matches.capture_the_flag.as_mut()?;
        let index = running.index_of(entity)?;
        let participant = running.participants[index].clone();
        let spot = team_spot(&running.map, participant.team, participant.place)?;
        let change = running.status_change(index, participant.status & !DEAD_BIT);
        let mode_entity = running.entity;
        self.change_game_mode_state(mode_entity, change);
        Some(spot)
    }

    /// Player `target` died to `attacker`: its status is dead, the deaths and kills count, and a
    /// carried flag drops where it died.
    pub(super) fn capture_the_flag_death(&mut self, target: u64, attacker: u64) {
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return;
        };
        let Some(index) = running
            .participants
            .iter()
            .position(|participant| participant.number == target)
        else {
            return;
        };
        let dead = running.participants[index].clone();
        let change = running.status_change(index, dead.status | DEAD_BIT);
        let entity = running.entity;
        let fighting = running.fighting();
        let killer = running
            .participants
            .iter()
            .find(|participant| participant.number == attacker)
            .map(|participant| participant.team)
            .filter(|team| *team != dead.team);
        if fighting {
            let stats = &mut running.stats;
            stats.deaths[usize::from(dead.team.min(1))] += 1;
            if let Some(team) = killer {
                stats.kills[usize::from(team.min(1))] += 1;
            }
            running.stats_shown = true;
        }
        let carried = running
            .flags
            .iter()
            .position(|flag| flag.carrier() == Some(dead.entity));
        self.change_game_mode_state(entity, change);
        if let Some(team) = carried {
            if let (Some(killer), Some(running)) =
                (killer, self.arena_matches.capture_the_flag.as_mut())
            {
                running.stats.flagbearers_killed[usize::from(killer.min(1))] += 1;
            }
            self.drop_flag(team);
        }
        if fighting {
            self.send_ctf_state();
        }
    }

    /// Flag `team` drops where its carrier stands: a pickup flag there whose interaction is
    /// off, the reset and interact timers.
    fn drop_flag(&mut self, team: usize) {
        let server_ms = self.server_ms;
        let Some(flag) = self
            .arena_matches
            .capture_the_flag
            .as_ref()
            .map(|running| running.flags[team].clone())
        else {
            return;
        };
        let Some(carrier) = flag.carrier() else {
            return;
        };
        let Some(at) = self
            .world
            .get::<Body>(carrier)
            .map(|body| body.motion.position())
        else {
            return;
        };
        // A death place out of the wire's height range gives the flag back to its base
        // (Inferred).
        let Some(place) = WorldPosition::from_metres(at.x, at.y, at.z) else {
            self.flag_home(team, RETURN_BUFFER_MS);
            return;
        };
        self.take_effect(carrier, FLAG_EFFECTS[team]);
        if let Some(carried) = flag.carried {
            self.stations.remove_camp(carried);
        }
        let pickup = self.place_pickup(team, place, None, false);
        if let Some(running) = self.arena_matches.capture_the_flag.as_mut() {
            let flag = &mut running.flags[team];
            flag.place = FlagPlace::Dropped;
            flag.carried = None;
            flag.pickup = Some(pickup);
            flag.reset_ms = Some(server_ms.saturating_add(FLAG_RESET_MS));
            flag.interact_ms = Some(server_ms.saturating_add(DROP_BUFFER_MS));
            flag.cannot_score = Some(false);
            flag.in_zone = false;
        }
        tracing::info!(team, "capture the flag flag dropped");
    }

    /// Flag `team` stands at its base again (a return, a capture or a reset) with the interact
    /// timer `buffer_ms`.
    fn flag_home(&mut self, team: usize, buffer_ms: u64) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return;
        };
        let flag = std::mem::replace(&mut running.flags[team], Flag::new());
        let base = running.layout.flag_bases[team];
        let other_raid = running.raids[1 - team];
        let gone = [
            flag.pickup.map(|(number, _)| number),
            flag.carried,
            flag.ghost,
        ];
        for number in gone.into_iter().flatten() {
            self.stations.remove_camp(number);
        }
        if let Some(carrier) = flag.carrier() {
            self.take_effect(carrier, FLAG_EFFECTS[team]);
        }
        let pickup = self.place_pickup(team, base, Some(other_raid), false);
        if let Some(running) = self.arena_matches.capture_the_flag.as_mut() {
            running.flags[team] = Flag {
                pickup: Some(pickup),
                interact_ms: Some(server_ms.saturating_add(buffer_ms)),
                warned: flag.warned,
                cannot_score: flag.cannot_score,
                ..Flag::new()
            };
        }
    }

    /// The flag interaction `request` of player `entity` (`number`): two 3743 of status 2 and,
    /// when the flag may be touched, the take or the return. `false` when it names no flag.
    pub(super) fn capture_the_flag_interaction(
        &mut self,
        entity: Entity,
        number: u64,
        request: InteractionRequest,
        name: &[u8],
    ) -> bool {
        if request.kind != TRIGGER_KIND {
            return false;
        }
        let Some(running) = self.arena_matches.capture_the_flag.as_ref() else {
            return false;
        };
        let Some(team) = running.flags.iter().position(|flag| {
            flag.pickup
                .is_some_and(|(_, gde_ref)| gde_ref == request.target)
        }) else {
            return false;
        };
        let answer = gather_answer(number, request, FLAG_INTERACTION_STATUS, name);
        let mut first = answer.clone();
        if let ServerMessage::InteractionValidated(message) = &mut first {
            message.id = [0; RAW_ID_LEN];
        }
        self.send_to(entity, &[first, answer]);
        let Some(running) = self.arena_matches.capture_the_flag.as_ref() else {
            return true;
        };
        let flag = &running.flags[team];
        let Some(player_team) = running
            .of(entity)
            .filter(|player| player.status & DEAD_BIT == 0)
            .map(|player| player.team)
        else {
            return true;
        };
        if !running.fighting() || flag.interact_ms.is_some() {
            return true;
        }
        // Two teams: a team index is 0 or 1.
        match touch_of(team as u8, flag.place, player_team) {
            Some(Touch::Take) => self.take_flag(team, entity, number, player_team),
            Some(Touch::Return) => self.return_flag(team),
            None => {}
        }
        true
    }

    /// Player `entity` (`number`) of team `carrier_team` takes flag `team`.
    fn take_flag(&mut self, team: usize, entity: Entity, number: u64, carrier_team: u8) {
        let Some(running) = self.arena_matches.capture_the_flag.as_ref() else {
            return;
        };
        let flag = running.flags[team].clone();
        let base = running.layout.flag_bases[team];
        let raid = running.raids[usize::from(carrier_team.min(1))];
        let Some(carried) = self.place_carried(team, entity, number, raid) else {
            return;
        };
        if let Some((pickup, _)) = flag.pickup {
            self.stations.remove_camp(pickup);
        }
        let ghost = match flag.ghost {
            Some(ghost) => ghost,
            None => self.place_ghost(team, base),
        };
        self.give_effect(entity, FLAG_EFFECTS[team]);
        if let Some(running) = self.arena_matches.capture_the_flag.as_mut() {
            let flag = &mut running.flags[team];
            flag.place = FlagPlace::Carried {
                entity,
                number,
                team: carrier_team,
            };
            flag.pickup = None;
            flag.carried = Some(carried);
            flag.ghost = Some(ghost);
            flag.reset_ms = None;
            flag.in_zone = false;
        }
        self.send_ctf_state();
        tracing::info!(team, player = number, "capture the flag flag taken");
    }

    /// The owner team brings dropped flag `team` back.
    fn return_flag(&mut self, team: usize) {
        if let Some(running) = self.arena_matches.capture_the_flag.as_mut() {
            running.stats.flags_returned[team] += 1;
        }
        self.flag_home(team, RETURN_BUFFER_MS);
        self.send_ctf_state();
        tracing::info!(team, "capture the flag flag returned");
    }

    /// The carrier of flag `team` (of team `scorer`) scored at its dropzone.
    fn capture_flag(&mut self, team: usize, scorer: u8) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return;
        };
        let scorer_index = usize::from(scorer.min(1));
        running.scores[scorer_index] += CAPTURE_POINTS;
        running.stats.captures[scorer_index] += 1;
        let dropzone = running.layout.flag_bases[scorer_index];
        let scores = running.scores;
        let overtime = running.state == OVERTIME;
        self.flag_home(team, SCORE_BUFFER_MS);
        if let Some(running) = self.arena_matches.capture_the_flag.as_mut() {
            running.flags[team].cannot_score = Some(false);
        }
        let place = placed([dropzone.x, dropzone.y], dropzone.z_raw, None);
        let celebration = self.place_prop(CELEBRATION, &place);
        if let Some(running) = self.arena_matches.capture_the_flag.as_mut() {
            running
                .celebrations
                .push((celebration, server_ms.saturating_add(CELEBRATION_MS)));
        }
        tracing::info!(team, scorer, ?scores, "capture the flag flag captured");
        if wins(scores, scorer) || overtime {
            self.end_capture_the_flag(Some(scorer));
        } else {
            self.send_ctf_state();
        }
    }

    /// Runs the match: a team with no participant left loses, the pregame's end, the flags,
    /// the touches, the power-ups, the time limit and the end of `Cleanup`.
    pub(super) fn tick_capture_the_flag(&mut self) {
        self.ask_ctf_companies();
        let now_ms = self.server_ms;
        let Some(running) = self.arena_matches.capture_the_flag.as_ref() else {
            return;
        };
        let (state, ends_ms) = (running.state, running.ends_ms);
        let absent = (0..TEAMS as u8).find(|team| !running.present(*team));
        if state != ENDED
            && let Some(team) = absent
            && (running.both_teams_seen || (state == PREGAME && now_ms >= ends_ms))
        {
            self.end_capture_the_flag(Some(1 - team));
            return;
        }
        self.end_celebrations(now_ms);
        match state {
            PREGAME if now_ms >= ends_ms => self.start_capture_the_flag(),
            RUNNING | OVERTIME => self.run_capture_the_flag(now_ms),
            ENDED if now_ms >= ends_ms => self.cleanup_capture_the_flag(),
            _ => {}
        }
    }

    /// `PregameTimer` ended: the barriers go, state 0 with `TimeLimit`.
    fn start_capture_the_flag(&mut self) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return;
        };
        let barriers = std::mem::take(&mut running.barriers);
        let change = running.script_change(RUNNING, server_ms.saturating_add(TIME_LIMIT_MS));
        let entity = running.entity;
        for barrier in barriers {
            self.stations.remove_camp(barrier);
        }
        self.change_game_mode_state(entity, change);
        tracing::info!("capture the flag match started");
    }

    /// Takes the celebrations whose time ended away.
    fn end_celebrations(&mut self, now_ms: u64) {
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return;
        };
        let (over, kept): (Vec<_>, Vec<_>) = running
            .celebrations
            .iter()
            .partition(|(_, ends_ms)| now_ms >= *ends_ms);
        running.celebrations = kept;
        for (number, _) in over {
            self.stations.remove_camp(number);
        }
    }

    /// The running match at server ms `now_ms`.
    fn run_capture_the_flag(&mut self, now_ms: u64) {
        let mut changed = self.tick_flag_timers(now_ms);
        changed |= self.tick_dropzones();
        if self
            .arena_matches
            .capture_the_flag
            .as_ref()
            .map(|running| running.state)
            == Some(ENDED)
        {
            return;
        }
        self.tick_power_ups(now_ms);
        self.tick_jump_pads();
        if changed {
            self.send_ctf_state();
        }
        let Some(running) = self.arena_matches.capture_the_flag.as_ref() else {
            return;
        };
        if now_ms >= running.ends_ms {
            match time_up(running.leader(), running.state == OVERTIME) {
                TimeUp::Win(winner) => self.end_capture_the_flag(winner),
                TimeUp::Overtime => self.start_ctf_overtime(),
            }
        }
    }

    /// The flag timers at `now_ms`: the interact buffers that ended, the reset warnings and the
    /// resets. `true` when a value or timer changed.
    fn tick_flag_timers(&mut self, now_ms: u64) -> bool {
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return false;
        };
        let mut changed = false;
        let mut opened = Vec::new();
        let mut resets = Vec::new();
        for (team, flag) in running.flags.iter_mut().enumerate() {
            if flag.interact_ms.is_some_and(|at| now_ms >= at) {
                flag.interact_ms = None;
                changed = true;
                opened.extend(flag.pickup.map(|(number, _)| number));
            }
            if let Some(reset_ms) = flag.reset_ms {
                if !flag.warned && now_ms.saturating_add(RESET_WARNING_MS) >= reset_ms {
                    flag.warned = true;
                    changed = true;
                }
                if now_ms >= reset_ms {
                    resets.push(team);
                }
            }
        }
        for number in opened {
            self.stations.change_camp(
                number,
                |members| {
                    for member in members {
                        if let MemberState::Interact(state) = &mut member.state {
                            state.enabled = Some(true);
                        }
                    }
                },
                vec![interact_member(true)],
            );
        }
        for team in resets {
            self.flag_home(team, RETURN_BUFFER_MS);
            changed = true;
            tracing::info!(team, "capture the flag flag reset");
        }
        changed
    }

    /// The carriers at their dropzones: a capture, or the cannot-score value while their own
    /// flag is away. `true` when a value changed.
    fn tick_dropzones(&mut self) -> bool {
        let Some(running) = self.arena_matches.capture_the_flag.as_ref() else {
            return false;
        };
        let mut entered = Vec::new();
        let mut zones = Vec::new();
        for (team, flag) in running.flags.iter().enumerate() {
            let FlagPlace::Carried {
                entity,
                team: carrier_team,
                ..
            } = flag.place
            else {
                continue;
            };
            let zone = position_of(running.layout.flag_bases[usize::from(carrier_team.min(1))]);
            let inside = self
                .world
                .get::<Body>(entity)
                .is_some_and(|body| touches(body.motion.position(), zone, DROPZONE));
            zones.push((team, inside));
            if inside && !flag.in_zone {
                let own_safe =
                    running.flags[usize::from(carrier_team.min(1))].place == FlagPlace::Base;
                entered.push((team, carrier_team, own_safe));
            }
        }
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return false;
        };
        let mut changed = false;
        for (team, inside) in zones {
            let flag = &mut running.flags[team];
            flag.in_zone = inside;
            if !inside && flag.cannot_score == Some(true) {
                flag.cannot_score = Some(false);
                changed = true;
            }
        }
        for (team, carrier_team, own_safe) in entered {
            if own_safe {
                self.capture_flag(team, carrier_team);
            } else if let Some(running) = self.arena_matches.capture_the_flag.as_mut() {
                running.flags[team].cannot_score = Some(true);
                changed = true;
            }
        }
        changed
    }

    /// The living participants and where they stand.
    fn ctf_living(&self) -> Vec<(Entity, Position)> {
        let Some(running) = self.arena_matches.capture_the_flag.as_ref() else {
            return Vec::new();
        };
        running
            .participants
            .iter()
            .filter(|participant| participant.status & DEAD_BIT == 0)
            .filter_map(|participant| {
                self.world
                    .get::<Body>(participant.entity)
                    .map(|body| (participant.entity, body.motion.position()))
            })
            .collect()
    }

    /// The power-ups: a touch takes one and gives its effect, a taken one comes back after
    /// [`POWER_UP_RESPAWN_MS`].
    fn tick_power_ups(&mut self, now_ms: u64) {
        let living = self.ctf_living();
        let Some(running) = self.arena_matches.capture_the_flag.as_ref() else {
            return;
        };
        let layout = running.layout;
        let mut taken = Vec::new();
        let mut respawns = Vec::new();
        for (slot, power_up) in running.power_ups.iter().enumerate() {
            let (kind, at) = layout.power_ups[power_up.index];
            match power_up.entity {
                Some(number) => {
                    if let Some((player, _)) = living
                        .iter()
                        .find(|(_, place)| touches(*place, at.position(), POWER_UP_TRIGGER))
                    {
                        taken.push((slot, number, *player, kind));
                    }
                }
                None if power_up.respawn_ms.is_some_and(|ms| now_ms >= ms) => {
                    respawns.push(slot);
                }
                None => {}
            }
        }
        for (slot, number, player, kind) in taken {
            self.stations.remove_camp(number);
            self.give_effect(player, kind.effect());
            if let Some(running) = self.arena_matches.capture_the_flag.as_mut() {
                running.power_ups[slot] = PowerUp {
                    entity: None,
                    respawn_ms: Some(now_ms.saturating_add(POWER_UP_RESPAWN_MS)),
                    ..running.power_ups[slot]
                };
            }
        }
        for slot in respawns {
            let Some(index) = self
                .arena_matches
                .capture_the_flag
                .as_ref()
                .map(|running| running.power_ups[slot].index)
            else {
                continue;
            };
            let number = self.place_power_up(layout, index);
            if let Some(running) = self.arena_matches.capture_the_flag.as_mut() {
                running.power_ups[slot] = PowerUp {
                    index,
                    entity: Some(number),
                    respawn_ms: None,
                };
            }
        }
    }

    /// The jump pads: a player who steps on one gets its effect (a carrier's own).
    fn tick_jump_pads(&mut self) {
        let living = self.ctf_living();
        let Some(running) = self.arena_matches.capture_the_flag.as_ref() else {
            return;
        };
        let pads = running.layout.jump_pads;
        let on: Vec<(Entity, usize)> = living
            .iter()
            .flat_map(|(player, place)| {
                pads.iter()
                    .enumerate()
                    .filter(|(_, (kind, pad))| touches(*place, pad.position(), kind.trigger()))
                    .map(|(index, _)| (*player, index))
            })
            .collect();
        let stepped: Vec<(Entity, JumpPadKind, bool)> = on
            .iter()
            .filter(|touch| !running.on_pads.contains(touch))
            .map(|(player, index)| {
                let carrier = running
                    .flags
                    .iter()
                    .any(|flag| flag.carrier() == Some(*player));
                (*player, pads[*index].0, carrier)
            })
            .collect();
        if let Some(running) = self.arena_matches.capture_the_flag.as_mut() {
            running.on_pads = on;
        }
        for (player, kind, carrier) in stepped {
            self.give_effect(player, kind.effect(carrier));
        }
    }

    /// `TimeLimit` ended in a tie: state 2 with `OvertimeTimer`.
    fn start_ctf_overtime(&mut self) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return;
        };
        let change = running.script_change(OVERTIME, server_ms.saturating_add(OVERTIME_MS));
        let entity = running.entity;
        self.change_game_mode_state(entity, change);
        tracing::info!("capture the flag overtime started");
    }

    /// The match ended with `winner` (`None`: a tie): the carried flags go, state 1 with the
    /// end values and `Cleanup`, each player gets its team's end event and its temporary
    /// affiliations go (Observed: empty 3563 lists for every participant at `CTF03` 494.317 s
    /// and `CTF04` 754.504 s, 0.1 s after the end; Inferred: in the end tick here).
    fn end_capture_the_flag(&mut self, winner: Option<u8>) {
        let server_ms = self.server_ms;
        let Some(running) = self.arena_matches.capture_the_flag.as_mut() else {
            return;
        };
        let carried: Vec<(usize, Option<Entity>, Option<u64>)> = running
            .flags
            .iter_mut()
            .enumerate()
            .map(|(team, flag)| {
                // The end sends `Cleanup` alone (Observed: `CTF01` 1222.217 s right after a
                // capture, `CTF04` 754.390 s with a dropped flag).
                flag.reset_ms = None;
                flag.interact_ms = None;
                (team, flag.carrier(), flag.carried.take())
            })
            .collect();
        running.winner = winner;
        let change = running.script_change(ENDED, server_ms.saturating_add(CLEANUP_MS));
        let entity = running.entity;
        running.list_version += 1;
        let list_version = running.list_version;
        let players: Vec<(Entity, u64, u8)> = running
            .participants
            .iter()
            .map(|participant| (participant.entity, participant.number, participant.team))
            .collect();
        for (team, carrier, number) in carried {
            if let Some(number) = number {
                self.stations.remove_camp(number);
            }
            if let Some(carrier) = carrier {
                self.take_effect(carrier, FLAG_EFFECTS[team]);
            }
        }
        self.change_game_mode_state(entity, change);
        for (player, number, team) in players {
            self.give_mode_event(player, number, end_event(team, winner));
            self.put_affiliations(player, list_version, Vec::new());
        }
        tracing::info!(?winner, "capture the flag match ended");
    }

    /// `Cleanup` ended: the players still in the match go back, the entities go and the
    /// instance closes.
    fn cleanup_capture_the_flag(&mut self) {
        let players: Vec<Entity> = self
            .arena_matches
            .capture_the_flag
            .iter()
            .flat_map(|running| {
                running
                    .participants
                    .iter()
                    .map(|participant| participant.entity)
            })
            .collect();
        for player in players {
            self.leave_capture_the_flag(player);
        }
        if let Some(running) = self.arena_matches.capture_the_flag.take() {
            let flags = running.flags.iter().flat_map(|flag| {
                [
                    flag.pickup.map(|(number, _)| number),
                    flag.carried,
                    flag.ghost,
                ]
            });
            let power_ups = running.power_ups.iter().map(|power_up| power_up.entity);
            let numbers = flags
                .chain(power_ups)
                .flatten()
                .chain(running.barriers.iter().copied())
                .chain(running.props.iter().copied())
                .chain(running.celebrations.iter().map(|(number, _)| *number))
                .chain(std::iter::once(running.entity));
            for number in numbers.collect::<Vec<_>>() {
                self.stations.remove_camp(number);
            }
        }
        if let Some(instance) = self.instance_run.as_ref().map(|run| run.id()) {
            self.close_instance(instance);
        }
    }

    /// Takes player `entity` out of the match's lists (its flag goes back to its base); a leave
    /// of a running match bars its queue. The participant and the list version, when it played.
    fn drop_ctf_participant(&mut self, entity: Entity) -> Option<(MatchParticipant, u64)> {
        let carried = self
            .arena_matches
            .capture_the_flag
            .as_ref()?
            .flags
            .iter()
            .position(|flag| flag.carrier() == Some(entity));
        if let Some(team) = carried {
            self.flag_home(team, RETURN_BUFFER_MS);
            self.send_ctf_state();
        }
        let now_ns = unix_ns(self.server_ms);
        let running = self.arena_matches.capture_the_flag.as_mut()?;
        let index = running.index_of(entity)?;
        let participant = running.participants.remove(index);
        let running_match = running.state != ENDED;
        let crc = running.crc;
        running.list_version += 1;
        let list_version = running.list_version;
        let version = running.next_version();
        let lists = participant_lists(&running.participants, version, true);
        let mode_entity = running.entity;
        self.change_game_mode_state(mode_entity, lists);
        if running_match && let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player
                .arena
                .bar_queue(crc, now_ns.saturating_add(LEAVER_PENALTY_NS));
        }
        Some((participant, list_version))
    }

    /// Player `entity` leaves the match (1980 `Leave`, 3309, or the end of `Cleanup`): what a
    /// player who leaves gets, and it goes back to where it came from.
    pub(super) fn leave_capture_the_flag(&mut self, entity: Entity) {
        if let Some((participant, list_version)) = self.drop_ctf_participant(entity) {
            self.send_match_left(entity, &participant, list_version);
            self.stand_arena_player(entity);
        }
        self.exit_instance(entity);
    }

    /// A player of the match leaves the sim (a logout or a lost connection).
    pub(super) fn capture_the_flag_left_sim(&mut self, entity: Entity) {
        self.drop_ctf_participant(entity);
    }

    /// Gives player `entity` the status effect `id` (a power-up, a jump pad or a flag).
    fn give_effect(&mut self, entity: Entity, id: &str) {
        let content = std::sync::Arc::clone(&self.content);
        let now_ms = self.server_ms;
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            let number = player.number;
            if !player.status_effects.add_by_id(
                &content.status_effects,
                id,
                PLAIN_POTENCY,
                number,
                now_ms,
            ) {
                tracing::warn!(
                    player = number,
                    effect = id,
                    "status effect not in the data"
                );
            }
        }
    }

    /// Takes the status effect `id` from player `entity`.
    fn take_effect(&mut self, entity: Entity, id: &str) {
        let Some(crc) = self.content.status_effects.by_id(id).map(|rule| rule.crc) else {
            return;
        };
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.status_effects.remove(crc);
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_bus::{InstanceOpened, InstanceStay, StoredPoint};
    use nw_content::instances::{CAPTURE_THE_FLAG_GAME_MODE, InstanceCatalog, InstanceMode};
    use nw_content::status_effects::{EffectSourceRow, StatusEffectRules, StatusEffectSource};
    use nw_messages::LevelSpec;
    use uuid::Uuid;

    use super::super::game_event_log::{PVP_XP, UMBRAL_CURRENCY};
    use super::super::tradeskill::AZOTH_CURRENCY;
    use super::*;
    use crate::player_content::PlayerContent;
    use crate::world::Placement;
    use crate::world::queue::tests::capture_the_flag;
    use crate::world::tests::{NOW, admit_at, slot};

    /// Players of the test sim.
    const MANY_PLAYERS: usize = 10;
    /// The two players: team 0 and team 1.
    const RED_PLAYER: u64 = 41;
    const BLUE_PLAYER: u64 = 42;
    /// The test instance and its phase.
    const INSTANCE: u128 = 9;
    const PHASE: u64 = 0x5a17_0c3e_99d1_4b21;
    /// The server time of the opening.
    const START_MS: u64 = 1_000_000;
    /// The red and the blue flag.
    const RED: usize = 0;
    const BLUE: usize = 1;

    #[test]
    fn the_named_keys_are_the_crcs_of_their_names() {
        for (key, name) in [
            (TEAM_0_KILLS, "Team0TotalKills"),
            (TEAM_1_KILLS, "Team1TotalKills"),
            (TEAM_0_DEATHS, "Team0TotalDeaths"),
            (TEAM_1_DEATHS, "Team1TotalDeaths"),
            (TEAM_0_FLAGBEARERS_KILLED, "Team0FlagbearersKilled"),
            (TEAM_1_FLAGBEARERS_KILLED, "Team1FlagbearersKilled"),
            (TEAM_0_FLAGS_RETURNED, "Team0FlagsReturned"),
            (TEAM_1_FLAGS_RETURNED, "Team1FlagsReturned"),
            (TEAM_0_REGULAR_CAPTURES, "Team0RegularFlagCaptures"),
            (TEAM_1_REGULAR_CAPTURES, "Team1RegularFlagCaptures"),
            (TEAM_0_WILD_CAPTURES, "Team0WildFlagCaptures"),
            (TEAM_1_WILD_CAPTURES, "Team1WildFlagCaptures"),
            (OVERTIME_TIMER, "OvertimeTimer"),
            (WINNING_TEAM, "WinningTeamIdx"),
            (CLEANUP_TIMER, "Cleanup"),
            (TIME_LIMIT_TIMER, "TimeLimit"),
        ] {
            assert_eq!(key, lowercase_crc32(name), "{name}");
        }
        assert_eq!(lowercase_crc32(FLAG_EFFECTS[RED]), 0x15bc_550c);
        assert_eq!(lowercase_crc32(FLAG_EFFECTS[BLUE]), 0x5f41_e0af);
        assert_eq!(lowercase_crc32(CAPTURE_THE_FLAG_GAME_MODE), 0x930f_ce8f);
        assert_eq!(RED_PICKUP[..4], [0x22, 0x4c, 0xb6, 0x95]);
    }

    #[test]
    fn a_touch_takes_the_other_flag_and_returns_the_own_dropped_one() {
        let dropped = FlagPlace::Dropped;
        assert_eq!(touch_of(0, FlagPlace::Base, 1), Some(Touch::Take));
        assert_eq!(
            touch_of(0, FlagPlace::Base, 0),
            None,
            "the own flag at home"
        );
        assert_eq!(touch_of(0, dropped, 0), Some(Touch::Return));
        assert_eq!(touch_of(0, dropped, 1), Some(Touch::Take));
    }

    #[test]
    fn three_points_win_and_a_tie_at_the_time_limit_runs_the_overtime() {
        assert!(!wins([2, 1], 0));
        assert!(wins([3, 0], 0), "CTF03 494.232");
        assert!(!wins([3, 3], 0));
        assert_eq!(time_up(Some(0), false), TimeUp::Win(Some(0)), "CTF04 2-1");
        assert_eq!(time_up(None, false), TimeUp::Overtime, "CTF02 1-1");
        assert_eq!(time_up(None, true), TimeUp::Win(None));
    }

    /// The slices' trigger shapes meet the player's capsule (radius 0.4 m, 2 m tall).
    #[test]
    fn the_triggers_are_the_slices_shapes_about_the_players_body() {
        let place = Position {
            x: 100.0,
            y: 100.0,
            z: 50.0,
        };
        let at = |east: f32, up: f32| Position {
            x: place.x + east,
            y: place.y,
            z: place.z + up,
        };
        // The dropzone: 2.25 m + 0.4 m about, 3 m above and below the place.
        assert!(touches(at(2.64, 0.0), place, DROPZONE));
        assert!(!touches(at(2.7, 0.0), place, DROPZONE));
        assert!(touches(at(0.0, 2.9), place, DROPZONE));
        assert!(!touches(at(0.0, 3.1), place, DROPZONE));
        assert!(
            touches(at(0.0, -4.9), place, DROPZONE),
            "the head in the zone"
        );
        assert!(!touches(at(0.0, -5.1), place, DROPZONE));
        // A power-up: 1.25 m + 0.4 m about its centre 1 m up.
        assert!(touches(at(1.6, 0.0), place, POWER_UP_TRIGGER));
        assert!(!touches(at(1.7, 0.0), place, POWER_UP_TRIGGER));
        assert!(!touches(at(0.0, 2.7), place, POWER_UP_TRIGGER), "above it");
        // A jump pad: 2.6 m + 0.4 m about its centre 0.2 m up.
        let pad = JumpPadKind::Long.trigger();
        assert!(touches(at(2.99, 0.0), place, pad));
        assert!(!touches(at(3.05, 0.0), place, pad));
        assert_eq!(
            JumpPadKind::Super.trigger(),
            TriggerShape::Sphere {
                radius_m: 2.599_975_6,
                up_m: 0.2
            }
        );
    }

    #[test]
    fn the_jump_pad_effects_differ_for_a_carrier() {
        assert_eq!(JumpPadKind::Long.effect(false), "CTF_JumpPad_Long");
        assert_eq!(JumpPadKind::High.effect(true), "CTF_Carrier_JumpPad_High");
        assert_eq!(PowerUpKind::Healing.effect(), "CTF_PowerUp_Health");
        assert_eq!(
            lowercase_crc32(JumpPadKind::Super.effect(true)),
            0xcb70_f8c5
        );
        assert_eq!(lowercase_crc32(PowerUpKind::Speed.effect()), 0x1128_fc2f);
    }

    /// The effects of the match as data rows (test rows with the retail ids).
    fn effect_rules() -> StatusEffectRules {
        let ids = [
            FLAG_EFFECTS[RED],
            FLAG_EFFECTS[BLUE],
            PowerUpKind::Speed.effect(),
            PowerUpKind::Damage.effect(),
            PowerUpKind::Healing.effect(),
            JumpPadKind::Long.effect(false),
            JumpPadKind::Long.effect(true),
            JumpPadKind::High.effect(false),
            JumpPadKind::High.effect(true),
        ];
        StatusEffectRules::build(StatusEffectSource {
            effects: ids
                .iter()
                .map(|id| EffectSourceRow {
                    status_id: (*id).to_owned(),
                    ..EffectSourceRow::default()
                })
                .collect(),
            ..StatusEffectSource::default()
        })
    }

    /// The capture the flag instance sim on `mode`'s first map with the match open and two
    /// players, one per team.
    fn ctf_sim_on(mode: &InstanceMode) -> SimWorld {
        ctf_sim_with(mode, PlayerContent::default(), None)
    }

    /// [`ctf_sim_on`] with `content` (its instances and status effects are the test's) and
    /// players of `backstory`.
    fn ctf_sim_with(
        mode: &InstanceMode,
        content: PlayerContent,
        backstory: Option<&str>,
    ) -> SimWorld {
        let mut settings = crate::world::tests::settings(MANY_PLAYERS, MANY_PLAYERS as u32);
        settings.instance_role = true;
        settings.level = LevelSpec::instance(
            &mode.map.folder,
            &mode.map.map_name,
            &mode.map.coatlicue_path,
            mode.map.frame,
        );
        let mut world = SimWorld::new(
            settings,
            crate::login::WorldIds::random(0).expect("ids"),
            std::sync::Arc::new(nw_sim_core::test_support::NoGround),
            slot("inst-1"),
            crate::combat::tests::test_combat(),
            std::sync::Arc::new(PlayerContent {
                instances: InstanceCatalog::build([mode.clone()]),
                status_effects: effect_rules(),
                ..content
            }),
        );
        let admits: Vec<_> = [(RED_PLAYER, 0), (BLUE_PLAYER, 1)]
            .into_iter()
            .map(|(number, team)| {
                let mut admit = admit_at(number, 940.0, false);
                admit.backstory = backstory.map(str::to_owned);
                admit.progression.arena.stay = Some(InstanceStay {
                    instance: Uuid::from_u128(INSTANCE),
                    game_mode: mode.id.clone(),
                    phase: PHASE,
                    return_level: "newworld_vitaeeterna".to_owned(),
                    return_position: StoredPoint::default(),
                    activated_ns: 1,
                    mutation_difficulty: None,
                    seat: Some(ArenaSeat { team, place: 0 }),
                });
                admit
            })
            .collect();
        let members = admits.iter().map(|admit| admit.character).collect();
        assert_eq!(
            world.open_instance(
                Uuid::from_u128(INSTANCE),
                &mode.id,
                PHASE,
                members,
                None,
                NOW
            ),
            InstanceOpened::Opened
        );
        for admit in &admits {
            world.admit(admit, NOW, START_MS).expect("admit");
        }
        world
    }

    /// The Long map's sim.
    fn ctf_sim() -> SimWorld {
        ctf_sim_on(&capture_the_flag())
    }

    /// Each arriving player joins its team's raid in the first group (`CTF01` 317.411 s,
    /// [`super::super::raids`]).
    #[test]
    fn each_arriving_player_joins_its_team_raid() {
        let mut world = ctf_sim();
        let raids = running(&world).raids;
        let joins: Vec<(u64, u8, u8)> = world
            .take_raid_inputs()
            .into_iter()
            .filter_map(|input| match input {
                nw_bus::SocialInput::RaidJoin { raid, group, .. } => {
                    Some((raid.raid, raid.kind, group))
                }
                _ => None,
            })
            .collect();
        let game_mode = nw_messages::social::raid_data::RAID_TYPE_GAME_MODE;
        assert_eq!(
            joins,
            vec![(raids[0], game_mode, 0), (raids[1], game_mode, 0)]
        );
        // A match is no run: no run raid besides the team's.
        let entity = world.player_by_number(RED_PLAYER).expect("player");
        world.join_run_raid(entity);
        assert!(world.take_raid_inputs().is_empty());
    }

    fn running(world: &SimWorld) -> &CaptureTheFlagMatch {
        world
            .arena_matches
            .capture_the_flag
            .as_ref()
            .expect("the match")
    }

    fn tick_at(world: &mut SimWorld, server_ms: u64) {
        world.tick(NOW, server_ms).expect("tick");
    }

    fn value(world: &SimWorld, key: u32) -> Option<u64> {
        running(world)
            .values()
            .iter()
            .find(|(held, _)| *held == key)
            .map(|(_, value)| *value)
    }

    fn timer(world: &SimWorld, key: u32) -> Option<u64> {
        running(world)
            .timers()
            .iter()
            .find(|(held, _)| *held == key)
            .map(|(_, value)| *value)
    }

    /// Puts player `number` at `at`.
    fn stand(world: &mut SimWorld, number: u64, at: Position) {
        let entity = world.player_by_number(number).expect("player");
        world.place_player(entity, at, None, Placement::Stand, NOW);
    }

    /// Player `number` uses the pickup of flag `team`.
    fn touch_flag(world: &mut SimWorld, number: u64, team: usize) -> bool {
        let entity = world.player_by_number(number).expect("player");
        let (_, target) = running(world).flags[team].pickup.expect("a pickup");
        let request = InteractionRequest {
            target,
            target_entity: mode_facet(&target),
            interaction: 77,
            kind: TRIGGER_KIND,
            gatherable: None,
        };
        world.capture_the_flag_interaction(entity, number, request, &[])
    }

    fn holds(world: &mut SimWorld, number: u64, id: &str) -> bool {
        let crc = lowercase_crc32(id);
        let entity = world.player_by_number(number).expect("player");
        world
            .world
            .get::<Player>(entity)
            .is_some_and(|player| player.status_effects.holds(crc))
    }

    /// A place far from every entity of both maps.
    const AWAY: Position = Position {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// The sim after the pregame.
    fn started() -> SimWorld {
        let mut world = ctf_sim();
        tick_at(&mut world, START_MS + PREGAME_MS);
        world
    }

    #[test]
    fn the_match_opens_in_the_pregame_with_the_level_entities_and_the_barriers_go_at_the_start() {
        let mut world = ctf_sim();
        let match_state = running(&world);
        assert_eq!(
            (match_state.state, match_state.participants.len()),
            (PREGAME, 2)
        );
        assert_eq!(match_state.barriers.len(), 6);
        let layout = match_state.layout;
        assert_eq!(match_state.power_ups.len(), layout.power_ups.len());
        assert_eq!(
            match_state.props.len(),
            layout.platforms.len() + layout.jump_pads.len() + TEAMS
        );
        let barrier = match_state.barriers[0];
        let members = world.stations.members(barrier).expect("a barrier");
        assert!(members.contains(&closed_door()), "the closed door");
        assert_eq!(value(&world, RED_STATUS), Some(STATUS_SAFE));
        assert_eq!(value(&world, BLUE_INTERACTABLE), Some(1));
        assert_eq!(value(&world, FLAG_RESET_SECONDS), Some(20));
        assert_eq!(running(&world).values().len(), 9, "the nine retail values");
        let opened_ms = running(&world).timeout_ms - GAME_MODE_TIMEOUT_MS;
        assert_eq!(timer(&world, PREGAME_TIMER), Some(opened_ms + PREGAME_MS));
        let (_, red_ref) = running(&world).flags[RED].pickup.expect("red flag");
        assert_eq!(value(&world, RED_ENTITY), Some(mode_facet(&red_ref)));
        assert!(touch_flag(&mut world, BLUE_PLAYER, RED));
        assert_eq!(
            running(&world).flags[RED].place,
            FlagPlace::Base,
            "no take in the pregame"
        );
        tick_at(&mut world, START_MS + PREGAME_MS);
        assert_eq!(running(&world).state, RUNNING);
        assert!(
            world.stations.members(barrier).is_none(),
            "the barriers went"
        );
        assert_eq!(
            timer(&world, TIME_LIMIT_TIMER),
            Some(START_MS + PREGAME_MS + TIME_LIMIT_MS)
        );
    }

    #[test]
    fn the_wide_map_opens_with_its_own_entities() {
        let mut mode = capture_the_flag();
        mode.map = mode.rotation[1].clone();
        let world = ctf_sim_on(&mode);
        let match_state = running(&world);
        assert_eq!(match_state.layout.folder, "nw_ctf_002_wide");
        assert_eq!(match_state.barriers.len(), 6);
        assert_eq!(match_state.power_ups.len(), 10);
    }

    #[test]
    fn a_take_carries_the_flag_and_the_carriers_death_drops_it_until_its_reset() {
        let mut world = started();
        let now = START_MS + PREGAME_MS;
        assert!(touch_flag(&mut world, RED_PLAYER, RED));
        assert_eq!(
            running(&world).flags[RED].place,
            FlagPlace::Base,
            "the own flag stays"
        );
        assert!(touch_flag(&mut world, BLUE_PLAYER, RED));
        let flag = &running(&world).flags[RED];
        assert!(matches!(flag.place, FlagPlace::Carried { team: 1, .. }));
        assert!(flag.carried.is_some() && flag.ghost.is_some() && flag.pickup.is_none());
        assert_eq!(value(&world, RED_STATUS), Some(1));
        assert_eq!(value(&world, RED_ENTITY), Some(facet_entity(BLUE_PLAYER)));
        assert!(holds(&mut world, BLUE_PLAYER, FLAG_EFFECTS[RED]));
        world.capture_the_flag_death(BLUE_PLAYER, RED_PLAYER);
        assert_eq!(value(&world, RED_STATUS), Some(STATUS_DROPPED));
        assert_eq!(value(&world, RED_INTERACTABLE), Some(0));
        assert_eq!(value(&world, TEAM_0_FLAGBEARERS_KILLED), Some(1));
        assert_eq!(value(&world, TEAM_0_KILLS), Some(1));
        assert_eq!(value(&world, TEAM_1_DEATHS), Some(1));
        assert_eq!(value(&world, RED_CANNOT_SCORE), Some(0));
        assert_eq!(timer(&world, RED_RESET_TIMER), Some(now + FLAG_RESET_MS));
        assert_eq!(
            timer(&world, RED_INTERACT_TIMER),
            Some(now + DROP_BUFFER_MS)
        );
        assert!(!holds(&mut world, BLUE_PLAYER, FLAG_EFFECTS[RED]));
        tick_at(&mut world, now + DROP_BUFFER_MS);
        assert_eq!(value(&world, RED_INTERACTABLE), Some(1));
        tick_at(&mut world, now + FLAG_RESET_MS - RESET_WARNING_MS);
        assert_eq!(value(&world, RED_RESET_WARNING), Some(1));
        tick_at(&mut world, now + FLAG_RESET_MS);
        assert_eq!(value(&world, RED_STATUS), Some(STATUS_SAFE));
        assert_eq!(
            value(&world, TEAM_0_FLAGS_RETURNED),
            None,
            "a reset counts no return"
        );
        assert!(running(&world).flags[RED].ghost.is_none());
    }

    #[test]
    fn the_owner_returns_its_dropped_flag() {
        let mut world = started();
        let now = START_MS + PREGAME_MS;
        touch_flag(&mut world, BLUE_PLAYER, RED);
        world.capture_the_flag_death(BLUE_PLAYER, RED_PLAYER);
        assert!(touch_flag(&mut world, RED_PLAYER, RED));
        assert_eq!(
            value(&world, RED_STATUS),
            Some(STATUS_DROPPED),
            "the drop buffer holds"
        );
        tick_at(&mut world, now + DROP_BUFFER_MS);
        touch_flag(&mut world, RED_PLAYER, RED);
        assert_eq!(value(&world, RED_STATUS), Some(STATUS_SAFE));
        assert_eq!(value(&world, TEAM_0_FLAGS_RETURNED), Some(1));
        assert_eq!(
            timer(&world, RED_INTERACT_TIMER),
            Some(now + DROP_BUFFER_MS + RETURN_BUFFER_MS)
        );
    }

    #[test]
    fn a_carrier_scores_at_its_dropzone_only_while_its_own_flag_is_home() {
        let mut world = started();
        let now = START_MS + PREGAME_MS;
        touch_flag(&mut world, RED_PLAYER, BLUE);
        touch_flag(&mut world, BLUE_PLAYER, RED);
        let red_base = position_of(running(&world).layout.flag_bases[RED]);
        stand(&mut world, RED_PLAYER, red_base);
        tick_at(&mut world, now + 100);
        assert_eq!(
            value(&world, BLUE_CANNOT_SCORE),
            Some(1),
            "the red flag is away"
        );
        assert_eq!(running(&world).scores, [0, 0]);
        stand(&mut world, RED_PLAYER, AWAY);
        tick_at(&mut world, now + 200);
        assert_eq!(value(&world, BLUE_CANNOT_SCORE), Some(0));
        world.capture_the_flag_death(BLUE_PLAYER, RED_PLAYER);
        let blue = world.player_by_number(BLUE_PLAYER).expect("player");
        world.capture_the_flag_respawn(blue);
        let returned_ms = now + 200 + DROP_BUFFER_MS;
        tick_at(&mut world, returned_ms);
        touch_flag(&mut world, RED_PLAYER, RED);
        assert_eq!(value(&world, RED_STATUS), Some(STATUS_SAFE));
        stand(&mut world, RED_PLAYER, red_base);
        let scored_ms = returned_ms + 100;
        tick_at(&mut world, scored_ms);
        assert_eq!(running(&world).scores, [1, 0]);
        assert_eq!(value(&world, TEAM_0_SCORE), Some(1));
        assert_eq!(value(&world, BLUE_STATUS), Some(STATUS_SAFE));
        assert_eq!(value(&world, BLUE_INTERACTABLE), Some(0));
        assert_eq!(
            timer(&world, BLUE_INTERACT_TIMER),
            Some(scored_ms + SCORE_BUFFER_MS)
        );
        assert_eq!(running(&world).celebrations.len(), 1);
        tick_at(&mut world, scored_ms + CELEBRATION_MS);
        assert!(
            running(&world).celebrations.is_empty(),
            "the celebration went"
        );
    }

    /// Team `team` scores one capture at `at_ms`: its player takes the other team's flag and
    /// stands at its own base.
    fn score(world: &mut SimWorld, team: usize, at_ms: u64) {
        let (player, other) = if team == RED {
            (RED_PLAYER, BLUE)
        } else {
            (BLUE_PLAYER, RED)
        };
        tick_at(world, at_ms);
        assert!(touch_flag(world, player, other));
        let base = position_of(running(world).layout.flag_bases[team]);
        stand(world, player, base);
        tick_at(world, at_ms + 100);
        stand(world, player, AWAY);
    }

    /// The time between two captures of a test: the flag's interact buffer after a capture and
    /// one second.
    const CAPTURE_GAP_MS: u64 = SCORE_BUFFER_MS + MS_PER_S;

    /// Each retail ledger's captures in order (the scoring teams) before `TimeLimit`, whether
    /// `TimeLimit` came, and the captures of the overtime, replayed: the state, the winner and the end values match the
    /// ledger's (`CTF01` with team 1's capture at 744.33 s that its 2343 lost).
    #[test]
    fn each_ledgers_score_sequence_ends_as_in_the_ledger() {
        struct Ledger {
            name: &'static str,
            captures: &'static [usize],
            time_limit: bool,
            overtime_captures: &'static [usize],
            state: u8,
            winner: Option<u8>,
        }
        let ledgers = [
            Ledger {
                name: "CTF01",
                time_limit: true,
                captures: &[BLUE, RED],
                overtime_captures: &[RED],
                state: ENDED,
                winner: Some(0),
            },
            Ledger {
                name: "CTF02",
                time_limit: true,
                captures: &[BLUE, RED],
                overtime_captures: &[BLUE],
                state: ENDED,
                winner: Some(1),
            },
            Ledger {
                name: "CTF03",
                time_limit: false,
                captures: &[RED, RED, RED],
                overtime_captures: &[],
                state: ENDED,
                winner: Some(0),
            },
            Ledger {
                name: "CTF04",
                time_limit: true,
                captures: &[RED, BLUE, RED],
                overtime_captures: &[],
                state: ENDED,
                winner: Some(0),
            },
            Ledger {
                name: "CTF_QJ",
                time_limit: false,
                captures: &[RED],
                overtime_captures: &[],
                state: RUNNING,
                winner: None,
            },
        ];
        let start = START_MS + PREGAME_MS;
        let limit = start + TIME_LIMIT_MS;
        for ledger in &ledgers {
            let mut world = started();
            let mut at_ms = start;
            for team in ledger.captures {
                score(&mut world, *team, at_ms);
                at_ms += CAPTURE_GAP_MS;
            }
            if ledger.time_limit {
                tick_at(&mut world, limit);
                at_ms = limit + 1;
            }
            if !ledger.overtime_captures.is_empty() {
                assert_eq!(running(&world).state, OVERTIME, "{} tied", ledger.name);
            }
            for team in ledger.overtime_captures {
                score(&mut world, *team, at_ms);
                at_ms += CAPTURE_GAP_MS;
            }
            let match_state = running(&world);
            assert_eq!(
                (match_state.state, match_state.winner),
                (ledger.state, ledger.winner),
                "{}",
                ledger.name
            );
            let mut captures = [0_u64; TEAMS];
            for team in ledger.captures.iter().chain(ledger.overtime_captures) {
                captures[*team] += 1;
            }
            if ledger.state == ENDED {
                assert_eq!(value(&world, TEAM_0_REGULAR_CAPTURES), Some(captures[RED]));
                assert_eq!(value(&world, TEAM_1_REGULAR_CAPTURES), Some(captures[BLUE]));
                let winner = ledger.winner.map(u64::from);
                assert_eq!(value(&world, WINNING_TEAM), winner, "{}", ledger.name);
            }
            assert_eq!(value(&world, TEAM_0_SCORE), Some(captures[RED]));
            assert_eq!(value(&world, TEAM_1_SCORE), Some(captures[BLUE]));
        }
    }

    #[test]
    fn three_captures_end_the_match_with_the_end_values_and_the_cleanup_sends_the_players_back() {
        let mut world = started();
        let now = START_MS + PREGAME_MS;
        for capture in 0..u64::from(POINTS_TO_WIN) {
            score(&mut world, RED, now + capture * CAPTURE_GAP_MS);
        }
        let match_state = running(&world);
        assert_eq!((match_state.state, match_state.winner), (ENDED, Some(0)));
        assert_eq!(value(&world, WINNING_TEAM), Some(0));
        assert_eq!(value(&world, TEAM_0_REGULAR_CAPTURES), Some(3));
        assert_eq!(value(&world, TEAM_1_WILD_CAPTURES), Some(0));
        assert_eq!(value(&world, RED_ENTITY), Some(NO_FLAG_ENTITY));
        assert_eq!(running(&world).timers().len(), 1, "only Cleanup");
        let end = now + 2 * CAPTURE_GAP_MS + 100;
        tick_at(&mut world, end + CLEANUP_MS);
        assert!(
            world.arena_matches.capture_the_flag.is_none(),
            "the players go back"
        );
    }

    #[test]
    fn a_tie_runs_the_overtime_whose_capture_ends_it_and_whose_end_is_a_tie() {
        let mut world = started();
        let limit = START_MS + PREGAME_MS + TIME_LIMIT_MS;
        tick_at(&mut world, limit);
        assert_eq!(running(&world).state, OVERTIME);
        assert_eq!(timer(&world, OVERTIME_TIMER), Some(limit + OVERTIME_MS));
        score(&mut world, RED, limit + 1);
        assert_eq!(
            (running(&world).state, running(&world).winner),
            (ENDED, Some(0))
        );
        let mut tied = started();
        tick_at(&mut tied, limit);
        tick_at(&mut tied, limit + OVERTIME_MS);
        assert_eq!(value(&tied, WINNING_TEAM), Some(u64::from(TIE)));
    }

    #[test]
    fn the_leader_wins_at_the_time_limit() {
        let mut world = started();
        score(&mut world, RED, START_MS + PREGAME_MS);
        tick_at(&mut world, START_MS + PREGAME_MS + TIME_LIMIT_MS);
        assert_eq!(
            (running(&world).state, running(&world).winner),
            (ENDED, Some(0))
        );
    }

    #[test]
    fn a_power_up_gives_its_effect_and_comes_back_after_thirty_seconds() {
        let mut world = started();
        let now = START_MS + PREGAME_MS;
        let layout = running(&world).layout;
        let (kind, at) = layout.power_ups[0];
        let first = running(&world).power_ups[0].entity.expect("a power-up");
        stand(&mut world, RED_PLAYER, at.position());
        tick_at(&mut world, now + 100);
        assert!(holds(&mut world, RED_PLAYER, kind.effect()));
        assert_eq!(running(&world).power_ups[0].entity, None);
        assert!(world.stations.members(first).is_none());
        stand(&mut world, RED_PLAYER, AWAY);
        tick_at(&mut world, now + 100 + POWER_UP_RESPAWN_MS);
        let again = running(&world).power_ups[0].entity.expect("back");
        assert_ne!(again, first, "a new entity");
    }

    #[test]
    fn a_jump_pad_gives_the_carriers_effect_to_a_carrier() {
        let mut world = started();
        let now = START_MS + PREGAME_MS;
        let layout = running(&world).layout;
        let (kind, pad) = layout.jump_pads[0];
        stand(&mut world, BLUE_PLAYER, pad.position());
        tick_at(&mut world, now + 100);
        assert!(holds(&mut world, BLUE_PLAYER, kind.effect(false)));
        stand(&mut world, BLUE_PLAYER, AWAY);
        tick_at(&mut world, now + 200);
        touch_flag(&mut world, BLUE_PLAYER, RED);
        stand(&mut world, BLUE_PLAYER, pad.position());
        tick_at(&mut world, now + 300);
        assert!(holds(&mut world, BLUE_PLAYER, kind.effect(true)));
    }

    #[test]
    fn a_leave_bars_the_queue_returns_the_carried_flag_and_the_other_team_wins() {
        let mut world = started();
        let now = START_MS + PREGAME_MS;
        let red = world.player_by_number(RED_PLAYER).expect("player");
        let blue = world.player_by_number(BLUE_PLAYER).expect("player");
        assert_eq!(world.capture_the_flag_foes(red, blue), Some(true));
        touch_flag(&mut world, BLUE_PLAYER, RED);
        world.leave_capture_the_flag(blue);
        assert_eq!(value(&world, RED_STATUS), Some(STATUS_SAFE));
        let barred = world
            .world
            .get::<Player>(blue)
            .expect("player")
            .arena
            .queue_eligible()
            .to_vec();
        assert_eq!(
            barred.first().map(|bar| bar.0),
            Some(lowercase_crc32(CAPTURE_THE_FLAG_GAME_MODE))
        );
        tick_at(&mut world, now + 1);
        assert_eq!(
            (running(&world).state, running(&world).winner),
            (ENDED, Some(0))
        );
        assert_eq!(world.capture_the_flag_foes(red, blue), Some(false));
    }

    #[test]
    fn the_respawn_is_at_the_team_spot_and_a_ready_up_sets_the_bit() {
        let mut world = ctf_sim();
        world.capture_the_flag_ready(RED_PLAYER, true);
        assert_eq!(
            running(&world).participants[0].status,
            PLAYING_STATUS | READY_BIT
        );
        tick_at(&mut world, START_MS + PREGAME_MS);
        world.capture_the_flag_death(BLUE_PLAYER, RED_PLAYER);
        assert_eq!(running(&world).participants[1].status & DEAD_BIT, DEAD_BIT);
        let blue = world.player_by_number(BLUE_PLAYER).expect("player");
        let spot = world.capture_the_flag_respawn(blue).expect("a spot");
        assert_eq!((spot.x, spot.y), (1005.846, 955.411), "team 1's teleport");
        assert_eq!(running(&world).participants[1].status & DEAD_BIT, 0);
    }

    /// A carried flag names its carrier's company once the world database answered, with the
    /// carrier's PvP flag, and turns with the carrier's look (`CTF04` 205.526 s).
    #[test]
    fn a_carried_flag_names_its_carriers_company_and_turns_with_its_look() {
        use crate::world::company_looks::tests::{join_company, listing};
        /// A made-up company.
        const COMPANY: Uuid = Uuid::from_u128(0xc0_ffee);
        const COMPANY_NAME: &str = "Test Company";
        const QUARTER_TURN: f32 = std::f32::consts::FRAC_PI_2;
        let mut world = started();
        let blue = world.player_by_number(BLUE_PLAYER).expect("player");
        join_company(&mut world, blue, COMPANY);
        tick_at(&mut world, START_MS + PREGAME_MS + 1);
        assert_eq!(world.take_company_reads(), vec![COMPANY]);
        tick_at(&mut world, START_MS + PREGAME_MS + 2);
        assert!(world.take_company_reads().is_empty(), "asked once");
        world.companies_read(&[listing(COMPANY, COMPANY_NAME)]);
        {
            let mut player = world.world.get_mut::<Player>(blue).expect("player");
            player.pvp_flagged = true;
            player.view = Some(nw_messages::LookDir {
                yaw: QUARTER_TURN,
                pitch: 0.0,
            });
        }
        assert!(touch_flag(&mut world, BLUE_PLAYER, RED));
        let carried = running(&world).flags[RED].carried.expect("carried");
        let members = world.stations.members(carried).expect("the carried flag");
        let ownership = members
            .iter()
            .find_map(|member| match &member.state {
                MemberState::Ownership(ownership) => Some(ownership.clone()),
                _ => None,
            })
            .expect("an ownership");
        assert_eq!(ownership.guild_id, Some(*COMPANY.as_bytes()));
        assert_eq!(
            ownership.guild_owner,
            Some(COMPANY_NAME.as_bytes().to_vec())
        );
        assert_eq!(ownership.owner_pvp, Some(true));
        assert_eq!(ownership.permission_rule, Some(TEAM_RULE));
        let half = QUARTER_TURN / 2.0;
        let turn = members.iter().find_map(|member| match &member.state {
            MemberState::Spell(spell) => spell.start_rotation,
            _ => None,
        });
        assert_eq!(
            turn,
            Some(Rotation::of_quaternion([0.0, 0.0, half.sin(), half.cos()]))
        );
        // The red carrier has no company: its flag names none.
        assert!(touch_flag(&mut world, RED_PLAYER, BLUE));
        let carried = running(&world).flags[BLUE].carried.expect("carried");
        let members = world.stations.members(carried).expect("the carried flag");
        assert!(members.iter().any(|member| matches!(
            &member.state,
            MemberState::Ownership(ownership) if ownership.guild_owner == Some(Vec::new())
        )));
    }

    /// A made-up reward item (a resource of the test kits).
    const REWARD_ITEM: &str = "CtfTestRewardToken";
    /// The faction of both test players and its token currency (content bundle 25810808).
    const FACTION: &str = "Covenant";
    const FACTION_TOKENS: &str = "CovenantTokens";
    /// The caps of the umbral shards and the azoth (retail remote config and `CTF01`).
    const UMBRAL_CAP: i64 = 100_000;
    const AZOTH_CAP: i64 = 2_500;
    /// The umbral shards before the end and the remote multiplier (Observed, `CTF01`).
    const UMBRAL_BEFORE: u32 = 99_853;
    const UMBRAL_MULTIPLIER: f64 = 3.0;
    /// A cap no test gain reaches.
    const HIGH_CAP: i64 = 10_000_000;

    /// The win and the loss rewards of the retail level 60 row (see
    /// [`nw_content::game_mode_rewards`]), each with a made-up item in the quantity of its loot
    /// table's `DarkMatter` (`CaptureTheFlagRewardT4_Qty` 20, `_T2_Qty` 10).
    fn reward_content() -> PlayerContent {
        use nw_content::game_mode_rewards::{ModeReward, ModeRewards};
        use nw_content::progression::{Progression, ProgressionSource};
        use nw_content::rewards::EventGrants;
        let reward = |pvp_xp, faction_tokens, azoth, umbral, items| ModeReward {
            coins: 25_000,
            experience: 0,
            pvp_xp,
            faction_reputation: 0.0,
            faction_tokens,
            azoth,
            umbral,
            seasons_xp: 0.0,
            item: Some((REWARD_ITEM.to_owned(), items)),
        };
        let currency = |id: &str, cap| ProgressionSource {
            id: id.to_owned(),
            max_level: 0,
            auto_rank_up: false,
            ranks: vec![(0, cap)],
        };
        let mut content = PlayerContent {
            kits: crate::world::gear::tests::kits_with(&[(REWARD_ITEM, "Resource")]),
            progression: Progression::default().with_progressions([
                currency(PVP_XP, HIGH_CAP),
                currency(UMBRAL_CURRENCY, UMBRAL_CAP),
                currency(AZOTH_CURRENCY, AZOTH_CAP),
                currency(FACTION_TOKENS, HIGH_CAP),
            ]),
            event_grants: EventGrants::default()
                .with_faction_currencies([(FACTION.to_owned(), FACTION_TOKENS.to_owned())]),
            mode_rewards: ModeRewards::build([
                (
                    WIN_EVENT.to_owned(),
                    reward(1_350.0, 1_350.0, 125.0, 150.0, 20),
                ),
                (LOSS_EVENT.to_owned(), reward(800.0, 800.0, 75.0, 40.0, 10)),
            ]),
            ..PlayerContent::default()
        };
        content.gameplay.xp.progression = [(UMBRAL_CURRENCY.to_owned(), UMBRAL_MULTIPLIER)].into();
        content
    }

    /// The points of categorical progression `id` of player `number`.
    fn points(world: &mut SimWorld, number: u64, id: &str) -> u32 {
        let entity = world.player_by_number(number).expect("player");
        let mastery = world
            .content
            .progression
            .categorical(id)
            .expect("progression");
        world
            .world
            .get::<Player>(entity)
            .map_or(0, |player| player.masteries.experience(mastery))
    }

    #[test]
    fn each_team_gets_its_end_event_and_a_tie_the_third() {
        assert_eq!(end_event(0, Some(0)), WIN_EVENT);
        assert_eq!(end_event(1, Some(0)), LOSS_EVENT);
        assert_eq!(end_event(0, None), TIE_EVENT);
        assert_eq!(end_event(1, None), TIE_EVENT);
    }

    /// The end gives each player its team's event (`CTF01` 1222.234 s: coins +25,000, `PvP_XP`
    /// +1,350, tokens +1,423 under a held share, `UmbralCurrency` 99,853 to its cap 100,000,
    /// the loot table's items) and empties its affiliations (`CTF04` 754.504 s).
    #[test]
    fn the_end_gives_the_win_and_the_loss_rewards_and_clears_the_affiliations() {
        let mut world = ctf_sim_with(
            &capture_the_flag(),
            reward_content(),
            Some(crate::world::gear::tests::BACKSTORY),
        );
        let faction = nw_content::factions::faction_named(FACTION).expect("faction");
        let umbral = world
            .content
            .progression
            .categorical(UMBRAL_CURRENCY)
            .cloned()
            .expect("umbral");
        for number in [RED_PLAYER, BLUE_PLAYER] {
            let entity = world.player_by_number(number).expect("player");
            let mut player = world.world.get_mut::<Player>(entity).expect("player");
            player.faction.faction = faction;
            player.masteries.set(&umbral, UMBRAL_BEFORE);
        }
        let coins = |world: &mut SimWorld, number| {
            let entity = world.player_by_number(number).expect("player");
            world
                .world
                .get::<Player>(entity)
                .map_or(0, |player| player.coins)
        };
        let coins_before = coins(&mut world, RED_PLAYER);
        tick_at(&mut world, START_MS + PREGAME_MS);
        world.end_capture_the_flag(Some(0));
        assert_eq!(coins(&mut world, RED_PLAYER) - coins_before, 25_000);
        assert_eq!(
            coins(&mut world, BLUE_PLAYER) - coins_before,
            25_000,
            "the loss the same"
        );
        assert_eq!(points(&mut world, RED_PLAYER, PVP_XP), 1_350);
        assert_eq!(points(&mut world, BLUE_PLAYER, PVP_XP), 800);
        assert_eq!(points(&mut world, RED_PLAYER, FACTION_TOKENS), 1_350);
        assert_eq!(points(&mut world, BLUE_PLAYER, FACTION_TOKENS), 800);
        assert_eq!(points(&mut world, RED_PLAYER, AZOTH_CURRENCY), 125);
        assert_eq!(points(&mut world, BLUE_PLAYER, AZOTH_CURRENCY), 75);
        assert_eq!(
            points(&mut world, RED_PLAYER, UMBRAL_CURRENCY),
            100_000,
            "450 to its cap"
        );
        for (number, items) in [(RED_PLAYER, 20), (BLUE_PLAYER, 10)] {
            let entity = world.player_by_number(number).expect("player");
            let player = world.world.get::<Player>(entity).expect("player");
            let gear = player.gear.as_ref().expect("gear");
            assert_eq!(gear.bag_count(REWARD_ITEM), items);
            // Its 497 entry: the event, the coins and the item given (`CTF01` 1222.234 s).
            let mut log = player.game_events.clone();
            let member = log.take_member(1).expect("the end event's entry");
            let MemberState::GameEvents(state) = member.state else {
                panic!("a game event member");
            };
            let Some(ReplicatedList::Full(list)) = state.game_events else {
                panic!("the first entry goes in full");
            };
            let entry = &list.elements[0];
            let event = if number == RED_PLAYER { WIN_EVENT } else { LOSS_EVENT };
            assert_eq!(entry.event, lowercase_crc32(event));
            assert_eq!((entry.value_a, entry.flag), (25_000, true));
            assert_eq!(entry.rewards[0].quantity, items);
            assert!(
                player.extra_members.iter().any(|member| matches!(
                    &member.state,
                    MemberState::TemporaryAffiliation(state)
                        if matches!(&state.affiliations, Some(ReplicatedList::Full(list))
                            if list.elements.is_empty())
                )),
                "the affiliations are emptied at the end"
            );
        }
    }
}
