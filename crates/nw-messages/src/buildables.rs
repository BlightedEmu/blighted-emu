// SPDX-License-Identifier: AGPL-3.0-or-later
//! The states of a placed buildable (a crafting station or camp): the members of its spawn group
//! (campfire note, retail campfire groups of `Playthrough_Part_01` 249091 ms and
//! `newchar-retail-2026-09-18` 448630 ms, decompile sweep of build 25810808).
//!
//! - `MB::BuildableControllerReplicatedState` (type 1594, constructor `FUN_1447b8e60`): the
//!   build state and the resources committed to it.
//! - `MB::PlacementObstructionComponentReplicatedState` (type 2187, `FUN_1429a7eb0`).
//! - `MB::TerritoryInterfaceComponentReplicatedState` (type 1746, `FUN_1429e4ea0`): slot 0 only.
//! - `MB::BuildableGridComponentReplicatedState` (type 2134, `FUN_142981120`).
//! - `MB::OwnershipComponentReplicatedState` (type 3217, `FUN_14360c430`): the class bytes
//!   `91 32` of every retail campfire group. This server sent 3218, an index without a class,
//!   and the beach campfire showed no prompt (live 2026-09-26; Inferred: the client drops the
//!   group at the unknown class).
//!
//! Each child's width is its reader at virtual `+0x30` of the child's vtable (the GDE id note
//! lists them). The lists are the replicated list form ([`FullList`]).

use crate::combat::{read_string, write_string};
use crate::item_descriptor::ItemDescriptor;
use crate::items::FullList;
use crate::slots::{SLOT_0, children_mask, read_slot_0, write_children};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `MB::BuildableControllerReplicatedState`.
pub const BUILDABLE_CONTROLLER_TYPE_INDEX: u64 = 1594;
/// Type index of `MB::PlacementObstructionComponentReplicatedState`.
pub const PLACEMENT_OBSTRUCTION_TYPE_INDEX: u64 = 2187;
/// Type index of `MB::TerritoryInterfaceComponentReplicatedState`.
pub const TERRITORY_INTERFACE_TYPE_INDEX: u64 = 1746;
/// Type index of `MB::BuildableGridComponentReplicatedState`.
pub const BUILDABLE_GRID_TYPE_INDEX: u64 = 2134;
/// Type index of `MB::OwnershipComponentReplicatedState`.
pub const OWNERSHIP_TYPE_INDEX: u64 = 3217;

/// Children of the buildable controller: `currentState`, `checkFoundationEvents`,
/// `commitedResources`, `ruinDamageType`, `territoryOwnerGuildId`, `upgradeTimePoint`,
/// `craftingStationTypeCrc`, `campTierBuildableId`, `isPvPCamp`.
const BUILDABLE_CHILDREN: u8 = 9;
/// Children of the placement obstruction: `hasCompletionObstruction`.
const OBSTRUCTION_CHILDREN: u8 = 1;
/// Children of the territory interface's slot 0: `territoryFunctionalityDown`,
/// `territoryRefsSet` (its slots 1 and 2, progression and governance data, are not read).
const TERRITORY_CHILDREN: u8 = 2;
/// Children of the buildable grid: `gridSidesActive`.
const GRID_CHILDREN: u8 = 1;
/// Children of the ownership state: `guildID`, `guildOwner`, `guildCrestData`,
/// `ownerPlayerIdentification`, `currentPermissionRule`, `guildStructureName`, `raidId`,
/// `factionType`, `ownerPvpFlag`, `ownerFFAFlag`, `permissionOptions`.
const OWNERSHIP_CHILDREN: u8 = 11;
/// Components of a crest colour (red, green, blue, alpha; `FUN_1415bd0c0`).
const COLOR_COMPONENTS: usize = 4;
/// Bytes of a guild id (`FUN_1433f63b0`).
pub const GUILD_ID_LEN: usize = RAW_ID_LEN;

/// One resource committed to a buildable: its item id and an item descriptor
/// ([`ItemDescriptor`]; the same fields the death recap carries after its item id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedResource {
    /// The map key: the item id text.
    pub item: Vec<u8>,
    /// The item's descriptor.
    pub descriptor: ItemDescriptor,
}

impl CommittedResource {
    /// `quantity` of item `item` (its id text) and `item_crc` (the CRC-32 of its lowercase id),
    /// every other field as the constructor and retail have it.
    pub fn of(item: &str, item_crc: u32, quantity: u32) -> Self {
        Self {
            item: item.as_bytes().to_vec(),
            descriptor: ItemDescriptor::of(item_crc, quantity),
        }
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            item: read_string(reader)?,
            descriptor: ItemDescriptor::read(reader)?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        write_string(writer, &self.item)?;
        self.descriptor.write(writer)
    }
}

/// `MB::BuildableControllerReplicatedState`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildableControllerState {
    /// `currentState` (a byte; retail 5 on a built station, the slice's `m_initialState`).
    pub current_state: Option<u8>,
    /// `checkFoundationEvents`.
    pub check_foundation_events: Option<bool>,
    /// `commitedResources`.
    pub committed_resources: Option<FullList<CommittedResource>>,
    /// `ruinDamageType` (a byte).
    pub ruin_damage_type: Option<u8>,
    /// `territoryOwnerGuildId` (16 bytes).
    pub territory_owner_guild_id: Option<[u8; GUILD_ID_LEN]>,
    /// `upgradeTimePoint` (a big-endian `u64`).
    pub upgrade_time_point: Option<u64>,
    /// `craftingStationTypeCrc`.
    pub crafting_station_type_crc: Option<u32>,
    /// `campTierBuildableId`.
    pub camp_tier_buildable_id: Option<u32>,
    /// `isPvPCamp`.
    pub is_pvp_camp: Option<bool>,
}

impl BuildableControllerState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, has a slot or child the table has not,
    /// or the resource list is a delta.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(
            reader,
            "buildable controller",
            BUILDABLE_CHILDREN,
            |reader, child| {
                match child {
                    0 => state.current_state = Some(reader.u8()?),
                    1 => state.check_foundation_events = Some(reader.boolean()?),
                    2 => {
                        state.committed_resources =
                            Some(FullList::read(reader, CommittedResource::read)?);
                    }
                    3 => state.ruin_damage_type = Some(reader.u8()?),
                    4 => state.territory_owner_guild_id = Some(reader.array()?),
                    5 => state.upgrade_time_point = Some(reader.u64()?),
                    6 => state.crafting_station_type_crc = Some(reader.u32()?),
                    7 => state.camp_tier_buildable_id = Some(reader.u32()?),
                    _ => state.is_pvp_camp = Some(reader.boolean()?),
                }
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a resource with a list or id too long.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let present = [
            self.current_state.is_some(),
            self.check_foundation_events.is_some(),
            self.committed_resources.is_some(),
            self.ruin_damage_type.is_some(),
            self.territory_owner_guild_id.is_some(),
            self.upgrade_time_point.is_some(),
            self.crafting_station_type_crc.is_some(),
            self.camp_tier_buildable_id.is_some(),
            self.is_pvp_camp.is_some(),
        ];
        write_slot_0(
            writer,
            children_mask(&present),
            BUILDABLE_CHILDREN,
            |writer, child| {
                match child {
                    0 => {
                        self.current_state.map(|state| writer.u8(state));
                    }
                    1 => {
                        self.check_foundation_events
                            .map(|check| writer.boolean(check));
                    }
                    2 => {
                        if let Some(list) = &self.committed_resources {
                            list.write(writer, |writer, resource| resource.write(writer))?;
                        }
                    }
                    3 => {
                        self.ruin_damage_type.map(|kind| writer.u8(kind));
                    }
                    4 => {
                        self.territory_owner_guild_id.map(|id| writer.bytes(&id));
                    }
                    5 => {
                        self.upgrade_time_point.map(|time| writer.u64(time));
                    }
                    6 => {
                        self.crafting_station_type_crc.map(|crc| writer.u32(crc));
                    }
                    7 => {
                        self.camp_tier_buildable_id.map(|id| writer.u32(id));
                    }
                    _ => {
                        self.is_pvp_camp.map(|pvp| writer.boolean(pvp));
                    }
                }
                Ok(())
            },
        )
    }
}

/// `MB::PlacementObstructionComponentReplicatedState`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlacementObstructionState {
    /// `hasCompletionObstruction` (a byte, `FUN_1417b4bd0`; retail 1 on the campfires, whose
    /// slice has a completion obstruction detector, 0 on town stations, whose slice has none).
    pub has_completion_obstruction: Option<u8>,
}

impl PlacementObstructionState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or has a slot the table has not.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(
            reader,
            "placement obstruction",
            OBSTRUCTION_CHILDREN,
            |reader, _| {
                state.has_completion_obstruction = Some(reader.u8()?);
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let present = [self.has_completion_obstruction.is_some()];
        write_known_slot_0(writer, &present, OBSTRUCTION_CHILDREN, |writer, _| {
            self.has_completion_obstruction.map(|flag| writer.u8(flag));
        });
    }
}

/// `MB::TerritoryInterfaceComponentReplicatedState`, slot 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerritoryInterfaceState {
    /// `territoryFunctionalityDown`.
    pub functionality_down: Option<bool>,
    /// `territoryRefsSet` (retail 1 on the campfires).
    pub refs_set: Option<bool>,
}

impl TerritoryInterfaceState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, a flag is not 0 or 1, or slot 1 or 2
    /// is set.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(
            reader,
            "territory interface",
            TERRITORY_CHILDREN,
            |reader, child| {
                if child == 0 {
                    state.functionality_down = Some(reader.boolean()?);
                } else {
                    state.refs_set = Some(reader.boolean()?);
                }
                Ok(())
            },
        )?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        let present = [self.functionality_down.is_some(), self.refs_set.is_some()];
        write_known_slot_0(writer, &present, TERRITORY_CHILDREN, |writer, child| {
            if child == 0 {
                self.functionality_down.map(|down| writer.boolean(down));
            } else {
                self.refs_set.map(|set| writer.boolean(set));
            }
        });
    }
}

/// One side of a buildable's grid (`FUN_142998110`: two bytes and a flag; names Unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSide {
    /// The first byte: the side (retail 1 to 4; the slice's `m_side`: front, back, right,
    /// left).
    pub side: u8,
    /// The second byte (retail 1; Inferred: the side's grid point count, 1 in the campfire
    /// slice).
    pub points: u8,
    /// The flag (retail 0; Inferred: the side's grid point is active, 0 in the campfire slice).
    pub active: bool,
}

impl GridSide {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            side: reader.u8()?,
            points: reader.u8()?,
            active: reader.boolean()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer.u8(self.side).u8(self.points).boolean(self.active);
    }
}

/// `MB::BuildableGridComponentReplicatedState`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildableGridState {
    /// `gridSidesActive`.
    pub grid_sides_active: Option<FullList<GridSide>>,
}

impl BuildableGridState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, has a slot the table has not, or the
    /// list is a delta.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, "buildable grid", GRID_CHILDREN, |reader, _| {
            state.grid_sides_active = Some(FullList::read(reader, GridSide::read)?);
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Never fails today; the list writer's form keeps the signature of the other states.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let present = [self.grid_sides_active.is_some()];
        write_slot_0(
            writer,
            children_mask(&present),
            GRID_CHILDREN,
            |writer, _| {
                if let Some(list) = &self.grid_sides_active {
                    list.write(writer, |writer, side| {
                        side.write(writer);
                        Ok(())
                    })?;
                }
                Ok(())
            },
        )
    }
}

/// A guild crest (`FUN_143603a70`): a background and a foreground, each an index (a big-endian
/// `u16`) and a colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuildCrest {
    /// The background index.
    pub background: u16,
    /// The background colour.
    pub background_color: [f32; COLOR_COMPONENTS],
    /// The foreground index.
    pub foreground: u16,
    /// The foreground colour.
    pub foreground_color: [f32; COLOR_COMPONENTS],
}

/// White, the crest colour of a structure no guild owns (retail campfires).
pub const NO_GUILD_COLOR: [f32; COLOR_COMPONENTS] = [1.0; COLOR_COMPONENTS];

impl GuildCrest {
    /// The crest of a structure no guild owns: indexes 0, white colours.
    pub const NONE: Self = Self {
        background: 0,
        background_color: NO_GUILD_COLOR,
        foreground: 0,
        foreground_color: NO_GUILD_COLOR,
    };

    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let color = |reader: &mut Reader<'_>| -> Result<[f32; COLOR_COMPONENTS], MessageError> {
            Ok([reader.f32()?, reader.f32()?, reader.f32()?, reader.f32()?])
        };
        Ok(Self {
            background: reader.u16()?,
            background_color: color(reader)?,
            foreground: reader.u16()?,
            foreground_color: color(reader)?,
        })
    }

    pub(crate) fn write(&self, writer: &mut Writer) {
        writer.u16(self.background);
        for component in self.background_color {
            writer.f32(component);
        }
        writer.u16(self.foreground);
        for component in self.foreground_color {
            writer.f32(component);
        }
    }
}

/// An owner's identification (`FUN_145ce8d80`): two strings and a byte (names Unknown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerIdentification {
    /// The first string (retail empty).
    pub first: Vec<u8>,
    /// The second string (retail empty).
    pub second: Vec<u8>,
    /// The byte (retail 2, the constructor's value).
    pub kind: u8,
}

/// The owner kind of a structure no player owns (constructor `FUN_14360c430` and retail: 2).
pub const NO_OWNER_KIND: u8 = 2;

impl OwnerIdentification {
    /// The identification of a structure no player owns.
    pub fn none() -> Self {
        Self {
            first: Vec::new(),
            second: Vec::new(),
            kind: NO_OWNER_KIND,
        }
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            first: read_string(reader)?,
            second: read_string(reader)?,
            kind: reader.u8()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        write_string(writer, &self.first)?;
        write_string(writer, &self.second)?;
        writer.u8(self.kind);
        Ok(())
    }
}

/// The ownership state (type 3217).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OwnershipState {
    /// `guildID` (16 bytes).
    pub guild_id: Option<[u8; GUILD_ID_LEN]>,
    /// `guildOwner` (a string).
    pub guild_owner: Option<Vec<u8>>,
    /// `guildCrestData`.
    pub guild_crest: Option<GuildCrest>,
    /// `ownerPlayerIdentification`.
    pub owner: Option<OwnerIdentification>,
    /// `currentPermissionRule` (a byte; retail 1 on the campfires, the slice's
    /// `m_initialPermissionRule`).
    pub permission_rule: Option<u8>,
    /// `guildStructureName` (a string; the slice's `m_initialStructureName`).
    pub structure_name: Option<Vec<u8>>,
    /// `raidId` (a big-endian `u64`).
    pub raid_id: Option<u64>,
    /// `factionType` (a byte).
    pub faction_type: Option<u8>,
    /// `ownerPvpFlag`.
    pub owner_pvp: Option<bool>,
    /// `ownerFFAFlag`.
    pub owner_ffa: Option<bool>,
    /// `permissionOptions` (a byte).
    pub permission_options: Option<u8>,
}

impl OwnershipState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early, a flag is not 0 or 1, a string is too
    /// long, or a slot is not in the table.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, "ownership", OWNERSHIP_CHILDREN, |reader, child| {
            match child {
                0 => state.guild_id = Some(reader.array()?),
                1 => state.guild_owner = Some(read_string(reader)?),
                2 => state.guild_crest = Some(GuildCrest::read(reader)?),
                3 => state.owner = Some(OwnerIdentification::read(reader)?),
                4 => state.permission_rule = Some(reader.u8()?),
                5 => state.structure_name = Some(read_string(reader)?),
                6 => state.raid_id = Some(reader.u64()?),
                7 => state.faction_type = Some(reader.u8()?),
                8 => state.owner_pvp = Some(reader.boolean()?),
                9 => state.owner_ffa = Some(reader.boolean()?),
                _ => state.permission_options = Some(reader.u8()?),
            }
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::CountTooLarge`] for a string too long.
    pub fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        let present = [
            self.guild_id.is_some(),
            self.guild_owner.is_some(),
            self.guild_crest.is_some(),
            self.owner.is_some(),
            self.permission_rule.is_some(),
            self.structure_name.is_some(),
            self.raid_id.is_some(),
            self.faction_type.is_some(),
            self.owner_pvp.is_some(),
            self.owner_ffa.is_some(),
            self.permission_options.is_some(),
        ];
        write_slot_0(
            writer,
            children_mask(&present),
            OWNERSHIP_CHILDREN,
            |writer, child| {
                match child {
                    0 => {
                        self.guild_id.map(|id| writer.bytes(&id));
                    }
                    1 => {
                        if let Some(owner) = &self.guild_owner {
                            write_string(writer, owner)?;
                        }
                    }
                    2 => {
                        if let Some(crest) = &self.guild_crest {
                            crest.write(writer);
                        }
                    }
                    3 => {
                        if let Some(owner) = &self.owner {
                            owner.write(writer)?;
                        }
                    }
                    4 => {
                        self.permission_rule.map(|rule| writer.u8(rule));
                    }
                    5 => {
                        if let Some(name) = &self.structure_name {
                            write_string(writer, name)?;
                        }
                    }
                    6 => {
                        self.raid_id.map(|id| writer.u64(id));
                    }
                    7 => {
                        self.faction_type.map(|faction| writer.u8(faction));
                    }
                    8 => {
                        self.owner_pvp.map(|pvp| writer.boolean(pvp));
                    }
                    9 => {
                        self.owner_ffa.map(|ffa| writer.boolean(ffa));
                    }
                    _ => {
                        self.permission_options.map(|options| writer.u8(options));
                    }
                }
                Ok(())
            },
        )
    }
}

/// Writes the slot byte and, when `children` sets a child, slot 0 with a child writer that can
/// fail.
fn write_slot_0(
    writer: &mut Writer,
    children: u16,
    count: u8,
    write_child: impl FnMut(&mut Writer, u8) -> Result<(), MessageError>,
) -> Result<(), MessageError> {
    if children == 0 {
        writer.u8(0);
        return Ok(());
    }
    writer.u8(SLOT_0);
    write_children(writer, children, count, write_child)
}

/// [`write_slot_0`] with a child writer that cannot fail.
fn write_known_slot_0(
    writer: &mut Writer,
    present: &[bool],
    count: u8,
    mut write_child: impl FnMut(&mut Writer, u8),
) {
    crate::slots::write_known(writer, children_mask(present), count, |writer, child| {
        write_child(writer, child);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::MemberState;

    /// The member bodies of the retail WW campfire group (`newchar-retail-2026-09-18`, 448630
    /// ms; the MB group at 249091 ms has the same bodies with list version `a507`).
    const GRID: &str = "01010001ac1004010100020100030100040100";
    const CONTROLLER: &str = "010705000001ac100206576f6f6454318a91442700000000000500ffffffff0000003300000000000007466c696e7454310c9280c800000000000100ffffffff00000033000000000000";
    const OWNERSHIP: &str = "01ff000000000000000000000000000000000000003f8000003f8000003f8000003f80000000003f8000003f8000003f8000003f800000000002010000000000000000000f00000000";
    const OBSTRUCTION: &str = "010101";
    const TERRITORY: &str = "010201";
    /// The WW group's list version (`ac10`).
    const WW_LIST_VERSION: u64 = 1068;

    fn bytes(hex: &str) -> Vec<u8> {
        hex::decode(hex).expect("hex")
    }

    fn round_trip(class: u64, hex: &str) -> MemberState {
        let state = MemberState::decode(class, &bytes(hex)).expect("decode");
        let member = crate::payload::BundleMember {
            component: 0,
            state: state.clone(),
        };
        let encoded = member.encode().expect("encode");
        assert!(encoded.ends_with(&bytes(hex)), "class {class} re-encodes");
        state
    }

    #[test]
    fn the_retail_campfire_states_read_and_round_trip() {
        let MemberState::BuildableController(controller) =
            round_trip(BUILDABLE_CONTROLLER_TYPE_INDEX, CONTROLLER)
        else {
            panic!("controller");
        };
        assert_eq!(controller.current_state, Some(5));
        let resources = controller.committed_resources.expect("resources");
        assert_eq!(resources.version, Some(WW_LIST_VERSION));
        assert_eq!(
            resources.elements,
            [
                CommittedResource::of("WoodT1", 0x8a91_4427, 5),
                CommittedResource::of("FlintT1", 0x0c92_80c8, 1),
            ]
        );
        let MemberState::Ownership(ownership) = round_trip(OWNERSHIP_TYPE_INDEX, OWNERSHIP) else {
            panic!("ownership");
        };
        assert_eq!(
            ownership,
            OwnershipState {
                guild_id: Some([0; GUILD_ID_LEN]),
                guild_owner: Some(Vec::new()),
                guild_crest: Some(GuildCrest::NONE),
                owner: Some(OwnerIdentification::none()),
                permission_rule: Some(1),
                structure_name: Some(Vec::new()),
                raid_id: Some(0),
                faction_type: Some(0),
                owner_pvp: Some(false),
                owner_ffa: Some(false),
                permission_options: Some(0),
            }
        );
        let MemberState::BuildableGrid(grid) = round_trip(BUILDABLE_GRID_TYPE_INDEX, GRID) else {
            panic!("grid");
        };
        let sides: Vec<u8> = grid
            .grid_sides_active
            .expect("sides")
            .elements
            .iter()
            .map(|side| side.side)
            .collect();
        assert_eq!(sides, [1, 2, 3, 4]);
        assert_eq!(
            round_trip(PLACEMENT_OBSTRUCTION_TYPE_INDEX, OBSTRUCTION),
            MemberState::PlacementObstruction(PlacementObstructionState {
                has_completion_obstruction: Some(1),
            })
        );
        assert_eq!(
            round_trip(TERRITORY_INTERFACE_TYPE_INDEX, TERRITORY),
            MemberState::TerritoryInterface(TerritoryInterfaceState {
                functionality_down: None,
                refs_set: Some(true),
            })
        );
    }

    #[test]
    fn a_territory_slot_beyond_0_is_not_read() {
        // Slot 1 (progression data) has no reader here.
        assert!(TerritoryInterfaceState::read(&mut Reader::new(&[0b10, 0x01])).is_err());
    }
}
