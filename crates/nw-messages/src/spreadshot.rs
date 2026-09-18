// SPDX-License-Identifier: AGPL-3.0-or-later
//! The blunderbuss shot (ranged note): the client asks to fire a spread of pellets and names the
//! targets they hit (2719 `ActionListComponentServerFacet_RequestSpreadshot`); the server answers
//! with 2570 `ActionListComponentClientFacet_OnServerSpreadshotProcessed` and one 2071 per pellet
//! that hit. Counts are wide prefix integers; every other number is big-endian.

use crate::facet::FacetHeader;
use crate::wire::{BodyCodec, MessageError, RAW_ID_LEN, Reader, Writer};

/// Type index of `ActionListComponentServerFacet_RequestSpreadshot`.
pub const SPREADSHOT_REQUEST_TYPE_INDEX: u64 = 2719;
/// Type index of `ActionListComponentClientFacet_OnServerSpreadshotProcessed`.
pub const SPREADSHOT_PROCESSED_TYPE_INDEX: u64 = 2570;
/// The pellet distance bytes that mark no hit.
const NO_HIT_DISTANCE: u8 = 0xfe;
/// The hit index bits of a pellet's first byte (0xf: no hit).
const HIT_INDEX_MASK: u8 = 0x0f;
/// Bytes of the aim direction (three half floats).
const DIRECTION_LEN: usize = 6;
/// Pellets, hits and times a request may hold; retail shots have 6 or 8 pellets.
const MAX_ENTRIES: u64 = 64;

/// One target a shot hit, as the client names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpreadHit {
    /// The target's raw id.
    pub raw_id: [u8; RAW_ID_LEN],
    /// The target's facet entity: its entity number with the bytes reversed.
    pub facet_entity: u64,
    /// A value per hit (Unknown; Inferred: the hit volume).
    pub extra: u64,
}

impl SpreadHit {
    /// The entity number of the target.
    pub const fn target(&self) -> u64 {
        self.facet_entity.swap_bytes()
    }
}

/// 2719: the client fires a spread shot.
#[derive(Debug, Clone, PartialEq)]
pub struct SpreadshotRequestMsg {
    /// The header: the shooter's `ActionListComponent`.
    pub header: FacetHeader,
    /// A frame counter (Inferred: about 30 per second).
    pub frame: u16,
    /// Always 1 in the captures (Unknown).
    pub flag: u8,
    /// The seed of the spread: the frame's low byte.
    pub seed: u8,
    /// Where the shot starts, x, y and z in metres.
    pub origin: [f32; 3],
    /// The aim, three half floats.
    pub direction: [u8; DIRECTION_LEN],
    /// The pellets: the hit index in the low 4 bits of the first byte, the distance in the second.
    pub pellets: Vec<[u8; 2]>,
    /// The targets the pellets hit.
    pub hits: Vec<SpreadHit>,
    /// One time per hit target, µs since 2000-01-01.
    pub times: Vec<u64>,
}

impl SpreadshotRequestMsg {
    /// The target of each pellet that hit, in pellet order: a pellet hits when its hit index
    /// names a listed target and its distance byte is below 0xfe (182 of 196 retail shots).
    pub fn pellet_targets(&self) -> Vec<u64> {
        self.pellets
            .iter()
            .filter(|[index_byte, distance]| *distance < NO_HIT_DISTANCE && *index_byte != u8::MAX)
            .filter_map(|[index_byte, _]| self.hits.get(usize::from(index_byte & HIT_INDEX_MASK)))
            .map(SpreadHit::target)
            .collect()
    }

    /// Writes the part the answer echoes: the aim to the end of the hits.
    fn write_echo(&self, writer: &mut Writer) {
        writer.bytes(&self.direction);
        writer.wide_prefixed(self.pellets.len() as u64);
        for pellet in &self.pellets {
            writer.bytes(pellet);
        }
        writer.wide_prefixed(self.hits.len() as u64);
        for hit in &self.hits {
            writer
                .bytes(&hit.raw_id)
                .u64(hit.facet_entity)
                .u64(hit.extra);
        }
    }
}

/// A count of at most [`MAX_ENTRIES`].
fn count(reader: &mut Reader<'_>) -> Result<usize, MessageError> {
    let count = reader.wide_prefixed()?;
    if count > MAX_ENTRIES {
        return Err(MessageError::CountTooLarge {
            count,
            limit: MAX_ENTRIES,
        });
    }
    // At most 64.
    Ok(count as usize)
}

impl BodyCodec for SpreadshotRequestMsg {
    const TYPE_INDEX: u64 = SPREADSHOT_REQUEST_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        writer.u16(self.frame).u8(self.flag).u8(self.seed);
        for value in self.origin {
            writer.f32(value);
        }
        self.write_echo(&mut writer);
        writer.wide_prefixed(self.times.len() as u64);
        for time in &self.times {
            writer.u64(*time);
        }
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let frame = reader.u16()?;
        let flag = reader.u8()?;
        let seed = reader.u8()?;
        let origin = [reader.f32()?, reader.f32()?, reader.f32()?];
        let direction = reader.array()?;
        let pellets = (0..count(&mut reader)?)
            .map(|_| reader.array())
            .collect::<Result<_, _>>()?;
        let hits = (0..count(&mut reader)?)
            .map(|_| {
                Ok(SpreadHit {
                    raw_id: reader.array()?,
                    facet_entity: reader.u64()?,
                    extra: reader.u64()?,
                })
            })
            .collect::<Result<_, MessageError>>()?;
        let times = (0..count(&mut reader)?)
            .map(|_| reader.u64())
            .collect::<Result<_, _>>()?;
        reader.finish()?;
        Ok(Self {
            header,
            frame,
            flag,
            seed,
            origin,
            direction,
            pellets,
            hits,
            times,
        })
    }
}

/// 2570: the server took a spread shot: the request's header and seed, its aim, pellets and hits
/// again, the CRC-32 of the lowercase damage row and a pellet config id (retail 230 on six-pellet
/// shots).
#[derive(Debug, Clone, PartialEq)]
pub struct SpreadshotProcessedMsg {
    /// The request.
    pub request: SpreadshotRequestMsg,
    /// CRC-32 of the lowercase damage row (`3ac35af9` `blunderbuss_primary_attack`).
    pub damage: u32,
    /// The pellet config id.
    pub config: u64,
}

impl BodyCodec for SpreadshotProcessedMsg {
    const TYPE_INDEX: u64 = SPREADSHOT_PROCESSED_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.request.header.write(&mut writer);
        writer.u8(self.request.seed);
        self.request.write_echo(&mut writer);
        writer.u32(self.damage).wide_prefixed(self.config);
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let seed = reader.u8()?;
        let direction = reader.array()?;
        let pellets = (0..count(&mut reader)?)
            .map(|_| reader.array())
            .collect::<Result<_, _>>()?;
        let hits = (0..count(&mut reader)?)
            .map(|_| {
                Ok(SpreadHit {
                    raw_id: reader.array()?,
                    facet_entity: reader.u64()?,
                    extra: reader.u64()?,
                })
            })
            .collect::<Result<_, MessageError>>()?;
        let damage = reader.u32()?;
        let config = reader.wide_prefixed()?;
        reader.finish()?;
        Ok(Self {
            request: SpreadshotRequestMsg {
                header,
                frame: 0,
                flag: 0,
                seed,
                origin: [0.0; 3],
                direction,
                pellets,
                hits,
                times: Vec::new(),
            },
            damage,
            config,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live 2026-09-16: a shot whose 6 pellets all hit mob 0x4_0000_0004.
    const LIVE_ONE_TARGET: &str = "aa491b4ea3c2ad756df00100000000000dbc01bc4427dca143c2725741dc3623\
        baedb80026c80600960096009600960097009601000000040000000400000004000000040400000004000000\
        b7c7848bfd504d58010003015ee81f90df";
    /// Live 2026-09-16: five pellets on two hits of one mob, one pellet that missed.
    const LIVE_TWO_HITS: &str = "aa491b4ea3c2ad756df00100000000000de001e04428797a43c217e241dc21c4\
        bbc5b32d2d4806005a01580157015a005bfffe02000000040000000400000004000000040400000004000000\
        d8c2690b864b5b1e000000040000000400000004000000040400000004000000161008bc4579b03701000301\
        5ee83a27e3";
    const MOB: u64 = 0x4_0000_0004;

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn live_requests_decode_and_encode_to_the_same_bytes() {
        for hex in [LIVE_ONE_TARGET, LIVE_TWO_HITS] {
            let body = bytes(hex);
            let request = SpreadshotRequestMsg::decode(&body).expect("decode");
            assert_eq!(request.encode().expect("encode"), body);
            assert_eq!(
                request.seed,
                request.frame.to_be_bytes()[1],
                "the seed is the frame"
            );
        }
    }

    #[test]
    fn each_pellet_that_hit_names_its_target() {
        let one = SpreadshotRequestMsg::decode(&bytes(LIVE_ONE_TARGET)).expect("decode");
        assert_eq!(one.pellet_targets(), [MOB; 6]);
        let two = SpreadshotRequestMsg::decode(&bytes(LIVE_TWO_HITS)).expect("decode");
        assert_eq!(
            two.pellet_targets(),
            [MOB; 5],
            "the 0xff 0xfe pellet missed"
        );
    }

    #[test]
    fn the_answer_echoes_the_shot_and_names_the_damage_row() {
        const BLUNDERBUSS_PRIMARY: u32 = 0x3ac3_5af9;
        const SIX_PELLETS: u64 = 230;
        let request = SpreadshotRequestMsg::decode(&bytes(LIVE_ONE_TARGET)).expect("decode");
        let answer = SpreadshotProcessedMsg {
            request: request.clone(),
            damage: BLUNDERBUSS_PRIMARY,
            config: SIX_PELLETS,
        };
        let body = answer.encode().expect("encode");
        // Header and seed, then the request from the aim to the end of the hits.
        let request_body = bytes(LIVE_ONE_TARGET);
        let aim_at = 16 + 2 + 2 + 12;
        let hits_end = request_body.len() - 1 - 8;
        assert_eq!(body[..16], request_body[..16]);
        assert_eq!(body[16], request.seed);
        assert_eq!(
            body[17..17 + hits_end - aim_at],
            request_body[aim_at..hits_end]
        );
        assert_eq!(body[body.len() - 6..], [0x3a, 0xc3, 0x5a, 0xf9, 0xa6, 0x03]);
        let read = SpreadshotProcessedMsg::decode(&body).expect("decode");
        assert_eq!(
            (read.damage, read.config),
            (BLUNDERBUSS_PRIMARY, SIX_PELLETS)
        );
    }
}
