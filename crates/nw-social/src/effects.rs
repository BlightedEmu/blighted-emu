// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the social state asks of the service around it: messages to send, rows to store, and the
//! two steps of a treasury transfer.

use nw_bus::{
    BusId, CoinTransfer, GroupEntityDelivery, OwnStateDelivery, RaidEntityDelivery,
    RaidInviteAnswer, SocialDelivery, TownProjectAnswer, UpkeepAnswer,
};
use nw_db::{
    CoinReturn, Company, CompanyInvite, CompanyMember, LinkKind, PlayerRaidRecord, PlayerReport,
    ProjectStart, SocialGroup, SocialSettings, TreasuryReason, WarRequest,
};
use uuid::Uuid;

/// One thing for the service to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Send a delivery to one sim.
    Deliver {
        /// The sim of the receiving players.
        sim: BusId,
        /// The delivery.
        delivery: SocialDelivery,
    },
    /// Send a replicated social state change of one player to its sim.
    OwnState {
        /// The sim of the player.
        sim: BusId,
        /// The state.
        delivery: OwnStateDelivery,
    },
    /// Send a player's group entity to its sim.
    GroupEntity {
        /// The sim of the player.
        sim: BusId,
        /// The group entity.
        delivery: GroupEntityDelivery,
    },
    /// Send a player's raid entity and its raid's other group entities to its sim.
    RaidEntity {
        /// The sim of the player.
        sim: BusId,
        /// The raid entity.
        delivery: RaidEntityDelivery,
    },
    /// Store a change.
    Store(StoreOp),
    /// Read the account of a character the state did not load at start (a character made since);
    /// the answer comes back with `SocialWorld::account_found`.
    FindAccount {
        /// The character.
        character: Uuid,
    },
    /// Move a player's coins through its sim; the answer comes back with
    /// `SocialWorld::coins_moved` and the same ticket.
    MoveCoins {
        /// The transfer's ticket.
        ticket: u64,
        /// The sim that has the player.
        sim: BusId,
        /// The change.
        transfer: CoinTransfer,
    },
    /// Change a treasury in one database transaction with its log row; the answer comes back with
    /// `SocialWorld::treasury_changed` and the same ticket.
    Treasury {
        /// The transfer's ticket.
        ticket: u64,
        /// The company.
        company: Uuid,
        /// The member.
        character: Uuid,
        /// The change, hundredths of a coin.
        change: i64,
        /// Why.
        reason: TreasuryReason,
    },
    /// Put a war declaration into its territory's lottery in one database transaction (the
    /// treasury pays in it when it pays); the answer comes back with
    /// `SocialWorld::war_declared` and the same ticket.
    DeclareWar {
        /// The declaration's ticket.
        ticket: u64,
        /// The declaration.
        request: WarRequest,
    },
    /// Start the town project `start` in the database (the treasury pays in its transaction
    /// when it pays); the answer comes back with `SocialWorld::town_project_started` and the
    /// same ticket.
    StartTownProject {
        /// The start's ticket.
        ticket: u64,
        /// The start.
        start: ProjectStart,
    },
    /// Cancel the running town project `project` of `territory` for `company` in the database;
    /// the answer comes back with `SocialWorld::town_project_cancelled` and the same ticket.
    CancelTownProject {
        /// The cancel's ticket.
        ticket: u64,
        /// The territory id.
        territory: u16,
        /// The CRC-32 of the project's lowercase `ProjectId`.
        project: u32,
        /// The company that must own the territory.
        company: Uuid,
        /// When the player asked, Unix milliseconds.
        at_ms: u64,
    },
    /// Tell the player's sim whether its town project start or cancel was done.
    TownProjectAnswered {
        /// The sim that has the player.
        sim: BusId,
        /// The answer.
        answer: TownProjectAnswer,
    },
    /// Tell the payer's sim whether its territory upkeep payment was taken.
    UpkeepAnswered {
        /// The sim that has the payer.
        sim: BusId,
        /// The answer.
        answer: UpkeepAnswer,
    },
    /// Tell the sim that asked for a raid invite its player's answer (the sim moves it into the
    /// war when it accepted).
    RaidInviteAnswered {
        /// The sim that asked for the invite.
        sim: BusId,
        /// The answer.
        answer: RaidInviteAnswer,
    },
}

/// One change to store.
#[derive(Debug, Clone, PartialEq)]
pub enum StoreOp {
    /// A character's options and offline look.
    Settings {
        /// The character.
        character: Uuid,
        /// The settings.
        settings: SocialSettings,
    },
    /// A new link.
    AddLink {
        /// The character.
        character: Uuid,
        /// The other.
        other: Uuid,
        /// The kind.
        kind: LinkKind,
    },
    /// A link that ended.
    RemoveLink {
        /// The character.
        character: Uuid,
        /// The other.
        other: Uuid,
        /// The kind.
        kind: LinkKind,
    },
    /// A player report.
    Report(PlayerReport),
    /// A new company with its founder.
    CreateCompany {
        /// The company.
        company: Company,
        /// The founder (governor).
        founder: Uuid,
    },
    /// A company's editable fields.
    UpdateCompany(Company),
    /// A company with no member left.
    DeleteCompany(Uuid),
    /// A member and its rank.
    SetMember(CompanyMember),
    /// A member that left.
    RemoveMember {
        /// The character.
        character: Uuid,
        /// Unix milliseconds when it left.
        left_at_ms: u64,
    },
    /// A company invite.
    AddInvite(CompanyInvite),
    /// A company invite that ended.
    RemoveInvite {
        /// The company.
        company: Uuid,
        /// The invitee.
        character: Uuid,
    },
    /// A group with its members.
    SaveGroup(SocialGroup),
    /// A group that ended.
    DeleteGroup(Uuid),
    /// A players' raid with its members ([`crate::player_raids`]).
    SavePlayerRaid(PlayerRaidRecord),
    /// A players' raid that ended, by raid entity id.
    DeletePlayerRaid(Uuid),
    /// Coin owed to a character (its sim did not take it).
    AddCoinReturn(CoinReturn),
    /// Coin owed to a character that its sim took now.
    RemoveCoinReturn(Uuid),
}

/// What the database did with a treasury change ([`Effect::Treasury`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreasuryOutcome {
    /// Stored; the treasury holds this much now, hundredths of a coin.
    Stored(u64),
    /// Refused: the treasury holds less than the change takes, or the company is gone.
    Refused,
    /// The database failed for good (a refused value, not a lost connection: the service runs a
    /// change again until the database is back).
    Failed,
}
