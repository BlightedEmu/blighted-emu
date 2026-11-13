// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bus between the sims and the world's social service (run 15 social study): chat, players,
//! groups, companies and voice-chat setup are one state per world, so a player's friends, group
//! and company reach it on any sim.
//!
//! - Each sim sends its players' social calls and their presence ([`SocialBatch`] on
//!   [`crate::subject::social_input`]) once per tick.
//! - The service sends each sim the messages for its players ([`SocialDeliveries`] on
//!   [`crate::subject::sim_social`]); the sim puts the facet header in front and sends them to
//!   their clients.
//! - The service also sends each sim the replicated social states of its players
//!   ([`OwnStateDelivery`]: groups, friends and blocks, company, chat mutes) and the state of
//!   each player's group entity ([`GroupEntityDelivery`]); the sim puts them into the player's
//!   next bundle.
//! - The treasury moves a player's coins through the sim that has the player ([`CoinTransfer`]
//!   on [`crate::subject::sim_coins`], request/reply).
//! - A sim asks the service to pay a territory's upkeep from the payer's company treasury
//!   ([`SocialInput::PayUpkeep`]); the service tells the payer's sim whether it paid
//!   ([`SocialDeliveries::upkeep_answers`]).
//! - The sim that runs a war, an arena match or an Outpost Rush match puts its players into the
//!   raid of their side or team ([`SocialInput::RaidJoin`], [`SocialInput::RaidLeave`]); the
//!   service owns the raid as it owns the groups and sends each member its raid entity and the
//!   raid's group entities ([`RaidEntityDelivery`]).
//! - Before a war's move, the sim of a roster player asks the service to invite it into its
//!   side's raid ([`SocialInput::RaidInvite`]); the service tells that sim the player's answer
//!   ([`SocialDeliveries::raid_invite_answers`]), and the sim moves it in when it accepted.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::id::BusId;

/// Bytes of a player icon (`nw_messages::appearance::PlayerIconData`).
pub const ICON_LEN: usize = 9;

/// The player component a delivery goes to (`nw_messages::social::SocialFacet`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SocialFacetKind {
    /// `ChatComponent`.
    Chat,
    /// `SocialComponent`.
    Social,
    /// `GroupsComponent`.
    Groups,
    /// `GuildsComponent`.
    Guilds,
    /// `VoiceChatComponent`.
    VoiceChat,
}

/// What the service knows of an online player from its sim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocialPlayer {
    /// The character.
    pub character: Uuid,
    /// The character's name.
    pub name: String,
    /// Its zero-based level (`PlayerDataView` level).
    pub level: u32,
    /// Its icon (`PlayerIconData`, 9 bytes).
    pub icon: [u8; ICON_LEN],
    /// Its pronoun type (`PlayerDataView` pronoun).
    #[serde(default)]
    pub pronoun: u8,
    /// The sim that has the player.
    pub sim: BusId,
    /// The level the player is in (the sim's level: area chat stays in it).
    pub level_name: String,
    /// The territories the player is in, innermost last.
    pub territories: Vec<u16>,
    /// The client's `FactionType` (0 none, 1 to 3).
    pub faction: u8,
    /// Where the player is, in metres (east, north): its positional voice channel.
    pub position: [f32; 2],
    /// The player is in an instance (a dungeon, a trial, the tutorial), not the open world: its
    /// positional voice channels are the instance's own.
    pub instanced: bool,
}

/// Why a player left a sim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeaveReason {
    /// The client is gone.
    Logout,
    /// Another sim took the player; its `Joined` follows or came first.
    Handoff,
}

/// One input of a sim to the service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SocialInput {
    /// A player is now on the sending sim (a login, a handoff or a resume).
    Joined(SocialPlayer),
    /// A player of the sending sim changed level, territory, faction or place.
    Updated(SocialPlayer),
    /// A player left the sending sim.
    Left {
        /// The character.
        character: Uuid,
        /// Why.
        reason: LeaveReason,
    },
    /// A player's health, mana or life changed (its group shows them).
    Vitals(PlayerVitals),
    /// A social call of a player's client: its type index and whole body (header first).
    Call {
        /// The character.
        character: Uuid,
        /// The type index.
        type_index: u64,
        /// The body.
        body: Vec<u8>,
    },
    /// A player asks to pay the upkeep of a territory claim from its company's treasury; the
    /// sim checked the claim and the cost.
    PayUpkeep(UpkeepPayment),
    /// The sim that runs a raid's war or match puts a player into group `group` (from 0) of raid
    /// `raid`; the service makes the raid at its first member.
    RaidJoin {
        /// The character.
        character: Uuid,
        /// The raid.
        raid: RaidSpec,
        /// The raid group, from 0.
        group: u8,
    },
    /// A player left the war or match of raid `raid` (its instance's exit).
    RaidLeave {
        /// The character.
        character: Uuid,
        /// The raid entity's id ([`RaidSpec::entity`]): raid ids are numbers of the sims and
        /// can repeat across them, the entity id names one raid in the world.
        raid: [u8; 16],
    },
    /// The sim of a war's roster player asks the service to invite it into group `group` (from
    /// 0) of raid `raid` for activity `activity` (the war id): an invite without an inviter.
    RaidInvite {
        /// The character.
        character: Uuid,
        /// The raid.
        raid: RaidSpec,
        /// The raid group, from 0.
        group: u8,
        /// The activity the invite is for (the war id).
        activity: Uuid,
    },
}

/// A raid as the sim that runs its war or match names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaidSpec {
    /// The raid id (`raidId`).
    pub raid: u64,
    /// The raid type (`raidType`, 1 war to 4 players' raid).
    pub kind: u8,
    /// The raid of the other side (`opposingRaidId`); 0 for none.
    pub opposing: u64,
    /// The raid entity's id (its card's `gde_ref`).
    pub entity: [u8; 16],
    /// The raid's groups in index order.
    pub groups: Vec<Uuid>,
}

/// An upkeep payment a player asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpkeepPayment {
    /// The payer.
    pub character: Uuid,
    /// The territory id of the claim.
    pub territory: u16,
    /// The cost, hundredths of a coin.
    pub cost: u64,
    /// When the player asked, Unix milliseconds.
    pub at_ms: u64,
}

/// The service's answer to an upkeep payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpkeepAnswer {
    /// The payer.
    pub character: Uuid,
    /// The territory id of the claim.
    pub territory: u16,
    /// Whether the service took the payment from the treasury; `false` for a refused one.
    pub paid: bool,
}

/// A player's answer to the raid invite of activity `activity` ([`SocialInput::RaidInvite`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaidInviteAnswer {
    /// The player.
    pub character: Uuid,
    /// The activity of the invite (the war id).
    pub activity: Uuid,
    /// Whether it accepted.
    pub accepted: bool,
}

/// What a player's group shows of its health.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerVitals {
    /// The character.
    pub character: Uuid,
    /// Health in percent of its maximum.
    pub health_percent: u8,
    /// Mana in percent of its maximum.
    pub mana_percent: u8,
    /// The player is dead.
    pub dead: bool,
}

/// A sim's inputs of one tick, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocialBatch {
    /// The sim.
    pub sim: BusId,
    /// The inputs.
    pub inputs: Vec<SocialInput>,
}

/// The header a delivery takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SocialHeader {
    /// The header of the call it answers (target, entity).
    Echo {
        /// The target id.
        target: u64,
        /// The entity.
        entity: u64,
    },
    /// The receiving player's header of this component.
    Facet(SocialFacetKind),
}

/// One server message for some players of a sim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialDelivery {
    /// The receiving characters.
    pub recipients: Vec<Uuid>,
    /// The header.
    pub header: SocialHeader,
    /// The type index.
    pub type_index: u64,
    /// The body after the header.
    pub args: Vec<u8>,
}

/// A replicated social state of a player (`nw_messages::social::states`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SocialStateKind {
    /// `GroupsComponentReplicatedState` (type 1994).
    Groups,
    /// `SocialReplicatedState` (type 4176).
    Social,
    /// `GuildsComponentReplicatedState` (type 3147).
    Guilds,
    /// `ChatReplicatedState` (type 1566).
    Chat,
}

/// A change of one replicated social state of one player: the state's encoded body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnStateDelivery {
    /// The player.
    pub character: Uuid,
    /// The state.
    pub kind: SocialStateKind,
    /// The encoded state body.
    pub body: Vec<u8>,
}

/// A player's group entity as it is now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupEntityView {
    /// The group.
    pub group: Uuid,
    /// The encoded group data body (`GroupDataComponentReplicatedState`, whole).
    pub body: Vec<u8>,
}

/// A player's group entity: its state now, or none when the player left its group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupEntityDelivery {
    /// The player.
    pub character: Uuid,
    /// The group entity; `None` removes it.
    pub view: Option<GroupEntityView>,
}

/// A raid member's raid entity and the raid's group entities but its own (its own group comes
/// as its [`GroupEntityView`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaidEntityView {
    /// The raid entity's id.
    pub entity: [u8; 16],
    /// The encoded raid data body (`RaidDataComponentReplicatedState`, whole).
    pub body: Vec<u8>,
    /// The raid's other groups.
    pub groups: Vec<GroupEntityView>,
}

/// A player's raid entity: its state now, or none when the player left its raid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaidEntityDelivery {
    /// The player.
    pub character: Uuid,
    /// The raid entity; `None` removes it.
    pub view: Option<RaidEntityView>,
}

/// The service's messages and states for the players of one sim, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialDeliveries {
    /// The deliveries.
    pub deliveries: Vec<SocialDelivery>,
    /// The replicated social state changes.
    pub own_states: Vec<OwnStateDelivery>,
    /// The group entity changes.
    pub group_entities: Vec<GroupEntityDelivery>,
    /// The service's answers to the upkeep payments of this sim's players.
    #[serde(default)]
    pub upkeep_answers: Vec<UpkeepAnswer>,
    /// The raid entity changes.
    #[serde(default)]
    pub raid_entities: Vec<RaidEntityDelivery>,
    /// The answers of this sim's players to the raid invites it asked for.
    #[serde(default)]
    pub raid_invite_answers: Vec<RaidInviteAnswer>,
}

/// Service to sim, request/reply: add `change` coins to a player (a negative change takes them).
/// The service asks again with the same `id` when no answer came; the sim moves the coins of one
/// id once and answers a repeat with the first answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoinTransfer {
    /// The move.
    pub id: Uuid,
    /// The character.
    pub character: Uuid,
    /// The change, in hundredths of a coin.
    pub change: i64,
}

/// The answer to a [`CoinTransfer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoinReply {
    /// The coins moved; the player has this many now.
    Done {
        /// The player's coins after the change.
        coins: u64,
    },
    /// The player is not on this sim.
    NotHere,
    /// The player has fewer coins than the change takes.
    TooFew,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn a_batch_and_deliveries_round_trip() {
        let sim = BusId::try_from("sim-1".to_owned()).expect("id");
        let character = Uuid::from_u128(7);
        let batch = SocialBatch {
            sim: sim.clone(),
            inputs: vec![
                SocialInput::Joined(SocialPlayer {
                    character,
                    name: "Ann".to_owned(),
                    level: 4,
                    icon: [1; ICON_LEN],
                    pronoun: 1,
                    sim,
                    level_name: "newworld_vitaeeterna".to_owned(),
                    territories: vec![3, 7],
                    faction: 1,
                    position: [10.0, 20.0],
                    instanced: true,
                }),
                SocialInput::Call {
                    character,
                    type_index: 1291,
                    body: vec![1, 2, 3],
                },
                SocialInput::Vitals(PlayerVitals {
                    character,
                    health_percent: 50,
                    mana_percent: 100,
                    dead: false,
                }),
                SocialInput::Left {
                    character,
                    reason: LeaveReason::Logout,
                },
                SocialInput::PayUpkeep(UpkeepPayment {
                    character,
                    territory: 6,
                    cost: 100_000,
                    at_ms: 1_000,
                }),
                SocialInput::RaidJoin {
                    character,
                    raid: RaidSpec {
                        raid: 9428,
                        kind: 1,
                        opposing: 9429,
                        entity: [7; 16],
                        groups: vec![Uuid::from_u128(3)],
                    },
                    group: 0,
                },
                SocialInput::RaidLeave {
                    character,
                    raid: [7; 16],
                },
                SocialInput::RaidInvite {
                    character,
                    raid: RaidSpec {
                        raid: 9428,
                        kind: 1,
                        opposing: 9429,
                        entity: [7; 16],
                        groups: vec![Uuid::from_u128(3)],
                    },
                    group: 0,
                    activity: Uuid::from_u128(4),
                },
            ],
        };
        assert_eq!(
            decode::<SocialBatch>(&encode(&batch).expect("encode")).expect("decode"),
            batch
        );
        let deliveries = SocialDeliveries {
            deliveries: vec![SocialDelivery {
                recipients: vec![character],
                header: SocialHeader::Facet(SocialFacetKind::Chat),
                type_index: 293,
                args: vec![0],
            }],
            own_states: vec![OwnStateDelivery {
                character,
                kind: SocialStateKind::Groups,
                body: vec![1, 1],
            }],
            group_entities: vec![
                GroupEntityDelivery {
                    character,
                    view: Some(GroupEntityView {
                        group: Uuid::from_u128(9),
                        body: vec![1],
                    }),
                },
                GroupEntityDelivery {
                    character,
                    view: None,
                },
            ],
            upkeep_answers: vec![UpkeepAnswer {
                character,
                territory: 6,
                paid: true,
            }],
            raid_entities: vec![RaidEntityDelivery {
                character,
                view: Some(RaidEntityView {
                    entity: [7; 16],
                    body: vec![1],
                    groups: vec![GroupEntityView {
                        group: Uuid::from_u128(3),
                        body: vec![2],
                    }],
                }),
            }],
            raid_invite_answers: vec![RaidInviteAnswer {
                character,
                activity: Uuid::from_u128(4),
                accepted: true,
            }],
        };
        assert_eq!(
            decode::<SocialDeliveries>(&encode(&deliveries).expect("encode")).expect("decode"),
            deliveries
        );
        let transfer = CoinTransfer {
            id: Uuid::from_u128(8),
            character,
            change: -5,
        };
        assert_eq!(
            decode::<CoinTransfer>(&encode(&transfer).expect("encode")).expect("decode"),
            transfer
        );
        let reply = CoinReply::Done { coins: 5 };
        assert_eq!(
            decode::<CoinReply>(&encode(&reply).expect("encode")).expect("decode"),
            reply
        );
    }
}
