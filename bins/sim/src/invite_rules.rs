// SPDX-License-Identifier: AGPL-3.0-or-later
//! The generic invite activities (`GenericInvites`, `javelindata_genericinvites`): how long an
//! invite waits for its answer (`DurationSec`; retail invite states expire ten seconds after
//! the invite, as the `Trade` row says) and how far apart the two players may stand
//! (`MaxDistance`, metres), by the CRC-32 of the lowercase `ActivityCrc` the client sends.

use std::collections::HashMap;

use nw_content::Tables;
use nw_formats::objectstream::lowercase_crc32;

/// Milliseconds per second.
const MS_PER_SECOND: u64 = 1_000;

/// One invite activity.
#[derive(Debug, Clone, PartialEq)]
pub struct InviteActivity {
    /// `ActivityCrc` (`Trade`, `Duel`, `MusicalPerformance`, `Shard`).
    pub name: String,
    /// `DurationSec`, in milliseconds.
    pub duration_ms: u64,
    /// `MaxDistance`, metres.
    pub max_distance_m: f32,
}

/// The invite activities by the CRC-32 of their lowercase name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InviteRules {
    activities: HashMap<u32, InviteActivity>,
}

impl InviteRules {
    /// The activities of `tables`; a row with a negative duration or distance is left out.
    pub fn from_tables(tables: &Tables) -> Self {
        Self::of(
            tables
                .generic_invite_data
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| {
                    let seconds = u64::try_from(row.duration_sec).ok()?;
                    let distance = u16::try_from(row.max_distance).ok()?;
                    Some(InviteActivity {
                        name: row.activity_crc.clone(),
                        duration_ms: seconds.saturating_mul(MS_PER_SECOND),
                        max_distance_m: f32::from(distance),
                    })
                }),
        )
    }

    /// The rules of `activities`.
    pub fn of(activities: impl IntoIterator<Item = InviteActivity>) -> Self {
        Self {
            activities: activities
                .into_iter()
                .map(|activity| (lowercase_crc32(&activity.name), activity))
                .collect(),
        }
    }

    /// The activity whose lowercase name has the CRC-32 `crc`.
    pub fn activity(&self, crc: u32) -> Option<&InviteActivity> {
        self.activities.get(&crc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The client's `Trade` row: 10 seconds, 10 metres.
    #[test]
    fn an_activity_is_found_by_the_crc_the_client_sends() {
        let rules = InviteRules::of([InviteActivity {
            name: "Trade".to_owned(),
            duration_ms: 10_000,
            max_distance_m: 10.0,
        }]);
        let trade = rules.activity(0x7e1a_4366).expect("trade");
        assert_eq!(trade.name, "Trade");
        assert!(rules.activity(0).is_none());
    }
}
