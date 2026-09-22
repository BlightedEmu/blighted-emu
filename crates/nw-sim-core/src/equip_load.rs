// SPDX-License-Identifier: AGPL-3.0-or-later
//! The equip load category of a character (dodge study 2026-09-20): the weight of its equipped
//! items against its equip load maximum (`javelindata_encumbrancelimits.datasheet`, row
//! `Player`: `EquipLoadBaseMax` 500 and the category ratios in percent, `EquipLoadRatioNormal`
//! 26, `EquipLoadRatioSlow` 46, `EquipLoadRatioOverburdened` 100). The category picks the dodge
//! (`playeractions_dodge.actionlist` `ActionConditionEquipLoadCategory`), and an overburdened
//! character neither sprints nor dodges.

/// Percent of a whole.
const PERCENT: f32 = 100.0;

/// The equip load category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EquipLoad {
    /// Under the normal ratio: the light roll.
    Fast,
    /// From the normal ratio: the quick step.
    #[default]
    Normal,
    /// From the slow ratio: the heavy quick step.
    Slow,
    /// From the overburdened ratio: no sprint, no dodge.
    Overburdened,
}

/// The limits of the categories.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EquipLoadLimits {
    /// The equip load maximum (`EquipLoadBaseMax`).
    pub base_max: f32,
    /// Percent of the maximum from which the load is normal (`EquipLoadRatioNormal`).
    pub normal_percent: f32,
    /// Percent from which it is slow (`EquipLoadRatioSlow`).
    pub slow_percent: f32,
    /// Percent from which it is overburdened (`EquipLoadRatioOverburdened`).
    pub overburdened_percent: f32,
}

impl EquipLoadLimits {
    /// The category of an equip load of `load` (a load on a ratio is in the higher category:
    /// Inferred, the client's comparison is not decoded).
    pub fn category(&self, load: f32) -> EquipLoad {
        let from = |percent: f32| self.base_max * percent / PERCENT;
        if load >= from(self.overburdened_percent) {
            EquipLoad::Overburdened
        } else if load >= from(self.slow_percent) {
            EquipLoad::Slow
        } else if load >= from(self.normal_percent) {
            EquipLoad::Normal
        } else {
            EquipLoad::Fast
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The player row of the retail encumbrance limits.
    const PLAYER: EquipLoadLimits = EquipLoadLimits {
        base_max: 500.0,
        normal_percent: 26.0,
        slow_percent: 46.0,
        overburdened_percent: 100.0,
    };

    #[test]
    fn the_retail_ratios_split_the_loads() {
        assert_eq!(PLAYER.category(0.0), EquipLoad::Fast);
        assert_eq!(PLAYER.category(129.0), EquipLoad::Fast);
        assert_eq!(PLAYER.category(130.0), EquipLoad::Normal);
        assert_eq!(PLAYER.category(229.0), EquipLoad::Normal);
        assert_eq!(PLAYER.category(230.0), EquipLoad::Slow);
        assert_eq!(PLAYER.category(499.0), EquipLoad::Slow);
        assert_eq!(PLAYER.category(500.0), EquipLoad::Overburdened);
    }

    #[test]
    fn the_tutorial_swordsman_is_slow() {
        // The Swordsman kit's heavy chest, hands, legs and feet (`WeightOverride` 110.56, 47.38,
        // 63.17, 47.38) and the tutorial sword (`Weight` 36): 304.5, 60.9 %; retail's newchar
        // dodges fit the slow clips.
        const TUTORIAL_LOAD: f32 = 110.56 + 47.38 + 63.17 + 47.38 + 36.0;
        assert_eq!(PLAYER.category(TUTORIAL_LOAD), EquipLoad::Slow);
    }
}
