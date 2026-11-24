// SPDX-License-Identifier: AGPL-3.0-or-later
//! The faction board check of `nw-testclient systems --check faction-board`: one bot of a
//! faction opens the faction board in a settlement, reads the missions it offers from its own
//! objective interactor state (3829), as a client's board screen does, and takes the first one
//! (3816: its index in the list), then a PvP mission of the list (3818: the CRC-32 of its id).
//! The server sends each taken element again, taken and no longer available
//! (`sim::world::faction_board`, retail `Hivefactionquests` 40.974 s).
//!
//! The mission the bot took is named in the step's proof line; `nw-testclient play --quests
//! <mission id>` plays it as a quest.
//!
//! **Inferred**: the board's name. A client names the faction representative NPC it used
//! (`@...`); the server takes any NPC name that is not the town project board's.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use nw_formats::objectstream::lowercase_crc32;
use nw_messages::client_input::{InteractionRequest, TOWN_BOARD_KIND};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{BundleMember, FacetHeader, MemberState, ObjectiveInteractorClientMsg};

use crate::activity::{Act, BOT_FACTION, Proof, STEP_WAIT, Step, faction_step, step, teleport};
use crate::play::{CALL_COMPONENT, entered};
use crate::systems::SystemsData;

/// The name the bot gives the faction board: a faction representative NPC (module text).
const FACTION_BOARD_NAME: &[u8] = b"@faction_representative";
/// The mission the bot takes: the first of the list (retail took index 0 to 2).
const FIRST_OFFER: u8 = 0;
/// What the id of a PvP faction mission holds (retail `FM_06_PVP_*`, `FM_10_PVP_*`).
const PVP_MARK: &str = "_PVP_";

/// The steps of the faction board check (module text), or what the content lacks.
///
/// # Errors
///
/// Names the missing settlement.
pub fn faction_board_steps(data: &SystemsData<'_>) -> Result<Vec<Step>, String> {
    let slot = data
        .activity
        .town_stations
        .slots()
        .first()
        .ok_or("the content has no settlement station slot")?;
    let town = BundleMember::read_from(&slot.place)
        .ok()
        .and_then(|(member, _)| match member.state {
            MemberState::Position(state) => state.position,
            _ => None,
        })
        .ok_or("the settlement station's place does not read")?;
    let territory = slot.territory;
    let names: Arc<HashMap<u32, String>> = Arc::new(
        data.activity
            .tables
            .mission_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (lowercase_crc32(&row.mission_id), row.mission_id.clone()))
            .collect(),
    );
    let offered = Arc::new(Mutex::new(None));
    let noted = Arc::clone(&offered);
    let taken_names = Arc::clone(&names);
    let pvp_names = Arc::clone(&names);
    let pvp_taken_names = Arc::clone(&names);
    let pvp_offered = Arc::new(Mutex::new(None));
    let pvp_noted = Arc::clone(&pvp_offered);
    let pvp_asked = Arc::clone(&pvp_offered);
    Ok(vec![
        faction_step(BOT_FACTION),
        step(
            format!("go to the settlement of territory {territory} and open its faction board"),
            vec![
                teleport([town.x, town.y, town.z()]),
                Act::Call(Box::new(move |own| {
                    let call = entered(FacetHeader::of(CALL_COMPONENT, own), territory);
                    Ok((call.type_index(), call.encode()?))
                })),
                Act::Interact {
                    request: InteractionRequest {
                        target: [0; RAW_ID_LEN],
                        target_entity: 0,
                        interaction: 0,
                        kind: TOWN_BOARD_KIND,
                        gatherable: None,
                    },
                    name: FACTION_BOARD_NAME.to_vec(),
                },
            ],
            Proof::Accepted,
            STEP_WAIT,
        ),
        step(
            "the board offers missions of the bot's faction (the own 3829 list)",
            Vec::new(),
            Proof::Check(
                Box::new(move |sight| {
                    let first = sight.board.first()?;
                    if !first.available {
                        return Some(Err(
                            "the first mission of the board is not offered".to_owned()
                        ));
                    }
                    *noted.lock().unwrap_or_else(PoisonError::into_inner) = Some(first.mission);
                    Some(Ok(()))
                }),
                "an offered mission in the own board list".to_owned(),
            ),
            STEP_WAIT,
        ),
        step(
            "take the first mission (3816): its element comes again, taken",
            vec![Act::Call(Box::new(|own| {
                let call = ObjectiveInteractorClientMsg::SelectObjective {
                    header: FacetHeader::of(CALL_COMPONENT, own),
                    index: FIRST_OFFER,
                };
                Ok((call.type_index(), call.encode()))
            }))],
            Proof::Check(
                Box::new(move |sight| {
                    let wanted = (*offered.lock().unwrap_or_else(PoisonError::into_inner))?;
                    let taken = sight
                        .board
                        .iter()
                        .find(|mission| mission.mission == wanted && !mission.available)?;
                    let name = taken_names
                        .get(&wanted)
                        .cloned()
                        .unwrap_or_else(|| format!("{wanted:08x}"));
                    // The name goes to the bot's log: the play run that follows names it.
                    println!(
                        "faction mission taken: {name} instance {:016x}",
                        taken.instance
                    );
                    (taken.taken_at_ns != 0).then_some(Ok(()))
                }),
                "the first mission taken (not available, with its take time)".to_owned(),
            ),
            STEP_WAIT,
        ),
        step(
            "the board offers a PvP mission too",
            Vec::new(),
            Proof::Check(
                Box::new(move |sight| {
                    let mission = sight.board.iter().find(|mission| {
                        mission.available
                            && pvp_names
                                .get(&mission.mission)
                                .is_some_and(|name| name.to_uppercase().contains(PVP_MARK))
                    })?;
                    *pvp_noted.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some(mission.mission);
                    Some(Ok(()))
                }),
                format!("an offered mission whose id holds {PVP_MARK}"),
            ),
            STEP_WAIT,
        ),
        step(
            "take that PvP mission (3818: the CRC-32 of its id): its element comes again, taken",
            vec![Act::Dynamic(Box::new(move |sight| {
                let mission = (*pvp_asked.lock().unwrap_or_else(PoisonError::into_inner))?;
                let call = ObjectiveInteractorClientMsg::SelectPvpMission {
                    header: FacetHeader::of(CALL_COMPONENT, sight.own),
                    mission,
                };
                Some(Ok(vec![(call.type_index(), call.encode())]))
            }))],
            Proof::Check(
                Box::new(move |sight| {
                    let wanted = (*pvp_offered.lock().unwrap_or_else(PoisonError::into_inner))?;
                    let taken = sight
                        .board
                        .iter()
                        .find(|mission| mission.mission == wanted && !mission.available)?;
                    let name = pvp_taken_names
                        .get(&wanted)
                        .cloned()
                        .unwrap_or_else(|| format!("{wanted:08x}"));
                    println!(
                        "faction mission taken: {name} instance {:016x}",
                        taken.instance
                    );
                    (taken.taken_at_ns != 0).then_some(Ok(()))
                }),
                "the PvP mission taken (not available, with its take time)".to_owned(),
            ),
            STEP_WAIT,
        ),
    ])
}
