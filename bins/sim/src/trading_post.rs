// SPDX-License-Identifier: AGPL-3.0-or-later
//! The trading post's rules and its wire form (economy note
//! `local-data/scratch-work/run15/economy/REPORT.md`): the `[trading_post]` section of
//! `config/sim.toml`, the trading post fields of every item (`MasterItemDefinitions`
//! `TradingCategory`, `TradingFamily`, `TradingGroup`, `Tier`, `RequiredLevel`), the posting
//! fee and tax of an order, and an order of the world database as the client's contract.
//!
//! Retail values (Observed): posting durations of 1, 3, 7 and 14 days (every captured order);
//! a trading tax of 5 % at the settlements of three captured characters without a territory
//! standing bonus; at most 40 open orders (`ContractConfigData` default, `FUN_145c173a0`).
//! The retail posting fee table (`ContractConfigData` `Buy/Sell Contract Duration Fee Map`) is
//! in no client pak and its values are Unknown; this server charges the fees the section lists,
//! none by default.

use std::collections::HashMap;

use nw_content::Tables;
use nw_db::{Order, OrderEnd, OrderItem, OrderKind, OrderStatus};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::contracts::{
    CONTRACT_ENTITY_IDS, Contract, ContractItemData, ContractKind, INVALID_ENTITY_ID,
    Participant, ParticipantEvent, reason, status,
};
use nw_messages::item_descriptor::ItemDescriptor;
use serde::Deserialize;
use uuid::Uuid;

use crate::config::SimConfigError;

/// Nanoseconds per millisecond: contract times are Unix nanoseconds.
const NS_PER_MS: u64 = 1_000_000;
/// Milliseconds per day: posting durations are whole days.
pub const MS_PER_DAY: u64 = 86_400_000;
/// The posting durations retail offers, in days.
pub const RETAIL_DURATIONS_DAYS: [u32; 4] = [1, 3, 7, 14];
/// Most open orders of one character (`ContractConfigData` `Base Number of Buy/Sell
/// Contracts`, constructor default).
pub const RETAIL_MAX_OPEN_ORDERS: u32 = 40;
/// The trading tax of a settlement without its own (Observed: 5 % at the settlements of three
/// retail characters without a standing bonus).
pub const RETAIL_TRADING_TAX: f64 = 0.05;
/// Default time between two takes of the deliveries of a sim's players (engineering: a sale
/// on another sim shows within a few seconds).
pub const DEFAULT_DELIVERY_POLL_MS: u64 = 2_000;
/// The wire required level of an item without one.
const NO_REQUIRED_LEVEL: u32 = u32::MAX;
/// The `BindOnPickup` value of an item that binds when taken: such an item cannot be traded.
const BINDS_ON_PICKUP: i64 = 1;

/// Which orders a posting fee is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeeKind {
    /// Buy orders.
    Buy,
    /// Sell orders.
    Sell,
}

/// The posting fee of one duration: a flat part and a share of the order's total.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostingFee {
    /// Buy or sell orders.
    pub kind: FeeKind,
    /// The duration, in days.
    pub days: u32,
    /// The flat part, hundredths of a coin.
    pub flat: u64,
    /// The share of the total, 0 to below 1.
    pub share: f64,
}

/// A settlement with its own trading tax.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutpostTax {
    /// The outpost id.
    pub outpost: u64,
    /// The tax, 0 to below 1.
    pub trading_tax: f64,
}

/// `[trading_post]` of `config/sim.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TradingPostConfig {
    /// The posting durations a player may pick, in days.
    pub durations_days: Vec<u32>,
    /// Most open orders of one character.
    pub max_open_orders: u32,
    /// The trading tax of a settlement without its own.
    pub trading_tax: f64,
    /// Settlements with their own tax.
    pub outposts: Vec<OutpostTax>,
    /// The posting fees; a duration without one costs nothing to post.
    pub posting_fees: Vec<PostingFee>,
    /// Time between two takes of the deliveries of this sim's players.
    pub delivery_poll_ms: u64,
}

impl Default for TradingPostConfig {
    fn default() -> Self {
        Self {
            durations_days: RETAIL_DURATIONS_DAYS.to_vec(),
            max_open_orders: RETAIL_MAX_OPEN_ORDERS,
            trading_tax: RETAIL_TRADING_TAX,
            outposts: Vec::new(),
            posting_fees: Vec::new(),
            delivery_poll_ms: DEFAULT_DELIVERY_POLL_MS,
        }
    }
}

impl TradingPostConfig {
    /// Checks the section.
    ///
    /// # Errors
    ///
    /// Returns [`SimConfigError::Invalid`] for a rate outside 0 to below 1, no duration, a zero
    /// duration, no open order allowed or a zero poll interval.
    pub fn validate(&self) -> Result<(), SimConfigError> {
        let invalid = |key, reason| Err(SimConfigError::Invalid { key, reason });
        let rate = |value: f64| value.is_finite() && (0.0..1.0).contains(&value);
        if self.durations_days.is_empty() || self.durations_days.contains(&0) {
            return invalid(
                "trading_post.durations_days",
                "must list at least one duration, each above 0",
            );
        }
        if self.max_open_orders == 0 {
            return invalid("trading_post.max_open_orders", "must be above 0");
        }
        if !rate(self.trading_tax) || self.outposts.iter().any(|tax| !rate(tax.trading_tax)) {
            return invalid("trading_post.trading_tax", "must be at least 0 and below 1");
        }
        if self.posting_fees.iter().any(|fee| !rate(fee.share)) {
            return invalid("trading_post.posting_fees", "shares must be at least 0 and below 1");
        }
        if self.delivery_poll_ms == 0 {
            return invalid("trading_post.delivery_poll_ms", "must be above 0");
        }
        Ok(())
    }

    /// Whether a player may post for `days`.
    pub fn allows(&self, days: u32) -> bool {
        self.durations_days.contains(&days)
    }

    /// The trading tax of `outpost`.
    pub fn tax_at(&self, outpost: u64) -> f64 {
        self.outposts
            .iter()
            .find(|tax| tax.outpost == outpost)
            .map_or(self.trading_tax, |tax| tax.trading_tax)
    }

    /// The posting fee of an order of `kind` for `days` worth `total`, hundredths; `None` when
    /// it does not fit the coin range.
    pub fn posting_fee(&self, kind: OrderKind, days: u32, total: u64) -> Option<u64> {
        let wanted = match kind {
            OrderKind::Buy => FeeKind::Buy,
            OrderKind::Sell => FeeKind::Sell,
        };
        let Some(fee) = self
            .posting_fees
            .iter()
            .find(|fee| fee.kind == wanted && fee.days == days)
        else {
            return Some(0);
        };
        fee.flat.checked_add(nw_db::tax_of(total, fee.share))
    }
}

/// The trading post fields of one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeItem {
    /// `ItemID`.
    pub item_id: String,
    /// `TradingCategory`.
    pub category: String,
    /// `TradingFamily`.
    pub family: String,
    /// `TradingGroup`.
    pub group: String,
    /// `Tier`.
    pub tier: u32,
    /// `RequiredLevel`; `None` for none.
    pub required_level: Option<u32>,
}

impl TradeItem {
    /// The order item of this item at `gear_score`.
    pub fn order_item(&self, gear_score: u32) -> OrderItem {
        OrderItem {
            item_id: self.item_id.clone(),
            item_crc: lowercase_crc32(&self.item_id),
            category: self.category.clone(),
            family: self.family.clone(),
            group: self.group.clone(),
            tier: self.tier,
            gear_score,
            required_level: self.required_level,
            perks: Vec::new(),
            gem_perk_count: 0,
        }
    }
}

/// The items that may be traded, by the CRC-32 of their lowercase id: those with a trading
/// category that do not bind when taken.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TradingCatalog {
    items: HashMap<u32, TradeItem>,
}

impl TradingCatalog {
    /// The tradable items of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let items = tables
            .master_item_definitions
            .iter()
            .flat_map(|table| &table.rows)
            .filter(|row| row.bind_on_pickup != Some(BINDS_ON_PICKUP))
            .filter_map(|row| {
                let category = row.trading_category.clone().filter(|text| !text.is_empty())?;
                Some((
                    lowercase_crc32(&row.item_id),
                    TradeItem {
                        item_id: row.item_id.clone(),
                        category,
                        family: row.trading_family.clone().unwrap_or_default(),
                        group: row.trading_group.clone().unwrap_or_default(),
                        tier: row
                            .tier
                            .and_then(|tier| u32::try_from(tier).ok())
                            .unwrap_or_default(),
                        required_level: row
                            .required_level
                            .and_then(|level| u32::try_from(level).ok()),
                    },
                ))
            })
            .collect();
        Self { items }
    }

    /// The catalog of `items`.
    #[cfg(test)]
    pub fn of(items: impl IntoIterator<Item = TradeItem>) -> Self {
        Self {
            items: items
                .into_iter()
                .map(|item| (lowercase_crc32(&item.item_id), item))
                .collect(),
        }
    }

    /// How many items may be traded.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether no item may be traded (content without item definitions).
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The tradable item whose lowercase id has the CRC-32 `crc`.
    pub fn by_crc(&self, crc: u32) -> Option<&TradeItem> {
        self.items.get(&crc)
    }

    /// The tradable item `item_id` (any case).
    pub fn by_id(&self, item_id: &str) -> Option<&TradeItem> {
        self.by_crc(lowercase_crc32(item_id))
    }
}

/// The contract kind of an order kind.
pub const fn contract_kind(kind: OrderKind) -> ContractKind {
    match kind {
        OrderKind::Buy => ContractKind::Buy,
        OrderKind::Sell => ContractKind::Sell,
    }
}

/// The order kind of a contract kind.
pub const fn order_kind(kind: ContractKind) -> OrderKind {
    match kind {
        ContractKind::Buy => OrderKind::Buy,
        ContractKind::Sell => OrderKind::Sell,
    }
}

const fn status_value(status: OrderStatus) -> u32 {
    match status {
        OrderStatus::Available => status::AVAILABLE,
        OrderStatus::Completed => status::COMPLETED,
        OrderStatus::Resolved => status::RESOLVED,
    }
}

fn ns(ms: u64) -> u64 {
    ms.saturating_mul(NS_PER_MS)
}

/// `order` as the contract that character `viewer` sees: its own orders carry their end in
/// their event (retail: the event's reason on the creator's lists, none on searches) and the
/// local creator flag.
pub fn contract_of(order: &Order, viewer: Uuid) -> Contract {
    let mine = order.creator == viewer;
    let event_reason = if mine {
        order.end.value()
    } else {
        OrderEnd::None.value()
    };
    let perk_count = u32::try_from(order.item.perks.len()).unwrap_or(u32::MAX);
    let gear_score = u16::try_from(order.item.gear_score).unwrap_or(u16::MAX);
    Contract {
        kind: contract_kind(order.kind),
        id: *order.id.as_bytes(),
        version: order.version,
        status: status_value(order.status),
        created_ns: ns(order.created_ms),
        completed_ns: order.completed_ms.map_or(0, ns),
        updated_ns: ns(order.updated_ms),
        duration_ns: ns(order.expires_ms.saturating_sub(order.created_ms)),
        visibility: Participant::NONE,
        local_creator: mine,
        events: vec![(
            Participant::NONE,
            ParticipantEvent::of(event_reason, order.posted),
        )],
        item: ContractItemData {
            category: order.item.category.as_bytes().to_vec(),
            family: order.item.family.as_bytes().to_vec(),
            group: order.item.group.as_bytes().to_vec(),
            name: order.item.item_id.as_bytes().to_vec(),
            gear_score: order.item.gear_score,
            tier: order.item.tier,
            perks: order.item.perks.clone(),
            perk_attributes: Vec::new(),
            rarity_level: 0,
            required_level: order.item.required_level.unwrap_or(NO_REQUIRED_LEVEL),
            perk_count,
            gem_perk_count: order.item.gem_perk_count,
            instance: [0; nw_messages::wire::RAW_ID_LEN],
        },
        entity_ids: [INVALID_ENTITY_ID; CONTRACT_ENTITY_IDS],
        invalid_participant: Participant::NONE,
        invalid_event: ParticipantEvent::invalid(),
        invalid_reason: reason::NONE,
        outpost: order.outpost,
        reserved: 0,
        price: order.price,
        quantity: order.open,
        sold_item: (order.kind == OrderKind::Sell).then(|| ItemDescriptor {
            gear_score,
            perks: order.item.perks.clone(),
            ..ItemDescriptor::of(order.item.item_crc, order.open)
        }),
    }
}

/// The next-page token of a search answer after `last` (retail: base64 of
/// `[price, expiry ms, "cn-<contract id>"]`).
pub fn next_page_token(last: &Order) -> Vec<u8> {
    let text = format!(
        "[{}, {}, \"cn-{}\"]",
        last.price,
        last.expires_ms,
        last.id.simple().to_string().to_uppercase()
    );
    data_encoding::BASE64.encode(text.as_bytes()).into_bytes()
}

/// The coins and items of `units` of order `order` that a completer pays and brings: a sell
/// order's buyer pays the total and its tax ([`nw_db::tax_of`] at the order's rate); a buy
/// order's seller brings the units.
pub fn completion_cost(order: &Order, units: u32) -> Option<(u64, Option<(String, u32)>)> {
    let total = order.price.checked_mul(u64::from(units))?;
    Some(match order.kind {
        OrderKind::Sell => (total.checked_add(nw_db::tax_of(total, order.tax_rate))?, None),
        OrderKind::Buy => (0, Some((order.item.item_id.clone(), units))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_messages::wire::{Reader, Writer};

    const NOW_MS: u64 = 1_781_457_196_968;

    fn beeswax() -> TradeItem {
        TradeItem {
            item_id: "BeeswaxT1".to_owned(),
            category: "Resources".to_owned(),
            family: "RawResources".to_owned(),
            group: "Beeswax".to_owned(),
            tier: 1,
            required_level: None,
        }
    }

    fn order(kind: OrderKind, creator: Uuid) -> Order {
        Order {
            id: Uuid::from_u128(0x8233_f5b0_d351_42a3_b433_1068_4f88_a957),
            kind,
            status: OrderStatus::Available,
            version: 1,
            creator,
            outpost: 0xb325_909b_3cc6_434f,
            item: beeswax().order_item(0),
            price: 10_000,
            posted: 1,
            open: 1,
            end: OrderEnd::None,
            tax_rate: RETAIL_TRADING_TAX,
            created_ms: NOW_MS,
            expires_ms: NOW_MS + 3 * MS_PER_DAY,
            completed_ms: None,
            updated_ms: NOW_MS,
        }
    }

    /// The retail one-result search answer's buy order (`buy-sell-TP` 358458 ms), as another
    /// player sees it: the same fields this server sends, from the item data and the order.
    #[test]
    fn an_order_is_the_retail_contract_form() {
        let contract = contract_of(&order(OrderKind::Buy, Uuid::from_u128(1)), Uuid::from_u128(2));
        assert_eq!(contract.kind, ContractKind::Buy);
        assert!(!contract.local_creator);
        assert_eq!(contract.duration_ns, 3 * MS_PER_DAY * NS_PER_MS);
        assert_eq!(contract.item.category, b"Resources");
        assert_eq!(contract.item.required_level, NO_REQUIRED_LEVEL);
        assert_eq!(contract.events[0].1.reason, reason::NONE);
        assert_eq!(contract.events[0].1.completion_count, 1);
        assert!(contract.sold_item.is_none());
        let mut writer = Writer::new();
        contract.write(&mut writer).expect("write");
        let bytes = writer.finish();
        assert_eq!(Contract::read(&mut Reader::new(&bytes)), Ok(contract));
    }

    #[test]
    fn a_sell_order_carries_its_units_and_its_creator_sees_its_end() {
        let creator = Uuid::from_u128(1);
        let mut sold = order(OrderKind::Sell, creator);
        sold.end = OrderEnd::Canceled;
        sold.status = OrderStatus::Resolved;
        let contract = contract_of(&sold, creator);
        assert!(contract.local_creator);
        assert_eq!(contract.status, status::RESOLVED);
        assert_eq!(contract.events[0].1.reason, reason::CANCELED);
        let item = contract.sold_item.expect("a sell order's item");
        assert_eq!((item.item_crc, item.quantity), (0xd0a8_26e7, 1));
    }

    #[test]
    fn the_retail_rules_are_the_defaults() {
        let config = TradingPostConfig::default();
        assert!(config.validate().is_ok());
        assert!(config.allows(14) && !config.allows(2));
        assert_eq!(config.tax_at(1), RETAIL_TRADING_TAX);
        assert_eq!(config.posting_fee(OrderKind::Sell, 3, 5_250), Some(0));
        let priced = TradingPostConfig {
            posting_fees: vec![PostingFee {
                kind: FeeKind::Sell,
                days: 3,
                flat: 100,
                share: 0.02,
            }],
            outposts: vec![OutpostTax {
                outpost: 7,
                trading_tax: 0.0235,
            }],
            ..TradingPostConfig::default()
        };
        assert_eq!(priced.posting_fee(OrderKind::Sell, 3, 5_250), Some(205));
        assert_eq!(priced.posting_fee(OrderKind::Buy, 3, 5_250), Some(0));
        assert_eq!(priced.tax_at(7), 0.0235);
        let broken = TradingPostConfig {
            trading_tax: 1.0,
            ..TradingPostConfig::default()
        };
        assert!(broken.validate().is_err());
    }

    #[test]
    fn a_buyer_pays_the_total_and_its_tax() {
        let sell = order(OrderKind::Sell, Uuid::from_u128(1));
        assert_eq!(completion_cost(&sell, 1), Some((10_500, None)));
        let buy = order(OrderKind::Buy, Uuid::from_u128(1));
        assert_eq!(
            completion_cost(&buy, 1),
            Some((0, Some(("BeeswaxT1".to_owned(), 1))))
        );
    }

    #[test]
    fn the_page_token_has_the_retail_form() {
        let token = next_page_token(&order(OrderKind::Sell, Uuid::from_u128(1)));
        let text = data_encoding::BASE64.decode(&token).expect("base64");
        assert_eq!(
            String::from_utf8(text).expect("text"),
            format!(
                "[10000, {}, \"cn-8233F5B0D35142A3B43310684F88A957\"]",
                NOW_MS + 3 * MS_PER_DAY
            )
        );
    }

    #[test]
    fn the_catalog_finds_an_item_by_its_crc() {
        let catalog = TradingCatalog::of([beeswax()]);
        assert_eq!(
            catalog.by_crc(0xd0a8_26e7).map(|item| item.item_id.as_str()),
            Some("BeeswaxT1")
        );
        assert!(catalog.by_id("beeswaxt1").is_some());
        assert!(catalog.by_crc(1).is_none());
    }
}
