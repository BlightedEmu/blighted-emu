// SPDX-License-Identifier: AGPL-3.0-or-later
//! The store check of `nw-testclient systems --check store`: one bot reads what its account
//! holds from its own entitlement state (3133), as a game client does, buys a store mount
//! (898) and reads the state the server sends after it.
//!
//! The state must hold the bit of the expansion every account has
//! (`Ent_Xpack_RiseOfTheAngryEarth_Owner`) and the balance of the store currency. After the
//! purchase (1285) it must hold the mount's bit too, and the balance must not be higher than
//! before. In a store with prices the account needs Marks of Fortune first:
//! `blighted-admin give-marks <login name> <amount>`.

use std::sync::{Arc, Mutex, PoisonError};

use nw_content::store::EntitlementIndexes;
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::facet::component;
use nw_messages::store::{REQUEST_SUCCESS_TYPE_INDEX, StoreCall, StorePurchase};
use nw_messages::wire::RAW_ID_LEN;
use nw_messages::{EntitlementState, FacetHeader};
use nw_protocol::store_api::STORE_CURRENCY;

use crate::activity::{Act, Proof, STEP_WAIT, Step, step};
use crate::systems::SystemsData;

/// The entitlement every account holds.
const EXPANSION: &str = "Ent_Xpack_RiseOfTheAngryEarth_Owner";
/// The store mount the bot buys, and its retail price in Marks of Fortune.
const MOUNT: &str = "Ent_Mount_MTX_1_wolf";
const MOUNT_PRICE: u32 = 20_000;
/// The locale, currency and country of a retail purchase call.
const LANGUAGE: &[u8] = b"en-US";
const CURRENCY: &[u8] = b"USD";
const COUNTRY: &[u8] = b"US";
/// The fixed word and bytes of a retail purchase call (`mount-menu-buying-all-store-horses`).
const PURCHASE_WORD: u32 = 1;
const PURCHASE_BYTE_B: u8 = 0;
const PURCHASE_BYTE_C: u8 = 1;
/// The request id of the bot's one purchase: the server's answers carry it back.
const REQUEST: [u8; RAW_ID_LEN] = [7; RAW_ID_LEN];

/// The Marks of Fortune of the account's state `state`; `None` when it holds no balance.
fn marks_of(state: &EntitlementState) -> Option<u32> {
    state.balance_of(lowercase_crc32(STORE_CURRENCY))
}

/// The steps of the store check (module text), or what the content lacks.
///
/// # Errors
///
/// Names the entitlement the game data does not hold.
pub fn store_steps(data: &SystemsData<'_>) -> Result<Vec<Step>, String> {
    let indexes = EntitlementIndexes::from_tables(data.activity.tables);
    let bit = |entitlement: &str| {
        indexes
            .index_of(entitlement)
            .ok_or_else(|| format!("the game data has no entitlement {entitlement}"))
    };
    let (expansion, mount) = (bit(EXPANSION)?, bit(MOUNT)?);
    let marks_before: Arc<Mutex<Option<u32>>> = Arc::default();
    let noted = Arc::clone(&marks_before);
    Ok(vec![
        step(
            "the own entitlement state holds the expansion and the Marks of Fortune",
            Vec::new(),
            Proof::Check(
                Box::new(move |sight| {
                    let state = sight.entitlements?;
                    if !state.owns(expansion) {
                        return Some(Err(format!("the state lacks the bit of {EXPANSION}")));
                    }
                    let Some(marks) = marks_of(state) else {
                        return Some(Err(format!("the state has no {STORE_CURRENCY} balance")));
                    };
                    *noted.lock().unwrap_or_else(PoisonError::into_inner) = Some(marks);
                    Some(Ok(()))
                }),
                format!("the bit of {EXPANSION} and a {STORE_CURRENCY} balance"),
            ),
            STEP_WAIT,
        ),
        step(
            format!("buy the store mount {MOUNT}"),
            vec![Act::Call(Box::new(|own| {
                let call = StoreCall::Purchase {
                    header: FacetHeader::of(component::ENTITLEMENT, own),
                    purchase: StorePurchase {
                        language: LANGUAGE.to_vec(),
                        currency: CURRENCY.to_vec(),
                        country: COUNTRY.to_vec(),
                        word_a: PURCHASE_WORD,
                        transaction: REQUEST,
                        offer: MOUNT.as_bytes().to_vec(),
                        offer_again: MOUNT.as_bytes().to_vec(),
                        byte_b: PURCHASE_BYTE_B,
                        product: STORE_CURRENCY.as_bytes().to_vec(),
                        price: MOUNT_PRICE,
                        byte_c: PURCHASE_BYTE_C,
                    },
                };
                Ok((call.type_index(), call.encode()?))
            }))],
            Proof::Message(REQUEST_SUCCESS_TYPE_INDEX),
            STEP_WAIT,
        ),
        step(
            "the own entitlement state holds the mount, and the purchase added no Marks",
            Vec::new(),
            Proof::Check(
                Box::new(move |sight| {
                    let state = sight.entitlements?;
                    if !state.owns(mount) {
                        return None;
                    }
                    let before = *marks_before.lock().unwrap_or_else(PoisonError::into_inner);
                    Some(match (before, marks_of(state)) {
                        (Some(before), Some(after)) if after <= before => Ok(()),
                        (before, after) => Err(format!(
                            "{STORE_CURRENCY}: {before:?} before the purchase, {after:?} after"
                        )),
                    })
                }),
                format!("the bit of {MOUNT} and the {STORE_CURRENCY} balance"),
            ),
            STEP_WAIT,
        ),
    ])
}
