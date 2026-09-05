// SPDX-License-Identifier: AGPL-3.0-or-later
//! What one test client learns from the world messages (M4 spec section 9): the order of the
//! login messages, its own entity number (from `ClientAddEntryMsg`), and every position of every
//! entity in the replication bundles, in order.

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

/// One world message, read.
enum Read {
    /// Not a world message.
    Skipped,
    /// A bundle: the entity numbers and positions of its members.
    Bundle(Vec<(u64, WorldPosition)>),
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
            let members = payload
                .groups
                .iter()
                .flat_map(|group| &group.members)
                .filter_map(|member| {
                    let MemberState::Position(state) = &member.state;
                    Some((member.u64_at_0, state.position?))
                })
                .collect();
            Read::Bundle(members)
        }
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
        Ok(match decode(received)? {
            Read::Bundle(members) => members
                .into_iter()
                .find(|(number, _)| Some(*number) == self.own)
                .map(|(_, position)| position),
            Read::Entry { own, .. } => {
                self.own = own;
                None
            }
            Read::Skipped | Read::Other { .. } => None,
        })
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
            Read::Bundle(members) => {
                self.login_before_first_bundle
                    .get_or_insert(self.login.len());
                for (number, position) in members {
                    self.tracks.entry(number).or_default().push(position);
                }
            }
            Read::Entry { type_index, own } => {
                self.own = own;
                self.login.push(type_index);
            }
            Read::Other { type_index } => self.login.push(type_index),
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
    use nw_messages::{BodyCodec, ClientAddEntryMsg, ReplicatedStateBundle};
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
