// SPDX-License-Identifier: AGPL-3.0-or-later
//! How many objectives of a kind a player may have in the journal at once (finding
//! `local-data/findings/2026-10-02-quest-journal-limits.md`).
//!
//! The client registers one variable per limit, with its default, in `FUN_14616d7b0` of build
//! 25810808 (`LEA RDX,<name>`, then `MOV R8D,<default>`), and its objectives component maps an
//! `eObjectiveType` to one of them in `FUN_143a390b0`. The retail remote config of 2026-09-18
//! sets none of these keys, so retail ran with the defaults. A type the map does not name has
//! no limit (the function returns 0 for it).
//!
//! The type numbers of the map follow the client's enum reflection order (`FUN_145a4fae0`:
//! `Objective` 0 to `WeeklyChallenge` 17, so `Journey` 4, `Mission` 5, `CommunityGoal` 6,
//! `MainStoryQuest` 13, `Event` 15). The later cases hold the variable of the type with the
//! same name (Inferred by name: `0x12` the skill progression variable, `0x16` to `0x18` the
//! faction story variable).

use crate::gameplay_config::{GameplayConfigError, KeyStack};

/// The limit groups. Each is one client variable; the three faction stories share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectiveLimitGroup {
    /// `Journey`: the side quests.
    Journey,
    /// `MainStoryQuest`.
    MainStory,
    /// `Mission`: the town board and the faction missions (both are `Mission` in the
    /// `Objectives` table).
    Missions,
    /// `CommunityGoal`.
    CommunityGoals,
    /// `Event`.
    Events,
    /// `DailyChallenge`.
    DailyChallenges,
    /// `WeeklyChallenge`.
    WeeklyChallenges,
    /// `FactionStory_Syndicate`, `FactionStory_Marauders`, `FactionStory_Covenant`.
    FactionStory,
    /// `Affinity`.
    Affinity,
    /// `SeasonQuest`.
    SeasonQuests,
    /// `SeasonEncounter`.
    SeasonEncounters,
    /// `SkillProgression`.
    SkillProgression,
    /// `MountRace`.
    MountRace,
    /// `MountUnlock`.
    MountUnlock,
    /// `EpicEquipment`.
    EpicEquipment,
    /// `Artifact`.
    ArtifactQuests,
}

/// Every group, in client registration order.
pub const ALL_GROUPS: [ObjectiveLimitGroup; 16] = [
    ObjectiveLimitGroup::Journey,
    ObjectiveLimitGroup::MainStory,
    ObjectiveLimitGroup::Missions,
    ObjectiveLimitGroup::CommunityGoals,
    ObjectiveLimitGroup::Events,
    ObjectiveLimitGroup::DailyChallenges,
    ObjectiveLimitGroup::WeeklyChallenges,
    ObjectiveLimitGroup::FactionStory,
    ObjectiveLimitGroup::Affinity,
    ObjectiveLimitGroup::SeasonQuests,
    ObjectiveLimitGroup::SeasonEncounters,
    ObjectiveLimitGroup::SkillProgression,
    ObjectiveLimitGroup::MountRace,
    ObjectiveLimitGroup::MountUnlock,
    ObjectiveLimitGroup::EpicEquipment,
    ObjectiveLimitGroup::ArtifactQuests,
];

impl ObjectiveLimitGroup {
    /// The group of an `Objectives` row's `Type`; `None` for a type with no limit.
    pub fn of_type(objective_type: &str) -> Option<Self> {
        let group = match objective_type.to_ascii_lowercase().as_str() {
            "journey" => Self::Journey,
            "mainstoryquest" => Self::MainStory,
            "mission" => Self::Missions,
            "communitygoal" => Self::CommunityGoals,
            "event" => Self::Events,
            "dailychallenge" => Self::DailyChallenges,
            "weeklychallenge" => Self::WeeklyChallenges,
            "factionstory_syndicate" | "factionstory_marauders" | "factionstory_covenant" => {
                Self::FactionStory
            }
            "affinity" => Self::Affinity,
            "seasonquest" => Self::SeasonQuests,
            "seasonencounter" => Self::SeasonEncounters,
            "skillprogression" => Self::SkillProgression,
            "mountrace" => Self::MountRace,
            "mountunlock" => Self::MountUnlock,
            "epicequipment" => Self::EpicEquipment,
            "artifact" => Self::ArtifactQuests,
            _ => return None,
        };
        Some(group)
    }

    /// The client variable of the group.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Journey => "javelin.objectives-max-journey",
            Self::MainStory => "javelin.objectives-max-MSQ",
            Self::Missions => "javelin.objectives-max-missions",
            Self::CommunityGoals => "javelin.objectives-max-communitygoals",
            Self::Events => "javelin.objectives-max-events",
            Self::DailyChallenges => "javelin.objectives-max-dailychallenges",
            Self::WeeklyChallenges => "javelin.objectives-max-weeklychallenges",
            Self::FactionStory => "javelin.objectives-max-factionstory",
            Self::Affinity => "javelin.objectives-max-affinity",
            Self::SeasonQuests => "javelin.objectives-max-season-quests",
            Self::SeasonEncounters => "javelin.objectives-max-season-encounters",
            Self::SkillProgression => "javelin.objectives-max-skillprogression",
            Self::MountRace => "javelin.objectives-max-mountrace",
            Self::MountUnlock => "javelin.objectives-max-mountunlock",
            Self::EpicEquipment => "javelin.objectives-max-epicequipment",
            Self::ArtifactQuests => "javelin.objectives-max-artifact-quests",
        }
    }

    /// The client default of the group's variable (build 25810808).
    pub const fn default_limit(self) -> u64 {
        match self {
            Self::Journey | Self::Missions | Self::Affinity => 30,
            Self::CommunityGoals => 50,
            Self::MainStory
            | Self::Events
            | Self::SeasonQuests
            | Self::SeasonEncounters
            | Self::SkillProgression
            | Self::EpicEquipment => 20,
            Self::FactionStory | Self::ArtifactQuests => 10,
            Self::MountUnlock => 9,
            Self::WeeklyChallenges => 7,
            Self::DailyChallenges => 3,
            Self::MountRace => 1,
        }
    }
}

/// The limits the remote config layers set; a group they do not set keeps its client default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectiveLimits {
    set: Vec<(ObjectiveLimitGroup, u64)>,
}

impl ObjectiveLimits {
    /// The limit keys of `stack`.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayConfigError::WrongType`] for a key that is not a whole number.
    pub fn from_stack(stack: &KeyStack) -> Result<Self, GameplayConfigError> {
        let mut set = Vec::new();
        for group in ALL_GROUPS {
            if let Some(limit) = stack.unsigned(group.key())? {
                set.push((group, limit));
            }
        }
        Ok(Self { set })
    }

    /// The most objectives of `group` a player may have at once.
    pub fn limit(&self, group: ObjectiveLimitGroup) -> u64 {
        self.set
            .iter()
            .find(|(set, _)| *set == group)
            .map_or(group.default_limit(), |(_, limit)| *limit)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const SET_JOURNEY_LIMIT: u64 = 12;

    #[test]
    fn the_types_of_the_game_data_fall_in_their_client_groups() {
        assert_eq!(
            ObjectiveLimitGroup::of_type("Journey"),
            Some(ObjectiveLimitGroup::Journey)
        );
        assert_eq!(
            ObjectiveLimitGroup::of_type("MainStoryQuest"),
            Some(ObjectiveLimitGroup::MainStory)
        );
        assert_eq!(
            ObjectiveLimitGroup::of_type("Mission"),
            Some(ObjectiveLimitGroup::Missions)
        );
        assert_eq!(
            ObjectiveLimitGroup::of_type("FactionStory_Covenant"),
            ObjectiveLimitGroup::of_type("FactionStory_Syndicate")
        );
        assert_eq!(ObjectiveLimitGroup::of_type("Objective"), None);
    }

    #[test]
    fn the_client_defaults_hold_until_a_layer_sets_a_limit() {
        let defaults = ObjectiveLimits::default();
        assert_eq!(defaults.limit(ObjectiveLimitGroup::Journey), 30);
        assert_eq!(defaults.limit(ObjectiveLimitGroup::Missions), 30);
        assert_eq!(defaults.limit(ObjectiveLimitGroup::MainStory), 20);
        assert_eq!(defaults.limit(ObjectiveLimitGroup::CommunityGoals), 50);
        let layer = json!({"javelin": {"objectives-max-journey": SET_JOURNEY_LIMIT}});
        let stack = KeyStack::merge([layer.as_object().expect("object")]);
        let set = ObjectiveLimits::from_stack(&stack).expect("limits");
        assert_eq!(set.limit(ObjectiveLimitGroup::Journey), SET_JOURNEY_LIMIT);
        assert_eq!(set.limit(ObjectiveLimitGroup::Missions), 30);
        let wrong = json!({"javelin": {"objectives-max-MSQ": "many"}});
        let stack = KeyStack::merge([wrong.as_object().expect("object")]);
        assert!(ObjectiveLimits::from_stack(&stack).is_err());
    }
}
