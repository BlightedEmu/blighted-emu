// SPDX-License-Identifier: AGPL-3.0-or-later
//! What one test client learns from the world messages (M4 spec section 9): the order of the
//! login messages, its own entity number (from `ClientAddEntryMsg`), and every position of every
//! entity in the replication bundles, in order. A hunt bot also keeps the mobs it is sent and
//! counts the combat messages for it (M6-B spec section 9.1).

use std::collections::BTreeMap;

use nw_messages::{
    BundlePayload, LOGIN_MESSAGE_TYPES, MemberState, MessageError, ServerMessage, WorldPosition,
};
use thiserror::Error;

use crate::rep::Received;

/// A world message that does not decode.
#[derive(Debug, Error)]
pub enum ViewError {
    /// A message of a world type, or a bundle payload, does not decode.
    #[error("message type {type_index} does not decode: {source}")]
    Message {
        /// Its type index.
        type_index: u64,
        /// Why.
        #[source]
        source: MessageError,
    },
}

/// A combat message, by the entity it is for.
enum CombatEvent {
    /// 2071: a hit the entity made.
    Dealt { attacker: u64 },
    /// 3601: a hit the entity took.
    Taken { target: u64 },
    /// 4299: the entity died.
    Died { player: u64 },
}

/// One world message, read.
enum Read {
    /// Not a world message.
    Skipped,
    /// A bundle: the entity numbers and positions of its members, and the entity numbers of its
    /// spawn cards.
    Bundle {
        positions: Vec<(u64, WorldPosition)>,
        cards: Vec<u64>,
    },
    /// A combat message.
    Combat { type_index: u64, event: CombatEvent },
    /// `ClientAddEntryMsg`, with the client's own entity number.
    Entry { type_index: u64, own: Option<u64> },
    /// Another world message.
    Other { type_index: u64 },
}

fn decode(received: &Received) -> Result<Read, ViewError> {
    let envelope = &received.envelope;
    let Some(decoded) = ServerMessage::from_envelope(envelope) else {
        return Ok(Read::Skipped);
    };
    let type_index = envelope.type_index().unwrap_or_default();
    let failed = |source| ViewError::Message { type_index, source };
    Ok(match decoded.map_err(failed)? {
        ServerMessage::Bundle(bundle) => {
            let payload = BundlePayload::decode(&bundle.payload).map_err(failed)?;
            let members = || payload.groups.iter().flat_map(|group| &group.members);
            let positions = members()
                .filter_map(|member| match &member.state {
                    MemberState::Position(state) => Some((member.u64_at_0, state.position?)),
                    _ => None,
                })
                .collect();
            let cards = members()
                .filter(|member| matches!(member.state, MemberState::GdeMetadata(_)))
                .map(|member| member.u64_at_0)
                .collect();
            Read::Bundle { positions, cards }
        }
        ServerMessage::DamageDealt(hit) => Read::Combat {
            type_index,
            event: CombatEvent::Dealt {
                attacker: hit.head_b,
            },
        },
        ServerMessage::OnDamage(hit) => Read::Combat {
            type_index,
            event: CombatEvent::Taken { target: hit.head_a },
        },
        ServerMessage::DeathRecap(death) => Read::Combat {
            type_index,
            event: CombatEvent::Died {
                player: death.head_a,
            },
        },
        ServerMessage::ClientAddEntry(entry) => Read::Entry {
            type_index,
            own: entry.entity_number(),
        },
        _ => Read::Other { type_index },
    })
}

/// What a bot keeps of the world messages: its own entity number only. Many bots run in one
/// process, so the others' positions are read and dropped (M5 load spec section 7.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OwnView {
    own: Option<u64>,
}

impl OwnView {
    /// Reads one more message; returns the client's own position when it is a bundle with it.
    ///
    /// # Errors
    ///
    /// Returns [`ViewError`] for a world message or a bundle payload that does not decode.
    pub fn read(&mut self, received: &Received) -> Result<Option<WorldPosition>, ViewError> {
        Ok(self.apply(&decode(received)?))
    }

    /// Learns the own number from `read`; returns the own position when it is in a bundle.
    fn apply(&mut self, read: &Read) -> Option<WorldPosition> {
        match read {
            Read::Bundle { positions, .. } => positions
                .iter()
                .find(|(number, _)| Some(*number) == self.own)
                .map(|(_, position)| *position),
            Read::Entry { own, .. } => {
                self.own = *own;
                None
            }
            Read::Skipped | Read::Combat { .. } | Read::Other { .. } => None,
        }
    }

    /// Whether `number` is the own entity number.
    fn is_own(&self, number: u64) -> bool {
        self.own == Some(number)
    }
}

/// Bundles with the own position that a mob may miss before the view drops it: each tick sends
/// the own position once, and a mob in view in every tick.
pub const MAX_MISSED_ROUNDS: u64 = 2;

/// A mob the bot is sent.
#[derive(Debug, Clone, Copy)]
struct SeenMob {
    /// `None` until its first position member.
    position: Option<WorldPosition>,
    /// The round it was last sent in.
    round: u64,
}

/// What a hunt bot keeps: its own position, the mobs it is sent, and counts of the combat
/// messages for it. An entity is a mob from its spawn card on; a mob that is not sent for more
/// than [`MAX_MISSED_ROUNDS`] rounds (one round per bundle with the own position) is dead or out
/// of view, and it comes back with a new card.
#[derive(Debug, Clone, Default)]
pub struct CombatView {
    own: OwnView,
    round: u64,
    mobs: BTreeMap<u64, SeenMob>,
    hits_dealt: u64,
    hits_taken: u64,
    deaths: u64,
}

impl CombatView {
    /// Reads one more message; returns the client's own position when it is a bundle with it.
    ///
    /// # Errors
    ///
    /// Returns [`ViewError`] for a world message or a bundle payload that does not decode.
    pub fn read(&mut self, received: &Received) -> Result<Option<WorldPosition>, ViewError> {
        let read = decode(received)?;
        let own_position = self.own.apply(&read);
        match read {
            Read::Bundle { positions, cards } => {
                let round = self.round;
                for card in cards {
                    self.mobs.insert(
                        card,
                        SeenMob {
                            position: None,
                            round,
                        },
                    );
                }
                for (number, position) in positions {
                    if let Some(mob) = self.mobs.get_mut(&number) {
                        *mob = SeenMob {
                            position: Some(position),
                            round,
                        };
                    }
                }
                if own_position.is_some() {
                    self.round += 1;
                    let now = self.round;
                    self.mobs
                        .retain(|_, mob| now - mob.round <= MAX_MISSED_ROUNDS);
                }
            }
            Read::Combat { event, .. } => {
                let (counter, number) = match event {
                    CombatEvent::Dealt { attacker } => (&mut self.hits_dealt, attacker),
                    CombatEvent::Taken { target } => (&mut self.hits_taken, target),
                    CombatEvent::Died { player } => (&mut self.deaths, player),
                };
                if self.own.is_own(number) {
                    *counter += 1;
                }
            }
            Read::Skipped | Read::Entry { .. } | Read::Other { .. } => {}
        }
        Ok(own_position)
    }

    /// The mobs with a position, by entity number.
    pub fn mobs(&self) -> impl Iterator<Item = (u64, WorldPosition)> + '_ {
        self.mobs
            .iter()
            .filter_map(|(number, mob)| Some((*number, mob.position?)))
    }

    /// 2071 messages for hits this bot made.
    pub const fn hits_dealt(&self) -> u64 {
        self.hits_dealt
    }

    /// 3601 messages for hits this bot took.
    pub const fn hits_taken(&self) -> u64 {
        self.hits_taken
    }

    /// 4299 messages for this bot's deaths.
    pub const fn deaths(&self) -> u64 {
        self.deaths
    }
}

/// The world as one client saw it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldView {
    /// Type indexes of the world messages that are not bundles, in arrival order.
    login: Vec<u64>,
    /// How many of them came before the first bundle.
    login_before_first_bundle: Option<usize>,
    own: Option<u64>,
    tracks: BTreeMap<u64, Vec<WorldPosition>>,
}

impl WorldView {
    /// Reads `messages` in order. Messages of other types are skipped.
    ///
    /// # Errors
    ///
    /// Returns [`ViewError`] for a world message or a bundle payload that does not decode.
    pub fn from_messages(messages: &[Received]) -> Result<Self, ViewError> {
        let mut view = Self::default();
        for received in messages {
            view.read(received)?;
        }
        Ok(view)
    }

    /// Reads one more message. Messages of other types are skipped.
    ///
    /// # Errors
    ///
    /// Returns [`ViewError`] for a world message or a bundle payload that does not decode.
    pub fn read(&mut self, received: &Received) -> Result<(), ViewError> {
        match decode(received)? {
            Read::Skipped => {}
            Read::Bundle { positions, .. } => {
                self.login_before_first_bundle
                    .get_or_insert(self.login.len());
                for (number, position) in positions {
                    self.tracks.entry(number).or_default().push(position);
                }
            }
            Read::Entry { type_index, own } => {
                self.own = own;
                self.login.push(type_index);
            }
            Read::Combat { type_index, .. } | Read::Other { type_index } => {
                self.login.push(type_index);
            }
        }
        Ok(())
    }

    /// Whether the login messages came first, in the captured order, and bundles after them.
    pub fn in_captured_order(&self) -> bool {
        self.login.starts_with(LOGIN_MESSAGE_TYPES)
            && self
                .login_before_first_bundle
                .is_some_and(|count| count >= LOGIN_MESSAGE_TYPES.len())
    }

    /// The client's own entity number.
    pub const fn own_number(&self) -> Option<u64> {
        self.own
    }

    /// The last position of entity `number`.
    pub fn position(&self, number: u64) -> Option<WorldPosition> {
        self.track(number).last().copied()
    }

    /// Every position of entity `number`, in order.
    pub fn track(&self, number: u64) -> &[WorldPosition] {
        self.tracks.get(&number).map_or(&[], Vec::as_slice)
    }

    /// The client's own last position.
    pub fn own_position(&self) -> Option<WorldPosition> {
        self.position(self.own?)
    }

    /// Every other entity number the client saw, in ascending order.
    pub fn others(&self) -> Vec<u64> {
        self.tracks
            .keys()
            .copied()
            .filter(|number| Some(*number) != self.own)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use nw_messages::test_support::login_samples;
    use nw_messages::{
        AssetId, BodyCodec, BundleMember, ClientAddEntryMsg, DamageDealtMsg, DeathRecapMsg,
        GDE_ID_LEN, GdeMetadataReplicatedState, OnDamageMsg, PositionInTheWorldReplicatedState,
        ReplicatedStateBundle,
    };
    use nw_reflect::{Envelope, TypeId};

    use super::*;

    const OWN: u64 = 1;
    const OTHER: u64 = 2;
    const LOGIN_CHANNEL: u8 = 0;
    const BUNDLE_CHANNEL: u8 = 1;
    /// `PingMsg`: not a world message.
    const OTHER_TYPE_INDEX: u64 = 349;
    const SHORT_BODY: [u8; 3] = [1, 2, 3];

    fn received(channel: u8, message: &ServerMessage) -> Received {
        let body = message.encode().expect("encode");
        Received {
            channel,
            envelope: Envelope::message(TypeId::FileSlot(message.type_index()), body),
        }
    }

    fn login() -> Vec<Received> {
        login_samples(OWN)
            .iter()
            .map(|message| received(LOGIN_CHANNEL, message))
            .collect()
    }

    fn at(x: f32) -> WorldPosition {
        WorldPosition::from_metres(x, x, x).expect("finite")
    }

    fn bundle(sequence: u64, positions: &[(u64, WorldPosition)]) -> Received {
        let bundle = ReplicatedStateBundle::of_positions(sequence, positions).expect("bundle");
        received(BUNDLE_CHANNEL, &ServerMessage::Bundle(bundle))
    }

    #[test]
    fn the_login_order_the_own_number_and_the_last_positions_are_learned() {
        let mut messages = login();
        messages.push(bundle(0, &[(OWN, at(1.0)), (OTHER, at(2.0))]));
        messages.push(Received {
            channel: LOGIN_CHANNEL,
            envelope: Envelope::message(TypeId::FileSlot(OTHER_TYPE_INDEX), SHORT_BODY.to_vec()),
        });
        messages.push(bundle(1, &[(OWN, at(1.0)), (OTHER, at(3.0))]));
        let view = WorldView::from_messages(&messages).expect("view");
        assert!(view.in_captured_order());
        assert_eq!(view.own_number(), Some(OWN));
        assert_eq!(view.own_position(), Some(at(1.0)));
        assert_eq!(view.position(OTHER), Some(at(3.0)));
        assert_eq!(view.track(OTHER), [at(2.0), at(3.0)]);
        assert!(view.track(OTHER + 1).is_empty());
        assert_eq!(view.others(), vec![OTHER]);
    }

    #[test]
    fn the_light_view_keeps_only_the_own_position() {
        let mut view = OwnView::default();
        let before = bundle(0, &[(OWN, at(1.0))]);
        assert_eq!(
            view.read(&before).expect("read"),
            None,
            "the own number is not known yet"
        );
        for message in login() {
            assert_eq!(view.read(&message).expect("read"), None);
        }
        let seen = bundle(1, &[(OTHER, at(2.0)), (OWN, at(3.0))]);
        assert_eq!(view.read(&seen).expect("read"), Some(at(3.0)));
        let others_only = bundle(2, &[(OTHER, at(4.0))]);
        assert_eq!(view.read(&others_only).expect("read"), None);
    }

    #[test]
    fn messages_can_be_read_one_by_one() {
        let mut messages = login();
        messages.push(bundle(0, &[(OWN, at(1.0))]));
        let mut view = WorldView::default();
        for received in &messages {
            view.read(received).expect("read");
        }
        assert_eq!(view, WorldView::from_messages(&messages).expect("view"));
    }

    #[test]
    fn a_bundle_before_the_login_or_a_changed_order_is_out_of_order() {
        let mut early = vec![bundle(0, &[(OWN, at(1.0))])];
        early.extend(login());
        assert!(
            !WorldView::from_messages(&early)
                .expect("view")
                .in_captured_order()
        );
        let mut swapped = login();
        swapped.swap(0, 1);
        swapped.push(bundle(0, &[(OWN, at(1.0))]));
        assert!(
            !WorldView::from_messages(&swapped)
                .expect("view")
                .in_captured_order()
        );
        assert!(
            !WorldView::from_messages(&login())
                .expect("view")
                .in_captured_order()
        );
    }

    const MOB: u64 = (1 << 34) + 5;
    const HIT: f32 = 42.0;

    /// A bundle with the spawn card of `card` (if any), then `positions`.
    fn mob_bundle(
        sequence: u64,
        card: Option<u64>,
        positions: &[(u64, WorldPosition)],
    ) -> Received {
        let cards = card.map(|number| BundleMember {
            u64_at_0: number,
            state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
                asset: Some(AssetId {
                    id: [0; GDE_ID_LEN],
                    sub_id: 2,
                }),
                gde_ref: None,
            }),
        });
        let members = cards
            .into_iter()
            .chain(positions.iter().map(|(number, position)| BundleMember {
                u64_at_0: *number,
                state: MemberState::Position(PositionInTheWorldReplicatedState {
                    position: Some(*position),
                    ..PositionInTheWorldReplicatedState::default()
                }),
            }))
            .collect();
        let bundle = ReplicatedStateBundle::of_members(sequence, members).expect("bundle");
        received(BUNDLE_CHANNEL, &ServerMessage::Bundle(bundle))
    }

    fn combat_view() -> CombatView {
        let mut view = CombatView::default();
        for message in login() {
            view.read(&message).expect("read");
        }
        view
    }

    #[test]
    fn the_view_keeps_mobs_while_they_are_sent() {
        let mut view = combat_view();
        let first = mob_bundle(
            0,
            Some(MOB),
            &[(OWN, at(1.0)), (MOB, at(2.0)), (OTHER, at(3.0))],
        );
        assert_eq!(view.read(&first).expect("read"), Some(at(1.0)));
        assert_eq!(
            view.mobs().collect::<Vec<_>>(),
            vec![(MOB, at(2.0))],
            "a player is no mob"
        );
        let moved = mob_bundle(1, None, &[(OWN, at(1.0)), (MOB, at(4.0))]);
        view.read(&moved).expect("read");
        assert_eq!(view.mobs().collect::<Vec<_>>(), vec![(MOB, at(4.0))]);
        for sequence in 2..=2 + MAX_MISSED_ROUNDS {
            view.read(&mob_bundle(sequence, None, &[(OWN, at(1.0))]))
                .expect("read");
        }
        assert_eq!(
            view.mobs().count(),
            0,
            "no longer sent: dead or out of view"
        );
        let back = mob_bundle(9, None, &[(OWN, at(1.0)), (MOB, at(4.0))]);
        view.read(&back).expect("read");
        assert_eq!(view.mobs().count(), 0, "a mob comes back with a new card");
    }

    #[test]
    fn the_view_counts_hits_damage_and_deaths() {
        let mut view = combat_view();
        let messages = [
            ServerMessage::DamageDealt(DamageDealtMsg::hit(MOB, OWN, HIT)),
            ServerMessage::DamageDealt(DamageDealtMsg::hit(MOB, OTHER, HIT)),
            ServerMessage::OnDamage(OnDamageMsg::hit(OWN, MOB, HIT)),
            ServerMessage::OnDamage(OnDamageMsg::hit(OTHER, MOB, HIT)),
            ServerMessage::DeathRecap(DeathRecapMsg::of(OWN, MOB)),
            ServerMessage::DeathRecap(DeathRecapMsg::of(OTHER, MOB)),
        ];
        for message in &messages {
            assert_eq!(
                view.read(&received(LOGIN_CHANNEL, message)).expect("read"),
                None
            );
        }
        assert_eq!(
            (view.hits_dealt(), view.hits_taken(), view.deaths()),
            (1, 1, 1)
        );
    }

    #[test]
    fn a_world_message_that_does_not_decode_is_an_error() {
        let broken = Received {
            channel: LOGIN_CHANNEL,
            envelope: Envelope::message(
                TypeId::FileSlot(ClientAddEntryMsg::TYPE_INDEX),
                SHORT_BODY.to_vec(),
            ),
        };
        assert!(matches!(
            WorldView::from_messages(&[broken]),
            Err(ViewError::Message {
                type_index: ClientAddEntryMsg::TYPE_INDEX,
                ..
            })
        ));
    }
}
