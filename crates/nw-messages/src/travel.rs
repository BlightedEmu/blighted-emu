// SPDX-License-Identifier: AGPL-3.0-or-later
//! The travel calls: fast travel to a landmark or home (the player housing component), unstuck,
//! the map marker (waypoints component), the camp skin, and the house plot and furniture calls,
//! with the server's answers (run 15 travel study, `local-data/scratch-work/run15/travel`).
//!
//! Every body is the facet header and the arguments; the layouts are those of the retail
//! captures (Azoth catalog shapes, checked on every retail body of the types: 3713 x 322,
//! 3696 x 429, 3042 x 16, 388 x 76, 3980 x 50, 2190 x 46, 2774 x 29, 2728 x 78, the housing
//! calls of the house, furniture and upkeep captures). A body of another length is refused.
//!
//! The component constants are the target less the entity of every retail body (one value per
//! type over every character), except the waypoints component, which sits on a child entity of
//! the player prefab: its child XOR and constant are the solution of ten characters' 2728 headers
//! (`(entity ^ child) + component`; the low bit of the child is free, every player entity has bit
//! 0 clear).

use crate::facet::FacetHeader;
use crate::slots::{children_mask, read_slot_0, write_known};
use crate::wire::{MessageError, RAW_ID_LEN, Reader, Writer};

/// The target-id constant of each travel component of the player entity.
pub mod component {
    /// `PlayerHousingComponent`: fast travel, home and furniture calls.
    pub const PLAYER_HOUSING: u64 = 0xb80e_9a25_19ef_6be6;
    /// `UnstuckComponent`.
    pub const UNSTUCK: u64 = 0x748b_574e_d9ea_acec;
    /// `CampingComponent`.
    pub const CAMPING: u64 = 0x19f0_d43f_2409_4b9d;
    /// `WaypointsComponent`, on the child entity [`WAYPOINTS_CHILD`].
    pub const WAYPOINTS: u64 = 0x7224_4ba4_bb4e_77da;
    /// The XOR of the player prefab's main entity and the child that holds its waypoints.
    pub const WAYPOINTS_CHILD: u64 = 0x3246_6444_0d3d_1c2a;
}

/// `PlayerHousingComponentServerFacet_ClientRequestFastTravelTerritoryLandmark`.
pub const FAST_TRAVEL_LANDMARK_TYPE_INDEX: u64 = 3713;
/// `PlayerHousingComponentServerFacet_ClientRequestFastTravelHome`.
pub const FAST_TRAVEL_HOME_TYPE_INDEX: u64 = 3712;
/// `PlayerHousingComponentServerFacet_ClientRequestCancelFastTravel`.
pub const CANCEL_FAST_TRAVEL_TYPE_INDEX: u64 = 6701;
/// `PlayerHousingComponentServerFacet_SendFastTravelTimeoutTelemetry`.
pub const FAST_TRAVEL_TIMEOUT_TYPE_INDEX: u64 = 6782;
/// `PlayerHousingComponentServerFacet_ClientRequestResetCooldown`.
pub const RESET_COOLDOWN_TYPE_INDEX: u64 = 1998;
/// `WaypointsComponentServerFacet_RequestSetWaypoint`.
pub const SET_WAYPOINT_TYPE_INDEX: u64 = 2728;
/// `UnstuckComponentServerFacet_OnClientRequestUnstuck`.
pub const UNSTUCK_REQUEST_TYPE_INDEX: u64 = 388;
/// `CampingComponentServerFacet_RequestSetCampSkinId`.
pub const SET_CAMP_SKIN_TYPE_INDEX: u64 = 742;
/// `PlayerHousingComponentServerFacet_ClientRequestEnterPlot`.
pub const ENTER_PLOT_TYPE_INDEX: u64 = 544;
/// `PlayerHousingComponentServerFacet_ClientRequestExitPlot`.
pub const EXIT_PLOT_TYPE_INDEX: u64 = 2149;
/// `PlayerHousingComponentServerFacet_ClientRequestBuyHome`.
pub const BUY_HOME_TYPE_INDEX: u64 = 3700;
/// `PlayerHousingComponentServerFacet_ClientRequestAbandonHome`.
pub const ABANDON_HOME_TYPE_INDEX: u64 = 3701;
/// `PlayerHousingComponentServerFacet_ClientRequestPayTaxes`.
pub const PAY_TAXES_TYPE_INDEX: u64 = 3716;
/// `PlayerHousingComponentServerFacet_ClientRequestTaxesDue`.
pub const TAXES_DUE_REQUEST_TYPE_INDEX: u64 = 3081;
/// `PlayerHousingComponentServerFacet_ClientRequestInteractingPlotAdditionalInfo`.
pub const PLOT_INFO_REQUEST_TYPE_INDEX: u64 = 3717;
/// `PlayerHousingComponentServerFacet_RequestTerritoryResidentData`.
pub const RESIDENT_DATA_REQUEST_TYPE_INDEX: u64 = 3606;
/// `PlayerHousingComponentServerFacet_InteractWithHousingItem`.
pub const INTERACT_HOUSING_ITEM_TYPE_INDEX: u64 = 3730;
/// `PlayerHousingComponentServerFacet_OnInteractWithHousingItemEnded`.
pub const HOUSING_ITEM_INTERACTION_ENDED_TYPE_INDEX: u64 = 695;
/// `PlayerHousingComponentServerFacet_PlaceNewItem`.
pub const PLACE_ITEM_TYPE_INDEX: u64 = 3277;
/// `PlayerHousingComponentServerFacet_MoveItem`.
pub const MOVE_ITEM_TYPE_INDEX: u64 = 3723;
/// `PlayerHousingComponentServerFacet_RemoveItem`.
pub const REMOVE_ITEM_TYPE_INDEX: u64 = 1435;
/// `PlayerHousingComponentServerFacet_ClientRequestRemoveAllFurniture`.
pub const REMOVE_ALL_FURNITURE_TYPE_INDEX: u64 = 5490;

/// `PlayerHousingComponentClientFacet_FastTravelChannelingCompleted`.
pub const FAST_TRAVEL_COMPLETED_TYPE_INDEX: u64 = 3696;
/// `PlayerHousingComponentClientFacet_FastTravelRequestFailed`.
pub const FAST_TRAVEL_FAILED_TYPE_INDEX: u64 = 3042;
/// `UnstuckComponentClientFacet_OnUnstuckRequestAccepted`.
pub const UNSTUCK_ACCEPTED_TYPE_INDEX: u64 = 3980;
/// `UnstuckComponentClientFacet_OnUnstuckTeleportComplete`.
pub const UNSTUCK_COMPLETED_TYPE_INDEX: u64 = 2190;
/// `UnstuckComponentClientFacet_OnUnstuckRequestDenied`.
pub const UNSTUCK_DENIED_TYPE_INDEX: u64 = 2774;
/// `PlayerComponentClientFacet_OnPlayerTeleport`.
pub const PLAYER_TELEPORT_TYPE_INDEX: u64 = 3954;
/// `PlayerComponentClientFacet_OnTeleportRequestFailedOrCancelled`.
pub const TELEPORT_FAILED_TYPE_INDEX: u64 = 7022;
/// `CampingComponentClientFacet_DemolishCamp`.
pub const DEMOLISH_CAMP_TYPE_INDEX: u64 = 6760;
/// `CampingComponentClientFacet_CampReconnectFailure`.
pub const CAMP_RECONNECT_FAILURE_TYPE_INDEX: u64 = 4249;
/// `PlayerHousingComponentClientFacet_OnHouseInfoResponse`.
pub const HOUSE_INFO_TYPE_INDEX: u64 = 3692;
/// `PlayerHousingComponentClientFacet_ClientReceiveTaxesDue`.
pub const TAXES_DUE_TYPE_INDEX: u64 = 3694;
/// `PlayerHousingComponentClientFacet_PayTaxRequestFinished`.
pub const PAY_TAX_FINISHED_TYPE_INDEX: u64 = 952;
/// `PlayerHousingComponentClientFacet_PlaceNewItemResult`.
pub const PLACE_ITEM_RESULT_TYPE_INDEX: u64 = 3691;
/// `PlayerHousingComponentClientFacet_MoveItemResult`.
pub const MOVE_ITEM_RESULT_TYPE_INDEX: u64 = 892;
/// `PlayerHousingComponentClientFacet_RemoveItemResult`.
pub const REMOVE_ITEM_RESULT_TYPE_INDEX: u64 = 3046;
/// `PlayerHousingComponentClientFacet_SendItemsRefundedNotification`.
pub const ITEMS_REFUNDED_TYPE_INDEX: u64 = 3733;
/// `PlayerHousingComponentClientFacet_OnResidentDataReceived`.
pub const RESIDENT_DATA_TYPE_INDEX: u64 = 3695;
/// `BuilderComponentServerFacet_RequestPlaceStructure` (a camp's placement).
pub const PLACE_STRUCTURE_TYPE_INDEX: u64 = 2236;
/// `BuilderComponentServerFacet_RequestCancelPlacingStructure`.
pub const CANCEL_PLACING_STRUCTURE_TYPE_INDEX: u64 = 1507;
/// `BuilderComponentClientFacet_SyncPlacementRequestResults`.
pub const PLACEMENT_RESULTS_TYPE_INDEX: u64 = 441;
/// The builder component of the player: the target of 2236, 1507 and 441 less the entity
/// (`Hive_camping`: 5 placements, 1 cancel and 5 answers, one value; the same session's housing
/// headers give the known [`component::PLAYER_HOUSING`]).
pub const BUILDER_COMPONENT: u64 = 0x8f08_486b_2ee1_2929;
/// Most entries of a resident data map or status effect list (the replicated list limit).
const MAX_RESIDENT_ENTRIES: u64 = 0x200_0000;

/// Type index of `MB::WaypointsComponentReplicatedState`.
pub const WAYPOINTS_TYPE_INDEX: u64 = 4321;
/// The player component of the waypoints state (component-slots note).
pub const WAYPOINTS_COMPONENT: u64 = 50;
/// The children of the waypoints state that retail sends: child 0, the marker.
const WAYPOINTS_CHILDREN: u8 = 1;
/// The field name of the waypoints state in errors.
const WAYPOINTS_FIELD: &str = "waypoints";

/// The flag byte of an item anchor that follows.
const ANCHOR_PRESENT: u8 = 1;
/// The flag byte of no item anchor.
const ANCHOR_ABSENT: u8 = 0;

/// Coordinates of a position.
const POSITION_LEN: usize = 3;
/// Parts of a rotation quaternion.
const ROTATION_LEN: usize = 4;

/// A territory landmark the client asks to fast travel to (the client's `TerritoryLandmarkData`
/// key): the landmark's territory, its type and its world position. Retail: shrines of territory
/// 20422 type 27, settlements type 1, the client sends the landmark's x and y with z 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandmarkRef {
    /// The `TerritoryID` of the landmark's territory.
    pub territory: u16,
    /// The landmark type (`eTerritoryLandmarkType`).
    pub landmark_type: u8,
    /// Its position, metres (z is 0 in every retail body).
    pub position: [f32; POSITION_LEN],
}

/// A level entity a housing call names: its raw id, its facet entity and an id of unknown
/// meaning (the plot calls 3712, 3716, 3081, 1998, 2149 and the house info 3692).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlotRef {
    /// The raw id; all zero for none (2149 outside a house).
    pub id: [u8; RAW_ID_LEN],
    /// Its facet entity; `0xffff_ffff_0000_0000` with an all-zero id.
    pub facet: u64,
    /// An id of unknown meaning (0 with an all-zero id).
    pub extra: u64,
}

impl PlotRef {
    /// Whether the call names no plot (all-zero id).
    pub fn is_none(&self) -> bool {
        self.id == [0; RAW_ID_LEN]
    }

    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            id: reader.raw_id()?,
            facet: reader.u64()?,
            extra: reader.u64()?,
        })
    }

    pub(crate) fn write(&self, writer: &mut Writer) {
        writer.bytes(&self.id).u64(self.facet).u64(self.extra);
    }
}

/// Where a piece of furniture goes: its position and rotation in the home's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemPlacement {
    /// Its position, metres.
    pub position: [f32; POSITION_LEN],
    /// Its rotation quaternion (x, y, z, w).
    pub rotation: [f32; ROTATION_LEN],
}

impl ItemPlacement {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            position: [reader.f32()?, reader.f32()?, reader.f32()?],
            rotation: [reader.f32()?, reader.f32()?, reader.f32()?, reader.f32()?],
        })
    }

    fn write(&self, writer: &mut Writer) {
        for value in self.position.iter().chain(&self.rotation) {
            writer.f32(*value);
        }
    }
}

/// The optional tail of a furniture call: a flag, then a `u16` and a byte. Retail: `ffff` and 0
/// on both placements, no tail on the move and the remove; the meaning is Unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemAnchor {
    /// The `u16` (0xffff in the captures).
    pub parent: u16,
    /// The byte (0 in the captures).
    pub extra: u8,
}

fn read_anchor(reader: &mut Reader<'_>) -> Result<Option<ItemAnchor>, MessageError> {
    match reader.u8()? {
        ANCHOR_ABSENT => Ok(None),
        ANCHOR_PRESENT => Ok(Some(ItemAnchor {
            parent: reader.u16()?,
            extra: reader.u8()?,
        })),
        value => Err(MessageError::NotBoolean { value }),
    }
}

fn write_anchor(writer: &mut Writer, anchor: Option<ItemAnchor>) {
    match anchor {
        Some(anchor) => writer.u8(ANCHOR_PRESENT).u16(anchor.parent).u8(anchor.extra),
        None => writer.u8(ANCHOR_ABSENT),
    };
}

fn read_position(reader: &mut Reader<'_>) -> Result<[f32; POSITION_LEN], MessageError> {
    Ok([reader.f32()?, reader.f32()?, reader.f32()?])
}

fn write_position(writer: &mut Writer, position: &[f32; POSITION_LEN]) {
    for value in position {
        writer.f32(*value);
    }
}

/// One travel call of the client.
#[derive(Debug, Clone, PartialEq)]
pub enum TravelClientMsg {
    /// 3713: fast travel to a territory landmark (a shrine, a settlement).
    FastTravelLandmark {
        /// The header.
        header: FacetHeader,
        /// The landmark.
        landmark: LandmarkRef,
    },
    /// 3712: fast travel to the home on a plot.
    FastTravelHome {
        /// The header.
        header: FacetHeader,
        /// The plot.
        plot: PlotRef,
    },
    /// 6701: the client cancels its fast travel.
    CancelFastTravel {
        /// The header.
        header: FacetHeader,
    },
    /// 6782: the client's fast travel timed out (retail sends no answer).
    FastTravelTimeout {
        /// The header.
        header: FacetHeader,
    },
    /// 1998: reset the home fast travel cooldown of a plot.
    ResetCooldown {
        /// The header.
        header: FacetHeader,
        /// The plot.
        plot: PlotRef,
    },
    /// 2728: set the map marker.
    SetWaypoint {
        /// The header (the waypoints component of a child entity).
        header: FacetHeader,
        /// The marker, metres (z is 0 in every retail body).
        position: [f32; POSITION_LEN],
    },
    /// 388: ask to be unstuck.
    Unstuck {
        /// The header.
        header: FacetHeader,
    },
    /// 742: set the camp skin.
    SetCampSkin {
        /// The header.
        header: FacetHeader,
        /// The skin.
        skin: u32,
    },
    /// 544: enter a house plot.
    EnterPlot {
        /// The header.
        header: FacetHeader,
        /// The raw id of the house entity.
        id: [u8; RAW_ID_LEN],
        /// Its facet entity.
        facet: u64,
    },
    /// 2149: leave the house plot the player is in (an all-zero plot outside one).
    ExitPlot {
        /// The header.
        header: FacetHeader,
        /// The plot.
        plot: PlotRef,
    },
    /// 3700: buy the home on the plot the player interacts with.
    BuyHome {
        /// The header.
        header: FacetHeader,
    },
    /// 3701: give up the home on the plot the player interacts with.
    AbandonHome {
        /// The header.
        header: FacetHeader,
    },
    /// 3716: pay the taxes of the home on a plot.
    PayTaxes {
        /// The header.
        header: FacetHeader,
        /// The plot.
        plot: PlotRef,
    },
    /// 3081: ask for the taxes due on a plot.
    TaxesDue {
        /// The header.
        header: FacetHeader,
        /// The plot.
        plot: PlotRef,
    },
    /// 3717: ask for the information of the plot the player interacts with.
    PlotInfo {
        /// The header.
        header: FacetHeader,
    },
    /// 3606: ask for the territory's resident list.
    ResidentData {
        /// The header.
        header: FacetHeader,
    },
    /// 3730: use a piece of furniture (sit, lie down).
    InteractHousingItem {
        /// The header.
        header: FacetHeader,
        /// The item index.
        index: u16,
    },
    /// 695: the use of a piece of furniture ended.
    HousingItemInteractionEnded {
        /// The header.
        header: FacetHeader,
    },
    /// 3277: place a new piece of furniture.
    PlaceItem {
        /// The header.
        header: FacetHeader,
        /// The item.
        item: u32,
        /// Where it goes.
        placement: ItemPlacement,
        /// The optional tail.
        anchor: Option<ItemAnchor>,
    },
    /// 3723: move a placed piece of furniture.
    MoveItem {
        /// The header.
        header: FacetHeader,
        /// The item index.
        index: u16,
        /// Where it goes.
        placement: ItemPlacement,
        /// The optional tail.
        anchor: Option<ItemAnchor>,
    },
    /// 1435: pick a placed piece of furniture up.
    RemoveItem {
        /// The header.
        header: FacetHeader,
        /// The item index.
        index: u16,
        /// The optional tail.
        anchor: Option<ItemAnchor>,
    },
    /// 5490: pick every placed piece of furniture up.
    RemoveAllFurniture {
        /// The header.
        header: FacetHeader,
    },
    /// 2236: place a structure (a camp) from build mode.
    PlaceStructure {
        /// The header (the player's builder component).
        header: FacetHeader,
        /// The structure.
        placement: StructurePlacement,
    },
    /// 1507: leave build mode without placing.
    CancelPlacingStructure {
        /// The header.
        header: FacetHeader,
    },
}

/// A structure placement (2236; retail `Hive_camping`, 5 camp placements, 73 bytes each).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StructurePlacement {
    /// Where it goes, metres.
    pub position: [f32; POSITION_LEN],
    /// Its rotation quaternion (x, y, z, w).
    pub rotation: [f32; ROTATION_LEN],
    /// The camp skin: the CRC-32 of the lowercase `CampSkinID` (the 742 skin of the session).
    pub skin: u32,
    /// The structure's id (the same 16 bytes in all 5 retail placements; meaning Unknown).
    pub structure: [u8; RAW_ID_LEN],
    /// The placing player's id (the header's halves swapped in every retail body).
    pub owner: [u8; RAW_ID_LEN],
    /// A number of unknown meaning (`0xffffffff` in every retail body).
    pub socket: u32,
    /// A number of unknown meaning (50 in every retail body).
    pub value: u32,
    /// A flag of unknown meaning (1 in every retail body).
    pub flag: bool,
}

impl StructurePlacement {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            position: read_position(reader)?,
            rotation: [reader.f32()?, reader.f32()?, reader.f32()?, reader.f32()?],
            skin: reader.u32()?,
            structure: reader.raw_id()?,
            owner: reader.raw_id()?,
            socket: reader.u32()?,
            value: reader.u32()?,
            flag: reader.boolean()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        write_position(writer, &self.position);
        for value in &self.rotation {
            writer.f32(*value);
        }
        writer
            .u32(self.skin)
            .bytes(&self.structure)
            .bytes(&self.owner)
            .u32(self.socket)
            .u32(self.value)
            .boolean(self.flag);
    }
}

impl TravelClientMsg {
    /// Decodes `body` when `type_index` is a travel call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |read_rest: fn(FacetHeader, &mut Reader<'_>) -> Result<Self, MessageError>| {
            let mut reader = Reader::new(body);
            let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
            reader.finish()?;
            Ok(message)
        };
        Some(match type_index {
            FAST_TRAVEL_LANDMARK_TYPE_INDEX => read(|header, reader| {
                Ok(Self::FastTravelLandmark {
                    header,
                    landmark: LandmarkRef {
                        territory: reader.u16()?,
                        landmark_type: reader.u8()?,
                        position: read_position(reader)?,
                    },
                })
            }),
            FAST_TRAVEL_HOME_TYPE_INDEX => read(|header, reader| {
                Ok(Self::FastTravelHome {
                    header,
                    plot: PlotRef::read(reader)?,
                })
            }),
            CANCEL_FAST_TRAVEL_TYPE_INDEX => {
                read(|header, _| Ok(Self::CancelFastTravel { header }))
            }
            FAST_TRAVEL_TIMEOUT_TYPE_INDEX => {
                read(|header, _| Ok(Self::FastTravelTimeout { header }))
            }
            RESET_COOLDOWN_TYPE_INDEX => read(|header, reader| {
                Ok(Self::ResetCooldown {
                    header,
                    plot: PlotRef::read(reader)?,
                })
            }),
            SET_WAYPOINT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SetWaypoint {
                    header,
                    position: read_position(reader)?,
                })
            }),
            UNSTUCK_REQUEST_TYPE_INDEX => read(|header, _| Ok(Self::Unstuck { header })),
            SET_CAMP_SKIN_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SetCampSkin {
                    header,
                    skin: reader.u32()?,
                })
            }),
            ENTER_PLOT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::EnterPlot {
                    header,
                    id: reader.raw_id()?,
                    facet: reader.u64()?,
                })
            }),
            EXIT_PLOT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::ExitPlot {
                    header,
                    plot: PlotRef::read(reader)?,
                })
            }),
            BUY_HOME_TYPE_INDEX => read(|header, _| Ok(Self::BuyHome { header })),
            ABANDON_HOME_TYPE_INDEX => read(|header, _| Ok(Self::AbandonHome { header })),
            PAY_TAXES_TYPE_INDEX => read(|header, reader| {
                Ok(Self::PayTaxes {
                    header,
                    plot: PlotRef::read(reader)?,
                })
            }),
            TAXES_DUE_REQUEST_TYPE_INDEX => read(|header, reader| {
                Ok(Self::TaxesDue {
                    header,
                    plot: PlotRef::read(reader)?,
                })
            }),
            PLOT_INFO_REQUEST_TYPE_INDEX => read(|header, _| Ok(Self::PlotInfo { header })),
            RESIDENT_DATA_REQUEST_TYPE_INDEX => read(|header, _| Ok(Self::ResidentData { header })),
            INTERACT_HOUSING_ITEM_TYPE_INDEX => read(|header, reader| {
                Ok(Self::InteractHousingItem {
                    header,
                    index: reader.u16()?,
                })
            }),
            HOUSING_ITEM_INTERACTION_ENDED_TYPE_INDEX => {
                read(|header, _| Ok(Self::HousingItemInteractionEnded { header }))
            }
            PLACE_ITEM_TYPE_INDEX => read(|header, reader| {
                Ok(Self::PlaceItem {
                    header,
                    item: reader.u32()?,
                    placement: ItemPlacement::read(reader)?,
                    anchor: read_anchor(reader)?,
                })
            }),
            MOVE_ITEM_TYPE_INDEX => read(|header, reader| {
                Ok(Self::MoveItem {
                    header,
                    index: reader.u16()?,
                    placement: ItemPlacement::read(reader)?,
                    anchor: read_anchor(reader)?,
                })
            }),
            REMOVE_ITEM_TYPE_INDEX => read(|header, reader| {
                Ok(Self::RemoveItem {
                    header,
                    index: reader.u16()?,
                    anchor: read_anchor(reader)?,
                })
            }),
            REMOVE_ALL_FURNITURE_TYPE_INDEX => {
                read(|header, _| Ok(Self::RemoveAllFurniture { header }))
            }
            PLACE_STRUCTURE_TYPE_INDEX => read(|header, reader| {
                Ok(Self::PlaceStructure {
                    header,
                    placement: StructurePlacement::read(reader)?,
                })
            }),
            CANCEL_PLACING_STRUCTURE_TYPE_INDEX => {
                read(|header, _| Ok(Self::CancelPlacingStructure { header }))
            }
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::FastTravelLandmark { .. } => FAST_TRAVEL_LANDMARK_TYPE_INDEX,
            Self::FastTravelHome { .. } => FAST_TRAVEL_HOME_TYPE_INDEX,
            Self::CancelFastTravel { .. } => CANCEL_FAST_TRAVEL_TYPE_INDEX,
            Self::FastTravelTimeout { .. } => FAST_TRAVEL_TIMEOUT_TYPE_INDEX,
            Self::ResetCooldown { .. } => RESET_COOLDOWN_TYPE_INDEX,
            Self::SetWaypoint { .. } => SET_WAYPOINT_TYPE_INDEX,
            Self::Unstuck { .. } => UNSTUCK_REQUEST_TYPE_INDEX,
            Self::SetCampSkin { .. } => SET_CAMP_SKIN_TYPE_INDEX,
            Self::EnterPlot { .. } => ENTER_PLOT_TYPE_INDEX,
            Self::ExitPlot { .. } => EXIT_PLOT_TYPE_INDEX,
            Self::BuyHome { .. } => BUY_HOME_TYPE_INDEX,
            Self::AbandonHome { .. } => ABANDON_HOME_TYPE_INDEX,
            Self::PayTaxes { .. } => PAY_TAXES_TYPE_INDEX,
            Self::TaxesDue { .. } => TAXES_DUE_REQUEST_TYPE_INDEX,
            Self::PlotInfo { .. } => PLOT_INFO_REQUEST_TYPE_INDEX,
            Self::ResidentData { .. } => RESIDENT_DATA_REQUEST_TYPE_INDEX,
            Self::InteractHousingItem { .. } => INTERACT_HOUSING_ITEM_TYPE_INDEX,
            Self::HousingItemInteractionEnded { .. } => HOUSING_ITEM_INTERACTION_ENDED_TYPE_INDEX,
            Self::PlaceItem { .. } => PLACE_ITEM_TYPE_INDEX,
            Self::MoveItem { .. } => MOVE_ITEM_TYPE_INDEX,
            Self::RemoveItem { .. } => REMOVE_ITEM_TYPE_INDEX,
            Self::RemoveAllFurniture { .. } => REMOVE_ALL_FURNITURE_TYPE_INDEX,
            Self::PlaceStructure { .. } => PLACE_STRUCTURE_TYPE_INDEX,
            Self::CancelPlacingStructure { .. } => CANCEL_PLACING_STRUCTURE_TYPE_INDEX,
        }
    }

    /// The header of the call.
    pub const fn header(&self) -> &FacetHeader {
        match self {
            Self::FastTravelLandmark { header, .. }
            | Self::FastTravelHome { header, .. }
            | Self::CancelFastTravel { header }
            | Self::FastTravelTimeout { header }
            | Self::ResetCooldown { header, .. }
            | Self::SetWaypoint { header, .. }
            | Self::Unstuck { header }
            | Self::SetCampSkin { header, .. }
            | Self::EnterPlot { header, .. }
            | Self::ExitPlot { header, .. }
            | Self::BuyHome { header }
            | Self::AbandonHome { header }
            | Self::PayTaxes { header, .. }
            | Self::TaxesDue { header, .. }
            | Self::PlotInfo { header }
            | Self::ResidentData { header }
            | Self::InteractHousingItem { header, .. }
            | Self::HousingItemInteractionEnded { header }
            | Self::PlaceItem { header, .. }
            | Self::MoveItem { header, .. }
            | Self::RemoveItem { header, .. }
            | Self::RemoveAllFurniture { header }
            | Self::PlaceStructure { header, .. }
            | Self::CancelPlacingStructure { header } => header,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        match self {
            Self::FastTravelLandmark { landmark, .. } => {
                writer.u16(landmark.territory).u8(landmark.landmark_type);
                write_position(&mut writer, &landmark.position);
            }
            Self::FastTravelHome { plot, .. }
            | Self::ResetCooldown { plot, .. }
            | Self::ExitPlot { plot, .. }
            | Self::PayTaxes { plot, .. }
            | Self::TaxesDue { plot, .. } => plot.write(&mut writer),
            Self::SetWaypoint { position, .. } => write_position(&mut writer, position),
            Self::SetCampSkin { skin, .. } => {
                writer.u32(*skin);
            }
            Self::EnterPlot { id, facet, .. } => {
                writer.bytes(id).u64(*facet);
            }
            Self::InteractHousingItem { index, .. } => {
                writer.u16(*index);
            }
            Self::PlaceItem {
                item,
                placement,
                anchor,
                ..
            } => {
                writer.u32(*item);
                placement.write(&mut writer);
                write_anchor(&mut writer, *anchor);
            }
            Self::MoveItem {
                index,
                placement,
                anchor,
                ..
            } => {
                writer.u16(*index);
                placement.write(&mut writer);
                write_anchor(&mut writer, *anchor);
            }
            Self::RemoveItem { index, anchor, .. } => {
                writer.u16(*index);
                write_anchor(&mut writer, *anchor);
            }
            Self::PlaceStructure { placement, .. } => placement.write(&mut writer),
            Self::CancelFastTravel { .. }
            | Self::CancelPlacingStructure { .. }
            | Self::FastTravelTimeout { .. }
            | Self::Unstuck { .. }
            | Self::BuyHome { .. }
            | Self::AbandonHome { .. }
            | Self::PlotInfo { .. }
            | Self::ResidentData { .. }
            | Self::HousingItemInteractionEnded { .. }
            | Self::RemoveAllFurniture { .. } => {}
        }
        writer.finish()
    }
}

/// Why an unstuck request is denied (2774; the client's message table `FUN_1468a01d0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnstuckDenial {
    /// `@ui_unstuck_request_denied_on_cooldown`: the cooldown runs.
    OnCooldown,
    /// `@ui_unstuck_request_denied_player_dead`.
    PlayerDead,
    /// `@ui_unstuck_request_denied_request_already_active`.
    RequestAlreadyActive,
    /// `@ui_unstuck_request_denied_no_breadcrumbs`: no safe place is known.
    NoBreadcrumbs,
    /// `@ui_unstuck_request_denied_teleport_failed`: the move at the end failed.
    TeleportFailed,
    /// `@ui_unstuck_request_denied_in_musical_performance`.
    InMusicalPerformance,
}

impl UnstuckDenial {
    /// Every reason, in the order of the client's numbers.
    const ALL: [Self; 6] = [
        Self::OnCooldown,
        Self::PlayerDead,
        Self::RequestAlreadyActive,
        Self::NoBreadcrumbs,
        Self::TeleportFailed,
        Self::InMusicalPerformance,
    ];

    /// The reason's number on the wire.
    pub const fn number(self) -> u32 {
        self as u32
    }

    /// The reason of number `number`.
    pub fn of_number(number: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|reason| reason.number() == number)
    }
}

/// One travel answer of the server.
#[derive(Debug, Clone, PartialEq)]
pub enum TravelServerMsg {
    /// 3696: the fast travel's channeling ended; the loading screen (3951) goes before it.
    FastTravelCompleted {
        /// The header.
        header: FacetHeader,
    },
    /// 3042: the fast travel was refused or cancelled.
    FastTravelFailed {
        /// The header.
        header: FacetHeader,
    },
    /// 3980: the unstuck request is accepted; the move comes after `delay_ns`.
    UnstuckAccepted {
        /// The header.
        header: FacetHeader,
        /// The wait before the move, nanoseconds (10 s in every retail body).
        delay_ns: u64,
    },
    /// 2190: the unstuck move happened.
    UnstuckCompleted {
        /// The header.
        header: FacetHeader,
    },
    /// 2774: the unstuck request is denied.
    UnstuckDenied {
        /// The header.
        header: FacetHeader,
        /// Why.
        reason: UnstuckDenial,
        /// The cooldown left, nanoseconds; negative once it ended (retail sends the end less now
        /// with every reason, 0 after a failed move).
        cooldown_ns: i64,
    },
    /// 3954: the player is teleported.
    PlayerTeleport {
        /// The header.
        header: FacetHeader,
    },
    /// 7022: a teleport request failed or was cancelled.
    TeleportFailedOrCancelled {
        /// The header.
        header: FacetHeader,
    },
    /// 6760: the player's camp is gone.
    DemolishCamp {
        /// The header.
        header: FacetHeader,
    },
    /// 4249: the camp could not be reconnected.
    CampReconnectFailure {
        /// The header.
        header: FacetHeader,
        /// A byte of unknown meaning (1 in the captures).
        flag: u8,
    },
    /// 3692: the information of a house plot.
    HouseInfo {
        /// The header.
        header: FacetHeader,
        /// When the top houses are evaluated next, Unix ns (the client's
        /// `Hud.Housing.Enter.NextRecompute`; retail steps of 172800 s,
        /// `javelin.housing.evaluate-topHouses-delay`).
        next_recompute_ns: u64,
        /// The plot's house score (`Hud.Housing.Enter.HouseScore`; retail 153 for a decorated
        /// house, 89 for another, 0 after its furniture was taken).
        house_score: u32,
        /// The plot.
        plot: PlotRef,
    },
    /// 3694: the taxes due on the plot asked for.
    TaxesDue {
        /// The header.
        header: FacetHeader,
        /// The amount.
        amount: u32,
    },
    /// 952: the taxes were paid.
    PayTaxFinished {
        /// The header.
        header: FacetHeader,
        /// Whether the payment went through (1 in every capture).
        paid: bool,
    },
    /// 3691: the result of a furniture placement.
    PlaceItemResult {
        /// The header.
        header: FacetHeader,
        /// The result (0 in every capture: placed).
        result: u8,
        /// The new item's index.
        index: u16,
    },
    /// 892: the result of a furniture move.
    MoveItemResult {
        /// The header.
        header: FacetHeader,
        /// The result (0: moved).
        result: u8,
        /// The item index.
        index: u16,
    },
    /// 3046: the result of a furniture pick-up.
    RemoveItemResult {
        /// The header.
        header: FacetHeader,
        /// The result (0: picked up).
        result: u8,
        /// The item index.
        index: u16,
    },
    /// 3695: the territory data of the territories the player resides in.
    ResidentData {
        /// The header.
        header: FacetHeader,
        /// Each territory's `TerritoryID` and data, in key order.
        territories: Vec<(u16, TerritoryResidentData)>,
    },
    /// 441: the result of a structure placement (2236).
    PlacementResults {
        /// The header (the player's builder component).
        header: FacetHeader,
        /// The result (0: placed, every retail body).
        result: u8,
        /// A number of unknown meaning (0 in every retail body).
        value: u32,
    },
    /// 3733: items were given back.
    ItemsRefunded {
        /// The header.
        header: FacetHeader,
        /// An id of unknown meaning.
        id: [u8; RAW_ID_LEN],
        /// A count of unknown meaning.
        count: u16,
    },
}

impl TravelServerMsg {
    /// Whether `type_index` is a travel answer.
    pub const fn is_type(type_index: u64) -> bool {
        matches!(
            type_index,
            FAST_TRAVEL_COMPLETED_TYPE_INDEX
                | FAST_TRAVEL_FAILED_TYPE_INDEX
                | UNSTUCK_ACCEPTED_TYPE_INDEX
                | UNSTUCK_COMPLETED_TYPE_INDEX
                | UNSTUCK_DENIED_TYPE_INDEX
                | PLAYER_TELEPORT_TYPE_INDEX
                | TELEPORT_FAILED_TYPE_INDEX
                | DEMOLISH_CAMP_TYPE_INDEX
                | CAMP_RECONNECT_FAILURE_TYPE_INDEX
                | HOUSE_INFO_TYPE_INDEX
                | TAXES_DUE_TYPE_INDEX
                | PAY_TAX_FINISHED_TYPE_INDEX
                | PLACE_ITEM_RESULT_TYPE_INDEX
                | MOVE_ITEM_RESULT_TYPE_INDEX
                | REMOVE_ITEM_RESULT_TYPE_INDEX
                | ITEMS_REFUNDED_TYPE_INDEX
                | RESIDENT_DATA_TYPE_INDEX
                | PLACEMENT_RESULTS_TYPE_INDEX
        )
    }

    /// Decodes `body` of travel answer `type_index`.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body does not read, and [`MessageError::BadTag`] for a
    /// type that is not a travel answer.
    pub fn decode(type_index: u64, body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let message = match type_index {
            FAST_TRAVEL_COMPLETED_TYPE_INDEX => Self::FastTravelCompleted { header },
            FAST_TRAVEL_FAILED_TYPE_INDEX => Self::FastTravelFailed { header },
            UNSTUCK_ACCEPTED_TYPE_INDEX => Self::UnstuckAccepted {
                header,
                delay_ns: reader.u64()?,
            },
            UNSTUCK_COMPLETED_TYPE_INDEX => Self::UnstuckCompleted { header },
            UNSTUCK_DENIED_TYPE_INDEX => {
                let number = reader.u32()?;
                let reason = UnstuckDenial::of_number(number).ok_or(MessageError::BadTag {
                    field: "unstuck denial reason",
                    tag: u8::try_from(number).unwrap_or(u8::MAX),
                })?;
                Self::UnstuckDenied {
                    header,
                    reason,
                    cooldown_ns: i64::from_be_bytes(reader.array()?),
                }
            }
            PLAYER_TELEPORT_TYPE_INDEX => Self::PlayerTeleport { header },
            TELEPORT_FAILED_TYPE_INDEX => Self::TeleportFailedOrCancelled { header },
            DEMOLISH_CAMP_TYPE_INDEX => Self::DemolishCamp { header },
            CAMP_RECONNECT_FAILURE_TYPE_INDEX => Self::CampReconnectFailure {
                header,
                flag: reader.u8()?,
            },
            HOUSE_INFO_TYPE_INDEX => Self::HouseInfo {
                header,
                next_recompute_ns: reader.u64()?,
                house_score: reader.u32()?,
                plot: PlotRef::read(&mut reader)?,
            },
            TAXES_DUE_TYPE_INDEX => Self::TaxesDue {
                header,
                amount: reader.u32()?,
            },
            PAY_TAX_FINISHED_TYPE_INDEX => Self::PayTaxFinished {
                header,
                paid: reader.boolean()?,
            },
            PLACE_ITEM_RESULT_TYPE_INDEX => Self::PlaceItemResult {
                header,
                result: reader.u8()?,
                index: reader.u16()?,
            },
            MOVE_ITEM_RESULT_TYPE_INDEX => Self::MoveItemResult {
                header,
                result: reader.u8()?,
                index: reader.u16()?,
            },
            REMOVE_ITEM_RESULT_TYPE_INDEX => Self::RemoveItemResult {
                header,
                result: reader.u8()?,
                index: reader.u16()?,
            },
            RESIDENT_DATA_TYPE_INDEX => Self::ResidentData {
                header,
                territories: read_resident_data(&mut reader)?,
            },
            PLACEMENT_RESULTS_TYPE_INDEX => Self::PlacementResults {
                header,
                result: reader.u8()?,
                value: reader.u32()?,
            },
            ITEMS_REFUNDED_TYPE_INDEX => Self::ItemsRefunded {
                header,
                id: reader.raw_id()?,
                count: reader.u16()?,
            },
            _ => {
                return Err(MessageError::BadTag {
                    field: "travel answer type",
                    tag: u8::MAX,
                });
            }
        };
        reader.finish()?;
        Ok(message)
    }

    /// The type index of the answer.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::FastTravelCompleted { .. } => FAST_TRAVEL_COMPLETED_TYPE_INDEX,
            Self::FastTravelFailed { .. } => FAST_TRAVEL_FAILED_TYPE_INDEX,
            Self::UnstuckAccepted { .. } => UNSTUCK_ACCEPTED_TYPE_INDEX,
            Self::UnstuckCompleted { .. } => UNSTUCK_COMPLETED_TYPE_INDEX,
            Self::UnstuckDenied { .. } => UNSTUCK_DENIED_TYPE_INDEX,
            Self::PlayerTeleport { .. } => PLAYER_TELEPORT_TYPE_INDEX,
            Self::TeleportFailedOrCancelled { .. } => TELEPORT_FAILED_TYPE_INDEX,
            Self::DemolishCamp { .. } => DEMOLISH_CAMP_TYPE_INDEX,
            Self::CampReconnectFailure { .. } => CAMP_RECONNECT_FAILURE_TYPE_INDEX,
            Self::HouseInfo { .. } => HOUSE_INFO_TYPE_INDEX,
            Self::TaxesDue { .. } => TAXES_DUE_TYPE_INDEX,
            Self::PayTaxFinished { .. } => PAY_TAX_FINISHED_TYPE_INDEX,
            Self::PlaceItemResult { .. } => PLACE_ITEM_RESULT_TYPE_INDEX,
            Self::MoveItemResult { .. } => MOVE_ITEM_RESULT_TYPE_INDEX,
            Self::RemoveItemResult { .. } => REMOVE_ITEM_RESULT_TYPE_INDEX,
            Self::ItemsRefunded { .. } => ITEMS_REFUNDED_TYPE_INDEX,
            Self::ResidentData { .. } => RESIDENT_DATA_TYPE_INDEX,
            Self::PlacementResults { .. } => PLACEMENT_RESULTS_TYPE_INDEX,
        }
    }

    /// The header of the answer.
    pub const fn header(&self) -> &FacetHeader {
        match self {
            Self::FastTravelCompleted { header }
            | Self::FastTravelFailed { header }
            | Self::UnstuckAccepted { header, .. }
            | Self::UnstuckCompleted { header }
            | Self::UnstuckDenied { header, .. }
            | Self::PlayerTeleport { header }
            | Self::TeleportFailedOrCancelled { header }
            | Self::DemolishCamp { header }
            | Self::CampReconnectFailure { header, .. }
            | Self::HouseInfo { header, .. }
            | Self::TaxesDue { header, .. }
            | Self::PayTaxFinished { header, .. }
            | Self::PlaceItemResult { header, .. }
            | Self::MoveItemResult { header, .. }
            | Self::RemoveItemResult { header, .. }
            | Self::ItemsRefunded { header, .. }
            | Self::ResidentData { header, .. }
            | Self::PlacementResults { header, .. } => header,
        }
    }

    /// The body bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.header().write(&mut writer);
        match self {
            Self::UnstuckAccepted { delay_ns, .. } => {
                writer.u64(*delay_ns);
            }
            Self::UnstuckDenied {
                reason,
                cooldown_ns,
                ..
            } => {
                writer.u32(reason.number()).bytes(&cooldown_ns.to_be_bytes());
            }
            Self::CampReconnectFailure { flag, .. } => {
                writer.u8(*flag);
            }
            Self::HouseInfo {
                next_recompute_ns,
                house_score,
                plot,
                ..
            } => {
                writer.u64(*next_recompute_ns).u32(*house_score);
                plot.write(&mut writer);
            }
            Self::TaxesDue { amount, .. } => {
                writer.u32(*amount);
            }
            Self::PayTaxFinished { paid, .. } => {
                writer.boolean(*paid);
            }
            Self::PlaceItemResult { result, index, .. }
            | Self::MoveItemResult { result, index, .. }
            | Self::RemoveItemResult { result, index, .. } => {
                writer.u8(*result).u16(*index);
            }
            Self::ItemsRefunded { id, count, .. } => {
                writer.bytes(id).u16(*count);
            }
            Self::ResidentData { territories, .. } => {
                write_resident_data(&mut writer, territories);
            }
            Self::PlacementResults { result, value, .. } => {
                writer.u8(*result).u32(*value);
            }
            Self::FastTravelCompleted { .. }
            | Self::FastTravelFailed { .. }
            | Self::UnstuckCompleted { .. }
            | Self::PlayerTeleport { .. }
            | Self::TeleportFailedOrCancelled { .. }
            | Self::DemolishCamp { .. } => {}
        }
        writer.finish()
    }
}

/// One territory's data of 3695 (`TerritoryResidentData`, reflection `FUN_142f95c90`):
/// `m_territoryActorId` and `m_territoryStatusEffectData`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerritoryResidentData {
    /// The territory's actor entity.
    pub territory_actor: [u8; RAW_ID_LEN],
    /// The territory's status effects: the CRC-32 of each effect id and its end, Unix ns.
    pub status_effects: Vec<(u32, u64)>,
}

fn read_count(reader: &mut Reader<'_>) -> Result<u64, MessageError> {
    let count = reader.wide_prefixed()?;
    if count > MAX_RESIDENT_ENTRIES {
        return Err(MessageError::CountTooLarge {
            count,
            limit: MAX_RESIDENT_ENTRIES,
        });
    }
    Ok(count)
}

/// The 3695 map: the count and the `TerritoryID` keys, then the count and the values (retail
/// `Hive_Social` 3 bodies with 2 territories, `RS_offensive_war` 1 body with 3).
fn read_resident_data(
    reader: &mut Reader<'_>,
) -> Result<Vec<(u16, TerritoryResidentData)>, MessageError> {
    let keys = (0..read_count(reader)?)
        .map(|_| reader.u16())
        .collect::<Result<Vec<_>, _>>()?;
    let values = (0..read_count(reader)?)
        .map(|_| {
            let territory_actor = reader.raw_id()?;
            let status_effects = (0..read_count(reader)?)
                .map(|_| Ok((reader.u32()?, reader.u64()?)))
                .collect::<Result<Vec<_>, MessageError>>()?;
            Ok(TerritoryResidentData {
                territory_actor,
                status_effects,
            })
        })
        .collect::<Result<Vec<_>, MessageError>>()?;
    if keys.len() != values.len() {
        return Err(MessageError::LengthMismatch {
            field: "resident data",
        });
    }
    Ok(keys.into_iter().zip(values).collect())
}

fn write_resident_data(writer: &mut Writer, territories: &[(u16, TerritoryResidentData)]) {
    // A `usize` count fits 64 bits.
    writer.wide_prefixed(territories.len() as u64);
    for (territory, _) in territories {
        writer.u16(*territory);
    }
    writer.wide_prefixed(territories.len() as u64);
    for (_, data) in territories {
        writer
            .bytes(&data.territory_actor)
            .wide_prefixed(data.status_effects.len() as u64);
        for (effect, end_ns) in &data.status_effects {
            writer.u32(*effect).u64(*end_ns);
        }
    }
}

/// `MB::WaypointsComponentReplicatedState` (type 4321, player component 50): slot 0, child 0 is
/// the map marker, three big-endian `f32`s. Retail sends it 31 to 53 ms after each 2728 with the
/// asked position (S8 capture: 7 of 7 calls).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WaypointsState {
    /// Child 0: the map marker, metres.
    pub waypoint: Option<[f32; POSITION_LEN]>,
}

impl WaypointsState {
    /// Reads the state from the bundle path.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] when the body ends early or sets an unsized child.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        let mut state = Self::default();
        read_slot_0(reader, WAYPOINTS_FIELD, WAYPOINTS_CHILDREN, |reader, _| {
            state.waypoint = Some(read_position(reader)?);
            Ok(())
        })?;
        Ok(state)
    }

    /// Writes the state for the bundle path.
    pub fn write(&self, writer: &mut Writer) {
        write_known(
            writer,
            children_mask(&[self.waypoint.is_some()]),
            WAYPOINTS_CHILDREN,
            |writer, _| {
                if let Some(position) = &self.waypoint {
                    write_position(writer, position);
                }
            },
        );
    }

    /// The body bytes.
    pub fn body(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        self.write(&mut writer);
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header (synthetic ids).
    const HEADER: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    /// Retail 3713 arguments: a shrine of territory 20422, type 27 (fast travel spam capture).
    const RETAIL_SHRINE: [u8; 15] = [
        0x4f, 0xc6, 0x1b, 0x45, 0xbd, 0x6f, 0x33, 0x46, 0x1a, 0x00, 0x0e, 0x00, 0x00, 0x00, 0x00,
    ];
    const SHRINE_TERRITORY: u16 = 20_422;
    const SHRINE_TYPE: u8 = 27;
    /// Retail 3712 arguments: a home plot (Hive house plot capture).
    const RETAIL_PLOT: [u8; 32] = [
        0xc7, 0x37, 0x66, 0xcd, 0xa0, 0x43, 0x41, 0x6a, 0, 0, 0, 0, 0, 0, 0, 0, 0x6a, 0x41, 0x43,
        0xa0, 0xcd, 0x66, 0x37, 0xc7, 0x99, 0xed, 0xe7, 0xe1, 0xd5, 0xa5, 0xb2, 0x4d,
    ];
    /// Retail 2149 arguments outside a house.
    const NO_PLOT: [u8; 32] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0,
    ];
    /// Retail 3277 arguments (furniture capture, first placement).
    const RETAIL_PLACE: [u8; 36] = [
        0x00, 0x00, 0xb3, 0x9e, 0xbe, 0x83, 0x00, 0x00, 0xbf, 0x9b, 0xc8, 0x00, 0x40, 0xb8, 0x25,
        0xa0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3f, 0x77, 0xab, 0x70, 0x3e, 0x82,
        0xf3, 0xd3, 0x01, 0xff, 0xff, 0x00,
    ];
    /// Retail 3723 arguments (furniture capture).
    const RETAIL_MOVE: [u8; 31] = [
        0x00, 0x19, 0xc0, 0x20, 0x80, 0x00, 0x3e, 0xc4, 0xe0, 0x00, 0x40, 0xb8, 0x22, 0x80, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3f, 0x7a, 0x40, 0xe3, 0x3e, 0x5a, 0x11, 0x3b,
        0x00,
    ];
    /// Retail 2728 arguments (S8 capture).
    const RETAIL_WAYPOINT: [u8; 12] = [
        0x46, 0x11, 0xc9, 0x2b, 0x45, 0x27, 0xbb, 0xd7, 0x00, 0x00, 0x00, 0x00,
    ];
    /// Retail 3980 arguments: 10 s in nanoseconds (every capture).
    const RETAIL_UNSTUCK_DELAY: [u8; 8] = [0x00, 0x00, 0x00, 0x02, 0x54, 0x0b, 0xe4, 0x00];
    const UNSTUCK_DELAY_NS: u64 = 10_000_000_000;
    /// Retail 2774 arguments: on cooldown, 28.2 s left (monarchs capture).
    const RETAIL_DENIED: [u8; 12] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x91, 0x5a, 0x6b, 0xf2,
    ];
    /// Retail 2774 arguments: dead, the cooldown ended 403.8 s before (brightwood capture).
    const RETAIL_DENIED_DEAD: [u8; 12] = [
        0x00, 0x00, 0x00, 0x01, 0xff, 0xff, 0xff, 0xa1, 0xfa, 0x65, 0xe4, 0x91,
    ];
    const RETAIL_DENIED_DEAD_COOLDOWN_NS: i64 = -403_820_911_471;
    /// Retail 3692 arguments (house purchase capture).
    const RETAIL_HOUSE_INFO: [u8; 44] = [
        0x18, 0xc1, 0xd2, 0xa6, 0x7b, 0x69, 0x6d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x1c, 0x11, 0x3b,
        0x54, 0x46, 0xd7, 0xa4, 0x92, 0, 0, 0, 0, 0, 0, 0, 0, 0x92, 0xa4, 0xd7, 0x46, 0x54, 0x3b,
        0x11, 0x1c, 0xc2, 0x51, 0x7b, 0x87, 0x5c, 0x7a, 0x8b, 0xa2,
    ];

    fn body(arguments: &[u8]) -> Vec<u8> {
        [&HEADER[..], arguments].concat()
    }

    fn header() -> FacetHeader {
        FacetHeader::read(&mut Reader::new(&HEADER)).expect("header")
    }

    fn client(type_index: u64, arguments: &[u8]) -> TravelClientMsg {
        let message = TravelClientMsg::decode(type_index, &body(arguments))
            .expect("a travel call")
            .expect("reads");
        assert_eq!(message.type_index(), type_index);
        assert_eq!(message.encode(), body(arguments), "writes back");
        message
    }

    fn server(type_index: u64, arguments: &[u8]) -> TravelServerMsg {
        assert!(TravelServerMsg::is_type(type_index));
        let message = TravelServerMsg::decode(type_index, &body(arguments)).expect("reads");
        assert_eq!(message.type_index(), type_index);
        assert_eq!(message.encode(), body(arguments), "writes back");
        message
    }

    #[test]
    fn the_retail_fast_travel_calls_read_and_write_back() {
        let TravelClientMsg::FastTravelLandmark { landmark, .. } =
            client(FAST_TRAVEL_LANDMARK_TYPE_INDEX, &RETAIL_SHRINE)
        else {
            panic!("a landmark travel");
        };
        assert_eq!(
            (landmark.territory, landmark.landmark_type),
            (SHRINE_TERRITORY, SHRINE_TYPE)
        );
        assert!((landmark.position[0] - 6_061.9).abs() < 0.1);
        assert!((landmark.position[1] - 9_856.0).abs() < 0.1);
        assert_eq!(landmark.position[2], 0.0);
        let TravelClientMsg::FastTravelHome { plot, .. } =
            client(FAST_TRAVEL_HOME_TYPE_INDEX, &RETAIL_PLOT)
        else {
            panic!("a home travel");
        };
        assert!(!plot.is_none());
        assert_eq!(plot.facet, 0x6a41_43a0_cd66_37c7);
        let TravelClientMsg::ExitPlot { plot, .. } = client(EXIT_PLOT_TYPE_INDEX, &NO_PLOT) else {
            panic!("an exit");
        };
        assert!(plot.is_none());
        for type_index in [
            CANCEL_FAST_TRAVEL_TYPE_INDEX,
            FAST_TRAVEL_TIMEOUT_TYPE_INDEX,
            UNSTUCK_REQUEST_TYPE_INDEX,
            BUY_HOME_TYPE_INDEX,
            ABANDON_HOME_TYPE_INDEX,
            PLOT_INFO_REQUEST_TYPE_INDEX,
            RESIDENT_DATA_REQUEST_TYPE_INDEX,
            HOUSING_ITEM_INTERACTION_ENDED_TYPE_INDEX,
            REMOVE_ALL_FURNITURE_TYPE_INDEX,
        ] {
            assert_eq!(client(type_index, &[]).header(), &header());
        }
        for type_index in [
            RESET_COOLDOWN_TYPE_INDEX,
            PAY_TAXES_TYPE_INDEX,
            TAXES_DUE_REQUEST_TYPE_INDEX,
        ] {
            client(type_index, &RETAIL_PLOT);
        }
    }

    #[test]
    fn the_retail_marker_skin_and_plot_calls_read() {
        let TravelClientMsg::SetWaypoint { position, .. } =
            client(SET_WAYPOINT_TYPE_INDEX, &RETAIL_WAYPOINT)
        else {
            panic!("a marker");
        };
        assert!((position[0] - 9_330.3).abs() < 0.1);
        let skin = client(SET_CAMP_SKIN_TYPE_INDEX, &[0x3e, 0x89, 0x16, 0x6b]);
        assert!(matches!(
            skin,
            TravelClientMsg::SetCampSkin {
                skin: 0x3e89_166b,
                ..
            }
        ));
        let enter = client(ENTER_PLOT_TYPE_INDEX, &RETAIL_PLOT[..24]);
        assert!(matches!(enter, TravelClientMsg::EnterPlot { .. }));
        let sit = client(INTERACT_HOUSING_ITEM_TYPE_INDEX, &[0x00, 0x19]);
        assert!(matches!(
            sit,
            TravelClientMsg::InteractHousingItem { index: 25, .. }
        ));
    }

    #[test]
    fn the_retail_furniture_calls_read_and_write_back() {
        let TravelClientMsg::PlaceItem {
            item,
            placement,
            anchor,
            ..
        } = client(PLACE_ITEM_TYPE_INDEX, &RETAIL_PLACE)
        else {
            panic!("a placement");
        };
        assert_eq!(item, 0xb39e);
        assert_eq!(
            anchor,
            Some(ItemAnchor {
                parent: u16::MAX,
                extra: 0
            })
        );
        assert!((placement.position[2] - 5.754).abs() < 0.001);
        assert!((placement.rotation[2] - 0.967).abs() < 0.001);
        let TravelClientMsg::MoveItem { index, anchor, .. } =
            client(MOVE_ITEM_TYPE_INDEX, &RETAIL_MOVE)
        else {
            panic!("a move");
        };
        assert_eq!((index, anchor), (25, None));
        let remove = client(REMOVE_ITEM_TYPE_INDEX, &[0x00, 0x19, 0x00]);
        assert!(matches!(
            remove,
            TravelClientMsg::RemoveItem {
                index: 25,
                anchor: None,
                ..
            }
        ));
        let mut bad_flag = body(&[0x00, 0x19]);
        bad_flag.push(2);
        assert!(matches!(
            TravelClientMsg::decode(REMOVE_ITEM_TYPE_INDEX, &bad_flag),
            Some(Err(MessageError::NotBoolean { value: 2 }))
        ));
    }

    #[test]
    fn other_types_and_long_bodies_are_refused() {
        assert!(TravelClientMsg::decode(1, &HEADER).is_none());
        let mut long = body(&RETAIL_SHRINE);
        long.push(0);
        assert!(matches!(
            TravelClientMsg::decode(FAST_TRAVEL_LANDMARK_TYPE_INDEX, &long),
            Some(Err(_))
        ));
        assert!(!TravelServerMsg::is_type(1));
        assert!(TravelServerMsg::decode(1, &HEADER).is_err());
        let mut long_answer = HEADER.to_vec();
        long_answer.push(0);
        assert!(TravelServerMsg::decode(FAST_TRAVEL_FAILED_TYPE_INDEX, &long_answer).is_err());
    }

    #[test]
    fn the_retail_answers_read_and_write_back() {
        for type_index in [
            FAST_TRAVEL_COMPLETED_TYPE_INDEX,
            FAST_TRAVEL_FAILED_TYPE_INDEX,
            UNSTUCK_COMPLETED_TYPE_INDEX,
            PLAYER_TELEPORT_TYPE_INDEX,
            TELEPORT_FAILED_TYPE_INDEX,
            DEMOLISH_CAMP_TYPE_INDEX,
        ] {
            assert_eq!(server(type_index, &[]).header(), &header());
        }
        assert_eq!(
            server(UNSTUCK_ACCEPTED_TYPE_INDEX, &RETAIL_UNSTUCK_DELAY),
            TravelServerMsg::UnstuckAccepted {
                header: header(),
                delay_ns: UNSTUCK_DELAY_NS,
            }
        );
        let TravelServerMsg::UnstuckDenied {
            reason,
            cooldown_ns,
            ..
        } = server(UNSTUCK_DENIED_TYPE_INDEX, &RETAIL_DENIED)
        else {
            panic!("a denial");
        };
        assert_eq!(reason, UnstuckDenial::OnCooldown);
        assert_eq!(cooldown_ns, 28_208_425_970);
        assert_eq!(
            server(UNSTUCK_DENIED_TYPE_INDEX, &RETAIL_DENIED_DEAD),
            TravelServerMsg::UnstuckDenied {
                header: header(),
                reason: UnstuckDenial::PlayerDead,
                cooldown_ns: RETAIL_DENIED_DEAD_COOLDOWN_NS,
            }
        );
        let TravelServerMsg::HouseInfo {
            house_score, plot, ..
        } = server(HOUSE_INFO_TYPE_INDEX, &RETAIL_HOUSE_INFO)
        else {
            panic!("house info");
        };
        assert_eq!((house_score, plot.facet), (0, 0x92a4_d746_543b_111c));
        server(TAXES_DUE_TYPE_INDEX, &[0x00, 0x00, 0x1a, 0x37]);
        server(PAY_TAX_FINISHED_TYPE_INDEX, &[0x01]);
        server(CAMP_RECONNECT_FAILURE_TYPE_INDEX, &[0x01]);
        for type_index in [
            PLACE_ITEM_RESULT_TYPE_INDEX,
            MOVE_ITEM_RESULT_TYPE_INDEX,
            REMOVE_ITEM_RESULT_TYPE_INDEX,
        ] {
            server(type_index, &[0x00, 0x00, 0x19]);
        }
        let mut refunded = vec![9; RAW_ID_LEN];
        refunded.extend([0x00, 0x03]);
        server(ITEMS_REFUNDED_TYPE_INDEX, &refunded);
    }

    #[test]
    fn every_unstuck_reason_has_its_client_number() {
        for (number, reason) in (0_u32..).zip(UnstuckDenial::ALL) {
            assert_eq!(reason.number(), number);
            assert_eq!(UnstuckDenial::of_number(number), Some(reason));
        }
        assert_eq!(UnstuckDenial::of_number(6), None);
    }

    #[test]
    fn the_waypoints_state_is_the_retail_marker_body() {
        // Retail bundle member after a 2728 (S8 capture): `01 01` and the asked position.
        let mut retail = vec![0x01, 0x01];
        retail.extend(RETAIL_WAYPOINT);
        let state = WaypointsState::read(&mut Reader::new(&retail)).expect("read");
        assert_eq!(state.body(), retail);
        assert_eq!(WaypointsState::default().body(), [0x00]);
    }

    #[test]
    fn the_retail_headers_name_the_travel_components() {
        // Retail (target, entity) pairs: a housing, an unstuck and a camping call, and a 2728 of
        // a child entity (the own, unstuck and Hive camping captures, the S8 capture).
        let pairs = [
            (
                0xd148_ba1d_2f6f_dbae_u64,
                0x193a_1ff8_1580_6fc8_u64,
                component::PLAYER_HOUSING,
            ),
            (
                0x9d22_2010_1bfa_d65a,
                0x2896_c8c1_4210_296e,
                component::UNSTUCK,
            ),
            (
                0xfd3e_e932_68b2_c069,
                0xe34e_14f3_44a9_74cc,
                component::CAMPING,
            ),
        ];
        for (target, entity, constant) in pairs {
            assert_eq!(target.wrapping_sub(entity), constant);
        }
        let waypoint_entity = 0x1828_a72f_a26b_918c_u64;
        assert_eq!(
            (waypoint_entity ^ component::WAYPOINTS_CHILD).wrapping_add(component::WAYPOINTS),
            0x9c93_0f10_6aa5_0580
        );
    }

    #[test]
    fn the_resident_data_and_the_camp_placement_read_and_write_back() {
        // The retail 3695 shape: two keys (8 and 16), then two values, the first with no effect
        // and the second with one (synthetic ids and times).
        let mut resident = vec![0x02, 0x00, 0x08, 0x00, 0x10, 0x02];
        resident.extend([3; RAW_ID_LEN]);
        resident.push(0x00);
        resident.extend([4; RAW_ID_LEN]);
        resident.push(0x01);
        resident.extend([0xc3, 0x6a, 0x43, 0xb4]);
        resident.extend(1_790_000_000_000_000_000_u64.to_be_bytes());
        let TravelServerMsg::ResidentData { territories, .. } =
            server(RESIDENT_DATA_TYPE_INDEX, &resident)
        else {
            panic!("resident data");
        };
        assert_eq!(
            territories.iter().map(|(territory, _)| *territory).collect::<Vec<_>>(),
            [8, 16]
        );
        assert_eq!(territories[1].1.status_effects.len(), 1);
        assert_eq!(server(RESIDENT_DATA_TYPE_INDEX, &[0x00, 0x00]).header(), &header());
        // The retail 2236 shape (73 bytes): position, rotation, skin, two ids, two numbers, a
        // flag; and its answer 441 (result 0, 0).
        let mut place = Vec::new();
        for value in [7_359.9_f32, 5_345.2, 72.8, 0.0, 0.0, -0.512, 0.859] {
            place.extend(value.to_be_bytes());
        }
        place.extend([0xbf, 0xf6, 0x8c, 0x5c]);
        place.extend([5; RAW_ID_LEN]);
        place.extend([6; RAW_ID_LEN]);
        place.extend([0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x32, 0x01]);
        let TravelClientMsg::PlaceStructure { placement, .. } =
            client(PLACE_STRUCTURE_TYPE_INDEX, &place)
        else {
            panic!("a placement");
        };
        assert_eq!((placement.skin, placement.value), (0xbff6_8c5c, 50));
        client(CANCEL_PLACING_STRUCTURE_TYPE_INDEX, &[]);
        server(PLACEMENT_RESULTS_TYPE_INDEX, &[0x00, 0x00, 0x00, 0x00, 0x00]);
    }
}
