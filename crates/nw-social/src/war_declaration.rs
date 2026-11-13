// SPDX-License-Identifier: AGPL-3.0-or-later
//! War declarations (2920) and their lotteries (`nw_db::advance_conflicts`).
//!
//! The service checks what it holds itself, in the order of the client's `CanDeclareWar`
//! results (wars and sieges study 2026-11-12): a company (`NotInAGuild`), the `Declare_War`
//! privilege (23, `FailNoPrivilege`), a faction (`FailNoFaction`), another company
//! (`FailOwnGuild`) of another faction (`FailSameFaction`), a war camp tier of the popup, and the
//! cost ([`nw_db::war_cost`]) and the defender's siege window the popup showed. The database then
//! checks the territory in one transaction ([`nw_db::declare_in_lottery`]: the conflict, its
//! faction and window, the owner, the open wars, one declaration per company, the company's
//! influence, the treasury). The client's `TooFewGuildMembers` check is off: its keys
//! (`javelin.social.enable-min-guild-members-for-war-dec`, `min-guild-member-count-for-war-dec`)
//! are not set in the retail remote configuration, and the client default is 0.
//!
//! **Coin** (the treasury module's two steps): a wallet declaration takes the coins from the
//! player through its sim first, then goes to the database; a refused one gives them back (or
//! owes them). A treasury declaration is paid in the database transaction (treasury log reason
//! `war declaration`, the client's `WarDeclaration` 4); a lost lottery gives it back there
//! (`war declaration failed`, 5), and a wallet gets its coins back through its sim.
//!
//! **Answers**: every refusal gets 941 with the cost and siege window the service holds (Observed:
//! the popup compares them with its own and shows which changed, or `unknown`); a declaration
//! in the lottery gets 1737 with `WarRequestStatus` Pending; the drawn company's declaring player
//! gets 193 with the war id (Inferred meanings, `nw_messages::social::war_requests`). Which
//! message shows a lost lottery's text (`OnWarDeclarationUnsuccessful`) is Unknown: no capture
//! holds a lottery and no client reader names it, so a losing player only gets its coin back.

use nw_db::{Declaration, LotteryDraw, LotteryEntry, WarRequest, war_cost};
use nw_messages::social::SocialPush;
use nw_messages::social::war_requests::{WAR_REQUEST_PENDING, WarCall, WarPush};
use uuid::Uuid;

use crate::companies::{Privilege, Right, may};
use crate::effects::Effect;
use crate::treasury::MoveResult;
use crate::world::{CoinMove, RESULT_LABEL, SocialWorld, Transfer};

/// Metric: war declarations by `result`.
const METRIC_DECLARATIONS: &str = "social_war_declarations_total";
/// A declaration the lottery took.
const IN_LOTTERY: &str = "in_lottery";
/// A declaration the service or the database refused.
const REFUSED: &str = "refused";
/// The right that declares a war (the client's `Declare_War` privilege, 23).
const DECLARE_RIGHT: Right = Right::Has(Privilege::DeclareWar);
/// The faction byte of no faction.
const NO_FACTION: u8 = 0;

/// Why the service refused a declaration before the database saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    NotInACompany,
    NoPrivilege,
    NoFaction,
    OwnCompany,
    UnknownDefender,
    SameFaction,
    NoSuchCamp,
    OtherCostOrWindow,
    TooFewCoins,
}

impl SocialWorld {
    /// Carries out the war call `call` of `character`.
    pub(crate) fn war_call(&mut self, character: Uuid, call: WarCall) {
        let WarCall::Declare {
            defender,
            cost,
            siege_window,
            territory,
            camp,
            from_treasury,
        } = call;
        let defender = Uuid::from_bytes(defender);
        let company = self.company_of(character);
        let held_cost = war_cost(camp).unwrap_or_default();
        let held_window = self
            .companies
            .get(&defender)
            .map_or(siege_window, |state| state.record.siege_hour);
        let checked = self.check_declaration(character, company, defender, camp);
        let request = checked.and_then(|(company, faction)| {
            let request = WarRequest {
                territory,
                company,
                faction,
                character,
                defender,
                camp,
                cost: held_cost,
                from_treasury,
            };
            if cost != held_cost || siege_window.to_bits() != held_window.to_bits() {
                return Err(Refusal::OtherCostOrWindow);
            }
            let treasury = self
                .companies
                .get(&company)
                .map_or(0, |state| state.record.treasury);
            if from_treasury && treasury < held_cost {
                return Err(Refusal::TooFewCoins);
            }
            Ok(request)
        });
        let request = match request {
            Ok(request) => request,
            Err(why) => {
                tracing::info!(%character, territory, ?why, "war declaration refused");
                let failed = WarPush::DeclarationFailed {
                    defender: *defender.as_bytes(),
                    territory,
                    company: company.map_or([0; 16], |(company, _)| *company.as_bytes()),
                    cost: held_cost,
                    siege_window: held_window,
                };
                metrics::counter!(METRIC_DECLARATIONS, RESULT_LABEL => REFUSED).increment(1);
                self.push(&[character], SocialPush::War(failed));
                return;
            }
        };
        if from_treasury {
            self.declare(request);
        } else if !self.move_coins(
            Transfer::WarFee(request),
            character,
            -i64::try_from(request.cost).unwrap_or(i64::MAX),
        ) {
            tracing::info!(%character, "war declaration of a player that left");
        }
    }

    /// The service's checks of a declaration of `character` (of `company` and its rank) against
    /// `defender` with camp tier `camp`: the company and its faction.
    fn check_declaration(
        &self,
        character: Uuid,
        company: Option<(Uuid, u8)>,
        defender: Uuid,
        camp: u8,
    ) -> Result<(Uuid, u8), Refusal> {
        let (company, rank) = company.ok_or(Refusal::NotInACompany)?;
        if !may(rank, DECLARE_RIGHT) {
            return Err(Refusal::NoPrivilege);
        }
        let faction = self
            .companies
            .get(&company)
            .map_or(NO_FACTION, |state| state.record.faction);
        if faction == NO_FACTION {
            return Err(Refusal::NoFaction);
        }
        if company == defender {
            return Err(Refusal::OwnCompany);
        }
        let defender_faction = self
            .companies
            .get(&defender)
            .map(|state| state.record.faction)
            .ok_or(Refusal::UnknownDefender)?;
        if defender_faction == faction {
            return Err(Refusal::SameFaction);
        }
        war_cost(camp).ok_or(Refusal::NoSuchCamp)?;
        tracing::debug!(%character, %company, "war declaration checked");
        Ok((company, faction))
    }

    /// Sends `request` to the database's lottery.
    fn declare(&mut self, request: WarRequest) {
        let ticket = self.ticket();
        self.war_requests.insert(ticket, request);
        self.out.push(Effect::DeclareWar { ticket, request });
    }

    /// The coins of the wallet declaration `request` moved with `result`.
    pub(crate) fn war_fee_moved(&mut self, request: WarRequest, result: MoveResult) {
        match result {
            MoveResult::Done => self.declare(request),
            MoveResult::Refused => self.declaration_failed(&request),
            MoveResult::Unanswered => {
                tracing::error!(character = %request.character, amount = request.cost,
                    "war fee coin move not answered: Unknown whether the coins left; owed back");
                self.owe(fee_of(&request));
                self.declaration_failed(&request);
            }
        }
    }

    /// Tells the declaring player of `request` that it failed, with the cost and window the
    /// service holds.
    fn declaration_failed(&mut self, request: &WarRequest) {
        metrics::counter!(METRIC_DECLARATIONS, RESULT_LABEL => REFUSED).increment(1);
        let siege_window = self
            .companies
            .get(&request.defender)
            .map_or(0.0, |state| state.record.siege_hour);
        let failed = WarPush::DeclarationFailed {
            defender: *request.defender.as_bytes(),
            territory: request.territory,
            company: *request.company.as_bytes(),
            cost: request.cost,
            siege_window,
        };
        self.push(&[request.character], SocialPush::War(failed));
    }

    /// Gives the wallet coins `coins` back to their player through its sim, or owes them.
    fn give_back(&mut self, coins: CoinMove) {
        let change = i64::try_from(coins.amount).unwrap_or(i64::MAX);
        if !self.move_coins(Transfer::DepositReturn(coins), coins.character, change) {
            self.owe(coins);
        }
    }

    /// Takes the treasury balance `balance` of `company` that the database stored.
    fn treasury_stored(&mut self, company: Uuid, balance: u64) {
        if let Some(state) = self.companies.get_mut(&company) {
            state.record.treasury = balance;
            let members = state.members.clone();
            for member in members {
                self.touch(member);
            }
        }
    }

    /// What the database did with the declaration of `ticket`: `None` when it failed.
    pub fn war_declared(
        &mut self,
        ticket: u64,
        declaration: Option<Declaration>,
        now_ms: u64,
    ) -> Vec<Effect> {
        let Some(request) = self.war_requests.remove(&ticket) else {
            return Vec::new();
        };
        match declaration {
            Some(Declaration::InLottery {
                lottery_end_ms,
                treasury,
            }) => {
                metrics::counter!(METRIC_DECLARATIONS, RESULT_LABEL => IN_LOTTERY).increment(1);
                tracing::info!(company = %request.company, territory = request.territory,
                    lottery_end_ms, "war declaration in the lottery");
                if let Some(balance) = treasury {
                    self.treasury_stored(request.company, balance);
                }
                let pending = WarPush::Pending {
                    defender: *request.defender.as_bytes(),
                    status: WAR_REQUEST_PENDING,
                };
                self.push(&[request.character], SocialPush::War(pending));
            }
            refused => {
                tracing::info!(company = %request.company, territory = request.territory,
                    ?refused, "war declaration refused by the database");
                if !request.from_treasury {
                    self.give_back(fee_of(&request));
                }
                self.declaration_failed(&request);
            }
        }
        self.refresh_views(now_ms);
        std::mem::take(&mut self.out)
    }

    /// Takes the lotteries `draws` the database drew: the drawn declaring player gets 193, the
    /// others their coin.
    pub fn lotteries_drawn(&mut self, draws: Vec<LotteryDraw>, now_ms: u64) -> Vec<Effect> {
        for draw in draws {
            if let Some((winner, war)) = draw.winner {
                tracing::info!(territory = draw.territory, company = %winner.company, %war,
                    "war lottery drawn");
                let won = WarPush::Successful {
                    war: *war.as_bytes(),
                };
                self.push(&[winner.character], SocialPush::War(won));
            }
            for entry in draw.refunds {
                match entry.treasury {
                    Some(balance) => self.treasury_stored(entry.company, balance),
                    None => self.give_back(refund_of(&entry)),
                }
            }
        }
        self.refresh_views(now_ms);
        std::mem::take(&mut self.out)
    }
}

/// The wallet coins of declaration `request`.
const fn fee_of(request: &WarRequest) -> CoinMove {
    CoinMove {
        character: request.character,
        company: request.company,
        amount: request.cost,
    }
}

/// The wallet coins of the lost declaration `entry`.
const fn refund_of(entry: &LotteryEntry) -> CoinMove {
    CoinMove {
        character: entry.character,
        company: entry.company,
        amount: entry.cost,
    }
}

#[cfg(test)]
mod tests {
    use nw_bus::CoinReply;
    use nw_db::{WAR_DECLARATION_COST, WarRefusal};
    use nw_messages::social::war_requests::{
        DECLARATION_FAILED_TYPE_INDEX, DECLARE_WAR_TYPE_INDEX, REQUEST_PENDING_TYPE_INDEX,
        REQUEST_SUCCESSFUL_TYPE_INDEX,
    };

    use super::*;
    use crate::companies::tests::company_of_two;
    use crate::world::tests::{NOW_MS, call, pushes};

    const TERRITORY: u16 = 6;
    const SIEGE_HOUR: f32 = 18.0;
    const ENEMY_FACTION: u8 = 3;
    const TREASURY: u64 = 1_000_000;

    /// The world of [`company_of_two`] with a defender company of another faction.
    fn world_with_defender() -> (SocialWorld, Uuid, Uuid) {
        let (mut world, company) = company_of_two();
        let defender = Uuid::from_u128(0xdefe);
        let mut record = world.companies[&company].record.clone();
        record.id = defender;
        record.name = "Made Up Defender".to_owned();
        record.faction = ENEMY_FACTION;
        record.siege_hour = SIEGE_HOUR;
        world.companies.insert(
            defender,
            crate::world::CompanyState {
                record,
                members: Vec::new(),
                invites: Default::default(),
            },
        );
        world
            .companies
            .get_mut(&company)
            .expect("company")
            .record
            .treasury = TREASURY;
        (world, company, defender)
    }

    fn declare(
        world: &mut SocialWorld,
        number: u128,
        defender: Uuid,
        cost: u64,
        from_treasury: bool,
    ) -> Vec<Effect> {
        let mut args = Vec::new();
        args.extend_from_slice(defender.as_bytes());
        args.extend_from_slice(&cost.to_be_bytes());
        args.extend_from_slice(&SIEGE_HOUR.to_be_bytes());
        args.extend_from_slice(&TERRITORY.to_be_bytes());
        args.push(0);
        args.push(u8::from(from_treasury));
        call(
            world,
            number,
            "sim-a",
            DECLARE_WAR_TYPE_INDEX,
            &args,
            NOW_MS,
        )
    }

    fn declared(effects: &[Effect]) -> Option<(u64, WarRequest)> {
        effects.iter().find_map(|effect| match effect {
            Effect::DeclareWar { ticket, request } => Some((*ticket, *request)),
            _ => None,
        })
    }

    fn coin_move(effects: &[Effect]) -> Option<(u64, i64)> {
        effects.iter().find_map(|effect| match effect {
            Effect::MoveCoins {
                ticket, transfer, ..
            } => Some((*ticket, transfer.change)),
            _ => None,
        })
    }

    #[test]
    fn a_treasury_declaration_goes_to_the_lottery_and_its_player_hears_pending() {
        let (mut world, company, defender) = world_with_defender();
        // Character 1 is the governor of `company_of_two`.
        let effects = declare(&mut world, 1, defender, WAR_DECLARATION_COST, true);
        let (ticket, request) = declared(&effects).expect("to the database");
        assert_eq!(request.company, company);
        assert_eq!(request.defender, defender);
        assert!(request.from_treasury);
        assert_eq!(
            coin_move(&effects),
            None,
            "the treasury pays in the database"
        );
        let balance = TREASURY - WAR_DECLARATION_COST;
        let answer = Declaration::InLottery {
            lottery_end_ms: NOW_MS,
            treasury: Some(balance),
        };
        let effects = world.war_declared(ticket, Some(answer), NOW_MS);
        assert_eq!(world.companies[&company].record.treasury, balance);
        assert_eq!(pushes(&effects, REQUEST_PENDING_TYPE_INDEX).len(), 1);
    }

    #[test]
    fn a_wrong_cost_or_a_settler_gets_941_with_the_held_values() {
        let (mut world, _, defender) = world_with_defender();
        let effects = declare(&mut world, 1, defender, WAR_DECLARATION_COST + 1, true);
        assert_eq!(declared(&effects), None);
        assert_eq!(pushes(&effects, DECLARATION_FAILED_TYPE_INDEX).len(), 1);
        // Character 2 is a settler: no `Declare_War` privilege.
        let effects = declare(&mut world, 2, defender, WAR_DECLARATION_COST, true);
        assert_eq!(declared(&effects), None);
        assert_eq!(pushes(&effects, DECLARATION_FAILED_TYPE_INDEX).len(), 1);
    }

    #[test]
    fn a_wallet_declaration_takes_the_coins_first_and_gives_them_back_when_refused() {
        let (mut world, _, defender) = world_with_defender();
        let effects = declare(&mut world, 1, defender, WAR_DECLARATION_COST, false);
        let (ticket, change) = coin_move(&effects).expect("coins taken first");
        assert_eq!(change, -(WAR_DECLARATION_COST as i64));
        let effects = world.coins_moved(ticket, Some(CoinReply::Done { coins: 0 }), NOW_MS);
        let (ticket, request) = declared(&effects).expect("then the database");
        assert!(!request.from_treasury);
        let refused = Declaration::Refused(WarRefusal::NoConflict);
        let effects = world.war_declared(ticket, Some(refused), NOW_MS);
        let (_, back) = coin_move(&effects).expect("coins back");
        assert_eq!(back, WAR_DECLARATION_COST as i64);
        assert_eq!(pushes(&effects, DECLARATION_FAILED_TYPE_INDEX).len(), 1);
    }

    #[test]
    fn a_drawn_lottery_tells_the_winner_and_refunds_the_others() {
        let (mut world, company, defender) = world_with_defender();
        let winner = LotteryEntry {
            company,
            character: crate::world::tests::character(1),
            cost: WAR_DECLARATION_COST,
            treasury: None,
        };
        let wallet_loser = LotteryEntry {
            character: crate::world::tests::character(2),
            ..winner
        };
        let treasury_loser = LotteryEntry {
            company: defender,
            treasury: Some(TREASURY),
            ..winner
        };
        let draw = LotteryDraw {
            territory: TERRITORY,
            winner: Some((winner, Uuid::from_u128(0x3a8))),
            refunds: vec![wallet_loser, treasury_loser],
        };
        let effects = world.lotteries_drawn(vec![draw], NOW_MS);
        assert_eq!(pushes(&effects, REQUEST_SUCCESSFUL_TYPE_INDEX).len(), 1);
        assert_eq!(
            coin_move(&effects).map(|(_, change)| change),
            Some(WAR_DECLARATION_COST as i64)
        );
        assert_eq!(world.companies[&defender].record.treasury, TREASURY);
    }
}
