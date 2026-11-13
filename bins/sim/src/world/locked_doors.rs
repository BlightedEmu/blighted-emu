// SPDX-License-Identifier: AGPL-3.0-or-later
//! The doors of the open world's locked interact plots: a level plot that shows an interact
//! door slice to the players who have the plot's achievement (door study 2026-11-12,
//! [`nw_messages::door`]).
//!
//! **The plot** (Observed, the client level and datasheets of build 25810808): the level places
//! the capital `Master_Locked_Interact_Gatherable` with the variant `CrownChamber_LightsHope` at
//! (7927.44, 2749.64, 92.45), and the variation table `LockedInteractGatherables`
//! (`javelindata_variations_locked_interact_gatherables`) gives that variant the slice
//! `slices/WorldEvents/MonarchsBluffs/CrownChamberDoor/IO_LightsHope_CrownChamberDoor` and the
//! achievement `12A_MB_Daggers_Complete` (set by the task `12A_MSQ_20_CollectCrown_Daggers_Collected`
//! of the quest Broken Oath).
//!
//! **The door** (Observed, retail `12a_msq_20_collectcrown` 123.437 s, `Playthrough_Part_01`
//! 5352.982 s, `12a_side_08_gatherfangs` 104.799 s, `12a_side_09_collectglass` 175.076 s): a
//! card of the door slice with a dynamic `GdeRef`, the position of the plot capital (x, y and
//! height step to the step, the capital's rotation), the gatherable controller in state 4 on
//! component 2, the interact state `Enabled` off on component 5 and the door `Closed` on
//! component 9. Nothing of it changes while the player sees it; the slot stops when the player
//! leaves (499.522 s). The player opens the door with interaction kind 67
//! (`TeleportToOtherSideOfDoor`, name `@12A_MSQ_20_Door_Interact`) at 337.881 s and again from
//! the inside at 469.224 s; the server accepts each with 3743 status 1 and the name 16 ms later
//! and sends nothing else ([`super::travel`] answers the kind); the client moves the player to
//! the other side, and the task `12A_MSQ_20_CollectCrown_UnlockDoor` (a `TaskTriggerArea`
//! `12A_EnterSanctum`) counts the entry.
//!
//! Observed: the `GdeRef` was the same in the three Monarch's Bluffs ledgers of one world and
//! another in `Playthrough_Part_01`: one id per plot and world, the same for every viewer and
//! session. This server makes it from the plot's index ([`dynamic_ref`]).
//!
//! Inferred: a player without the plot's achievement gets no door (the level's locked capital
//! shows `@12A_MSQ_20_Door_Interact_Locked`, "Requires Seals of the Knight", with no network
//! entity); the door shows on the first view tick after the achievement is set. Every retail
//! door life began with the player already past the dagger task, so no capture shows the
//! moment the achievement is set.
//!
//! Not built: the other rows of the table (`IO_CorbenicCastle_CrownChamberDoor_Active`, the
//! Season 2 egg chamber, temple, map room and chest room doors, `IO_0601_DirtHoleB`): no retail
//! capture shows their spawn group, so their components are Unknown; their plots stay level
//! objects only.

use nw_content::Tables;
use nw_messages::position::{PositionInTheWorldReplicatedState, Rotation, WorldPosition};
use nw_messages::{
    BundleGroup, BundleMember, DoorComponentState, DoorState, GatherableControllerState,
    InteractState, MemberState,
};
use nw_sim_core::Position;
use nw_world::records::Capital;

use super::crafting::dynamic_ref;
use super::encounter_roots::card;
use super::slots::Slot;

/// The entity numbers of the locked plot doors start here. Keys are never sent; they stay above
/// every other entity range.
pub(super) const LOCKED_DOOR_ENTITY_BASE: u64 = 1 << 46;
/// Names the ids this server makes for locked plot doors ("lockdoor").
const LOCKED_DOOR_NAMESPACE: u64 = 0x6c6f_636b_646f_6f72;
/// The part of the variation table's pak path that names the locked interact table.
const LOCKED_TABLE_PATH: &str = "variations_locked_interact_gatherables";
/// The slice name of the level capital of a locked plot (Observed).
const LOCKED_PLOT_SLICE: &str = "Master_Locked_Interact_Gatherable";

/// The retail spawn group layout of one door slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DoorLayout {
    /// The slice's pak path, lowercase with `/`.
    slice: &'static str,
    /// The slice's asset id.
    asset: [u8; 16],
    /// The component of the position.
    position: u64,
    /// The component of the gatherable controller, and its state.
    gatherable: u64,
    gatherable_state: u8,
    /// The component of the interact state, and its `Enabled`.
    interact: u64,
    interact_enabled: bool,
    /// The component of the door.
    door: u64,
}

/// The door slices whose spawn group a retail capture shows (Observed: see the module notes).
const DOOR_LAYOUTS: [DoorLayout; 1] = [DoorLayout {
    slice: "slices/worldevents/monarchsbluffs/crownchamberdoor/io_lightshope_crownchamberdoor.dynamicslice",
    asset: [
        0x86, 0xe8, 0x00, 0x65, 0xc0, 0xbb, 0x59, 0xc6, 0x9f, 0xc5, 0xbe, 0x54, 0x6b, 0xa9, 0x5f,
        0x02,
    ],
    position: 1,
    gatherable: 2,
    gatherable_state: 4,
    interact: 5,
    interact_enabled: false,
    door: 9,
}];

/// One row of the locked interact table: the plot variant, the slice it shows and the
/// achievement a player needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedRow {
    /// `VariantID`.
    pub variant: String,
    /// The slice's pak path.
    pub slice: String,
    /// The achievement.
    pub achievement: String,
}

/// One door of a locked plot.
#[derive(Debug, Clone, PartialEq)]
struct LockedDoor {
    /// The achievement a viewer needs.
    achievement: String,
    /// Where it stands.
    position: Position,
    /// Its spawn group members.
    members: Vec<BundleMember>,
}

/// The doors of the level's locked plots whose slice has a retail layout.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LockedDoors {
    doors: Vec<LockedDoor>,
}

/// The rows of the locked interact table among `tables`: each with a variant, a slice and an
/// achievement.
pub fn locked_rows(tables: &Tables) -> Vec<LockedRow> {
    tables
        .variation_data
        .iter()
        .filter(|table| table.source_path.to_lowercase().contains(LOCKED_TABLE_PATH))
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            Some(LockedRow {
                variant: row.variant_id.clone()?,
                slice: row.interact_gatherable_slice.clone()?,
                achievement: row.required_achievement_conditional.clone()?,
            })
        })
        .collect()
}

/// The pak path `path` in the form of [`DoorLayout::slice`].
fn normal_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

/// The spawn group members of a door of `layout` at `capital` with the id `gde_ref`.
fn door_members(
    layout: &DoorLayout,
    capital: &Capital,
    gde_ref: [u8; nw_messages::RAW_ID_LEN],
) -> Vec<BundleMember> {
    let [x, y, z] = capital.position;
    vec![
        card(layout.asset, gde_ref),
        BundleMember {
            component: layout.position,
            state: MemberState::Position(PositionInTheWorldReplicatedState {
                position: WorldPosition::from_metres(x, y, z),
                rotation: Some(Rotation::of_quaternion(capital.rotation)),
                scale: None,
            }),
        },
        BundleMember {
            component: layout.gatherable,
            state: MemberState::GatherableController(GatherableControllerState {
                gatherable_state: Some(layout.gatherable_state),
            }),
        },
        BundleMember {
            component: layout.interact,
            state: MemberState::Interact(InteractState {
                enabled: Some(layout.interact_enabled),
                has_interactors: None,
            }),
        },
        BundleMember {
            component: layout.door,
            state: MemberState::Door(DoorComponentState::of(DoorState::Closed)),
        },
    ]
}

impl LockedDoors {
    /// The doors of the locked plots among `capitals` whose variant has a row in `rows` and
    /// whose row's slice has a retail layout.
    pub fn build(capitals: &[Capital], rows: &[LockedRow]) -> Self {
        let doors = capitals
            .iter()
            .filter(|capital| {
                capital
                    .slice_name
                    .rsplit(['/', '\\'])
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(LOCKED_PLOT_SLICE))
            })
            .filter_map(|capital| {
                let variant = capital.variant_name.as_deref()?;
                let row = rows.iter().find(|row| row.variant == variant)?;
                let slice = normal_path(&row.slice);
                let layout = DOOR_LAYOUTS.iter().find(|layout| layout.slice == slice)?;
                Some((capital, row, layout))
            })
            .enumerate()
            .map(|(index, (capital, row, layout))| {
                let [x, y, z] = capital.position;
                LockedDoor {
                    achievement: row.achievement.clone(),
                    position: Position { x, y, z },
                    members: door_members(
                        layout,
                        capital,
                        dynamic_ref(LOCKED_DOOR_NAMESPACE, index),
                    ),
                }
            })
            .collect();
        Self { doors }
    }

    /// How many doors the level has.
    pub fn len(&self) -> usize {
        self.doors.len()
    }

    /// Whether the level has no door.
    pub fn is_empty(&self) -> bool {
        self.doors.is_empty()
    }

    /// The entity numbers of the doors within `radius_m` of `at` (across the ground) whose
    /// achievement the viewer has (`has_achievement`).
    pub(super) fn numbers_near<'a>(
        &'a self,
        at: Position,
        radius_m: f32,
        has_achievement: impl Fn(&str) -> bool + 'a,
    ) -> impl Iterator<Item = u64> + 'a {
        self.doors
            .iter()
            .enumerate()
            .filter(move |(_, door)| {
                door.position.horizontal_distance(at) <= radius_m
                    && has_achievement(&door.achievement)
            })
            .map(|(index, _)| LOCKED_DOOR_ENTITY_BASE + index as u64)
    }

    /// The spawn group of door `number` for a viewer whose slot `slot` is new; the door never
    /// changes, so a held slot gets nothing.
    pub(super) fn group(&self, number: u64, slot: Slot) -> Option<BundleGroup> {
        let index = usize::try_from(number.checked_sub(LOCKED_DOOR_ENTITY_BASE)?).ok()?;
        let door = self.doors.get(index)?;
        slot.new.then(|| BundleGroup {
            interest: u64::from(slot.interest),
            members: door.members.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::position::RotationComponent;

    use super::*;

    /// The retail door's id (`12a_msq_20_collectcrown`), for a group test.
    const GDE_REF: [u8; nw_messages::RAW_ID_LEN] = [7; nw_messages::RAW_ID_LEN];
    /// The retail member bodies of the crown chamber door (Observed, 123.437 s): gatherable
    /// controller, interact, door and position, each the whole member.
    const RETAIL_GATHERABLE: &str = "020c010104";
    const RETAIL_INTERACT: &str = "05b22d010100";
    const RETAIL_DOOR: &str = "099a24010101";
    /// The retail position: x, y, height step and the rotation's z.
    const RETAIL_X: f32 = 7927.443;
    const RETAIL_Y: f32 = 2749.6392;
    const RETAIL_Z_RAW: u16 = 6005;
    const RETAIL_ROTATION_Z: u16 = 9597;
    /// The achievement of the crown chamber plot.
    const DAGGERS: &str = "12A_MB_Daggers_Complete";
    /// The id of the crown chamber plot's capital in the open world's world file.
    const CROWN_CAPITAL: &str = "6ba512ee-59e3-4fe7-896b-9d24c2f1932e";
    /// A view radius for the tests.
    const RADIUS_M: f32 = 100.0;

    /// The level capital of the crown chamber plot as the world file holds it (Observed:
    /// `12mb04_props.capitals.json`, id `6ba512ee-...`, rotation (0, 0, -0.70710677,
    /// 0.70710677): -90 degrees about the up axis, the retail door's z 9597).
    fn crown_plot() -> Capital {
        let half_yaw = -std::f32::consts::FRAC_PI_4;
        Capital {
            id: CROWN_CAPITAL.into(),
            position: [7927.443, 2749.6392, 92.4478],
            rotation: [0.0, 0.0, half_yaw.sin(), half_yaw.cos()],
            scale: None,
            slice_name: LOCKED_PLOT_SLICE.into(),
            variant_name: Some("CrownChamber_LightsHope".into()),
            slice_asset_id: String::new(),
        }
    }

    fn crown_row() -> LockedRow {
        LockedRow {
            variant: "CrownChamber_LightsHope".into(),
            slice: r"slices\WorldEvents\MonarchsBluffs\CrownChamberDoor\IO_LightsHope_CrownChamberDoor.dynamicslice".into(),
            achievement: DAGGERS.into(),
        }
    }

    fn encoded(member: &BundleMember) -> String {
        hex::encode(member.encode().expect("encode"))
    }

    #[test]
    fn the_crown_door_spawns_with_the_retail_states_at_its_plot() {
        let plot = crown_plot();
        let members = door_members(&DOOR_LAYOUTS[0], &plot, GDE_REF);
        assert!(matches!(
            &members[0].state,
            MemberState::GdeMetadata(card)
                if card.asset.is_some_and(|asset| asset.id == DOOR_LAYOUTS[0].asset)
                    && card.gde_ref == Some(GDE_REF)
        ));
        assert_eq!(encoded(&members[2]), RETAIL_GATHERABLE);
        assert_eq!(encoded(&members[3]), RETAIL_INTERACT);
        assert_eq!(encoded(&members[4]), RETAIL_DOOR);
        let MemberState::Position(PositionInTheWorldReplicatedState {
            position: Some(position),
            rotation: Some(rotation),
            ..
        }) = &members[1].state
        else {
            panic!("a position");
        };
        assert!((position.x - RETAIL_X).abs() < 0.01);
        assert!((position.y - RETAIL_Y).abs() < 0.01);
        assert!(
            position.z_raw.abs_diff(RETAIL_Z_RAW) <= 1,
            "{}",
            position.z_raw
        );
        let RotationComponent::Value(z) = rotation.components[2] else {
            panic!("a turn about the up axis");
        };
        assert!(z.abs_diff(RETAIL_ROTATION_Z) <= 8, "{z}");
        assert!(!rotation.w_negative);
    }

    #[test]
    fn only_a_player_with_the_achievement_sees_the_door() {
        let doors = LockedDoors::build(&[crown_plot()], &[crown_row()]);
        assert_eq!(doors.len(), 1);
        let at = Position {
            x: RETAIL_X,
            y: RETAIL_Y + 10.0,
            z: 92.0,
        };
        let with: Vec<u64> = doors
            .numbers_near(at, RADIUS_M, |id| id == DAGGERS)
            .collect();
        assert_eq!(with, [LOCKED_DOOR_ENTITY_BASE]);
        assert_eq!(doors.numbers_near(at, RADIUS_M, |_| false).count(), 0);
        let far = Position {
            x: RETAIL_X + 2.0 * RADIUS_M,
            ..at
        };
        assert_eq!(doors.numbers_near(far, RADIUS_M, |_| true).count(), 0);
    }

    #[test]
    fn a_new_slot_gets_the_card_and_a_held_one_nothing() {
        let doors = LockedDoors::build(&[crown_plot()], &[crown_row()]);
        let new = Slot {
            interest: 31,
            new: true,
        };
        let group = doors.group(LOCKED_DOOR_ENTITY_BASE, new).expect("a group");
        assert_eq!(group.interest, 31);
        assert_eq!(group.members.len(), 5);
        let held = Slot { new: false, ..new };
        assert!(doors.group(LOCKED_DOOR_ENTITY_BASE, held).is_none());
        assert!(doors.group(LOCKED_DOOR_ENTITY_BASE + 1, new).is_none());
    }

    #[test]
    fn a_plot_without_a_row_or_a_retail_layout_has_no_door() {
        let other = LockedRow {
            slice: r"slices\WorldEvents\Seasons\Season_02\IO_Season_02_Temple_Door.dynamicslice"
                .into(),
            ..crown_row()
        };
        assert!(LockedDoors::build(&[crown_plot()], &[other]).is_empty());
        let unnamed = Capital {
            variant_name: None,
            ..crown_plot()
        };
        assert!(LockedDoors::build(&[unnamed], &[crown_row()]).is_empty());
    }

    #[test]
    #[ignore = "needs the build 25810808 content bundle in NW_CONTENT_DIR"]
    fn the_game_tables_give_the_crown_plot_its_slice_and_achievement() {
        const BUILD: &str = "25810808";
        let dir = std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR");
        let path = nw_content::bundle::bundle_path(std::path::Path::new(&dir), BUILD);
        let bundle = nw_content::ContentBundle::load(&path, BUILD).expect("bundle");
        let rows = locked_rows(&bundle.tables);
        let row = rows
            .iter()
            .find(|row| row.variant == "CrownChamber_LightsHope")
            .expect("the crown chamber row");
        assert_eq!(normal_path(&row.slice), DOOR_LAYOUTS[0].slice);
        assert_eq!(row.achievement, DAGGERS);
    }
}

#[cfg(all(test, feature = "local-data"))]
mod retail_tests {
    use std::path::Path;

    use nw_content::ContentBundle;
    use nw_content::bundle::bundle_path;
    use nw_content::client_build::load_client_build;
    use nw_messages::position::RotationComponent;
    use nw_world::dungeon_scripts::OPEN_WORLD_LEVEL;
    use nw_world::records::WorldFile;
    use nw_world::world_file::world_file_path;

    use super::*;

    const CLIENT_BUILD_CONFIG: &str = "config/client-build.toml";
    /// The retail crown chamber door's rotation z (`12a_msq_20_collectcrown` 123.437 s).
    const RETAIL_ROTATION_Z: u16 = 9597;

    #[test]
    fn the_crown_door_takes_the_retail_turn_from_its_world_file_capital() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let build = load_client_build(&repo.join(CLIENT_BUILD_CONFIG)).expect("client build");
        let dir = std::env::var("NW_CONTENT_DIR")
            .map_or_else(|_| repo.join("local-data/content"), Into::into);
        let bundle = ContentBundle::load(&bundle_path(&dir, &build), &build).expect("bundle");
        let level = WorldFile::load(&world_file_path(&dir, &build, OPEN_WORLD_LEVEL), &build)
            .expect("world");
        let doors = LockedDoors::build(&level.capitals, &locked_rows(&bundle.tables));
        let rotations: Vec<&Rotation> = doors
            .doors
            .iter()
            .flat_map(|door| &door.members)
            .filter_map(|member| match &member.state {
                MemberState::Position(PositionInTheWorldReplicatedState {
                    rotation: Some(rotation),
                    ..
                }) => Some(rotation),
                _ => None,
            })
            .collect();
        let [rotation] = rotations.as_slice() else {
            panic!("one crown chamber door: {}", rotations.len());
        };
        assert_eq!(
            rotation.components[2],
            RotationComponent::Value(RETAIL_ROTATION_Z)
        );
        assert!(!rotation.w_negative);
    }
}
