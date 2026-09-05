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

/// A player with a checked ticket joins the world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Join {
    /// The connection.
    pub connection: ConnectionRef,
    /// The account.
    pub account: Uuid,
    /// The character.
    pub character: Uuid,
}

/// The answer to [`Join`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinReply {
    /// The character is in the world.
    Joined,
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
        });
        round_trip(JoinReply::Joined);
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
            connection: CONNECTION,
            channel: 1,
            reliable: false,
            envelopes: vec![vec![4], vec![5, 6]],
        });
        round_trip(GatewayOutput::Close {
            connection: CONNECTION,
        });
        round_trip(Heartbeat {
            id: connection().gateway,
            started_at_ms: STARTED_AT_MS,
        });
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
