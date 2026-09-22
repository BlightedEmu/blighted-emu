// SPDX-License-Identifier: AGPL-3.0-or-later
//! The put-away of a player's sheathe action (`playeractions_sheathe.actionlist`, sheathe audit
//! 2026-09-20).
//!
//! The action of main-hand option 1 (`Sheathe_MainHand_1_UB`) starts on the sheathe key, in deep
//! water, or in shallow water while crouched. It plays its fragment (`Sheathe_UpperBody`) and
//! puts the weapon away when the fragment's marker clip starts: the `CAGE-Named` clip named as
//! the `IfMannequinMarker` of the activity that sheathes a main-hand slot (`sheathe_weapon`).
//!
//! On enter the action's activities change the Mannequin tags in file order, and the fragment's
//! entry is picked with the tags they leave:
//! - A `SetMannequinTag` applies the first of its `Conditional` entries whose named condition
//!   holds, else its `Default` (the client's `FUN_142ad4140` walks the conditionals in order and
//!   stops at the first whose condition is true). So a greatsword (`2H_Melee,GreatSword`) gets
//!   `From_Greatsword`: `MainHand_Greatsword` comes before `2H_Melee` in the list.
//! - A `SetMannequinTagFromSource` from the sheathed slot clears the weapon's own tags.
//!
//! The named conditions (the list's `<Conditions>`) test the character's tags
//! (`IfMannequinTag`) and the tags of the weapon in a slot (`IfMannequinTagInItemSlot`): the
//! weapon of the action's slot and the active main-hand weapon are the sheathed weapon, whose
//! tags the item keeps after the character's are cleared. Any other test (an input, an item in
//! another slot, a status effect) does not hold.

use nw_formats::mannequin::{Fragment, MannequinError, Node, ProcClip, parse_tree};

/// Elements and attributes of an action list.
const ACTION_ELEMENT: &str = "Action";
const NAME_ATTRIBUTE: &str = "Name";
const FRAGMENT_NAME_ATTRIBUTE: &str = "FragmentName";
const ACTIVITIES_ELEMENT: &str = "Activities";
const CONDITIONS_ELEMENT: &str = "Conditions";
const IF_THEN_ELSE_ELEMENT: &str = "IfThenElse";
const IF_CONDITION_ELEMENT: &str = "IfCondition";
const THEN_ACTIVITY_ELEMENT: &str = "ThenActivity";
const MARKER_TEST_ELEMENT: &str = "IfMannequinMarker";
const MARKER_NAME_ATTRIBUTE: &str = "MarkerName";
const SHEATHE_WEAPON_ELEMENT: &str = "SheatheWeapon";
const SHEATHE_OPTION_ATTRIBUTE: &str = "SheatheOption";
const SHEATHE_OPTION: &str = "Sheathe";
const SLOT_TO_SHEATHE_ATTRIBUTE: &str = "SlotToSheathe";
const SET_TAG_ELEMENT: &str = "SetMannequinTag";
const SET_TAG_FROM_SOURCE_ELEMENT: &str = "SetMannequinTagFromSource";
const DEFAULT_ELEMENT: &str = "Default";
const CONDITIONAL_ELEMENT: &str = "Conditional";
const TAG_NAME_ATTRIBUTE: &str = "TagName";
const ON_ENTER_ATTRIBUTE: &str = "OnEnter";
const SET_VALUE: &str = "Set";
const UNSET_VALUE: &str = "Unset";
const SOURCE_SLOT_ATTRIBUTE: &str = "SourceSlot";
/// Elements of a condition.
const AND_ELEMENT: &str = "And";
const OR_ELEMENT: &str = "Or";
const NOT_ELEMENT: &str = "Not";
const TRUE_ELEMENT: &str = "True";
const TAG_TEST_ELEMENT: &str = "IfMannequinTag";
const SLOT_TAG_TEST_ELEMENT: &str = "IfMannequinTagInItemSlot";
const SLOT_ALIAS_ATTRIBUTE: &str = "SlotToCheckAlias";
/// The slot alias of the weapon in the hand.
const ACTIVE_MAIN_HAND_ALIAS: &str = "ACTIVE_MAIN_HAND_WEAPON";
/// The slot aliases of the main-hand options start with this.
const MAIN_HAND_OPTION_PREFIX: &str = "MAIN_HAND_OPTION_";
/// The clip type of a fragment marker and its name parameter.
const NAMED_CLIP: &str = "CAGE-Named";
const NAMED_CLIP_NAME: &str = "Name";

/// A test of a named condition, for a character with the sheathed weapon.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Test {
    All(Vec<Test>),
    Any(Vec<Test>),
    Not(Box<Test>),
    True,
    /// The character has the tag.
    Tag(String),
    /// The sheathed weapon has the tag.
    WeaponTag(String),
    /// A test of other state: it does not hold.
    Other,
}

impl Test {
    /// The test of `node`; `weapon_slots` are the slot aliases of the sheathed weapon.
    fn of(node: &Node, weapon_slots: &[&str]) -> Self {
        let parts = || {
            node.children
                .iter()
                .map(|child| Self::of(child, weapon_slots))
                .collect()
        };
        let tag = || {
            node.attribute(TAG_NAME_ATTRIBUTE)
                .unwrap_or_default()
                .to_owned()
        };
        match node.name.as_str() {
            AND_ELEMENT => Self::All(parts()),
            OR_ELEMENT => Self::Any(parts()),
            NOT_ELEMENT => Self::Not(Box::new(Self::All(parts()))),
            TRUE_ELEMENT => Self::True,
            TAG_TEST_ELEMENT => Self::Tag(tag()),
            SLOT_TAG_TEST_ELEMENT
                if node
                    .attribute(SLOT_ALIAS_ATTRIBUTE)
                    .is_some_and(|slot| weapon_slots.contains(&slot)) =>
            {
                Self::WeaponTag(tag())
            }
            _ => Self::Other,
        }
    }

    /// Whether it holds for a character with the tags `tags` and a sheathed weapon of the tags
    /// `weapon` (any case).
    fn holds(&self, tags: &[String], weapon: &[String]) -> bool {
        let has = |list: &[String], wanted: &str| {
            list.iter().any(|own| own.eq_ignore_ascii_case(wanted))
        };
        match self {
            Self::All(parts) => parts.iter().all(|part| part.holds(tags, weapon)),
            Self::Any(parts) => parts.iter().any(|part| part.holds(tags, weapon)),
            Self::Not(part) => !part.holds(tags, weapon),
            Self::True => true,
            Self::Tag(tag) => has(tags, tag),
            Self::WeaponTag(tag) => has(weapon, tag),
            Self::Other => false,
        }
    }
}

/// What one entry of a `SetMannequinTag` does on enter.
#[derive(Debug, Clone, PartialEq, Eq)]
enum OnEnter {
    Set(String),
    Unset(String),
    Nothing,
}

impl OnEnter {
    fn of(entry: &Node) -> Self {
        let tag = entry
            .attribute(TAG_NAME_ATTRIBUTE)
            .unwrap_or_default()
            .to_owned();
        match entry.attribute(ON_ENTER_ATTRIBUTE) {
            Some(SET_VALUE) => Self::Set(tag),
            Some(UNSET_VALUE) => Self::Unset(tag),
            _ => Self::Nothing,
        }
    }
}

/// One activity that changes the tags on enter.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TagChange {
    /// A `SetMannequinTag`: its conditional entries (named condition and change) in order, and
    /// its default.
    Set {
        conditionals: Vec<(String, OnEnter)>,
        default: OnEnter,
    },
    /// A `SetMannequinTagFromSource` that clears the sheathed weapon's tags.
    ClearWeapon,
}

/// The sheathe action of a main-hand option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheatheAction {
    /// The fragment it plays (`Sheathe_UpperBody`).
    pub fragment: String,
    /// The marker at which it puts the weapon away (`sheathe_weapon`).
    pub marker: String,
    changes: Vec<TagChange>,
    conditions: Vec<(String, Test)>,
}

/// The marker and the main-hand slot of an `IfThenElse` that sheathes a main-hand slot at a
/// marker.
fn marker_sheathe(branch: &Node) -> Option<(String, String)> {
    let condition = branch
        .children
        .iter()
        .find(|child| child.name == IF_CONDITION_ELEMENT)?;
    let mut markers = Vec::new();
    condition.descendants(MARKER_TEST_ELEMENT, &mut markers);
    let marker = markers.first()?.attribute(MARKER_NAME_ATTRIBUTE)?;
    let then = branch
        .children
        .iter()
        .find(|child| child.name == THEN_ACTIVITY_ELEMENT)?;
    let mut sheathes = Vec::new();
    then.descendants(SHEATHE_WEAPON_ELEMENT, &mut sheathes);
    let slot = sheathes
        .iter()
        .flat_map(|sheathe| &sheathe.children)
        .filter(|entry| entry.name == DEFAULT_ELEMENT)
        .filter(|entry| entry.attribute(SHEATHE_OPTION_ATTRIBUTE) == Some(SHEATHE_OPTION))
        .find_map(|entry| {
            entry
                .attribute(SLOT_TO_SHEATHE_ATTRIBUTE)
                .filter(|slot| slot.starts_with(MAIN_HAND_OPTION_PREFIX))
        })?;
    Some((marker.to_owned(), slot.to_owned()))
}

impl SheatheAction {
    /// The action `action` of the action list `text`; `None` when the list has no such action,
    /// or the action has no fragment or no activity that sheathes a main-hand slot at a marker.
    ///
    /// # Errors
    ///
    /// Returns [`MannequinError`] when the XML is not well formed.
    pub fn parse(text: &str, action: &str) -> Result<Option<Self>, MannequinError> {
        let root = parse_tree(text)?;
        let mut actions = Vec::new();
        root.descendants(ACTION_ELEMENT, &mut actions);
        let Some(node) = actions
            .into_iter()
            .find(|node| node.attribute(NAME_ATTRIBUTE) == Some(action))
        else {
            return Ok(None);
        };
        let Some(fragment) = node
            .attribute(FRAGMENT_NAME_ATTRIBUTE)
            .filter(|fragment| !fragment.is_empty())
        else {
            return Ok(None);
        };
        let Some(activities) = node
            .children
            .iter()
            .find(|child| child.name == ACTIVITIES_ELEMENT)
        else {
            return Ok(None);
        };
        let Some((marker, slot)) = activities
            .children
            .iter()
            .filter(|child| child.name == IF_THEN_ELSE_ELEMENT)
            .find_map(marker_sheathe)
        else {
            return Ok(None);
        };
        let weapon_slots = [slot.as_str(), ACTIVE_MAIN_HAND_ALIAS];
        let changes = activities
            .children
            .iter()
            .filter_map(|activity| match activity.name.as_str() {
                SET_TAG_ELEMENT => {
                    let entries = || activity.children.iter();
                    let default = entries()
                        .find(|entry| entry.name == DEFAULT_ELEMENT)
                        .map_or(OnEnter::Nothing, OnEnter::of);
                    let conditionals = entries()
                        .filter(|entry| entry.name == CONDITIONAL_ELEMENT)
                        .filter_map(|entry| {
                            Some((entry.attribute(NAME_ATTRIBUTE)?.to_owned(), OnEnter::of(entry)))
                        })
                        .collect();
                    Some(TagChange::Set {
                        conditionals,
                        default,
                    })
                }
                SET_TAG_FROM_SOURCE_ELEMENT => activity
                    .children
                    .iter()
                    .filter(|entry| entry.name == DEFAULT_ELEMENT)
                    .any(|entry| {
                        entry
                            .attribute(SOURCE_SLOT_ATTRIBUTE)
                            .is_some_and(|source| weapon_slots.contains(&source))
                            && entry.attribute(ON_ENTER_ATTRIBUTE) == Some(UNSET_VALUE)
                    })
                    .then_some(TagChange::ClearWeapon),
                _ => None,
            })
            .collect();
        let conditions = root
            .children
            .iter()
            .filter(|child| child.name == CONDITIONS_ELEMENT)
            .flat_map(|conditions| &conditions.children)
            .filter_map(|condition| {
                Some((
                    condition.attribute(NAME_ATTRIBUTE)?.to_owned(),
                    Test::of(condition, &weapon_slots),
                ))
            })
            .collect();
        Ok(Some(Self {
            fragment: fragment.to_owned(),
            marker,
            changes,
            conditions,
        }))
    }

    /// Whether the named condition `name` holds; a name the list does not have does not.
    fn holds(&self, name: &str, tags: &[String], weapon: &[String]) -> bool {
        self.conditions
            .iter()
            .find(|(known, _)| known == name)
            .is_some_and(|(_, test)| test.holds(tags, weapon))
    }

    /// The tags the fragment's entry is picked with for a character whose only tags are those
    /// of its weapon `weapon_tags` (a stance's `MannequinTag` tags): the tags after the
    /// action's changes on enter.
    pub fn fragment_tags(&self, weapon_tags: &[String]) -> Vec<String> {
        let mut tags = weapon_tags.to_vec();
        for change in &self.changes {
            match change {
                TagChange::Set {
                    conditionals,
                    default,
                } => {
                    let picked = conditionals
                        .iter()
                        .find(|(condition, _)| self.holds(condition, &tags, weapon_tags))
                        .map_or(default, |(_, change)| change);
                    match picked {
                        OnEnter::Set(tag) => {
                            if !tags.iter().any(|own| own.eq_ignore_ascii_case(tag)) {
                                tags.push(tag.clone());
                            }
                        }
                        OnEnter::Unset(tag) => tags.retain(|own| !own.eq_ignore_ascii_case(tag)),
                        OnEnter::Nothing => {}
                    }
                }
                TagChange::ClearWeapon => tags.retain(|own| {
                    !weapon_tags
                        .iter()
                        .any(|weapon| weapon.eq_ignore_ascii_case(own))
                }),
            }
        }
        tags
    }

    /// The marker clip of the fragment entry `entry`: the first `CAGE-Named` clip named as the
    /// marker.
    pub fn marker_clip<'a>(&self, entry: &'a Fragment) -> Option<&'a ProcClip> {
        entry
            .clips_of(NAMED_CLIP)
            .find(|clip| clip.param(NAMED_CLIP_NAME) == Some(self.marker.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use nw_formats::mannequin::AnimDatabase;

    use super::*;

    const ACTION: &str = "Sheathe_MainHand_1_UB";

    /// The parts of `playeractions_sheathe.actionlist` (build 25810808) that pick the fragment's
    /// tags, in file order.
    const ACTIONS: &str = r#"<ActionList><Actions>
 <Action Name="Sheathe_MainHand_1_UB" FragmentName="Sheathe_UpperBody">
  <ActivationCondition><IfInput InputName="sheathe"/></ActivationCondition>
  <Activities>
   <IfThenElse ResetWhenReinstalled="false">
    <IfCondition><IfMannequinMarker MarkerName="sheathe_weapon"/></IfCondition>
    <ThenActivity><Group><SheatheWeapon ResetWhenReinstalled="false">
     <Default SheatheOption="Sheathe" SlotToSheathe="MAIN_HAND_OPTION_1" OnEnter="Set" OnExit="NoEffect"/>
    </SheatheWeapon></Group></ThenActivity>
   </IfThenElse>
   <IfThenElse ResetWhenReinstalled="false">
    <IfCondition><IfMannequinMarker MarkerName="Sheathe_Offhand"/></IfCondition>
    <ThenActivity><SheatheWeapon ResetWhenReinstalled="false">
     <Default SheatheOption="Sheathe" SlotToSheathe="ACTIVE_OFF_HAND_WEAPON" OnEnter="Set" OnExit="NoEffect"/>
    </SheatheWeapon></ThenActivity>
   </IfThenElse>
   <SetMannequinTag ResetWhenReinstalled="false">
    <Default TagName="From_Unarmed" OnEnter="Set" OnExit="NoEffect"/>
    <Conditional Name="1H_Melee" TagName="From_1H_Melee" OnEnter="Set" OnExit="NoEffect"/>
    <Conditional Name="1H_Melee_Sword" TagName="From_1H_Melee_Sword" OnEnter="Set" OnExit="NoEffect"/>
    <Conditional Name="MainHand_Greatsword" TagName="From_Greatsword" OnEnter="Set" OnExit="NoEffect"/>
    <Conditional Name="2H_Melee" TagName="From_2H_Melee" OnEnter="Set" OnExit="NoEffect"/>
    <Conditional Name="MainHand_2H_Spear" TagName="From_Spear" OnEnter="Set" OnExit="NoEffect"/>
    <Conditional Name="MainHand_MagicStaff_Life" TagName="From_MagicStaff_Life" OnEnter="Set" OnExit="NoEffect"/>
   </SetMannequinTag>
   <SetMannequinTag ResetWhenReinstalled="false">
    <Default TagName="-" OnEnter="NoEffect" OnExit="NoEffect"/>
    <Conditional Name="Shield_OH" TagName="From_Shield_OH" OnEnter="Set" OnExit="NoEffect"/>
   </SetMannequinTag>
   <SetMannequinTagFromSource ResetWhenReinstalled="false">
    <Default TagSource="PaperdollSlot" SourceSlot="MAIN_HAND_OPTION_1" OnEnter="Unset" OnExit="NoEffect"/>
   </SetMannequinTagFromSource>
   <SetMannequinTagFromSource ResetWhenReinstalled="false">
    <Default TagSource="PaperdollSlot" SourceSlot="ACTIVE_OFF_HAND_WEAPON" OnEnter="Unset" OnExit="NoEffect"/>
   </SetMannequinTagFromSource>
   <SetMannequinTag ResetWhenReinstalled="false"><Default TagName="Unarmed" OnEnter="Set" OnExit="NoEffect"/></SetMannequinTag>
   <SetMannequinTag ResetWhenReinstalled="false"><Default TagName="AutoSheathe" OnEnter="Unset" OnExit="NoEffect"/></SetMannequinTag>
  </Activities>
 </Action>
</Actions>
<Conditions>
 <And Name="1H_Melee">
  <IfMannequinTag Name="1H_Melee" TagName="1H_Melee"/>
  <Not><IfMannequinTag Name="Sword" TagName="Sword"/></Not>
  <Not><IfMannequinTag Name="Flail" TagName="Flail"/></Not>
 </And>
 <And Name="1H_Melee_Sword">
  <IfMannequinTag Name="1H_Melee" TagName="1H_Melee"/>
  <IfMannequinTag Name="Sword" TagName="Sword"/>
 </And>
 <And Name="2H_Melee">
  <IfMannequinTag TagName="2H_Melee"/>
  <Not><IfMannequinTagInItemSlot TagName="Spear" SlotToCheckAlias="ACTIVE_MAIN_HAND_WEAPON"/></Not>
 </And>
 <IfMannequinTag Name="Shield_OH" TagName="Shield_OH"/>
 <And Name="MainHand_2H_Spear">
  <Or><IfMannequinTag TagName="2H_Melee"/><IfMannequinTag TagName="From_Spear"/></Or>
  <IfMannequinTagInItemSlot TagName="Spear" SlotToCheckAlias="ACTIVE_MAIN_HAND_WEAPON"/>
 </And>
 <And Name="MainHand_Greatsword"><IfMannequinTag TagName="Greatsword"/></And>
 <And Name="MainHand_MagicStaff_Life">
  <Or><IfMannequinTag TagName="MagicStaff"/><IfMannequinTag TagName="From_MagicStaff_Life"/></Or>
  <IfMannequinTagInItemSlot TagName="Heal" SlotToCheckAlias="ACTIVE_MAIN_HAND_WEAPON"/>
 </And>
</Conditions></ActionList>"#;

    /// `Sheathe_UpperBody` entries of `player.adb` (build 25810808), with their `sheathe_weapon`
    /// markers: crouched sword 0.167 s, sword 0.400 s (after `CanBuffer` at 0.100 s), 1H melee
    /// 0.300 s, 2H melee 0.733 s, greatsword 0.833 s.
    const PLAYER_ADB: &str = r#"<AnimDB FragDef="x" TagDef="y"><FragmentList><Sheathe_UpperBody>
 <Fragment BlendOutDuration="0.2" Tags="Crouch+From_1H_Melee_Sword">
  <ProcLayer><Blend ExitTime="0.16666667" StartTime="0" Duration="0" CurveType="0"/>
   <Procedural type="CAGE-Named" contextType="NamedContext"><ProceduralParams><Name value="sheathe_weapon"/></ProceduralParams></Procedural></ProcLayer>
 </Fragment>
 <Fragment BlendOutDuration="0.2" Tags="From_1H_Melee_Sword">
  <ProcLayer><Blend ExitTime="0.1" StartTime="0" Duration="0" CurveType="0"/>
   <Procedural type="CAGE-Named" contextType="NamedContext"><ProceduralParams><Name value="CanBuffer"/></ProceduralParams></Procedural>
   <Blend ExitTime="0.30000001" StartTime="0" Duration="0" CurveType="0"/>
   <Procedural type="CAGE-Named" contextType="NamedContext"><ProceduralParams><Name value="sheathe_weapon"/></ProceduralParams></Procedural></ProcLayer>
 </Fragment>
 <Fragment BlendOutDuration="0.2" Tags="From_1H_Melee">
  <ProcLayer><Blend ExitTime="0.30000001" StartTime="0" Duration="0" CurveType="0"/>
   <Procedural type="CAGE-Named" contextType="NamedContext"><ProceduralParams><Name value="sheathe_weapon"/></ProceduralParams></Procedural></ProcLayer>
 </Fragment>
 <Fragment BlendOutDuration="0.2" Tags="From_2H_Melee">
  <ProcLayer><Blend ExitTime="0.73333335" StartTime="0" Duration="0" CurveType="0"/>
   <Procedural type="CAGE-Named" contextType="NamedContext"><ProceduralParams><Name value="sheathe_weapon"/></ProceduralParams></Procedural></ProcLayer>
 </Fragment>
 <Fragment BlendOutDuration="0.2" Tags="From_Greatsword">
  <ProcLayer><Blend ExitTime="0.83333331" StartTime="0" Duration="0" CurveType="0"/>
   <Procedural type="CAGE-Named" contextType="NamedContext"><ProceduralParams><Name value="sheathe_weapon"/></ProceduralParams></Procedural></ProcLayer>
 </Fragment>
</Sheathe_UpperBody></FragmentList></AnimDB>"#;

    /// Milliseconds per second.
    const MS_PER_SECOND: f64 = 1_000.0;

    fn action() -> SheatheAction {
        SheatheAction::parse(ACTIONS, ACTION)
            .expect("xml")
            .expect("action")
    }

    fn tags(cell: &str) -> Vec<String> {
        crate::player_attacks::stance_tags(cell)
    }

    /// The put-away time of a weapon of `cell`, ms.
    // Marker times are a few seconds.
    #[allow(clippy::cast_possible_truncation)]
    fn marker_ms(cell: &str) -> Option<i64> {
        let action = action();
        let picked = action.fragment_tags(&tags(cell));
        let refs: Vec<&str> = picked.iter().map(String::as_str).collect();
        let adb = AnimDatabase::parse(PLAYER_ADB).expect("adb");
        let entry = adb.best(&action.fragment, &refs)?;
        action
            .marker_clip(entry)
            .map(|clip| (clip.start_s * MS_PER_SECOND).round() as i64)
    }

    #[test]
    fn the_action_names_its_fragment_and_put_away_marker() {
        let action = action();
        assert_eq!(action.fragment, "Sheathe_UpperBody");
        assert_eq!(action.marker, "sheathe_weapon", "not the offhand's marker");
        assert_eq!(SheatheAction::parse(ACTIONS, "Nothing").expect("xml"), None);
    }

    #[test]
    fn the_first_conditional_that_holds_picks_the_stance_transition_tag() {
        let action = action();
        let picked = |cell: &str| action.fragment_tags(&tags(cell));
        assert_eq!(
            picked("2H_Melee,GreatSword"),
            ["From_Greatsword", "Unarmed"],
            "MainHand_Greatsword comes before 2H_Melee; the weapon's tags are cleared"
        );
        assert_eq!(picked("1H_Melee,Sword"), ["From_1H_Melee_Sword", "Unarmed"]);
        assert_eq!(picked("1H_Melee,Axe,Tool"), ["From_1H_Melee", "Unarmed"]);
        assert_eq!(
            picked("2H_Melee,Spear,Throwable"),
            ["From_Spear", "Unarmed"],
            "the weapon's own tags: 2H_Melee does not hold for a spear"
        );
        assert_eq!(
            picked("MagicStaff,Heal"),
            ["From_MagicStaff_Life", "Unarmed"]
        );
        assert_eq!(picked(""), ["From_Unarmed", "Unarmed"], "the default");
    }

    #[test]
    fn the_put_away_comes_at_the_marker_of_the_picked_entry() {
        assert_eq!(marker_ms("1H_Melee,Sword"), Some(400), "not the crouched entry");
        assert_eq!(marker_ms("1H_Melee,Rapier"), Some(300));
        assert_eq!(marker_ms("2H_Melee,GreatSword"), Some(833));
        assert_eq!(marker_ms("2H_Melee,Warhammer"), Some(733));
    }
}
