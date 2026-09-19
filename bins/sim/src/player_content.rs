// SPDX-License-Identifier: AGPL-3.0-or-later
//! The content a sim needs for its players besides combat: the starter kit of each archetype
//! pick, the bit of each achievement, the look of each mob prefab, and the tutorial quest.

use nw_content::gathering::Gatherables;
use nw_content::loot::LootTables;
use std::collections::HashMap;
use std::sync::Arc;

use nw_content::Tables;
use nw_content::achievements::AchievementIndexes;
use nw_content::ftue_entities::FtueLevel;
use nw_content::mob_prefabs::{ASSET_ID_LEN, MobPrefabs, PrefabKey, PrefabMember};
use nw_content::starter::StarterKits;
use nw_messages::{AlcState, AssetId, BundleMember, MemberState, VitalsState};

use crate::objectives::{KillTypes, ObjectiveTree, task_rows};

/// The tutorial quest's objective id (`Objectives`).
pub const TUTORIAL_OBJECTIVE: &str = "01_SoleSurvivor2";
/// The first open-world quest, which a player gets when it leaves the tutorial (retail: in the
/// first open-world state, xp-potion-exit note, section 3.3).
pub const OPEN_WORLD_OBJECTIVE: &str = "12_GetOffBeach";
/// Its root task.
pub const OPEN_WORLD_QUEST_ROOT: &str = "12_getoffbeach_container";
/// The root task of the tutorial quest `01_SoleSurvivor2` (live-tutorial note, appendix A).
pub const TUTORIAL_QUEST_ROOT: &str = "01_SoleSurvivor2_Container";

/// What a client needs to show one mob prefab (mob-cards note, section 2).
#[derive(Debug, Clone, PartialEq)]
pub struct MobLook {
    /// The creature slice asset of its spawn card.
    pub asset: AssetId,
    /// The component of its `ALCReplicatedState`.
    pub alc_component: u64,
    /// The component of its vitals state.
    pub vitals_component: u64,
    /// Its retail first `ALCReplicatedState`, moved to each spawn point; `None` when the table
    /// has none.
    pub alc: Option<AlcState>,
    /// Its retail first vitals state, with each mob's health written in; `None` when the table
    /// has none.
    pub vitals: Option<VitalsState>,
    /// Its other states when it comes into view.
    pub first: Vec<BundleMember>,
    /// Its other states when it dies.
    pub death: Vec<BundleMember>,
}

/// The look of every mob prefab, by vitals id and slice asset.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MobLooks {
    by_key: HashMap<PrefabKey, MobLook>,
    /// The asset of each vitals id's most seen prefab.
    default_asset: HashMap<u32, [u8; ASSET_ID_LEN]>,
}

impl MobLooks {
    /// The looks of `prefabs`. A state whose body does not decode is left out of its prefab;
    /// the second value counts them.
    pub fn of(prefabs: &MobPrefabs) -> (Self, usize) {
        let mut skipped = 0;
        let mut members = |rows: &[PrefabMember]| -> Vec<BundleMember> {
            rows.iter()
                .filter_map(|row| match MemberState::decode(row.class, &row.body) {
                    Ok(state) => Some(BundleMember {
                        component: row.component,
                        state,
                    }),
                    Err(_) => {
                        skipped += 1;
                        None
                    }
                })
                .collect()
        };
        let by_key = prefabs
            .iter()
            .map(|(key, prefab)| {
                // The ALC and vitals states of the first group are taken apart: each mob gets
                // them with its own place and health.
                let mut alc = None;
                let mut vitals = None;
                let mut first = Vec::new();
                for member in members(&prefab.first_members) {
                    match member.state {
                        MemberState::Alc(state) => alc = Some(state),
                        MemberState::Vitals(state) => vitals = Some(state),
                        state => first.push(BundleMember {
                            component: member.component,
                            state,
                        }),
                    }
                }
                let look = MobLook {
                    asset: AssetId {
                        id: prefab.asset,
                        sub_id: prefab.sub_id,
                    },
                    alc_component: prefab.alc_component,
                    vitals_component: prefab.vitals_component,
                    alc,
                    vitals,
                    first,
                    death: members(&prefab.death_members),
                };
                (key, look)
            })
            .collect();
        let looks = Self {
            by_key,
            default_asset: prefabs.default_assets().collect(),
        };
        (looks, skipped)
    }

    /// The look of vitals id `vitals_id` with slice asset `asset`; without an asset, or when the
    /// table has no prefab of it, the vitals id's most seen look.
    pub fn look(&self, vitals_id: u32, asset: Option<[u8; ASSET_ID_LEN]>) -> Option<&MobLook> {
        asset
            .and_then(|asset| self.by_key.get(&(vitals_id, asset)))
            .or_else(|| {
                let asset = self.default_asset.get(&vitals_id)?;
                self.by_key.get(&(vitals_id, *asset))
            })
    }

    /// Prefabs with a look.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// Whether there is no look.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

/// The player content of one content bundle and mob prefab table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerContent {
    /// The login kit of each archetype pick.
    pub kits: StarterKits,
    /// The bit of each achievement.
    pub achievements: AchievementIndexes,
    /// The look of each mob prefab.
    pub mob_looks: MobLooks,
    /// The achievement each archetype pick unlocks: (backstory id, achievement id), from
    /// `BackstoryDefinition.AchievementUnlockOverride`.
    pub archetype_unlocks: Vec<(String, String)>,
    /// The vitals id and categories of every mob type, for quest kill tasks.
    pub kill_types: KillTypes,
    /// The task tree of the tutorial quest; `None` when the content lacks it.
    pub tutorial_quest: Option<Arc<ObjectiveTree>>,
    /// The task tree of the first open-world quest; `None` when the content lacks it.
    pub open_world_quest: Option<Arc<ObjectiveTree>>,
    /// The tutorial level's entities: its static entities, its asset entities and the retail
    /// causes of their groups.
    pub tutorial_level: FtueLevel,
    /// The gatherables.
    pub gatherables: Gatherables,
    /// Every loot table and bucket, for gathering and mob kills.
    pub loot: LootTables,
}

impl PlayerContent {
    /// The player content of `tables` with the mob looks `mob_looks` and the tutorial level's
    /// entities `tutorial_level`.
    pub fn new(tables: &Tables, mob_looks: MobLooks, tutorial_level: FtueLevel) -> Self {
        Self {
            kits: StarterKits::from_tables(tables),
            achievements: AchievementIndexes::from_tables(tables),
            mob_looks,
            archetype_unlocks: tables
                .backstory_definition
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let unlock = row.achievement_unlock_override.clone()?;
                    Some((row.backstory_id.clone(), unlock))
                })
                .collect(),
            kill_types: KillTypes::from_tables(tables),
            tutorial_quest: ObjectiveTree::build(&task_rows(tables), TUTORIAL_QUEST_ROOT)
                .map(Arc::new),
            open_world_quest: ObjectiveTree::build(&task_rows(tables), OPEN_WORLD_QUEST_ROOT)
                .map(Arc::new),
            tutorial_level,
            gatherables: Gatherables::from_tables(tables),
            loot: LootTables::from_tables(tables),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAILOR: u32 = 2_288_616_137;

    #[test]
    fn prefab_bodies_become_states_and_bad_ones_are_left_out() {
        let text = format!(
            r#"[{{"vitals_id":{SAILOR},"vitals_name":null,
                "asset":"b7e33b294bcf57b8a476abdc3a31fae7","sub_id":2,
                "alc_component":5,"vitals_component":9,
                "first_members":[{{"component":6,"class":12,"body":"010104"}},
                                 {{"component":3,"class":1528,"body":"01"}}],
                "death_members":[{{"component":20,"class":3139,"body":"010201"}}],
                "seen":1}}]"#
        );
        let prefabs = MobPrefabs::parse(&text).expect("table");
        let (looks, skipped) = MobLooks::of(&prefabs);
        assert_eq!(skipped, 1, "a cut damage receiver body");
        let sailor = looks.look(SAILOR, None).expect("sailor");
        assert_eq!(sailor.asset.sub_id, 2);
        assert_eq!(sailor.first.len(), 1);
        assert_eq!(sailor.first[0].component, 6);
        assert_eq!(sailor.death.len(), 1);
    }

    #[test]
    fn a_mob_with_its_own_asset_gets_its_own_look() {
        // Retail: the door legionnaire (179393) and the signifier (202592) share their vitals id.
        const LEGIONNAIRE: u32 = 3_705_015_359;
        const DOOR: [u8; ASSET_ID_LEN] =
            0xc05f_c957_8487_5559_afe8_74a7_8f95_fb64_u128.to_be_bytes();
        const SIGNIFIER: [u8; ASSET_ID_LEN] =
            0x87a3_35e7_6b86_50d5_a465_fe08_10bd_a5df_u128.to_be_bytes();
        let row = |asset: [u8; ASSET_ID_LEN], sub_id: u32, seen: u32| {
            let asset: String = asset.iter().map(|byte| format!("{byte:02x}")).collect();
            format!(
                r#"{{"vitals_id":{LEGIONNAIRE},"vitals_name":null,"asset":"{asset}",
                    "sub_id":{sub_id},"alc_component":5,"vitals_component":9,
                    "first_members":[],"death_members":[],"seen":{seen}}}"#
            )
        };
        let text = format!("[{},{}]", row(DOOR, 1, 2), row(SIGNIFIER, 2, 3));
        let (looks, _) = MobLooks::of(&MobPrefabs::parse(&text).expect("table"));
        let sub_id = |asset| looks.look(LEGIONNAIRE, asset).map(|look| look.asset.sub_id);
        assert_eq!(sub_id(Some(DOOR)), Some(1));
        assert_eq!(sub_id(Some(SIGNIFIER)), Some(2));
        assert_eq!(sub_id(None), Some(2), "the most seen look without an asset");
        assert_eq!(
            sub_id(Some([0; ASSET_ID_LEN])),
            Some(2),
            "and for an asset not in the table"
        );
    }
}
