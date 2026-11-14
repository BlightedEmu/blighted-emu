// SPDX-License-Identifier: AGPL-3.0-or-later
//! The scrub of a pack table: owner and turret states set to no one, the capturing player's ids
//! in the tutorial bodies zeroed, capture names and tool notes removed, and the retail times and
//! capture counts that no code reads removed.

use nw_content::ftue_entities::{FACET_LEN, GDE_REF_LEN};
use serde_json::Value;

use super::PackError;
use super::owner::{neutral_identity, rewrite_identities};

/// The text every `source` note becomes.
pub const NEUTRAL_SOURCE: &str = "Blighted Emu realm data pack";

/// The key of a capture file name.
const LEDGER_KEY: &str = "ledger";
/// The key of a tool note on where a table came from.
const SOURCE_KEY: &str = "source";
/// The gate rules table and its list of capture file names per level.
const GATE_RULES_TABLE: &str = "dungeon-gate-rules";
const LEVELS_KEY: &str = "levels";
const LEDGERS_KEY: &str = "ledgers";
/// The keys of a tutorial member body and the offsets of the capturing player's `GdeRef` and
/// facet id in it (`EntityMember`; the sim writes the session player's ids there).
const BODY_KEY: &str = "body";
const PLAYER_GDE_REF_AT_KEY: &str = "player_gde_ref_at";
const PLAYER_FACET_AT_KEY: &str = "player_facet_at";

/// Retail times and capture counts that no loader declares, per table (checked 2026-11-13 over
/// every field, private rows too, of the `nw-content` loaders). A declared field stays, such as
/// the `seen` count of `mob-prefabs` and the `ledgers` count of `breach-tiers`.
const UNREAD_FIELDS: &[(&str, &[&str])] = &[
    ("war-forts", &["unix_ms"]),
    ("invasion-forts", &["start_ms"]),
    ("shot-entities", &["seen"]),
];

/// What a scrub did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scrubbed {
    /// Owner, turret and territory interactor states found.
    pub identity_states: usize,
    /// The capturing player's ids zeroed in the bodies (each value once; the scan refuses them
    /// anywhere in the pack).
    pub player_ids: Vec<Vec<u8>>,
}

/// Scrubs `value`, the JSON of pack table `table`, in place.
///
/// # Errors
///
/// Returns [`PackError::MemberDecode`] when an identity state does not decode or a player id
/// offset is outside its body.
pub fn scrub_table(table: &str, value: &mut Value) -> Result<Scrubbed, PackError> {
    let visit = rewrite_identities(table, value, neutral_identity)?;
    let mut player_ids = Vec::new();
    zero_player_ids(table, value, &mut String::new(), &mut player_ids)?;
    let unread = UNREAD_FIELDS
        .iter()
        .find(|(name, _)| *name == table)
        .map_or(&[][..], |(_, keys)| *keys);
    scrub_keys(value, unread);
    if table == GATE_RULES_TABLE
        && let Some(Value::Array(levels)) = value.get_mut(LEVELS_KEY)
    {
        for level in levels {
            if let Some(ledgers) = level.get_mut(LEDGERS_KEY) {
                *ledgers = Value::Array(Vec::new());
            }
        }
    }
    Ok(Scrubbed {
        identity_states: visit.identity_states,
        player_ids,
    })
}

/// Zeroes the capturing player's `GdeRef` and facet id at their offsets in every body, and
/// keeps each value that was there (not all zero) in `ids`.
fn zero_player_ids(
    table: &str,
    value: &mut Value,
    path: &mut String,
    ids: &mut Vec<Vec<u8>>,
) -> Result<(), PackError> {
    match value {
        Value::Object(map) => {
            let offsets: Vec<(usize, usize)> = [
                (PLAYER_GDE_REF_AT_KEY, GDE_REF_LEN),
                (PLAYER_FACET_AT_KEY, FACET_LEN),
            ]
            .into_iter()
            .flat_map(|(key, len)| {
                map.get(key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_u64)
                    .filter_map(|at| usize::try_from(at).ok())
                    .map(move |at| (at, len))
            })
            .collect();
            if !offsets.is_empty()
                && let Some(Value::String(body)) = map.get_mut(BODY_KEY)
            {
                let body_path = format!("{path}/{BODY_KEY}");
                let error = |reason: String| PackError::MemberDecode {
                    table: table.to_owned(),
                    path: body_path.clone(),
                    reason,
                };
                let mut bytes = hex::decode(&*body).map_err(|e| error(e.to_string()))?;
                for (at, len) in offsets {
                    let id = bytes
                        .get_mut(at..at + len)
                        .ok_or_else(|| error(format!("a player id at {at} is outside the body")))?;
                    if id.iter().any(|byte| *byte != 0) && !ids.iter().any(|known| known == id) {
                        ids.push(id.to_vec());
                    }
                    id.fill(0);
                }
                *body = hex::encode(bytes);
            }
            for (key, child) in map.iter_mut() {
                let len = path.len();
                path.push('/');
                path.push_str(key);
                zero_player_ids(table, child, path, ids)?;
                path.truncate(len);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                let len = path.len();
                path.push('/');
                path.push_str(&index.to_string());
                zero_player_ids(table, item, path, ids)?;
                path.truncate(len);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Removes every `ledger` key and every key of `unread`, and sets every `source` to
/// [`NEUTRAL_SOURCE`], at any depth.
fn scrub_keys(value: &mut Value, unread: &[&str]) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| key != LEDGER_KEY && !unread.contains(&key.as_str()));
            if let Some(source) = map.get_mut(SOURCE_KEY) {
                *source = Value::String(NEUTRAL_SOURCE.to_owned());
            }
            for child in map.values_mut() {
                scrub_keys(child, unread);
            }
        }
        Value::Array(items) => {
            for item in items {
                scrub_keys(item, unread);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::owner::tests::{MADE_UP_COMPANY, owned_state, owner_member_hex};
    use super::*;

    #[test]
    fn zeroes_the_player_ids_of_a_tutorial_body() {
        let mut body = vec![0xaa; 4];
        body.extend_from_slice(&[7; GDE_REF_LEN]);
        body.extend_from_slice(&[8; FACET_LEN]);
        let mut table = json!({"entities": [{"spawns": [{"members": [{
            "class": 1, "body": hex::encode(&body),
            "player_gde_ref_at": [4], "player_facet_at": [4 + GDE_REF_LEN]
        }]}]}]});
        let scrubbed = scrub_table("ftue-entities", &mut table).unwrap();
        assert_eq!(
            scrubbed.player_ids,
            [vec![7; GDE_REF_LEN], vec![8; FACET_LEN]]
        );
        let mut zeroed = vec![0xaa; 4];
        zeroed.extend_from_slice(&[0; GDE_REF_LEN + FACET_LEN]);
        assert_eq!(
            table["entities"][0]["spawns"][0]["members"][0]["body"],
            json!(hex::encode(zeroed))
        );
    }

    #[test]
    fn a_player_id_outside_its_body_is_an_error() {
        let mut table = json!([{"class": 1, "body": "00", "player_gde_ref_at": [0]}]);
        assert!(matches!(
            scrub_table("ftue-entities", &mut table),
            Err(PackError::MemberDecode { .. })
        ));
    }

    #[test]
    fn removes_ledger_fields_at_any_depth() {
        let mut table =
            json!({"forts": [{"ledger": "made-up-0123456789abcdef/ledger.bin", "kind": 1}]});
        scrub_table("invasion-forts", &mut table).unwrap();
        assert_eq!(table, json!({"forts": [{"kind": 1}]}));
    }

    #[test]
    fn sets_source_to_the_neutral_text() {
        let mut table = json!({"source": "made-up tool over 3 ledgers", "shots": []});
        scrub_table("shot-entities", &mut table).unwrap();
        assert_eq!(table["source"], json!(NEUTRAL_SOURCE));
    }

    #[test]
    fn empties_gate_rule_ledgers() {
        let mut table = json!({
            "source": "made-up",
            "levels": [{"script_slice": "a/b.slice", "ledgers": ["made-up-0123456789abcdef/ledger.bin"], "gates": []}]
        });
        scrub_table(GATE_RULES_TABLE, &mut table).unwrap();
        assert_eq!(table["levels"][0]["ledgers"], json!([]));
        assert_eq!(table["levels"][0]["script_slice"], json!("a/b.slice"));
        nw_content::dungeon_gates::GateRules::parse(&table.to_string()).unwrap();
    }

    #[test]
    fn removes_unread_fields() {
        let mut table = json!({"forts": [{"timers": [{"unix_ms": 1, "delay_s": 2.0}]}]});
        scrub_table("war-forts", &mut table).unwrap();
        assert_eq!(table, json!({"forts": [{"timers": [{"delay_s": 2.0}]}]}));
    }

    #[test]
    fn keeps_declared_fields() {
        let mut table = json!([{"x": 1.0, "y": 2.0, "difficulty": 3, "ledgers": 4}]);
        let before = table.clone();
        scrub_table("breach-tiers", &mut table).unwrap();
        assert_eq!(table, before);
        let mut prefabs = json!([{"class": 1, "seen": 3}]);
        let before = prefabs.clone();
        scrub_table("mob-prefabs", &mut prefabs).unwrap();
        assert_eq!(prefabs, before, "the loader reads the seen count");
    }

    #[test]
    fn neutralizes_owner_states() {
        let mut table =
            json!({"props": [{"asset": "00", "members": [owner_member_hex(owned_state())]}]});
        let visit = scrub_table("town-props", &mut table).unwrap();
        assert_eq!(visit.identity_states, 1);
        assert!(!table.to_string().contains(&hex::encode(MADE_UP_COMPANY)));
    }
}
