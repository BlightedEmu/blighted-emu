// SPDX-License-Identifier: AGPL-3.0-or-later
//! The state a bot keeps over a sim crossing: its health, the state of its running quest steps
//! and its group, as the own player's states in its bundles give them. Before each crossing the
//! bot keeps what it had; [`SETTLE`] after the crossing it compares what the new sim sent with
//! that, and each difference is a finding of the run (a handoff that lost or changed the player's
//! state). A state the bot never got before the crossing has nothing to compare.

use std::time::Duration;

use nw_messages::MemberState;

/// How long after a crossing the bot waits for the new sim's own states before it compares them
/// (our choice: the handoff and the first bundles of the new sim come within a second).
pub const SETTLE: Duration = Duration::from_secs(3);

/// The own states the check compares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnState {
    /// Health, whole points (`HealthAmount` of the vitals).
    pub health: Option<u32>,
    /// The running quest steps: (task CRC, progress, state) of each task of the objectives state,
    /// sorted.
    pub quest_steps: Option<Vec<(u32, u32, String)>>,
    /// The group (`groupId` of the groups state; all zero for none).
    pub group: Option<[u8; nw_messages::RAW_ID_LEN]>,
}

impl OwnState {
    /// Takes the own player's state `state` into account.
    pub fn apply(&mut self, state: &MemberState) {
        match state {
            MemberState::Vitals(vitals) => {
                if let Some(health) = vitals.slot_0.as_ref().and_then(|slot| slot.health_amount) {
                    // A health of a few thousand points.
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a health"
                    )]
                    let whole = health.max(0.0).round() as u32;
                    self.health = Some(whole);
                }
            }
            MemberState::Objectives(objectives) => {
                if let Some(tasks) = &objectives.tasks {
                    let mut steps: Vec<(u32, u32, String)> = tasks
                        .elements
                        .iter()
                        .map(|task| (task.task, task.progress, format!("{:?}", task.state)))
                        .collect();
                    steps.sort();
                    self.quest_steps = Some(steps);
                }
            }
            MemberState::Groups(groups) => {
                if let Some(group) = groups.group_id {
                    self.group = Some(group);
                }
            }
            _ => {}
        }
    }

    /// The differences of `after` from this state, in words: only the states both hold.
    pub fn differences(&self, after: &Self) -> Vec<String> {
        let mut found = Vec::new();
        if let (Some(before), Some(now)) = (self.health, after.health)
            && before != now
        {
            found.push(format!("health {before} -> {now}"));
        }
        if let (Some(before), Some(now)) = (&self.quest_steps, &after.quest_steps)
            && before != now
        {
            found.push(format!(
                "quest steps {} -> {} (first change {:?})",
                before.len(),
                now.len(),
                before
                    .iter()
                    .zip(now)
                    .find(|(was, is)| was != is)
                    .map(|(was, is)| (was.clone(), is.clone()))
            ));
        }
        if let (Some(before), Some(now)) = (self.group, after.group)
            && before != now
        {
            found.push(format!(
                "group {} -> {}",
                data_encoding::HEXLOWER.encode(&before),
                data_encoding::HEXLOWER.encode(&now)
            ));
        }
        found
    }
}

/// One bot's check over its crossings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CrossingCheck {
    /// The own state now.
    now: OwnState,
    /// A crossing whose compare is due: when, its number and the state before it.
    pending: Option<(Duration, u32, OwnState)>,
    /// The crossings compared so far.
    compared: u32,
    /// The differences found, each with its crossing's number.
    findings: Vec<String>,
}

impl CrossingCheck {
    /// The own player's state `state` came.
    pub fn apply(&mut self, state: &MemberState) {
        self.now.apply(state);
    }

    /// Crossing `number` happened at `since`: the state before it is the one held now. A
    /// crossing while another waits for its compare compares that one first.
    pub fn crossed(&mut self, number: u32, since: Duration) {
        self.compare();
        self.pending = Some((since, number, self.now.clone()));
    }

    /// At `since`: the compare of a crossing [`SETTLE`] ago.
    pub fn tick(&mut self, since: Duration) {
        if self
            .pending
            .as_ref()
            .is_some_and(|(at, _, _)| since.saturating_sub(*at) >= SETTLE)
        {
            self.compare();
        }
    }

    fn compare(&mut self) {
        let Some((_, number, before)) = self.pending.take() else {
            return;
        };
        self.compared += 1;
        self.findings.extend(
            before
                .differences(&self.now)
                .into_iter()
                .map(|difference| format!("crossing {number}: {difference}")),
        );
    }

    /// The crossings compared and the differences found, the one still waiting compared too.
    pub fn finish(mut self) -> (u32, Vec<String>) {
        self.compare();
        (self.compared, self.findings)
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::objectives::{ObjectivesState, TaskEntry, TaskWireState};
    use nw_messages::social::states::GroupsState;
    use nw_messages::{FullList, VitalsSlot0, VitalsState};

    use super::*;

    fn health(points: f32) -> MemberState {
        MemberState::Vitals(VitalsState {
            slot_0: Some(VitalsSlot0 {
                health_amount: Some(points),
                ..VitalsSlot0::default()
            }),
            slot_1: None,
        })
    }

    fn quest(progress: u32) -> MemberState {
        MemberState::Objectives(ObjectivesState {
            tasks: Some(FullList::first(vec![TaskEntry {
                instance: 1,
                index: 0,
                task: 7,
                progress,
                state: TaskWireState::Active,
            }])),
            ..ObjectivesState::default()
        })
    }

    fn group(id: u8) -> MemberState {
        MemberState::Groups(GroupsState {
            group_id: Some([id; nw_messages::RAW_ID_LEN]),
            ..GroupsState::default()
        })
    }

    const AT: Duration = Duration::from_secs(10);

    #[test]
    fn a_crossing_that_keeps_the_state_finds_nothing_and_one_that_changes_it_is_named() {
        let mut check = CrossingCheck::default();
        for state in [health(1000.0), quest(1), group(3)] {
            check.apply(&state);
        }
        check.crossed(1, AT);
        check.apply(&health(1000.0));
        check.tick(AT + SETTLE);
        check.crossed(2, AT * 2);
        for state in [health(900.0), quest(0), group(4)] {
            check.apply(&state);
        }
        check.tick(AT * 2 + SETTLE - Duration::from_millis(1));
        let (compared, findings) = check.clone().finish();
        assert_eq!(compared, 2);
        assert_eq!(findings.len(), 3, "{findings:?}");
        assert!(findings[0].starts_with("crossing 2: health 1000 -> 900"));
        assert!(findings[1].starts_with("crossing 2: quest steps"));
        assert!(findings[2].starts_with("crossing 2: group"));
    }

    #[test]
    fn a_state_never_seen_before_the_crossing_has_nothing_to_compare() {
        let mut check = CrossingCheck::default();
        check.crossed(1, AT);
        check.apply(&health(500.0));
        check.tick(AT + SETTLE);
        assert_eq!(check.finish(), (1, Vec::new()));
    }
}
