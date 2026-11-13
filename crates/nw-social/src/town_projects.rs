// SPDX-License-Identifier: AGPL-3.0-or-later
//! Town project starts (453) and cancels (2739) of the territory planning board (wars and sieges
//! findings "Town projects", 2026-11-13).
//!
//! The player's sim checked the board, the project, its slot and the upgrade's tier
//! ([`nw_bus::SocialInput::StartTownProject`]). The service checks the member's
//! `TerritoryProjectManagement` privilege (27; Observed: the client's own server handler of 453,
//! `FUN_144818ce0`, checks privilege 0x1b before the payment) and, for a treasury payment, the
//! treasury. The database then checks the owner, the slot and the tier and pays from the
//! treasury in one transaction ([`nw_db::start_town_project`], reason `territory progression`,
//! the client's `TerritoryProgression` 2).
//!
//! **Coin** (the treasury module's two steps, as a war declaration's): a wallet start takes the
//! coins from the player through its sim first, then goes to the database; a refused one gives
//! them back (or owes them). Each start has an id, so a start the database stored whose answer
//! was lost is found again on its retry and never refused into a refund.
//!
//! **Answers**: retail has no answer message for 453 or 2739 (the planning board shows the
//! project in its progression data); the player's sim is told whether it was done
//! ([`nw_bus::TownProjectAnswer`]) and shows the board again. Inferred: a cancel gives no coin
//! back (no client text names a refund).

use nw_bus::{TownProjectAnswer, TownProjectCancel, TownProjectStart};
use nw_db::{ProjectStart, ProjectStarted};
use uuid::Uuid;

use crate::companies::{Privilege, Right, may};
use crate::effects::Effect;
use crate::treasury::MoveResult;
use crate::world::{CoinMove, RESULT_LABEL, SocialWorld, Transfer};

/// Metric: town project starts and cancels by `result`.
const METRIC_PROJECTS: &str = "social_town_projects_total";
/// A start or cancel that was done.
const DONE: &str = "done";
/// A start or cancel the service or the database refused.
const REFUSED: &str = "refused";
/// The right that starts and cancels a town project (the client's `Territory_ProjectManagement`
/// privilege, 27).
const PROJECT_RIGHT: Right = Right::Has(Privilege::TerritoryProjectManagement);

/// A cancel the database works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingCancel {
    character: Uuid,
    territory: u16,
}

impl SocialWorld {
    /// The company of `character` when its rank may manage town projects.
    fn project_company(&self, character: Uuid) -> Result<Uuid, &'static str> {
        match self.company_of(character) {
            None => Err("not a member"),
            Some((_, rank)) if !may(rank, PROJECT_RIGHT) => Err("rank without the right"),
            Some((company, _)) => Ok(company),
        }
    }

    /// Starts `start` of its player: the right and the treasury here, then the coin and the
    /// database (module doc).
    pub(crate) fn start_town_project(&mut self, start: TownProjectStart) {
        let checked = self.project_company(start.character).and_then(|company| {
            let funds = self
                .companies
                .get(&company)
                .map_or(0, |state| state.record.treasury);
            if start.from_treasury && funds < start.cost {
                Err("treasury too low")
            } else {
                Ok(company)
            }
        });
        let company = match checked {
            Ok(company) => company,
            Err(why) => {
                tracing::info!(character = %start.character, territory = start.territory, why,
                    "town project start refused");
                self.answer_project(start.character, start.territory, false);
                return;
            }
        };
        let request = ProjectStart {
            id: Uuid::new_v4(),
            territory: start.territory,
            kind: start.kind,
            project: start.project,
            upgrade: start.upgrade,
            level: start.level,
            needed: start.needed,
            cost: start.cost,
            company,
            character: start.character,
            from_treasury: start.from_treasury,
            at_ms: start.at_ms,
            ends_ms: start.ends_ms,
        };
        if start.from_treasury {
            self.store_project(request);
        } else if !self.move_coins(
            Transfer::ProjectFee(request),
            start.character,
            -i64::try_from(start.cost).unwrap_or(i64::MAX),
        ) {
            tracing::info!(character = %start.character, "town project start of a player that left");
        }
    }

    /// Sends `request` to the database.
    fn store_project(&mut self, request: ProjectStart) {
        let ticket = self.ticket();
        self.project_starts.insert(ticket, request);
        self.out.push(Effect::StartTownProject {
            ticket,
            start: request,
        });
    }

    /// The coins of the wallet start `request` moved with `result`.
    pub(crate) fn project_fee_moved(&mut self, request: ProjectStart, result: MoveResult) {
        match result {
            MoveResult::Done => self.store_project(request),
            MoveResult::Refused => self.answer_project(request.character, request.territory, false),
            MoveResult::Unanswered => {
                tracing::error!(character = %request.character, amount = request.cost,
                    "town project coin move not answered: Unknown whether the coins left; owed back");
                self.owe(fee_of(&request));
                self.answer_project(request.character, request.territory, false);
            }
        }
    }

    /// What the database did with the start of `ticket`: `None` when it failed.
    pub fn town_project_started(
        &mut self,
        ticket: u64,
        started: Option<ProjectStarted>,
        now_ms: u64,
    ) -> Vec<Effect> {
        let Some(request) = self.project_starts.remove(&ticket) else {
            return Vec::new();
        };
        match started {
            Some(ProjectStarted::Started { treasury }) => {
                tracing::info!(company = %request.company, territory = request.territory,
                    project = request.project, "town project started");
                if let Some(balance) = treasury {
                    self.treasury_stored(request.company, balance);
                }
                self.answer_project(request.character, request.territory, true);
            }
            refused => {
                tracing::info!(company = %request.company, territory = request.territory,
                    ?refused, "town project start refused by the database");
                if !request.from_treasury {
                    self.give_back(fee_of(&request));
                }
                self.answer_project(request.character, request.territory, false);
            }
        }
        self.refresh_views(now_ms);
        std::mem::take(&mut self.out)
    }

    /// Cancels the running project of `cancel` for its player's company.
    pub(crate) fn cancel_town_project(&mut self, cancel: TownProjectCancel) {
        let company = match self.project_company(cancel.character) {
            Ok(company) => company,
            Err(why) => {
                tracing::info!(character = %cancel.character, territory = cancel.territory, why,
                    "town project cancel refused");
                self.answer_project(cancel.character, cancel.territory, false);
                return;
            }
        };
        let ticket = self.ticket();
        self.project_cancels.insert(
            ticket,
            PendingCancel {
                character: cancel.character,
                territory: cancel.territory,
            },
        );
        self.out.push(Effect::CancelTownProject {
            ticket,
            territory: cancel.territory,
            project: cancel.project,
            company,
            at_ms: cancel.at_ms,
        });
    }

    /// What the database did with the cancel of `ticket`: `None` when it failed.
    pub fn town_project_cancelled(
        &mut self,
        ticket: u64,
        cancelled: Option<bool>,
        now_ms: u64,
    ) -> Vec<Effect> {
        let Some(pending) = self.project_cancels.remove(&ticket) else {
            return Vec::new();
        };
        self.answer_project(
            pending.character,
            pending.territory,
            cancelled == Some(true),
        );
        self.refresh_views(now_ms);
        std::mem::take(&mut self.out)
    }

    /// Tells the sim of `character`, when it is on one, whether its start or cancel for
    /// `territory` was `done`.
    fn answer_project(&mut self, character: Uuid, territory: u16, done: bool) {
        let result = if done { DONE } else { REFUSED };
        metrics::counter!(METRIC_PROJECTS, RESULT_LABEL => result).increment(1);
        if let Some(sim) = self.sim_of(character) {
            let answer = TownProjectAnswer {
                character,
                territory,
                done,
            };
            self.out.push(Effect::TownProjectAnswered { sim, answer });
        }
    }
}

/// The wallet coins of start `request`.
const fn fee_of(request: &ProjectStart) -> CoinMove {
    CoinMove {
        character: request.character,
        company: request.company,
        amount: request.cost,
    }
}

#[cfg(test)]
mod tests {
    use nw_bus::CoinReply;
    use nw_db::ProjectRefusal;

    use super::*;
    use crate::companies::tests::company_of_two;
    use crate::world::tests::{NOW_MS, character};

    const TERRITORY: u16 = 6;
    /// `CookingProjectId2`'s cost and points (build 25810808).
    const COST: u64 = 500_000;
    const NEEDED: u32 = 1500;
    const PROJECT: u32 = 0x00c0_ffee;
    const DAY_MS: u64 = 86_400_000;

    fn start(character: Uuid, from_treasury: bool) -> TownProjectStart {
        TownProjectStart {
            character,
            territory: TERRITORY,
            kind: 2,
            project: PROJECT,
            upgrade: 3,
            level: 2,
            needed: NEEDED,
            cost: COST,
            from_treasury,
            at_ms: NOW_MS,
            ends_ms: NOW_MS + DAY_MS,
        }
    }

    fn answers(effects: &[Effect]) -> Vec<bool> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::TownProjectAnswered { answer, .. } => Some(answer.done),
                _ => None,
            })
            .collect()
    }

    fn ticket_of(effects: &[Effect]) -> Option<u64> {
        effects.iter().find_map(|effect| match effect {
            Effect::StartTownProject { ticket, .. } | Effect::CancelTownProject { ticket, .. } => {
                Some(*ticket)
            }
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
    fn a_treasury_start_needs_the_right_and_the_coin_and_goes_to_the_database() {
        let (mut world, company) = company_of_two();
        let (leader, member) = (character(1), character(2));
        world.start_town_project(start(member, true));
        assert_eq!(
            answers(&world.out),
            [false],
            "a settler has no project right"
        );
        world.out.clear();
        world.start_town_project(start(leader, true));
        assert_eq!(answers(&world.out), [false], "the treasury is empty");
        world.out.clear();
        world
            .companies
            .get_mut(&company)
            .expect("company")
            .record
            .treasury = COST;
        world.start_town_project(start(leader, true));
        let ticket = ticket_of(&world.out).expect("to the database");
        world.out.clear();
        let effects = world.town_project_started(
            ticket,
            Some(ProjectStarted::Started { treasury: Some(0) }),
            NOW_MS,
        );
        assert_eq!(answers(&effects), [true]);
        assert_eq!(world.companies[&company].record.treasury, 0);
    }

    #[test]
    fn a_wallet_start_takes_the_coin_first_and_gives_it_back_when_refused() {
        let (mut world, _) = company_of_two();
        let leader = character(1);
        world.start_town_project(start(leader, false));
        let (moved, change) = coin_move(&world.out).expect("coins asked");
        assert_eq!(change, -i64::try_from(COST).expect("cost"));
        world.out.clear();
        let effects = world.coins_moved(moved, Some(CoinReply::Done { coins: COST }), NOW_MS);
        let ticket = ticket_of(&effects).expect("to the database");
        let effects = world.town_project_started(
            ticket,
            Some(ProjectStarted::Refused(ProjectRefusal::SlotBusy)),
            NOW_MS,
        );
        assert_eq!(answers(&effects), [false]);
        let (_, back) = coin_move(&effects).expect("coins back");
        assert_eq!(back, i64::try_from(COST).expect("cost"));
    }

    #[test]
    fn a_cancel_needs_the_right_and_answers_what_the_database_did() {
        let (mut world, _) = company_of_two();
        let (leader, member) = (character(1), character(2));
        let cancel = |character| TownProjectCancel {
            character,
            territory: TERRITORY,
            project: PROJECT,
            at_ms: NOW_MS,
        };
        world.cancel_town_project(cancel(member));
        assert_eq!(answers(&world.out), [false]);
        world.out.clear();
        world.cancel_town_project(cancel(leader));
        let ticket = ticket_of(&world.out).expect("to the database");
        world.out.clear();
        assert_eq!(
            answers(&world.town_project_cancelled(ticket, Some(true), NOW_MS)),
            [true]
        );
        assert!(
            world
                .town_project_cancelled(ticket, Some(true), NOW_MS)
                .is_empty(),
            "a ticket answers once"
        );
    }
}
