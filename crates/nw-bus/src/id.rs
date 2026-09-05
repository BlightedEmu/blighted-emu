// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ids of gateways and `sim` processes inside subjects.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Longest id.
pub const MAX_BUS_ID_LEN: usize = 64;

/// An id that is safe inside a NATS subject: 1 to 64 of `A-Z`, `a-z`, `0-9`, `_`, `-`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BusId(String);

/// An id with a character that a subject does not allow, or of a wrong length.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("bus id must be 1 to {MAX_BUS_ID_LEN} of A-Z, a-z, 0-9, _ and -: {0:?}")]
pub struct BusIdError(pub String);

impl BusId {
    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BusId {
    type Error = BusIdError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let allowed = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
        if text.is_empty() || text.len() > MAX_BUS_ID_LEN || !text.chars().all(allowed) {
            return Err(BusIdError(text));
        }
        Ok(Self(text))
    }
}

impl From<BusId> for String {
    fn from(id: BusId) -> Self {
        id.0
    }
}

impl fmt::Display for BusId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_tokens_are_checked() {
        assert!(BusId::try_from("gateway-1".to_owned()).is_ok());
        assert!(BusId::try_from("sim_A".to_owned()).is_ok());
        for bad in ["", "a.b", "a*", "a>", "a b", "ü"] {
            assert!(BusId::try_from(bad.to_owned()).is_err(), "{bad:?}");
        }
        assert!(BusId::try_from("x".repeat(MAX_BUS_ID_LEN + 1)).is_err());
    }
}
