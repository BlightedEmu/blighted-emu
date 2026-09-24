// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client's point calls (progression note, sections 6 and 9): spending attribute points
//! (1502), refunding them (2409), assigning a weapon point (1112), applying the assigned points
//! (2857), mapping an ability into a slot (2701) and resetting a weapon tree (186). Each body is
//! the facet header and the arguments; the layouts are those of the retail captures (newchar
//! 2026-09-18: 4 x 1502, 8 x 1112, 8 x 2857; WEAPON-CODEX: 99 x 2701, 17 x 186; open world
//! 2026-08-26: 38 x 2409). A body of another length is refused.

use crate::facet::FacetHeader;
use crate::wire::{MessageError, Reader, Writer};

/// `AttributeComponentServerFacet_RequestSpendAttributePoints`.
pub const SPEND_ATTRIBUTE_POINTS_TYPE_INDEX: u64 = 1502;
/// `ProgressionPointComponentServerFacet_RequestAssignPoolPoints`.
pub const ASSIGN_POOL_POINTS_TYPE_INDEX: u64 = 1112;
/// `AbilityComponentServerFacet_RequestApplyAbilityPoints`.
pub const APPLY_ABILITY_POINTS_TYPE_INDEX: u64 = 2857;
/// `AttributeComponentServerFacet_RequestRefundSpentPoints`.
pub const REFUND_SPENT_POINTS_TYPE_INDEX: u64 = 2409;
/// `AbilityComponentServerFacet_RequestChangeMappedAbility`.
pub const CHANGE_MAPPED_ABILITY_TYPE_INDEX: u64 = 2701;
/// `AbilityComponentServerFacet_RequestMasteryTreeRespec`.
pub const MASTERY_TREE_RESPEC_TYPE_INDEX: u64 = 186;
/// `ProgressionPointComponentServerFacet_RequestSpendTerritoryStandingPoint`.
pub const SPEND_TERRITORY_STANDING_POINT_TYPE_INDEX: u64 = 4040;
/// The most entries of a 1502 list: one per attribute (client enum `CharacterAttributeType`).
const MAX_ATTRIBUTES: u64 = 5;

/// One point call of the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointsClientMsg {
    /// 1502: spend points on attributes; the lists are parallel.
    SpendAttributePoints {
        /// The header.
        header: FacetHeader,
        /// Each attribute (`CharacterAttributeType`).
        attributes: Vec<u32>,
        /// The points for each attribute, in the same order.
        points: Vec<u32>,
    },
    /// 1112: put points of a pool into one progression point.
    AssignPoolPoints {
        /// The header.
        header: FacetHeader,
        /// The pool id (CRC-32 of the lowercase pool id; a weapon pool's is its mastery's).
        pool: u32,
        /// The progression point (CRC-32 of the lowercase `ProgressionPointId`).
        point: u32,
        /// How many points (retail sent 1 each time; the point's level in 3681 rose by it).
        points: u32,
    },
    /// 2857: apply the assigned points to the abilities (no arguments; retail sends it with
    /// each 1112).
    ApplyAbilityPoints {
        /// The header.
        header: FacetHeader,
    },
    /// 2409: refund every spent attribute point (no arguments; retail answered with the whole
    /// attribute state and took the level's `AttributeRespecCost` coins).
    RefundSpentPoints {
        /// The header.
        header: FacetHeader,
    },
    /// 2701: put an ability into one mapped slot of an ability table.
    ChangeMappedAbility {
        /// The header.
        header: FacetHeader,
        /// The table: its position in the list of the player's ability state (type 185; retail
        /// sent 27 for the sword table, the 28th of the 29 tables it lists).
        table: u16,
        /// The slot, 0 to 2.
        slot: u32,
        /// The ability (CRC-32 of the lowercase ability id).
        ability: u32,
    },
    /// 4040: spend one territory standing point of a territory pool (retail
    /// `refining_all_stations_single_batch_territorylvlup`: two big-endian `u32`s, the pool
    /// and the progression point; three spends of one session share the pool).
    SpendTerritoryStandingPoint {
        /// The header.
        header: FacetHeader,
        /// The territory pool (CRC-32 of its lowercase id).
        pool: u32,
        /// The progression point (CRC-32 of the lowercase `ProgressionPointId`).
        point: u32,
    },
    /// 186: reset the weapon tree of one ability table (its points go back to the pool).
    MasteryTreeRespec {
        /// The header.
        header: FacetHeader,
        /// The table, as in 2701.
        table: u16,
        /// One byte of Unknown meaning (0 in all 17 retail calls).
        flag: u8,
    },
}

impl PointsClientMsg {
    /// Decodes `body` when `type_index` is a point call; `None` otherwise.
    pub fn decode(type_index: u64, body: &[u8]) -> Option<Result<Self, MessageError>> {
        let read = |read_rest: fn(FacetHeader, &mut Reader<'_>) -> Result<Self, MessageError>| {
            let mut reader = Reader::new(body);
            let message = read_rest(FacetHeader::read(&mut reader)?, &mut reader)?;
            reader.finish()?;
            Ok(message)
        };
        Some(match type_index {
            SPEND_ATTRIBUTE_POINTS_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SpendAttributePoints {
                    header,
                    attributes: read_list(reader)?,
                    points: read_list(reader)?,
                })
            }),
            ASSIGN_POOL_POINTS_TYPE_INDEX => read(|header, reader| {
                Ok(Self::AssignPoolPoints {
                    header,
                    pool: reader.u32()?,
                    point: reader.u32()?,
                    points: reader.u32()?,
                })
            }),
            APPLY_ABILITY_POINTS_TYPE_INDEX => {
                read(|header, _| Ok(Self::ApplyAbilityPoints { header }))
            }
            REFUND_SPENT_POINTS_TYPE_INDEX => {
                read(|header, _| Ok(Self::RefundSpentPoints { header }))
            }
            CHANGE_MAPPED_ABILITY_TYPE_INDEX => read(|header, reader| {
                Ok(Self::ChangeMappedAbility {
                    header,
                    table: reader.u16()?,
                    slot: reader.u32()?,
                    ability: reader.u32()?,
                })
            }),
            SPEND_TERRITORY_STANDING_POINT_TYPE_INDEX => read(|header, reader| {
                Ok(Self::SpendTerritoryStandingPoint {
                    header,
                    pool: reader.u32()?,
                    point: reader.u32()?,
                })
            }),
            MASTERY_TREE_RESPEC_TYPE_INDEX => read(|header, reader| {
                Ok(Self::MasteryTreeRespec {
                    header,
                    table: reader.u16()?,
                    flag: reader.u8()?,
                })
            }),
            _ => return None,
        })
    }

    /// The type index of the call.
    pub const fn type_index(&self) -> u64 {
        match self {
            Self::SpendAttributePoints { .. } => SPEND_ATTRIBUTE_POINTS_TYPE_INDEX,
            Self::AssignPoolPoints { .. } => ASSIGN_POOL_POINTS_TYPE_INDEX,
            Self::ApplyAbilityPoints { .. } => APPLY_ABILITY_POINTS_TYPE_INDEX,
            Self::RefundSpentPoints { .. } => REFUND_SPENT_POINTS_TYPE_INDEX,
            Self::ChangeMappedAbility { .. } => CHANGE_MAPPED_ABILITY_TYPE_INDEX,
            Self::SpendTerritoryStandingPoint { .. } => {
                SPEND_TERRITORY_STANDING_POINT_TYPE_INDEX
            }
            Self::MasteryTreeRespec { .. } => MASTERY_TREE_RESPEC_TYPE_INDEX,
        }
    }

    /// The body bytes, as the client sends them.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::SpendAttributePoints {
                header,
                attributes,
                points,
            } => {
                header.write(&mut writer);
                write_list(&mut writer, attributes);
                write_list(&mut writer, points);
            }
            Self::AssignPoolPoints {
                header,
                pool,
                point,
                points,
            } => {
                header.write(&mut writer);
                writer.u32(*pool).u32(*point).u32(*points);
            }
            Self::ApplyAbilityPoints { header } | Self::RefundSpentPoints { header } => {
                header.write(&mut writer);
            }
            Self::ChangeMappedAbility {
                header,
                table,
                slot,
                ability,
            } => {
                header.write(&mut writer);
                writer.u16(*table).u32(*slot).u32(*ability);
            }
            Self::SpendTerritoryStandingPoint { header, pool, point } => {
                header.write(&mut writer);
                writer.u32(*pool).u32(*point);
            }
            Self::MasteryTreeRespec {
                header,
                table,
                flag,
            } => {
                header.write(&mut writer);
                writer.u16(*table).u8(*flag);
            }
        }
        writer.finish()
    }
}

/// A count, then that many `u32`s.
fn read_list(reader: &mut Reader<'_>) -> Result<Vec<u32>, MessageError> {
    let count = reader.wide_prefixed()?;
    if count > MAX_ATTRIBUTES {
        return Err(MessageError::CountTooLarge {
            count,
            limit: MAX_ATTRIBUTES,
        });
    }
    (0..count).map(|_| reader.u32()).collect()
}

fn write_list(writer: &mut Writer, values: &[u32]) {
    // A `usize` count fits 64 bits.
    writer.wide_prefixed(values.len() as u64);
    for value in values {
        writer.u32(*value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header (synthetic ids).
    const HEADER: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    /// Retail 1502 arguments: Constitution 5 and Strength 0 (newchar 2026-09-18, third spend).
    const RETAIL_SPEND: [u8; 18] = [
        0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x02, 0x00, 0x00, 0x00, 0x05, 0x00,
        0x00, 0x00, 0x00,
    ];
    /// Retail 1112 arguments: the greatsword pool, `Ability_Greatsword_Combo`, 1 point.
    const RETAIL_ASSIGN: [u8; 12] = [
        0x18, 0x0b, 0x9a, 0x9f, 0x2e, 0xdf, 0x28, 0xbe, 0x00, 0x00, 0x00, 0x01,
    ];
    const GREATSWORD_POOL: u32 = 0x180b_9a9f;
    const GREATSWORD_COMBO: u32 = 0x2edf_28be;

    fn body(arguments: &[u8]) -> Vec<u8> {
        [&HEADER[..], arguments].concat()
    }

    fn header() -> FacetHeader {
        FacetHeader::read(&mut Reader::new(&HEADER)).expect("header")
    }

    #[test]
    fn the_retail_bodies_read_and_write_back() {
        let spend =
            PointsClientMsg::decode(SPEND_ATTRIBUTE_POINTS_TYPE_INDEX, &body(&RETAIL_SPEND))
                .expect("a point call")
                .expect("reads");
        assert_eq!(
            spend,
            PointsClientMsg::SpendAttributePoints {
                header: header(),
                attributes: vec![0, 2],
                points: vec![5, 0],
            }
        );
        assert_eq!(spend.encode(), body(&RETAIL_SPEND));
        let assign = PointsClientMsg::decode(ASSIGN_POOL_POINTS_TYPE_INDEX, &body(&RETAIL_ASSIGN))
            .expect("a point call")
            .expect("reads");
        assert_eq!(
            assign,
            PointsClientMsg::AssignPoolPoints {
                header: header(),
                pool: GREATSWORD_POOL,
                point: GREATSWORD_COMBO,
                points: 1,
            }
        );
        assert_eq!(assign.encode(), body(&RETAIL_ASSIGN));
        let apply = PointsClientMsg::decode(APPLY_ABILITY_POINTS_TYPE_INDEX, &HEADER)
            .expect("a point call")
            .expect("reads");
        assert_eq!(apply.type_index(), APPLY_ABILITY_POINTS_TYPE_INDEX);
        assert_eq!(apply.encode(), HEADER);
    }

    /// Retail 2701 arguments (WEAPON-CODEX): sword table 27, slot 2, an ability.
    const RETAIL_MAP: [u8; 10] = [0x00, 0x1b, 0x00, 0x00, 0x00, 0x02, 0xe6, 0xe1, 0xd6, 0x78];
    /// Retail 186 arguments (WEAPON-CODEX): the sword table, flag 0.
    const RETAIL_RESPEC: [u8; 3] = [0x00, 0x1b, 0x00];
    /// The sword table's position in retail's ability state.
    const SWORD_TABLE: u16 = 27;

    #[test]
    fn the_retail_respec_refund_and_map_bodies_read_and_write_back() {
        let map = PointsClientMsg::decode(CHANGE_MAPPED_ABILITY_TYPE_INDEX, &body(&RETAIL_MAP))
            .expect("a point call")
            .expect("reads");
        assert_eq!(
            map,
            PointsClientMsg::ChangeMappedAbility {
                header: header(),
                table: SWORD_TABLE,
                slot: 2,
                ability: 0xe6e1_d678,
            }
        );
        assert_eq!(map.encode(), body(&RETAIL_MAP));
        let respec =
            PointsClientMsg::decode(MASTERY_TREE_RESPEC_TYPE_INDEX, &body(&RETAIL_RESPEC))
                .expect("a point call")
                .expect("reads");
        assert_eq!(
            respec,
            PointsClientMsg::MasteryTreeRespec {
                header: header(),
                table: SWORD_TABLE,
                flag: 0,
            }
        );
        assert_eq!(respec.encode(), body(&RETAIL_RESPEC));
        assert_eq!(respec.type_index(), MASTERY_TREE_RESPEC_TYPE_INDEX);
        let refund = PointsClientMsg::decode(REFUND_SPENT_POINTS_TYPE_INDEX, &HEADER)
            .expect("a point call")
            .expect("reads");
        assert_eq!(refund, PointsClientMsg::RefundSpentPoints { header: header() });
        assert_eq!(refund.encode(), HEADER);
        assert!(matches!(
            PointsClientMsg::decode(REFUND_SPENT_POINTS_TYPE_INDEX, &body(&[0])),
            Some(Err(_))
        ));
    }

    /// Retail 4040 arguments (territory level-up ledger): one pool, three points.
    const RETAIL_TERRITORY_POOL: u32 = 0xd65a_1577;

    #[test]
    fn the_retail_territory_spend_reads_and_writes_back() {
        for point in [0x68c1_f9aa_u32, 0xb945_4238, 0xfa6c_80ba] {
            let mut arguments = RETAIL_TERRITORY_POOL.to_be_bytes().to_vec();
            arguments.extend_from_slice(&point.to_be_bytes());
            let call = PointsClientMsg::decode(
                SPEND_TERRITORY_STANDING_POINT_TYPE_INDEX,
                &body(&arguments),
            )
            .expect("a point call")
            .expect("reads");
            assert_eq!(
                call,
                PointsClientMsg::SpendTerritoryStandingPoint {
                    header: header(),
                    pool: RETAIL_TERRITORY_POOL,
                    point,
                }
            );
            assert_eq!(call.encode(), body(&arguments));
        }
    }

    #[test]
    fn other_types_long_bodies_and_long_lists_are_refused() {
        assert!(PointsClientMsg::decode(1, &HEADER).is_none());
        let mut long = body(&RETAIL_ASSIGN);
        long.push(0);
        assert!(matches!(
            PointsClientMsg::decode(ASSIGN_POOL_POINTS_TYPE_INDEX, &long),
            Some(Err(_))
        ));
        let six = body(&[0x06]);
        assert!(matches!(
            PointsClientMsg::decode(SPEND_ATTRIBUTE_POINTS_TYPE_INDEX, &six),
            Some(Err(MessageError::CountTooLarge { .. }))
        ));
    }
}
