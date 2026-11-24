// SPDX-License-Identifier: AGPL-3.0-or-later
//! The housing check of `nw-testclient systems --check housing`: one bot buys a house, asks for
//! and pays its taxes, goes in, places a piece of furniture, takes it back, leaves and gives
//! the house up, each by the client's own call (`PlayerHousingComponent`,
//! [`nw_messages::TravelClientMsg`]) and proved by the server's answer.
//!
//! The plot is the house plot of the level nearest the first settlement station slot, in that
//! slot's territory: a house is bought in the territory the player is in. The bot gives the
//! house up at the end, and before the purchase too (a run that was cut left its house), so
//! each run buys the house again.

use std::sync::{Arc, Mutex, PoisonError};

use nw_messages::client_input::InteractionRequest;
use nw_messages::entity_ids::facet_of_raw_id;
use nw_messages::travel::{
    HOUSE_INFO_TYPE_INDEX, ItemPlacement, PAY_TAX_FINISHED_TYPE_INDEX,
    PLACE_ITEM_RESULT_TYPE_INDEX, PlotRef, REMOVE_ITEM_RESULT_TYPE_INDEX, TAXES_DUE_TYPE_INDEX,
};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{
    BundleMember, FacetHeader, MemberState, PLAYER_HOUSING_COMPONENT, TravelClientMsg,
    TravelServerMsg,
};
use nw_world::travel_slices::HousingPlot;

use crate::activity::{Act, Proof, STEP_WAIT, Step, gm_done, step, teleport};
use crate::play::entered;
use crate::systems::{SystemsData, bag_count, item_index};

/// The interaction kinds of a house plot (`sim::world::travel::kind`): its purchase screen and
/// the quick entry.
const HOUSING_PURCHASE_KIND: u8 = 57;
const HOUSING_QUICK_ENTER_KIND: u8 = 59;
/// The name a client gives a plot interaction.
const HOUSING_NAME: &[u8] = b"@ui_housing";
/// A piece of furniture of the retail housing captures (the settler's bed).
const FURNITURE: &str = "House_HousingItem_Bed_Settler_Bed02_C";
/// Where retail put that bed, from the house's origin, and its rotation.
const FURNITURE_PLACEMENT: ItemPlacement = ItemPlacement {
    position: [-0.256, -1.217, 5.754],
    rotation: [0.0, 0.0, 0.967, 0.256],
};
/// The result of a furniture call that worked (0 in every retail capture).
const DECORATION_SUCCESS: u8 = 0;
/// The first piece of furniture of a house.
const FIRST_ITEM: u16 = 0;
/// Coin hundredths for the dearest house type (20,000 coins) and its taxes: a game master's
/// coins (live 2026-11-24 06:08:16: a tier 3 house was refused with 1,000 coins).
const HOUSE_COINS: u64 = 5_000_000;
/// The plot reference's third id: its meaning is Unknown, and the server learns it from the
/// call.
const PLOT_EXTRA: u64 = 0;

/// The raw id of plot `plot`.
fn plot_id(plot: &HousingPlot) -> [u8; RAW_ID_LEN] {
    let mut id = [0; RAW_ID_LEN];
    id[..size_of::<u64>()].copy_from_slice(&plot.id.to_le_bytes());
    id
}

/// A housing call of the bot.
fn housing(make: impl Fn(FacetHeader) -> TravelClientMsg + 'static) -> Act {
    Act::Call(Box::new(move |own| {
        let call = make(FacetHeader::of(PLAYER_HOUSING_COMPONENT, own));
        Ok((call.type_index(), call.encode()))
    }))
}

/// An interaction of kind `kind` with the plot of raw id `id`.
fn plot_interaction(id: [u8; RAW_ID_LEN], kind: u8) -> Act {
    Act::Interact {
        request: InteractionRequest {
            target: id,
            target_entity: facet_of_raw_id(&id),
            interaction: 0,
            kind,
            gatherable: None,
        },
        name: HOUSING_NAME.to_vec(),
    }
}

/// The proof of a furniture call: the server's answer of type `answer` (a placement's 3691, a
/// removal's 3046) with the result [`DECORATION_SUCCESS`]; another result fails the step and is
/// named.
fn decorated(answer: u64) -> Proof {
    Proof::Check(
        Box::new(move |sight| {
            sight.messages.iter().find_map(|received| {
                if received.envelope.type_index()? != answer {
                    return None;
                }
                match TravelServerMsg::decode(answer, &received.envelope.body) {
                    Ok(
                        TravelServerMsg::PlaceItemResult { result, .. }
                        | TravelServerMsg::RemoveItemResult { result, .. },
                    ) => Some(if result == DECORATION_SUCCESS {
                        Ok(())
                    } else {
                        Err(format!("the server answered {answer} with result {result}"))
                    }),
                    _ => None,
                }
            })
        }),
        format!("a {answer} answer with result {DECORATION_SUCCESS}"),
    )
}

/// The house plot the check buys and the territory it lies in (module text).
fn plot_of<'a>(data: &SystemsData<'a>) -> Result<(&'a HousingPlot, u16), String> {
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
    let distance = |plot: &HousingPlot| {
        let [x, y, _] = plot.position;
        (x - town.x).hypot(y - town.y)
    };
    let plot = data
        .activity
        .world
        .plots
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .ok_or("the level has no house plot")?;
    Ok((plot, slot.territory))
}

/// The steps of the housing check (module text), or what the content lacks.
///
/// # Errors
///
/// Names the missing plot, settlement or furniture item.
pub fn housing_steps(data: &SystemsData<'_>) -> Result<Vec<Step>, String> {
    let (plot, territory) = plot_of(data)?;
    let furniture = item_index(data, FURNITURE)?;
    let id = plot_id(plot);
    let plot_ref = PlotRef {
        id,
        facet: facet_of_raw_id(&id),
        extra: PLOT_EXTRA,
    };
    let at = plot.position;
    let had = Arc::new(Mutex::new(None));
    let noted = Arc::clone(&had);
    let placed = Arc::clone(&had);
    Ok(vec![
        gm_done(format!("!coins {HOUSE_COINS}")),
        gm_done(format!("!give {FURNITURE} 1")),
        step(
            format!(
                "go to the house plot {:016x} ({}) in territory {territory} and open its purchase screen",
                plot.id, plot.house_type
            ),
            vec![
                teleport(at),
                Act::Call(Box::new(move |own| {
                    let call =
                        entered(FacetHeader::of(crate::play::CALL_COMPONENT, own), territory);
                    Ok((call.type_index(), call.encode()?))
                })),
                plot_interaction(id, HOUSING_PURCHASE_KIND),
            ],
            Proof::Accepted,
            STEP_WAIT,
        ),
        step(
            "give up a house that an earlier, cut run left on the plot (3701)",
            vec![housing(|header| TravelClientMsg::AbandonHome { header })],
            Proof::Sent,
            STEP_WAIT,
        ),
        step(
            "buy the house (3700): the house info comes (3692)",
            vec![housing(|header| TravelClientMsg::BuyHome { header })],
            Proof::Message(HOUSE_INFO_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "ask for the taxes due (3081): the amount comes (3694)",
            vec![housing(move |header| TravelClientMsg::TaxesDue {
                header,
                plot: plot_ref,
            })],
            Proof::Message(TAXES_DUE_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "pay the taxes (3716): the result comes (952)",
            vec![housing(move |header| TravelClientMsg::PayTaxes {
                header,
                plot: plot_ref,
            })],
            Proof::Message(PAY_TAX_FINISHED_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "go into the house by its quick entry",
            vec![
                plot_interaction(id, HOUSING_QUICK_ENTER_KIND),
                housing(move |header| TravelClientMsg::EnterPlot {
                    header,
                    id,
                    facet: plot_ref.facet,
                }),
                Act::Dynamic(Box::new(move |sight| {
                    let count = bag_count(sight.bag, furniture);
                    *noted.lock().unwrap_or_else(PoisonError::into_inner) = Some(count);
                    Some(Ok(Vec::new()))
                })),
            ],
            Proof::Accepted,
            STEP_WAIT,
        ),
        step(
            format!("place the {FURNITURE} (3277): its result comes (3691)"),
            vec![housing(move |header| TravelClientMsg::PlaceItem {
                header,
                item: furniture,
                placement: FURNITURE_PLACEMENT,
                anchor: None,
            })],
            decorated(PLACE_ITEM_RESULT_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "the placed furniture left the bag",
            Vec::new(),
            Proof::Check(
                Box::new(move |sight| {
                    let before = (*placed.lock().unwrap_or_else(PoisonError::into_inner))?;
                    (bag_count(sight.bag, furniture) + 1 == before).then_some(Ok(()))
                }),
                format!("one {FURNITURE} less in the bag"),
            ),
            STEP_WAIT,
        ),
        step(
            "take the furniture back (1435): its result comes (3046)",
            vec![housing(|header| TravelClientMsg::RemoveItem {
                header,
                index: FIRST_ITEM,
                anchor: None,
            })],
            decorated(REMOVE_ITEM_RESULT_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "the furniture is in the bag again",
            Vec::new(),
            Proof::Check(
                Box::new(move |sight| {
                    let before = (*had.lock().unwrap_or_else(PoisonError::into_inner))?;
                    (bag_count(sight.bag, furniture) == before).then_some(Ok(()))
                }),
                format!("as many {FURNITURE} in the bag as before the placement"),
            ),
            STEP_WAIT,
        ),
        step(
            "leave the house (2149) and give it up (3701) at its purchase screen",
            vec![
                housing(move |header| TravelClientMsg::ExitPlot {
                    header,
                    plot: plot_ref,
                }),
                plot_interaction(id, HOUSING_PURCHASE_KIND),
                housing(|header| TravelClientMsg::AbandonHome { header }),
            ],
            Proof::Accepted,
            STEP_WAIT,
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plot_id_is_the_plot_s_number_then_zeros() {
        const ID: u64 = 0x0102_0304_0506_0708;
        let plot = HousingPlot {
            id: ID,
            slice: String::new(),
            position: [0.0; 3],
            house_type: "HouseTypeTier1".to_owned(),
        };
        let id = plot_id(&plot);
        assert_eq!(id[..size_of::<u64>()], ID.to_le_bytes());
        assert_eq!(id[size_of::<u64>()..], [0; size_of::<u64>()]);
        assert_eq!(facet_of_raw_id(&id), ID, "the facet entity of the id");
    }
}
