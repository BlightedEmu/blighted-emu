// SPDX-License-Identifier: AGPL-3.0-or-later
//! The loot tools of the studio (admin studio spec section 7.6): the drop simulator, which rolls a
//! table many times with the server's own roll code over the retail tables and the design files
//! (merged in memory, nothing written), and the mob types that drop nothing.

use std::collections::BTreeMap;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use nw_common::time::unix_ms;
use nw_content::loot::{LootLevels, LootTables};
use nw_design::{Kind, merged_bundle};
use nw_sim_core::Dice;
use serde::{Deserialize, Serialize};

use crate::server::AppState;

/// `POST`: rolls a loot table many times.
pub const SIMULATE_ROUTE: &str = "/api/studio/loot/simulate";
/// `GET`: the mob types with no loot table.
pub const NO_LOOT_ROUTE: &str = "/api/studio/loot/no-loot";
/// Most rolls of one simulation.
pub const MAX_ROLLS: u32 = 100_000;
/// The field of a mob record that names its loot table.
const LOOT_TABLE_FIELD: &str = "loot_table_id";
const BAD_ROLLS: &str = "rolls must be 1 to 100000";
const BUSY: &str = "an Apply or a Check runs";

/// The loot routes; the simulator needs the admin header (the guard) and local mode.
pub fn loot_routes() -> Router<AppState> {
    Router::new()
        .route(SIMULATE_ROUTE, post(simulate))
        .route(NO_LOOT_ROUTE, get(no_loot))
}

/// What to simulate.
#[derive(Debug, Clone, Deserialize)]
pub struct SimulateRequest {
    /// The loot table id.
    pub table: String,
    /// How many rolls.
    pub rolls: u32,
    /// `EnemyLevel`.
    pub enemy_level: u32,
    /// `MinContLevel`.
    pub content_level: u32,
}

/// One item of a simulation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimulatedItem {
    /// The item id.
    pub item: String,
    /// Rolls that dropped it.
    pub rolls_with: u32,
    /// Share of the rolls that dropped it, 0 to 1.
    pub chance: f64,
    /// Items per roll that dropped it.
    pub mean_quantity: f64,
}

/// A simulation: every item that dropped, most likely first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Simulation {
    /// The table.
    pub table: String,
    /// The rolls.
    pub rolls: u32,
    /// The items.
    pub items: Vec<SimulatedItem>,
}

/// Rolls `request.table` of `tables` `request.rolls` times with `dice`; `None` when the table
/// does not exist.
pub fn simulate_rolls(
    tables: &LootTables,
    request: &SimulateRequest,
    dice: &mut Dice,
) -> Option<Simulation> {
    if !tables.contains(&request.table) {
        return None;
    }
    let levels = LootLevels {
        enemy: request.enemy_level,
        content: request.content_level,
    };
    let mut totals: BTreeMap<String, (u32, u64)> = BTreeMap::new();
    for _ in 0..request.rolls {
        for (item, quantity) in tables.roll(&request.table, levels, &mut |below| dice.below(below))
        {
            let entry = totals.entry(item).or_default();
            entry.0 += 1;
            entry.1 += u64::from(quantity);
        }
    }
    let rolls = f64::from(request.rolls.max(1));
    let mut items: Vec<SimulatedItem> = totals
        .into_iter()
        .map(|(item, (rolls_with, quantity))| SimulatedItem {
            item,
            rolls_with,
            chance: f64::from(rolls_with) / rolls,
            mean_quantity: quantity as f64 / f64::from(rolls_with.max(1)),
        })
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.rolls_with));
    Some(Simulation {
        table: request.table.clone(),
        rolls: request.rolls,
        items,
    })
}

async fn simulate(State(state): State<AppState>, Json(request): Json<SimulateRequest>) -> Response {
    let Some(studio) = state.studio.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !(1..=MAX_ROLLS).contains(&request.rolls) {
        return (StatusCode::BAD_REQUEST, BAD_ROLLS).into_response();
    }
    let Ok(permit) = std::sync::Arc::clone(&studio.work).try_acquire_owned() else {
        return (StatusCode::CONFLICT, BUSY).into_response();
    };
    let paths = std::sync::Arc::clone(&studio.paths);
    let build = std::sync::Arc::clone(&studio.client_build);
    let simulated = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let merged = merged_bundle(&paths, &build).map_err(|error| error.to_string())?;
        let tables = LootTables::from_tables(&merged.bundle.tables);
        Ok::<_, String>(simulate_rolls(&tables, &request, &mut Dice::new(unix_ms())))
    })
    .await;
    match simulated {
        Ok(Ok(Some(simulation))) => Json(simulation).into_response(),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "no such loot table").into_response(),
        Ok(Err(error)) => (StatusCode::INTERNAL_SERVER_ERROR, error).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// One mob type that drops nothing.
#[derive(Debug, Serialize)]
struct NoLoot<'a> {
    id: &'a str,
    name: Option<&'a str>,
}

async fn no_loot(State(state): State<AppState>) -> Response {
    let Some(studio) = state.studio.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match studio.catalog().await {
        Ok(catalog) => {
            let mobs: Vec<NoLoot<'_>> = catalog
                .entries(Kind::Mob)
                .iter()
                .filter(|entry| {
                    entry
                        .fields()
                        .get(LOOT_TABLE_FIELD)
                        .and_then(serde_json::Value::as_str)
                        .is_none_or(|table| table.trim().is_empty())
                })
                .map(|entry| NoLoot {
                    id: &entry.id,
                    name: entry.name.as_deref(),
                })
                .collect();
            Json(mobs).into_response()
        }
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use nw_content::loot::{LootRow, LootSource};

    use super::*;

    fn row(
        id: &str,
        cells: &[(u32, &str)],
        and_or: Option<&str>,
        max_roll: Option<i64>,
    ) -> LootRow {
        LootRow {
            id: id.to_owned(),
            and_or: and_or.map(str::to_owned),
            conditions: None,
            max_roll,
            cells: cells
                .iter()
                .map(|(index, cell)| (*index, (*cell).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn a_simulation_gives_each_items_chance() {
        const ROLLS: u32 = 10_000;
        let tables = LootTables::build(LootSource {
            rows: vec![
                row("Chest", &[(1, "Coin"), (2, "Gem")], Some("AND"), None),
                row("Chest_Qty", &[(1, "2-4"), (2, "1")], None, None),
                row("Chest_Probs", &[(1, "0"), (2, "90")], None, Some(100)),
            ],
            bucket_items: Vec::new(),
        });
        let request = SimulateRequest {
            table: "Chest".to_owned(),
            rolls: ROLLS,
            enemy_level: 1,
            content_level: 1,
        };
        let simulation = simulate_rolls(&tables, &request, &mut Dice::new(1)).expect("table");
        assert_eq!(simulation.items[0].item, "Coin");
        assert_eq!(simulation.items[0].rolls_with, ROLLS);
        assert!((simulation.items[0].mean_quantity - 3.0).abs() < 0.05);
        assert!(
            (simulation.items[1].chance - 0.1).abs() < 0.01,
            "{:?}",
            simulation.items[1]
        );
        let missing = SimulateRequest {
            table: "Nope".to_owned(),
            ..request
        };
        assert_eq!(simulate_rolls(&tables, &missing, &mut Dice::new(1)), None);
    }
}
