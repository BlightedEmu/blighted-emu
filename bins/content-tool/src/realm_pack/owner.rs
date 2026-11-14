// SPDX-License-Identifier: AGPL-3.0-or-later
//! The replicated states inside the pack tables that can name a player or a company:
//! - owner states (`OwnershipState`, 3217): a retail company's name, ID and crest, an owner's
//!   identification and a raid; the pack sets them to the values retail sends for a structure no
//!   one owns;
//! - turret states (`TurretState`, 4276): the `GdeRef` of the player at the turret; the pack sets
//!   it to zero (no one);
//! - territory interactor states (`TerritoryInteractorState`, 1880): a war roster of players and
//!   companies; the pack cannot hold one with players, so the scan refuses it.
//!
//! These states sit in two shapes: hex strings in a `members` list (a whole replicated member:
//! component, class, state) and objects with a numeric `class` and a hex `body` (the state
//! alone), at any depth, also inside a `members` list.

use nw_messages::buildables::{GUILD_ID_LEN, NO_OWNER_KIND};
use nw_messages::war_states::{GDE_REF_LEN, TURRET_TYPE_INDEX};
use nw_messages::{
    BundleMember, GuildCrest, MemberState, MessageError, OWNERSHIP_TYPE_INDEX, OwnerIdentification,
    OwnershipState, Reader, TERRITORY_INTERACTOR_TYPE_INDEX, TerritoryInteractorState, TurretState,
    Writer,
};
use serde_json::Value;

use super::PackError;

/// The key of a list of hex members.
const MEMBERS_KEY: &str = "members";
/// The keys of a class and its state body.
const CLASS_KEY: &str = "class";
const BODY_KEY: &str = "body";
/// The raid id of no raid.
const NO_RAID: u64 = 0;
/// The `GdeRef` of no one.
const NO_GDE_REF: [u8; GDE_REF_LEN] = [0; GDE_REF_LEN];

/// What a walk over a table found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OwnerVisit {
    /// Hex members read.
    pub members: usize,
    /// Hex members of a class this code has no reader for (left as they are).
    pub unknown_class_members: usize,
    /// Owner, turret and territory interactor states found (both shapes).
    pub identity_states: usize,
}

/// A replicated state that can name a player or a company.
#[derive(Debug, Clone, PartialEq)]
pub enum IdentityState {
    /// An owner state.
    Owner(OwnershipState),
    /// A turret state.
    Turret(TurretState),
    /// A territory interactor state (a war roster).
    Roster(Box<TerritoryInteractorState>),
}

impl IdentityState {
    fn of_member(state: &MemberState) -> Option<Self> {
        match state {
            MemberState::Ownership(state) => Some(Self::Owner(state.clone())),
            MemberState::Turret(state) => Some(Self::Turret(*state)),
            MemberState::TerritoryInteractor(state) => Some(Self::Roster(state.clone())),
            _ => None,
        }
    }

    fn into_member(self) -> MemberState {
        match self {
            Self::Owner(state) => MemberState::Ownership(state),
            Self::Turret(state) => MemberState::Turret(state),
            Self::Roster(state) => MemberState::TerritoryInteractor(state),
        }
    }

    /// The state of a `class` plus `body` object, when `class` is one of the identity classes.
    fn read_body(class: u64, reader: &mut Reader<'_>) -> Option<Result<Self, MessageError>> {
        match class {
            OWNERSHIP_TYPE_INDEX => Some(OwnershipState::read(reader).map(Self::Owner)),
            TURRET_TYPE_INDEX => Some(TurretState::read(reader).map(Self::Turret)),
            TERRITORY_INTERACTOR_TYPE_INDEX => Some(
                TerritoryInteractorState::read(reader).map(|state| Self::Roster(Box::new(state))),
            ),
            _ => None,
        }
    }

    fn write_body(&self, writer: &mut Writer) -> Result<(), MessageError> {
        match self {
            Self::Owner(state) => state.write(writer),
            Self::Turret(state) => {
                state.write(writer);
                Ok(())
            }
            Self::Roster(state) => state.write(writer),
        }
    }
}

/// `state` with the company and owner fields set to the values retail sends for a structure no
/// one owns (Observed: the retail `OwnershipState` of an unowned station); every other field kept.
pub fn neutral_owner(state: &OwnershipState) -> OwnershipState {
    OwnershipState {
        guild_id: state.guild_id.map(|_| [0; GUILD_ID_LEN]),
        guild_owner: state.guild_owner.as_ref().map(|_| Vec::new()),
        guild_crest: state.guild_crest.map(|_| GuildCrest::NONE),
        owner: state.owner.as_ref().map(|_| OwnerIdentification {
            first: Vec::new(),
            second: Vec::new(),
            kind: NO_OWNER_KIND,
        }),
        raid_id: state.raid_id.map(|_| NO_RAID),
        ..state.clone()
    }
}

/// Whether `state` still names a company, a crest, an owner or a raid.
pub fn has_owner_identity(state: &OwnershipState) -> bool {
    state.guild_id.is_some_and(|id| id != [0; GUILD_ID_LEN])
        || state
            .guild_owner
            .as_ref()
            .is_some_and(|name| !name.is_empty())
        || state
            .guild_crest
            .is_some_and(|crest| crest != GuildCrest::NONE)
        || state.owner.as_ref().is_some_and(|owner| {
            !owner.first.is_empty() || !owner.second.is_empty() || owner.kind != NO_OWNER_KIND
        })
        || state.raid_id.is_some_and(|raid| raid != NO_RAID)
}

/// `state` with every player and company field set to no one; none when nothing changes. A war
/// roster with players cannot be made neutral here: the scan refuses it.
pub fn neutral_identity(state: &IdentityState) -> Option<IdentityState> {
    match state {
        IdentityState::Owner(owner) if has_owner_identity(owner) => {
            Some(IdentityState::Owner(neutral_owner(owner)))
        }
        IdentityState::Turret(turret) if has_identity(state) => {
            Some(IdentityState::Turret(TurretState {
                interactor: Some(NO_GDE_REF),
                ..*turret
            }))
        }
        _ => None,
    }
}

/// Whether `state` still names a player or a company.
pub fn has_identity(state: &IdentityState) -> bool {
    match state {
        IdentityState::Owner(owner) => has_owner_identity(owner),
        IdentityState::Turret(turret) => turret
            .interactor
            .is_some_and(|interactor| interactor != NO_GDE_REF),
        IdentityState::Roster(roster) => !roster.players.is_empty(),
    }
}

/// Rewrites every identity state of `value` (the JSON of table `table`) in place with
/// `rewrite`, which gives the new state or none to keep the bytes.
///
/// # Errors
///
/// Returns [`PackError::MemberDecode`] for a member or an identity body that does not decode,
/// or that leaves bytes after its state.
pub fn rewrite_identities(
    table: &str,
    value: &mut Value,
    mut rewrite: impl FnMut(&IdentityState) -> Option<IdentityState>,
) -> Result<OwnerVisit, PackError> {
    let mut visit = OwnerVisit::default();
    walk(
        table,
        value,
        &mut String::new(),
        &mut visit,
        &mut |_, state| rewrite(state),
    )?;
    Ok(visit)
}

/// Every identity state of `value` (the JSON of table `table`) with its JSON path. Every hex
/// member is decoded on the way, so this is also the pack's member decode check.
///
/// # Errors
///
/// As [`rewrite_identities`].
pub fn collect_identities(
    table: &str,
    value: &Value,
) -> Result<(OwnerVisit, Vec<(String, IdentityState)>), PackError> {
    // The walk takes a mutable value; it writes nothing back when the callback returns none.
    let mut copy = value.clone();
    let mut found = Vec::new();
    let mut visit = OwnerVisit::default();
    walk(
        table,
        &mut copy,
        &mut String::new(),
        &mut visit,
        &mut |path, state| {
            found.push((path.to_owned(), state.clone()));
            None
        },
    )?;
    Ok((visit, found))
}

/// The callback of a walk: the JSON path and the identity state; the state to write back, or
/// none to leave the bytes as they are.
type OnIdentity<'a> = dyn FnMut(&str, &IdentityState) -> Option<IdentityState> + 'a;

/// The class of an object with a numeric `class` and a hex `body`.
fn body_class(map: &serde_json::Map<String, Value>) -> Option<u64> {
    map.get(BODY_KEY)
        .is_some_and(Value::is_string)
        .then(|| map.get(CLASS_KEY).and_then(Value::as_u64))
        .flatten()
}

fn walk(
    table: &str,
    value: &mut Value,
    path: &mut String,
    visit: &mut OwnerVisit,
    on_identity: &mut OnIdentity<'_>,
) -> Result<(), PackError> {
    match value {
        Value::Object(map) => {
            let class = body_class(map);
            for (key, child) in map.iter_mut() {
                let len = path.len();
                path.push('/');
                path.push_str(key);
                if let Some(class) = class
                    && key == BODY_KEY
                    && let Value::String(body) = child
                {
                    rewrite_body(table, path, class, body, visit, on_identity)?;
                } else if key == MEMBERS_KEY
                    && let Value::Array(members) = child
                {
                    // A list holds hex members, or objects with a class and a body
                    // (`ftue-entities` spawns).
                    for (index, member) in members.iter_mut().enumerate() {
                        let member_path = format!("{path}/{index}");
                        if let Value::String(text) = member {
                            rewrite_member(table, &member_path, text, visit, on_identity)?;
                        } else {
                            let mut nested = member_path;
                            walk(table, member, &mut nested, visit, on_identity)?;
                        }
                    }
                } else {
                    walk(table, child, path, visit, on_identity)?;
                }
                path.truncate(len);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                let len = path.len();
                path.push('/');
                path.push_str(&index.to_string());
                walk(table, item, path, visit, on_identity)?;
                path.truncate(len);
            }
        }
        _ => {}
    }
    Ok(())
}

fn decode_error(table: &str, path: &str, reason: impl ToString) -> PackError {
    PackError::MemberDecode {
        table: table.to_owned(),
        path: path.to_owned(),
        reason: reason.to_string(),
    }
}

fn rewrite_member(
    table: &str,
    path: &str,
    text: &mut String,
    visit: &mut OwnerVisit,
    on_identity: &mut OnIdentity<'_>,
) -> Result<(), PackError> {
    let bytes = hex::decode(&*text).map_err(|error| decode_error(table, path, error))?;
    visit.members += 1;
    let (member, used) = match BundleMember::read_from(&bytes) {
        Ok(read) => read,
        Err(MessageError::UnknownMemberClass { .. }) => {
            visit.unknown_class_members += 1;
            return Ok(());
        }
        Err(error) => return Err(decode_error(table, path, error)),
    };
    if used != bytes.len() {
        return Err(decode_error(
            table,
            path,
            format!("{} bytes after the state", bytes.len() - used),
        ));
    }
    let Some(state) = IdentityState::of_member(&member.state) else {
        return Ok(());
    };
    visit.identity_states += 1;
    if let Some(new) = on_identity(path, &state) {
        let encoded = BundleMember {
            component: member.component,
            state: new.into_member(),
        }
        .encode()
        .map_err(|error| decode_error(table, path, error))?;
        *text = hex::encode(encoded);
    }
    Ok(())
}

fn rewrite_body(
    table: &str,
    path: &str,
    class: u64,
    text: &mut String,
    visit: &mut OwnerVisit,
    on_identity: &mut OnIdentity<'_>,
) -> Result<(), PackError> {
    let bytes = hex::decode(&*text).map_err(|error| decode_error(table, path, error))?;
    let mut reader = Reader::new(&bytes);
    let Some(state) = IdentityState::read_body(class, &mut reader) else {
        return Ok(());
    };
    let state = state.map_err(|error| decode_error(table, path, error))?;
    reader
        .finish()
        .map_err(|error| decode_error(table, path, error))?;
    visit.identity_states += 1;
    if let Some(new) = on_identity(path, &state) {
        let mut writer = Writer::new();
        new.write_body(&mut writer)
            .map_err(|error| decode_error(table, path, error))?;
        *text = hex::encode(writer.finish());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::json;

    use super::*;

    /// A made-up company name.
    pub(crate) const MADE_UP_COMPANY: &[u8] = b"Made Up Company";
    /// The slice structure name of the made-up owner state (game data, kept by the scrub).
    pub(crate) const STRUCTURE_NAME: &[u8] = b"Station";
    const TABLE: &str = "town-props";
    const COMPONENT: u64 = 4;
    /// A class index with no reader and no wire layout.
    const UNKNOWN_CLASS: u64 = 60_000;

    /// An owner state with made-up company and owner data.
    pub(crate) fn owned_state() -> OwnershipState {
        OwnershipState {
            guild_id: Some([7; GUILD_ID_LEN]),
            guild_owner: Some(MADE_UP_COMPANY.to_vec()),
            guild_crest: Some(GuildCrest {
                background: 3,
                ..GuildCrest::NONE
            }),
            owner: Some(OwnerIdentification {
                first: b"made-up-first".to_vec(),
                second: b"made-up-second".to_vec(),
                kind: 1,
            }),
            permission_rule: Some(1),
            structure_name: Some(STRUCTURE_NAME.to_vec()),
            raid_id: Some(7),
            faction_type: Some(1),
            owner_pvp: Some(false),
            owner_ffa: Some(false),
            permission_options: Some(0),
        }
    }

    /// The hex of a whole member holding `state`.
    pub(crate) fn owner_member_hex(state: OwnershipState) -> String {
        hex::encode(
            BundleMember {
                component: COMPONENT,
                state: MemberState::Ownership(state),
            }
            .encode()
            .unwrap(),
        )
    }

    /// The hex of `state` alone.
    pub(crate) fn owner_body_hex(state: &OwnershipState) -> String {
        let mut writer = Writer::new();
        state.write(&mut writer).unwrap();
        hex::encode(writer.finish())
    }

    fn contains_company(value: &Value) -> bool {
        value.to_string().contains(&hex::encode(MADE_UP_COMPANY))
    }

    #[test]
    fn neutral_owner_clears_identity_and_keeps_the_rest() {
        let neutral = neutral_owner(&owned_state());
        assert!(has_owner_identity(&owned_state()));
        assert!(!has_owner_identity(&neutral));
        assert_eq!(neutral.faction_type, Some(1));
        assert_eq!(neutral.structure_name, Some(STRUCTURE_NAME.to_vec()));
        assert_eq!(neutral.permission_rule, Some(1));
    }

    #[test]
    fn rewrites_an_owner_member_in_a_members_list() {
        let mut table =
            json!({"props": [{"asset": "00", "members": [owner_member_hex(owned_state())]}]});
        assert!(contains_company(&table));
        let visit = rewrite_identities(TABLE, &mut table, neutral_identity).unwrap();
        assert_eq!(visit.identity_states, 1);
        assert!(!contains_company(&table));
        let (_, states) = collect_identities(TABLE, &table).unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].0, "/props/0/members/0");
        assert!(!has_identity(&states[0].1));
    }

    #[test]
    fn rewrites_a_class_3217_body() {
        let mut table = json!([{"components": [{"class": OWNERSHIP_TYPE_INDEX, "body": owner_body_hex(&owned_state())}]}]);
        let visit = rewrite_identities("mob-prefabs", &mut table, neutral_identity).unwrap();
        assert_eq!(visit.identity_states, 1);
        assert!(!contains_company(&table));
        let (_, states) = collect_identities("mob-prefabs", &table).unwrap();
        assert_eq!(states[0].0, "/0/components/0/body");
        assert!(!has_identity(&states[0].1));
    }

    #[test]
    fn rewrites_a_class_3217_body_inside_a_members_list() {
        let mut table = json!({"entities": [{"spawns": [{"members": [
            {"component": 4, "class": OWNERSHIP_TYPE_INDEX, "body": owner_body_hex(&owned_state())}
        ]}]}]});
        let visit = rewrite_identities("ftue-entities", &mut table, neutral_identity).unwrap();
        assert_eq!(visit.identity_states, 1);
        assert!(!contains_company(&table));
        let (_, states) = collect_identities("ftue-entities", &table).unwrap();
        assert_eq!(states[0].0, "/entities/0/spawns/0/members/0/body");
        assert!(!has_identity(&states[0].1));
    }

    #[test]
    fn zeroes_the_player_at_a_turret() {
        let turret = TurretState {
            interactor: Some([9; GDE_REF_LEN]),
            ..TurretState::default()
        };
        let member = hex::encode(
            BundleMember {
                component: COMPONENT,
                state: MemberState::Turret(turret),
            }
            .encode()
            .unwrap(),
        );
        let mut table = json!({"forts": [{"members": [member]}]});
        let visit = rewrite_identities("war-forts", &mut table, neutral_identity).unwrap();
        assert_eq!(visit.identity_states, 1);
        let (_, states) = collect_identities("war-forts", &table).unwrap();
        assert!(!has_identity(&states[0].1));
        assert_eq!(
            states[0].1,
            IdentityState::Turret(TurretState {
                interactor: Some(NO_GDE_REF),
                ..turret
            })
        );
    }

    #[test]
    fn a_roster_with_players_keeps_its_identity_for_the_scan() {
        let roster = IdentityState::Roster(Box::default());
        assert!(!has_identity(&roster), "an empty roster names no one");
        assert_eq!(neutral_identity(&roster), None);
    }

    #[test]
    fn keeps_other_members_byte_for_byte() {
        let other = hex::encode(
            BundleMember {
                component: COMPONENT,
                state: MemberState::Ownership(neutral_owner(&owned_state())),
            }
            .encode()
            .unwrap(),
        );
        let mut table = json!({"props": [{"members": [other.clone()]}]});
        rewrite_identities(TABLE, &mut table, |state| Some(state.clone())).unwrap();
        assert_eq!(table["props"][0]["members"][0], json!(other));
    }

    #[test]
    fn counts_unknown_class_members() {
        // Component 1, a class index no reader or wire layout knows, no body.
        let unknown = hex::encode(
            BundleMember {
                component: 1,
                state: MemberState::Recorded {
                    class: UNKNOWN_CLASS,
                    body: Vec::new(),
                },
            }
            .encode()
            .unwrap(),
        );
        let mut table = json!({"props": [{"members": [unknown]}]});
        let visit = rewrite_identities(TABLE, &mut table, neutral_identity).unwrap();
        assert_eq!(visit.members, 1);
        assert_eq!(visit.unknown_class_members, 1);
    }

    #[test]
    fn a_bad_owner_body_is_an_error() {
        let body = owner_body_hex(&owned_state());
        let mut table = json!([{"class": OWNERSHIP_TYPE_INDEX, "body": body[..body.len() - 4]}]);
        match rewrite_identities("mob-prefabs", &mut table, neutral_identity) {
            Err(PackError::MemberDecode { path, .. }) => assert_eq!(path, "/0/body"),
            other => panic!("expected a decode error, got {other:?}"),
        }
    }
}
