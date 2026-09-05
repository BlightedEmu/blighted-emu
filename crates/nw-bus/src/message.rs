// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bus message types (M4 spec section 6.1). Game messages travel as envelope bytes; the gateway
//! does not read them.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::client::BusError;
use crate::id::BusId;

/// One player connection: the gateway and the gateway's connection id. Ids of two gateways never
/// meet.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionRef {
    /// The gateway.
    pub gateway: BusId,
    /// The connection id inside that gateway.
    pub connection: u64,
}

/// Gateway to director: a player with a checked ticket joins the world, or a player of a lost
/// sim resumes on another (M5 spec sections 3.1 and 3.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Join {
    /// The connection.
    pub connection: ConnectionRef,
    /// The account.
    pub account: Uuid,
    /// The character.
    pub character: Uuid,
    /// A resumed player; `None` for a new login.
    pub resume: Option<ResumeOf>,
}

/// What a resumed player keeps (M5 spec section 3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeOf {
    /// Its entity number.
    pub entity: u64,
    /// The handoff count its new sim starts from: far above the count of the lost sim, so a late
    /// route of the lost sim loses at the gateway.
    pub handoffs: u64,
}

/// The answer to [`Join`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinReply {
    /// The character is in the world.
    Joined {
        /// The sim that has the player.
        sim: BusId,
        /// That sim's generation.
        generation: u64,
        /// The player's entity number.
        entity: u64,
    },
    /// The character cannot join; the gateway sends registration status 1.
    Refused {
        /// Why, for the log.
        reason: String,
    },
}

/// Gateway to sim, after a join.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimInput {
    /// One client message envelope, unchanged.
    ClientMessage {
        /// The connection.
        connection: ConnectionRef,
        /// The envelope bytes.
        envelope: Vec<u8>,
    },
    /// The connection is gone.
    Left {
        /// The connection.
        connection: ConnectionRef,
    },
}

/// Sim to gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayOutput {
    /// Server message envelopes for one connection, on one carrier channel.
    ServerMessages {
        /// The sim that sends them; it hears `Left` when the gateway no longer has the
        /// connection.
        sim: BusId,
        /// The connection id inside the gateway.
        connection: u64,
        /// The carrier channel.
        channel: u8,
        /// Whether the carrier sends them reliably.
        reliable: bool,
        /// The envelope bytes, in order.
        envelopes: Vec<Vec<u8>>,
    },
    /// Close the connection.
    Close {
        /// The connection id inside the gateway.
        connection: u64,
    },
    /// The connection's player moved to another sim by a handoff (M5 spec section 3.3).
    Route {
        /// The connection id inside the gateway.
        connection: u64,
        /// The sim that has the player now.
        sim: BusId,
        /// That sim's generation.
        generation: u64,
        /// The player's handoff count; only a higher one changes the route.
        sequence: u64,
    },
}

/// A world position in metres, as `sim` keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// East.
    pub x: f32,
    /// North.
    pub y: f32,
    /// Up.
    pub z: f32,
}

impl Point {
    /// Whether every coordinate is a finite number.
    pub const fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

/// Director to sim: take a player (M5 spec section 3.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Admit {
    /// The connection.
    pub connection: ConnectionRef,
    /// The account.
    pub account: Uuid,
    /// The character.
    pub character: Uuid,
    /// The entity number the director chose.
    pub entity: u64,
    /// Where the player appears.
    pub position: Point,
    /// Whether to send the login messages; a resumed player already has them.
    pub login: bool,
    /// The handoff count the player starts with: 0, or the count of a resume.
    pub handoffs: u64,
}

/// The answer to [`Admit`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdmitReply {
    /// The player is in the world.
    Admitted,
    /// The sim cannot take the player.
    Refused {
        /// Why, for the log.
        reason: String,
    },
}

/// A live sim in the cell table. `generation` is its `started_at_ms`; a restart is a new one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SimSlot {
    /// The sim.
    pub id: BusId,
    /// Its start time, Unix milliseconds.
    pub generation: u64,
}

/// The director's cell table (M5 spec section 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellTable {
    /// Rises with every change of one director run.
    pub version: u64,
    /// Edge length of a cell, in metres.
    pub cell_size_m: f32,
    /// Columns of the grid.
    pub columns: u32,
    /// Rows of the grid.
    pub rows: u32,
    /// The live sims.
    pub sims: Vec<SimSlot>,
    /// The owner of every cell, row by row, as an index into `sims`.
    pub owners: Vec<Option<u16>>,
}

/// One player moving to another sim (M5 spec section 3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffOffer {
    /// The connection.
    pub connection: ConnectionRef,
    /// The character.
    pub character: Uuid,
    /// The entity number; it stays the same.
    pub entity: u64,
    /// The last good position.
    pub position: Point,
    /// Milliseconds since the last good move.
    pub move_age_ms: u64,
    /// The movement budget left, in metres.
    pub budget_m: f32,
    /// Bundles sent to the player so far; the sequence goes on.
    pub bundles_sent: u64,
    /// Milliseconds since the last `TimeSynchMsg`.
    pub time_synch_age_ms: u64,
    /// Handoffs of this player so far, this one included; the gateway's route sequence.
    pub handoffs: u64,
}

/// Sim to sim, on the taker's handoff subject. Both go on one subject, so NATS keeps their order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HandoffMessage {
    /// Take this player (request/reply).
    Offer(HandoffOffer),
    /// The offer of this connection timed out or its player left; undo a late take.
    Cancel {
        /// The connection.
        connection: ConnectionRef,
        /// The `handoffs` of that offer; a take of another attempt stays.
        handoffs: u64,
    },
}

/// The answer to [`HandoffMessage::Offer`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandoffReply {
    /// The player is in the taker's world.
    Accepted,
    /// The taker cannot take the player.
    Refused {
        /// Why, for the log.
        reason: String,
    },
}

/// Largest bus message: the NATS default payload limit (M4 spec section 6.3).
pub const MAX_BUS_MESSAGE_BYTES: usize = 1024 * 1024;

/// Most bytes of one [`Ghost`] in `postcard`: a 10-byte entity number and three 4-byte floats.
pub const MAX_GHOST_BYTES: usize = 22;

/// A read-only copy of an entity near a border.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ghost {
    /// The entity number.
    pub entity: u64,
    /// Its position.
    pub position: Point,
}

/// The ghosts of one sim for another, every tick (M5 spec section 3.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ghosts {
    /// The sender.
    pub from: BusId,
    /// Its players near the receiver's cells; an empty list clears the last one.
    pub entities: Vec<Ghost>,
}

/// A sim is alive, with its load for the director.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimHeartbeat {
    /// The sim.
    pub id: BusId,
    /// Unix milliseconds when the sim started.
    pub started_at_ms: u64,
    /// Players in its world.
    pub players: u32,
    /// Players it takes at most.
    pub max_players: u32,
}

/// A process is alive. `started_at_ms` changes when the process restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    /// The process: a gateway id or a sim id.
    pub id: BusId,
    /// Unix milliseconds when the process started.
    pub started_at_ms: u64,
}

/// How long a process may stay silent before it counts as gone: `misses` heartbeat intervals. A
/// huge setting saturates instead of overflowing.
pub fn heartbeat_silence(interval: Duration, misses: u32) -> Duration {
    interval.saturating_mul(misses)
}

/// The `postcard` bytes of a bus message.
///
/// # Errors
///
/// Returns [`BusError::Encode`] when `postcard` fails.
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, BusError> {
    postcard::to_allocvec(message).map_err(BusError::Encode)
}

/// A bus message from its `postcard` bytes.
///
/// # Errors
///
/// Returns [`BusError::Decode`] for bytes that are not that message.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, BusError> {
    postcard::from_bytes(bytes).map_err(BusError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONNECTION: u64 = 7;
    const STARTED_AT_MS: u64 = 1_800_000_000_000;
    const MISSES: u32 = 3;

    fn connection() -> ConnectionRef {
        ConnectionRef {
            gateway: BusId::try_from("gw-1".to_owned()).expect("id"),
            connection: CONNECTION,
        }
    }

    fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(message: T) {
        let bytes = encode(&message).expect("encode");
        assert_eq!(decode::<T>(&bytes).expect("decode"), message);
    }

    #[test]
    fn every_bus_message_round_trips() {
        round_trip(Join {
            connection: connection(),
            account: Uuid::new_v4(),
            character: Uuid::new_v4(),
            resume: Some(ResumeOf {
                entity: CONNECTION,
                handoffs: 1 << 32,
            }),
        });
        round_trip(JoinReply::Joined {
            sim: connection().gateway,
            generation: STARTED_AT_MS,
            entity: CONNECTION,
        });
        round_trip(JoinReply::Refused {
            reason: "no position".to_owned(),
        });
        round_trip(SimInput::ClientMessage {
            connection: connection(),
            envelope: vec![1, 2, 3],
        });
        round_trip(SimInput::Left {
            connection: connection(),
        });
        round_trip(GatewayOutput::ServerMessages {
            sim: connection().gateway,
            connection: CONNECTION,
            channel: 1,
            reliable: false,
            envelopes: vec![vec![4], vec![5, 6]],
        });
        round_trip(GatewayOutput::Close {
            connection: CONNECTION,
        });
        round_trip(GatewayOutput::Route {
            connection: CONNECTION,
            sim: connection().gateway,
            generation: STARTED_AT_MS,
            sequence: 2,
        });
        round_trip(Heartbeat {
            id: connection().gateway,
            started_at_ms: STARTED_AT_MS,
        });
    }

    fn sim() -> BusId {
        BusId::try_from("sim-1".to_owned()).expect("id")
    }

    const POINT: Point = Point {
        x: 1.5,
        y: -2.0,
        z: 300.25,
    };

    #[test]
    fn every_m5_message_round_trips() {
        round_trip(Admit {
            connection: connection(),
            account: Uuid::new_v4(),
            character: Uuid::new_v4(),
            entity: CONNECTION,
            position: POINT,
            login: true,
            handoffs: 0,
        });
        round_trip(AdmitReply::Admitted);
        round_trip(AdmitReply::Refused {
            reason: "full".to_owned(),
        });
        round_trip(CellTable {
            version: 3,
            cell_size_m: 2048.0,
            columns: 2,
            rows: 1,
            sims: vec![SimSlot {
                id: sim(),
                generation: STARTED_AT_MS,
            }],
            owners: vec![Some(0), None],
        });
        let offer = HandoffOffer {
            connection: connection(),
            character: Uuid::new_v4(),
            entity: CONNECTION,
            position: POINT,
            move_age_ms: 40,
            budget_m: 3.5,
            bundles_sent: 900,
            time_synch_age_ms: 1200,
            handoffs: 2,
        };
        round_trip(HandoffMessage::Offer(offer));
        round_trip(HandoffMessage::Cancel {
            connection: connection(),
            handoffs: 2,
        });
        round_trip(HandoffReply::Accepted);
        round_trip(HandoffReply::Refused {
            reason: "already here".to_owned(),
        });
        round_trip(Ghosts {
            from: sim(),
            entities: vec![Ghost {
                entity: CONNECTION,
                position: POINT,
            }],
        });
        round_trip(SimHeartbeat {
            id: sim(),
            started_at_ms: STARTED_AT_MS,
            players: 10,
            max_players: 2000,
        });
    }

    #[test]
    fn the_largest_ghost_fits_its_byte_limit() {
        let ghost = Ghost {
            entity: u64::MAX,
            position: Point {
                x: f32::MAX,
                y: f32::MIN,
                z: f32::NAN,
            },
        };
        assert!(encode(&ghost).expect("encode").len() <= MAX_GHOST_BYTES);
    }

    #[test]
    fn a_point_is_finite_only_with_three_numbers() {
        assert!(POINT.is_finite());
        assert!(
            !Point {
                z: f32::INFINITY,
                ..POINT
            }
            .is_finite()
        );
    }

    #[test]
    fn the_silence_limit_is_the_missed_intervals_and_saturates() {
        let second = Duration::from_secs(1);
        assert_eq!(heartbeat_silence(second, MISSES), second * MISSES);
        assert_eq!(heartbeat_silence(Duration::MAX, MISSES), Duration::MAX);
    }

    #[test]
    fn a_bad_gateway_id_or_other_bytes_do_not_decode() {
        let bytes = encode(&"a.b".to_owned()).expect("encode");
        assert!(decode::<BusId>(&bytes).is_err());
        assert!(decode::<Join>(&[0xFF; 3]).is_err());
    }
}
