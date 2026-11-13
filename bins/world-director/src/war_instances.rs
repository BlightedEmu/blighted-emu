// SPDX-License-Identifier: AGPL-3.0-or-later
//! The instances of the territory wars, with no I/O: which war needs its instance now, and which
//! wars have one (findings `2026-11-12-wars-and-sieges.md`, section "The framed instance and the
//! war fight").
//!
//! Observed (`arvaysa_ES_attacker_war_45vs45`): the war runs in an instance of the open world
//! level cut to the fort's 512 m frame ([`nw_content::war_forts`]), with a phase of its own; the
//! roster's player was moved into it at 1783536301572, 898.4 s before the war's start
//! (1783537200000). Inferred: the director opens the instance [`WAR_INSTANCE_LEAD_MS`] before
//! the start (the retail opening time itself is not in the captures), for the players on the
//! roster at that time, and it lasts at most to the end of the war's time and its result time
//! (the players that do not leave are moved out then: Inferred, the retail player left by itself
//! 47.6 s after the end). A war whose fort no capture shows gets no instance.

use std::collections::{BTreeMap, BTreeSet};

use nw_bus::WarInstance;
use nw_content::war_forts::WarForts;
use nw_db::{TerritoryWar, WAR_RESULT_SHOWN_MS};
use uuid::Uuid;

/// How long before its start a war's instance opens (Inferred from the retail move 898.4 s
/// before the start: 15 minutes).
pub const WAR_INSTANCE_LEAD_MS: u64 = 900_000;

/// How long before its start an invasion's instance opens (Observed: the invite of
/// `Playthrough_Part_21`, its 715, came 449.9 s before the 3454).
pub const INVASION_INSTANCE_LEAD_MS: u64 = 450_000;

/// How long before its start the instance of `war` opens.
pub const fn instance_lead_ms(war: &TerritoryWar) -> u64 {
    if war.is_invasion() {
        INVASION_INSTANCE_LEAD_MS
    } else {
        WAR_INSTANCE_LEAD_MS
    }
}

/// A war instance to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarOpening {
    /// The war.
    pub war: WarInstance,
    /// The characters on its roster.
    pub members: Vec<Uuid>,
    /// Unix ms when the instance ends at the latest: the end of the war's time and of its result
    /// time.
    pub ends_at_ms: u64,
}

/// The wars that have an instance or wait for one.
#[derive(Debug, Default)]
pub struct WarInstances {
    /// The instance of each war that has one.
    opened: BTreeMap<Uuid, Uuid>,
    /// The wars whose instance is opening.
    opening: BTreeSet<Uuid>,
}

impl WarInstances {
    /// Takes the wars that the open instances run (`running`: war, instance), so a war whose
    /// instance ended or that a restarted director adopted is known as it is.
    pub fn sync(&mut self, running: impl Iterator<Item = (Uuid, Uuid)>) {
        self.opened = running.collect();
    }

    /// The war instances to open at Unix ms `now_ms` for `wars`: each war without a result that
    /// starts within [`WAR_INSTANCE_LEAD_MS`] or runs, whose fort `forts` has, with players on
    /// its roster, and that has no instance yet. Each one is marked as opening.
    pub fn due(&mut self, wars: &[TerritoryWar], forts: &WarForts, now_ms: u64) -> Vec<WarOpening> {
        let mut openings = Vec::new();
        for war in wars {
            let soon = now_ms >= war.starts_ms.saturating_sub(instance_lead_ms(war));
            let open = war.result.is_none() && soon && now_ms < war.ends_ms();
            if !open || self.opened.contains_key(&war.war) || self.opening.contains(&war.war) {
                continue;
            }
            // A fort of an invasion capture only has no capture point: no war runs there.
            let fort = forts.fort(war.territory);
            if fort.is_none_or(|fort| fort.is_invasion_fort() && !war.is_invasion()) {
                continue;
            }
            let members: Vec<Uuid> = war.roster().map(|(character, _)| character).collect();
            if members.is_empty() {
                continue;
            }
            self.opening.insert(war.war);
            openings.push(WarOpening {
                war: WarInstance {
                    war: war.war,
                    territory: war.territory,
                },
                members,
                ends_at_ms: war.ends_ms().saturating_add(WAR_RESULT_SHOWN_MS),
            });
        }
        openings
    }

    /// The opening of `war`'s instance ended: it runs `instance`, or it did not open (`None`: the
    /// next read tries again).
    pub fn opened(&mut self, war: Uuid, instance: Option<Uuid>) {
        self.opening.remove(&war);
        if let Some(instance) = instance {
            self.opened.insert(war, instance);
        }
    }
}

#[cfg(test)]
mod tests {
    use nw_db::{RosterSeat, WAR_LENGTH_MS, WarSignup};

    use super::*;

    const STARTS_MS: u64 = 1_783_537_200_000;
    /// A made-up fort of territory 8 (the war forts table form).
    const FORTS: &str = r#"{"forts": [{
        "territory": 8, "frame": [5632.0, 5120.0, 512.0, 512.0],
        "arrivals": [{"side": 1, "position": [6001.5, 5291.0, 56.6]}],
        "siege": {"asset": "11111111111111111111111111111111", "gde_ref": "22222222222222222222222222222222",
                  "card_component": 0, "state_component": 2},
        "points": [{"number": 4, "asset": "33333333333333333333333333333333",
           "gde_ref": "08070605040302011111111111111111", "facet": "0a0b0c0d0e0f0001",
           "place": [5902.48, 5428.12, 57.7], "kind": 6, "position": "020d0103",
           "card_component": 0, "position_component": 2, "ownership_component": 1, "point_component": 4}]}]}"#;

    fn signup(character: u128, place: Option<(u8, u8)>) -> WarSignup {
        WarSignup {
            character: Uuid::from_u128(character),
            name: "Testa".to_owned(),
            company: None,
            side: 1,
            gear_score: 700,
            roster: RosterSeat {
                place,
                permission: 0,
            },
            kicked: false,
        }
    }

    fn war(territory: u16) -> TerritoryWar {
        TerritoryWar {
            war: Uuid::from_u128(0x77),
            territory,
            attacker: (Uuid::from_u128(0xa1), 2),
            defender: (Uuid::from_u128(0xd1), 1),
            number: 4714,
            starts_ms: STARTS_MS,
            result: None,
            signups: vec![signup(1, Some((0, 0))), signup(2, None)],
        }
    }

    #[test]
    fn a_war_gets_its_instance_from_the_lead_time_once_for_its_roster() {
        let forts = WarForts::parse(FORTS).expect("forts");
        let mut wars = WarInstances::default();
        let early = STARTS_MS - WAR_INSTANCE_LEAD_MS - 1;
        assert!(wars.due(&[war(8)], &forts, early).is_empty(), "too early");
        let openings = wars.due(&[war(8)], &forts, early + 1);
        assert_eq!(openings.len(), 1);
        assert_eq!(openings[0].members, [Uuid::from_u128(1)], "the roster only");
        assert_eq!(openings[0].war.territory, 8);
        assert_eq!(
            openings[0].ends_at_ms,
            STARTS_MS + WAR_LENGTH_MS + WAR_RESULT_SHOWN_MS
        );
        assert!(wars.due(&[war(8)], &forts, early + 2).is_empty(), "opening");
        wars.opened(Uuid::from_u128(0x77), None);
        assert_eq!(
            wars.due(&[war(8)], &forts, early + 3).len(),
            1,
            "a failed opening again"
        );
        wars.opened(Uuid::from_u128(0x77), Some(Uuid::from_u128(9)));
        assert!(
            wars.due(&[war(8)], &forts, STARTS_MS).is_empty(),
            "it has one"
        );
        wars.sync(std::iter::empty());
        assert_eq!(
            wars.due(&[war(8)], &forts, STARTS_MS).len(),
            1,
            "its instance ended"
        );
    }

    #[test]
    fn a_war_without_a_fort_a_roster_or_time_left_gets_none() {
        let forts = WarForts::parse(FORTS).expect("forts");
        let mut wars = WarInstances::default();
        assert!(
            wars.due(&[war(12)], &forts, STARTS_MS).is_empty(),
            "no capture shows the fort"
        );
        let mut empty = war(8);
        empty.signups.clear();
        assert!(wars.due(&[empty], &forts, STARTS_MS).is_empty());
        let mut won = war(8);
        won.result = Some((Uuid::from_u128(0xa1), STARTS_MS));
        assert!(wars.due(&[won], &forts, STARTS_MS).is_empty());
        assert!(
            wars.due(&[war(8)], &forts, STARTS_MS + WAR_LENGTH_MS)
                .is_empty()
        );
    }

    #[test]
    fn an_invasion_fort_takes_only_an_invasion_from_its_own_lead() {
        let start = FORTS.find("\"points\": [").expect("points");
        let invasion_fort = format!("{}\"points\": []}}]}}", &FORTS[..start]);
        let forts = WarForts::parse(&invasion_fort).expect("forts");
        let mut wars = WarInstances::default();
        assert!(
            wars.due(&[war(8)], &forts, STARTS_MS).is_empty(),
            "no war at an invasion fort"
        );
        let invasion = TerritoryWar {
            attacker: nw_db::NO_ATTACKER,
            ..war(8)
        };
        let early = STARTS_MS - INVASION_INSTANCE_LEAD_MS - 1;
        assert!(
            wars.due(std::slice::from_ref(&invasion), &forts, early)
                .is_empty()
        );
        let due = wars.due(&[invasion], &forts, early + 1);
        assert_eq!(due.len(), 1);
        assert_eq!(
            due[0].ends_at_ms,
            STARTS_MS + nw_db::INVASION_LENGTH_MS + WAR_RESULT_SHOWN_MS
        );
    }
}
