// SPDX-License-Identifier: AGPL-3.0-or-later
//! The store table ([`nw_protocol::store_api`]) from the game data: every enabled entitlement of
//! `javelindata_entitlements` with `RewardType` `Mount` and `SourceType` `Store` (52 in build
//! 25810808; its `Reward(s)` is the `MountId`) that has a store product, and the entitlements they
//! require (`RequiredEntitlements`: `Ent_Xpack_RiseOfTheAngryEarth_Owner` for 50 of them), which
//! every account holds on this server (Inferred: every retail player who rode a mount owned the
//! expansion).
//!
//! A mount's store product is the `javelindata_storeproducts` row with the mount's `DisplayName`
//! (Observed: 31 of 31 retail mount offers, `unlock_store_content` capture: `Mount_Wolf_1` for
//! `Ent_Mount_MTX_1_wolf`). Two rows with that name: the one whose `UniqueTagID` holds the mount's
//! variant, the last part of its `MountId` (`Mount_Lion_Reaper` for `Mount_MTX_8_lion_Reaper`).
//! A mount without a product is not sold: the client leaves out an offer whose alias names no
//! product (`entitlementsdatahandler.luac`: "No StoreProductData in JavelinData_StoreProducts.csv
//! found"). Build 25810808 sells 47 of the 52.
//!
//! The store also sells the consumables of [`SOLD_TOKENS`], those this server can honour: the
//! barbershop token, which the barber spends (6420), and the boost tokens whose status effects
//! the sim applies (the 5818 debit starts the effect, [`EffectTokens`]): gathering experience,
//! weapon experience, territory standing and gathering yield. Not sold: the season experience
//! boost (the sim has no season pass), the premium emotes (it has no emote system), the dyes
//! and the season pass tiers (Observed, catalog of `buy_barbertoken_and_change_style`).

use std::collections::{BTreeMap, HashMap};

use nw_protocol::store_api::{StoreEntitlement, StoreMount, StoreTable, StoreToken};

use crate::bundle::english;
use crate::generated::{EntitlementData, StoreProductData, Tables};

/// `RewardType` of a mount entitlement.
const MOUNT_REWARD: &str = "Mount";
/// `SourceType` of a store entitlement.
const STORE_SOURCE: &str = "Store";
/// Between the entitlements of a `RequiredEntitlements` cell (Inferred: every cell of build
/// 25810808 holds one).
const REQUIRED_SEPARATOR: char = ',';
/// Between the parts of a `MountId`; the last part is the mount's variant.
const MOUNT_ID_SEPARATOR: char = '_';

/// One consumable the store sells.
struct SoldToken {
    /// The entitlement: its `UniqueTagID`.
    entitlement: &'static str,
    /// Its `RewardType`: what spends it.
    reward_type: &'static str,
    /// The entitlement's name (its row has no `DisplayName`).
    name: &'static str,
    /// The store product: its `UniqueTagID`.
    product: &'static str,
    /// Tokens per purchase.
    amount: u32,
    /// Listed price in Marks of Fortune.
    list_price: u32,
}

/// The `RewardType` of an entitlement that starts a status effect (its `Reward(s)`).
const STATUS_EFFECT_REWARD: &str = "StatusEffect";
/// Boost tokens per retail purchase (Observed: each boost offer of the catalog holds 5).
const BOOST_TOKENS: u32 = 5;
/// The listed price of a boost token offer (Observed: `originalPrice` 5000 on each).
const BOOST_PRICE: u32 = 5_000;

/// The consumables the store sells. The barbershop token: Observed, retail catalog offer
/// `StoreItem_Token_Barbershop_X1` (`buy_barbertoken_and_change_style`): one `Consumable`
/// `Ent_Barbershop_Token` named `Barbershop Token`, `originalPrice` 10000, sold for 0; the
/// purchase raised the count from 14 to 15. The boosts: Observed, the same catalog's offers
/// `Token_GatheringXP_Boost`, `Token_WeaponXP_Boost`, `Token_TerritoryStanding_Boost` and
/// `Token_Proficiency_Boost`, each 5 of its `Ent_StatusEffect_Token_*` named as here, listed at
/// 5000.
const SOLD_TOKENS: [SoldToken; 5] = [
    SoldToken {
        entitlement: "Ent_Barbershop_Token",
        reward_type: "BarbershopToken",
        name: "Barbershop Token",
        product: "StoreItem_Token_Barbershop_X1",
        amount: 1,
        list_price: 10_000,
    },
    SoldToken {
        entitlement: "Ent_StatusEffect_Token_Gather_1",
        reward_type: STATUS_EFFECT_REWARD,
        name: "Gathering XP Boost Token",
        product: "Token_GatheringXP_Boost",
        amount: BOOST_TOKENS,
        list_price: BOOST_PRICE,
    },
    SoldToken {
        entitlement: "Ent_StatusEffect_Token_Weapon_1",
        reward_type: STATUS_EFFECT_REWARD,
        name: "Weapon XP Boost Token",
        product: "Token_WeaponXP_Boost",
        amount: BOOST_TOKENS,
        list_price: BOOST_PRICE,
    },
    SoldToken {
        entitlement: "Ent_StatusEffect_Token_Territory_1",
        reward_type: STATUS_EFFECT_REWARD,
        name: "Territory Standing Boost Token",
        product: "Token_TerritoryStanding_Boost",
        amount: BOOST_TOKENS,
        list_price: BOOST_PRICE,
    },
    SoldToken {
        entitlement: "Ent_StatusEffect_Token_Proficiency_1",
        reward_type: STATUS_EFFECT_REWARD,
        name: "Proficiency Boost Token",
        product: "Token_Proficiency_Boost",
        amount: BOOST_TOKENS,
        list_price: BOOST_PRICE,
    },
];

/// The status effect each enabled consumable entitlement of `RewardType` `StatusEffect` starts
/// when the 5818 `RequestTokenDebit` spends it: its `Reward(s)`, a `StatusID` (build 25810808:
/// the five `Ent_StatusEffect_Token_*` boosts and the premium emote timers; Observed,
/// `Playthrough_Part_02`: each debit of `Ent_StatusEffect_Token_Gather_1` and the other boosts
/// was followed by its `Token_*` effect).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectTokens {
    /// The `StatusID` by lowercase entitlement.
    by_entitlement: HashMap<String, String>,
}

impl EffectTokens {
    /// The tokens of the entitlement rows of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::build(
            tables
                .entitlement_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter(|row| {
                    row.is_enabled
                        && row.is_consumable == Some(true)
                        && row.reward_type.as_deref() == Some(STATUS_EFFECT_REWARD)
                })
                .filter_map(|row| {
                    let effect = row.reward_s.as_deref()?.trim();
                    (!effect.is_empty()).then_some((row.unique_tag_id.as_str(), effect))
                }),
        )
    }

    /// The tokens of the (entitlement, `StatusID`) pairs `pairs`.
    pub fn build<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self {
            by_entitlement: pairs
                .into_iter()
                .map(|(entitlement, effect)| (entitlement.to_lowercase(), effect.to_owned()))
                .collect(),
        }
    }

    /// The `StatusID` the token `entitlement` (any case) starts; `None` for another entitlement.
    pub fn effect_of(&self, entitlement: &str) -> Option<&str> {
        self.by_entitlement
            .get(&entitlement.to_lowercase())
            .map(String::as_str)
    }
}

/// The store products by lowercase `DisplayName`: the `UniqueTagID` of each.
#[derive(Debug, Default)]
pub struct StoreProducts {
    by_name: HashMap<String, Vec<String>>,
}

impl StoreProducts {
    /// The products of `products`: (`UniqueTagID`, `DisplayName`).
    pub fn build<'a>(products: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for (product, name) in products {
            by_name
                .entry(name.to_ascii_lowercase())
                .or_default()
                .push(product.to_owned());
        }
        Self { by_name }
    }

    /// The product of mount `mount` named `name`; `None` without one, or with two that its
    /// variant does not tell apart.
    fn of(&self, mount: &str, name: &str) -> Option<&str> {
        let named = self.by_name.get(&name.to_ascii_lowercase())?;
        if let [product] = named.as_slice() {
            return Some(product);
        }
        let variant = mount
            .rsplit(MOUNT_ID_SEPARATOR)
            .next()?
            .to_ascii_lowercase();
        let mut matching = named
            .iter()
            .filter(|product| product.to_ascii_lowercase().contains(&variant));
        match (matching.next(), matching.next()) {
            (Some(product), None) => Some(product),
            _ => None,
        }
    }
}

/// The store table of `tables`, with the English names of `text`.
pub fn store_table(tables: &Tables, text: &BTreeMap<String, String>) -> StoreTable {
    let rows: Vec<&EntitlementData> = tables
        .entitlement_data
        .iter()
        .flat_map(|table| &table.rows)
        .collect();
    let mount_names: HashMap<String, &str> = tables
        .mount_data
        .iter()
        .flat_map(|table| &table.rows)
        .map(|row| (row.mount_id.to_ascii_lowercase(), row.display_name.as_str()))
        .collect();
    let products = StoreProducts::build(
        tables
            .store_product_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| (row.unique_tag_id.as_str(), row.display_name.as_str())),
    );
    let product_rows: Vec<&StoreProductData> = tables
        .store_product_data
        .iter()
        .flat_map(|table| &table.rows)
        .collect();
    StoreTable {
        tokens: sold_tokens(&rows, &product_rows, text),
        ..store_table_of(&rows, text, &mount_names, &products)
    }
}

/// The tokens of [`SOLD_TOKENS`] that the game data has: an enabled consumable entitlement of
/// that reward type, and an enabled product, with its English name from `text`.
fn sold_tokens(
    rows: &[&EntitlementData],
    products: &[&StoreProductData],
    text: &BTreeMap<String, String>,
) -> Vec<StoreToken> {
    SOLD_TOKENS
        .iter()
        .filter(|sold| {
            rows.iter().any(|row| {
                row.unique_tag_id == sold.entitlement
                    && row.is_enabled
                    && row.is_consumable == Some(true)
                    && row.reward_type.as_deref() == Some(sold.reward_type)
            })
        })
        .filter_map(|sold| {
            let product = products
                .iter()
                .find(|row| row.unique_tag_id == sold.product && row.is_enabled != 0)?;
            Some(StoreToken {
                token: StoreEntitlement {
                    entitlement: sold.entitlement.to_owned(),
                    name: sold.name.to_owned(),
                },
                product: sold.product.to_owned(),
                product_name: english(text, &product.display_name)
                    .unwrap_or(sold.product)
                    .to_owned(),
                amount: sold.amount,
                list_price: sold.list_price,
            })
        })
        .collect()
}

/// The store table of the entitlement rows `rows`, with the English names of `text`, the
/// `DisplayName` of each mount by lowercase `MountId` (`mount_names`) and the store products.
pub fn store_table_of(
    rows: &[&EntitlementData],
    text: &BTreeMap<String, String>,
    mount_names: &HashMap<String, &str>,
    products: &StoreProducts,
) -> StoreTable {
    let entitlement = |row: &EntitlementData| StoreEntitlement {
        entitlement: row.unique_tag_id.clone(),
        name: row
            .display_name
            .as_deref()
            .and_then(|key| english(text, key))
            .unwrap_or(&row.unique_tag_id)
            .to_owned(),
    };
    let sold: Vec<&EntitlementData> = rows
        .iter()
        .copied()
        .filter(|row| {
            row.is_enabled
                && row.reward_type.as_deref() == Some(MOUNT_REWARD)
                && row.source_type.as_deref() == Some(STORE_SOURCE)
        })
        .collect();
    let mut required: Vec<&str> = sold
        .iter()
        .filter_map(|row| row.required_entitlements.as_deref())
        .flat_map(|cell| cell.split(REQUIRED_SEPARATOR))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .collect();
    required.sort_unstable();
    required.dedup();
    StoreTable {
        tokens: Vec::new(),
        mounts: sold
            .iter()
            .filter_map(|row| {
                let mount = row.reward_s.as_deref().filter(|id| !id.is_empty())?;
                let name = mount_names.get(&mount.to_ascii_lowercase())?;
                Some(StoreMount {
                    unlock: entitlement(row),
                    mount: mount.to_owned(),
                    product: products.of(mount, name)?.to_owned(),
                })
            })
            .collect(),
        held_by_all: required
            .into_iter()
            .map(|id| {
                rows.iter().find(|row| row.unique_tag_id == id).map_or_else(
                    || StoreEntitlement {
                        entitlement: id.to_owned(),
                        name: id.to_owned(),
                    },
                    |row| entitlement(row),
                )
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        id: &str,
        reward_type: &str,
        source: &str,
        reward: &str,
        required: &str,
    ) -> EntitlementData {
        EntitlementData {
            achievement_id: None,
            description: None,
            display_name: Some(format!("@{id}_name")),
            entitlement_index: 1,
            entitlement_info: None,
            icon: None,
            is_consumable: None,
            is_enabled: true,
            notes: None,
            objective_id: None,
            required_entitlements: Some(required.to_owned()),
            reward_s: Some(reward.to_owned()),
            reward_type: Some(reward_type.to_owned()),
            source_type: Some(source.to_owned()),
            unique_tag_id: id.to_owned(),
        }
    }

    #[test]
    fn the_store_sells_each_enabled_store_mount_and_everyone_holds_what_they_require() {
        const XPACK: &str = "Ent_Xpack_RiseOfTheAngryEarth_Owner";
        let mut disabled = row("Ent_Mount_Off", "Mount", "Store", "Mount_Off", "");
        disabled.is_enabled = false;
        let rows = [
            row(
                "Ent_Mount_MTX_7_bear_Ronin",
                "Mount",
                "Store",
                "Mount_MTX_7_bear_Ronin",
                XPACK,
            ),
            row(
                "Ent_Mount_MTX_1_wolf",
                "Mount",
                "Store",
                "Mount_MTX_1_wolf",
                XPACK,
            ),
            row(
                "Ent_Mount_Preorder",
                "Mount",
                "Preorder",
                "Mount_Preorder",
                "",
            ),
            row("Ent_Skin", "Skin", "Store", "Skin_1", ""),
            disabled,
            row(XPACK, "Expansion2023", "", "", ""),
        ];
        let rows: Vec<&EntitlementData> = rows.iter().collect();
        let text = BTreeMap::from([
            (
                "Ent_Mount_MTX_7_bear_Ronin_name".to_owned(),
                "Ronin Bear Mount".to_owned(),
            ),
            (
                format!("{}_name", XPACK.to_lowercase()),
                "Rise of the Angry Earth".to_owned(),
            ),
        ]);
        // Build 25810808: each mount's `DisplayName` and the products that carry it.
        let mount_names = HashMap::from([
            (
                "mount_mtx_7_bear_ronin".to_owned(),
                "@ui_mounts_MTX_7_bear_name",
            ),
            ("mount_mtx_1_wolf".to_owned(), "@ui_mounts_MTX_1_wolf_name"),
        ]);
        let products = StoreProducts::build([
            ("Mount_Bear_MTX7_Ronin", "@ui_mounts_MTX_7_bear_name"),
            ("Mount_Wolf_1", "@ui_mounts_MTX_1_wolf_name"),
        ]);
        let table = store_table_of(&rows, &text, &mount_names, &products);
        let sold: Vec<(&str, &str, &str, &str)> = table
            .mounts
            .iter()
            .map(|m| {
                (
                    m.unlock.entitlement.as_str(),
                    m.unlock.name.as_str(),
                    m.mount.as_str(),
                    m.product.as_str(),
                )
            })
            .collect();
        assert_eq!(
            sold,
            [
                (
                    "Ent_Mount_MTX_7_bear_Ronin",
                    "Ronin Bear Mount",
                    "Mount_MTX_7_bear_Ronin",
                    "Mount_Bear_MTX7_Ronin"
                ),
                (
                    "Ent_Mount_MTX_1_wolf",
                    "Ent_Mount_MTX_1_wolf",
                    "Mount_MTX_1_wolf",
                    "Mount_Wolf_1"
                ),
            ]
        );
        assert_eq!(
            table.held_by_all,
            [StoreEntitlement {
                entitlement: XPACK.to_owned(),
                name: "Rise of the Angry Earth".to_owned(),
            }]
        );
        let unsold = store_table_of(&rows, &text, &mount_names, &StoreProducts::default());
        assert!(
            unsold.mounts.is_empty(),
            "a mount without a product is not sold"
        );
    }

    fn product(id: &str, enabled: bool) -> StoreProductData {
        StoreProductData {
            description: None,
            display_name: format!("@{id}_name"),
            is_enabled: i64::from(enabled),
            landscape_image: String::new(),
            portrait_image: String::new(),
            square_image: String::new(),
            store_product_type: "TokenSingle".to_owned(),
            thumbnail_image: String::new(),
            type_description: String::new(),
            unique_tag_id: id.to_owned(),
        }
    }

    #[test]
    fn the_store_sells_the_barbershop_token_when_the_data_has_it_and_its_product() {
        const TOKEN: &str = "Ent_Barbershop_Token";
        const PRODUCT: &str = "StoreItem_Token_Barbershop_X1";
        let mut token = row(TOKEN, "BarbershopToken", "Store", "BarbershopToken", "");
        token.is_consumable = Some(true);
        token.display_name = None;
        let mut boost = row(
            "Ent_StatusEffect_Token_Weapon_1",
            "StatusEffect",
            "SeasonRewardTrack",
            "Token_Weapon_1",
            "",
        );
        boost.is_consumable = Some(true);
        let text = BTreeMap::from([(format!("{PRODUCT}_name"), "Barbershop Token X1".to_owned())]);
        let sold = sold_tokens(&[&token, &boost], &[&product(PRODUCT, true)], &text);
        assert_eq!(
            sold,
            [StoreToken {
                token: StoreEntitlement {
                    entitlement: TOKEN.to_owned(),
                    name: "Barbershop Token".to_owned(),
                },
                product: PRODUCT.to_owned(),
                product_name: "Barbershop Token X1".to_owned(),
                amount: 1,
                list_price: 10_000,
            }]
        );
        assert!(sold_tokens(&[&token], &[&product(PRODUCT, false)], &text).is_empty());
        assert!(sold_tokens(&[&boost], &[&product(PRODUCT, true)], &text).is_empty());
        token.is_consumable = Some(false);
        assert!(sold_tokens(&[&token], &[&product(PRODUCT, true)], &text).is_empty());
    }

    #[test]
    fn the_store_sells_the_boosts_it_honours_and_a_debit_names_the_token_s_effect() {
        const WEAPON: &str = "Ent_StatusEffect_Token_Weapon_1";
        const SEASON: &str = "Ent_StatusEffect_Token_Season_1";
        let boost = |id: &str, effect: &str| {
            let mut boost = row(id, STATUS_EFFECT_REWARD, "SeasonRewardTrack", effect, "");
            boost.is_consumable = Some(true);
            boost
        };
        let weapon = boost(WEAPON, "Token_Weapon_1");
        let season = boost(SEASON, "Token_Season_1");
        let products = [
            product("Token_WeaponXP_Boost", true),
            product("Token_SeasonXP_Boost", true),
        ];
        let text = BTreeMap::from([(
            "Token_WeaponXP_Boost_name".to_owned(),
            "Weapon XP Boost Token".to_owned(),
        )]);
        let sold = sold_tokens(&[&weapon, &season], &[&products[0], &products[1]], &text);
        assert_eq!(
            sold,
            [StoreToken {
                token: StoreEntitlement {
                    entitlement: WEAPON.to_owned(),
                    name: "Weapon XP Boost Token".to_owned(),
                },
                product: "Token_WeaponXP_Boost".to_owned(),
                product_name: "Weapon XP Boost Token".to_owned(),
                amount: BOOST_TOKENS,
                list_price: BOOST_PRICE,
            }],
            "the season boost is not sold: the sim has no season pass"
        );
        let tokens = EffectTokens::build([(WEAPON, "Token_Weapon_1"), (SEASON, "Token_Season_1")]);
        assert_eq!(
            tokens.effect_of("ent_statuseffect_token_weapon_1"),
            Some("Token_Weapon_1")
        );
        assert_eq!(tokens.effect_of("Ent_Barbershop_Token"), None);
    }

    #[test]
    fn a_shared_product_name_is_told_apart_by_the_mount_s_variant() {
        // Build 25810808: two lion products share the Reaper lion's name, and two products the
        // Blood grunt's.
        const LION_NAME: &str = "@ui_mounts_MTX_8_lion_name";
        const GRUNT_NAME: &str = "@ui_mounts_MTX_2_grunt_name";
        let products = StoreProducts::build([
            ("Mount_Lion_Reaper", LION_NAME),
            ("StoreItem_Mount_Lion_GreekGods", LION_NAME),
            ("StoreItem_Mount_Grunt_2_Blood", GRUNT_NAME),
            ("StoreItem_Mount_Horse_Coquette", GRUNT_NAME),
        ]);
        assert_eq!(
            products.of("Mount_MTX_8_lion_Reaper", LION_NAME),
            Some("Mount_Lion_Reaper")
        );
        assert_eq!(
            products.of("Mount_MTX_2_Grunt_Blood", GRUNT_NAME),
            Some("StoreItem_Mount_Grunt_2_Blood")
        );
        assert_eq!(products.of("Mount_MTX_8_lion_Plain", LION_NAME), None);
        assert_eq!(products.of("Mount_MTX_1_wolf", "@nothing"), None);
    }
}
