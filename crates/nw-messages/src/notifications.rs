// SPDX-License-Identifier: AGPL-3.0-or-later
//! The event notification calls (conversation-notify note, section 2): the server shows a
//! notification of `notification_data` (3233, or 3234 with text for its placeholders), the client
//! reports one it showed itself (3239), and the server keeps how many times each one was shown
//! (3236). Every id is the CRC-32 of the lowercase `NotificationID`.
//!
//! The calls go to the `EventNotificationComponent` on the player prefab's child entity
//! `GameEvents`: the target is (entity XOR [`GAME_EVENTS_CHILD`]) plus
//! [`EVENT_NOTIFICATION_COMPONENT`] (solved from the headers of 17 retail players; both ids are
//! in the player prefab).

use crate::combat::{read_string, write_string};
use crate::facet::FacetHeader;
use crate::wire::{BodyCodec, MessageError, Reader, Writer};

/// Type index of `EventNotificationComponentClientFacet_SyncNotifications`.
pub const SYNC_NOTIFICATIONS_TYPE_INDEX: u64 = 3236;
/// Type index of `EventNotificationComponentClientFacet_RequestSendNotification`.
pub const SEND_NOTIFICATION_TYPE_INDEX: u64 = 3233;
/// Type index of `EventNotificationComponentClientFacet_RequestSendNotificationWithReplacements`.
pub const SEND_NOTIFICATION_WITH_REPLACEMENTS_TYPE_INDEX: u64 = 3234;
/// Type index of `EventNotificationComponentServerFacet_NotificationSent`.
pub const NOTIFICATION_SENT_TYPE_INDEX: u64 = 3239;
/// The `Id` of the `EventNotificationComponent` in the player prefab.
pub const EVENT_NOTIFICATION_COMPONENT: u64 = 0x168b_3f98_1ab6_6a77;
/// The XOR of the player prefab's main entity `RootPlayer` and its child `GameEvents`.
pub const GAME_EVENTS_CHILD: u64 = 0x35ac_1128_02b6_2ece;
/// The last byte of every retail 3236 (988 of 988 bodies; its meaning is Unknown).
pub const SYNC_TRAILER: u8 = 1;

/// Bytes of one shown count.
const COUNT_LEN: usize = 8;
/// The fewest bytes of one replacement: its key and an empty text.
const MIN_REPLACEMENT_LEN: usize = 5;
const FIELD: &str = "event notification";

/// How many times one notification was shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationCount {
    /// CRC-32 of the lowercase `NotificationID`.
    pub notification: u32,
    /// How many times it was shown (kept per character across sessions).
    pub count: u32,
}

/// 3236: the shown counts of the player's notifications. Retail sends it in the login burst
/// (after the `ClientAddEntryMsg`, before the `LevelInfoChangedMsg`), 18 to 90 ms after each 3239
/// and in the same tick after each 3233 or 3234, each time the whole map with the count of that
/// notification one higher. A character with no shown notification gets none (both new
/// characters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncNotificationsMsg {
    /// The header (the player's `EventNotificationComponent`).
    pub header: FacetHeader,
    /// The counts.
    pub counts: Vec<NotificationCount>,
    /// The last byte ([`SYNC_TRAILER`]).
    pub trailer: u8,
}

impl SyncNotificationsMsg {
    /// The counts `counts` to player `player`.
    pub fn to(player: u64, counts: Vec<NotificationCount>) -> Self {
        Self {
            header: notification_header(player),
            counts,
            trailer: SYNC_TRAILER,
        }
    }
}

impl BodyCodec for SyncNotificationsMsg {
    const TYPE_INDEX: u64 = SYNC_NOTIFICATIONS_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        // A `usize` count fits 64 bits.
        writer.wide_prefixed(self.counts.len() as u64);
        for count in &self.counts {
            writer.u32(count.notification).u32(count.count);
        }
        Ok(writer.u8(self.trailer).finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let count = reader.wide_prefixed()?;
        // Each count takes `COUNT_LEN` bytes.
        if count > (reader.remaining() / COUNT_LEN) as u64 {
            return Err(MessageError::LengthMismatch { field: FIELD });
        }
        let counts = (0..count)
            .map(|_| {
                Ok(NotificationCount {
                    notification: reader.u32()?,
                    count: reader.u32()?,
                })
            })
            .collect::<Result<_, MessageError>>()?;
        let message = Self {
            header,
            counts,
            trailer: reader.u8()?,
        };
        reader.finish()?;
        Ok(message)
    }
}

/// 3233: the client shows notification `notification` (retail: `Place_Camp`,
/// `New_Items_Dropped`, `Trade_Failure_*`, `Damage_Ignored_Siege_Unclaimed_Points`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendNotificationMsg {
    /// The header (the player's `EventNotificationComponent`).
    pub header: FacetHeader,
    /// CRC-32 of the lowercase `NotificationID`.
    pub notification: u32,
}

impl SendNotificationMsg {
    /// Player `player` is shown notification `notification`.
    pub fn to(player: u64, notification: u32) -> Self {
        Self {
            header: notification_header(player),
            notification,
        }
    }
}

impl BodyCodec for SendNotificationMsg {
    const TYPE_INDEX: u64 = SEND_NOTIFICATION_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        encode_id(self.header, self.notification)
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let (header, notification) = decode_id(body)?;
        Ok(Self {
            header,
            notification,
        })
    }
}

/// One placeholder text of a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// CRC-32 of the lowercase placeholder name (retail `perkname` and `itemname`).
    pub key: u32,
    /// The text: a localization key such as `@Gambit4Name`, or plain text.
    pub text: Vec<u8>,
}

/// 3234: the client shows notification `notification` with `replacements` in its text (retail:
/// `CharmSlotRequestsSucceeded` with the perk and the item, 17 bodies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendNotificationWithReplacementsMsg {
    /// The header (the player's `EventNotificationComponent`).
    pub header: FacetHeader,
    /// CRC-32 of the lowercase `NotificationID`.
    pub notification: u32,
    /// The placeholder texts.
    pub replacements: Vec<Replacement>,
}

impl SendNotificationWithReplacementsMsg {
    /// Player `player` is shown notification `notification` with `replacements`.
    pub fn to(player: u64, notification: u32, replacements: Vec<Replacement>) -> Self {
        Self {
            header: notification_header(player),
            notification,
            replacements,
        }
    }
}

impl BodyCodec for SendNotificationWithReplacementsMsg {
    const TYPE_INDEX: u64 = SEND_NOTIFICATION_WITH_REPLACEMENTS_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        let mut writer = Writer::new();
        self.header.write(&mut writer);
        // A `usize` count fits 64 bits.
        writer
            .u32(self.notification)
            .wide_prefixed(self.replacements.len() as u64);
        for replacement in &self.replacements {
            writer.u32(replacement.key);
            write_string(&mut writer, &replacement.text)?;
        }
        Ok(writer.finish())
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let mut reader = Reader::new(body);
        let header = FacetHeader::read(&mut reader)?;
        let notification = reader.u32()?;
        let count = reader.wide_prefixed()?;
        // Each replacement takes at least `MIN_REPLACEMENT_LEN` bytes.
        if count > (reader.remaining() / MIN_REPLACEMENT_LEN) as u64 {
            return Err(MessageError::LengthMismatch { field: FIELD });
        }
        let replacements = (0..count)
            .map(|_| {
                Ok(Replacement {
                    key: reader.u32()?,
                    text: read_string(&mut reader)?,
                })
            })
            .collect::<Result<_, MessageError>>()?;
        reader.finish()?;
        Ok(Self {
            header,
            notification,
            replacements,
        })
    }
}

/// 3239: the client showed notification `notification` itself (retail: `Enter_Sanctuary`,
/// `Toggle_PvP`, `Exit_World_Bounds`, `Catacombs_Erosion_Lv1` to `Lv3`); the server counts it
/// and answers with 3236.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationSentMsg {
    /// The header (the player's `EventNotificationComponent`).
    pub header: FacetHeader,
    /// CRC-32 of the lowercase `NotificationID`.
    pub notification: u32,
}

impl BodyCodec for NotificationSentMsg {
    const TYPE_INDEX: u64 = NOTIFICATION_SENT_TYPE_INDEX;

    fn encode(&self) -> Result<Vec<u8>, MessageError> {
        encode_id(self.header, self.notification)
    }

    fn decode(body: &[u8]) -> Result<Self, MessageError> {
        let (header, notification) = decode_id(body)?;
        Ok(Self {
            header,
            notification,
        })
    }
}

/// The header of a call to player `player`'s `EventNotificationComponent`.
fn notification_header(player: u64) -> FacetHeader {
    FacetHeader::of_child(EVENT_NOTIFICATION_COMPONENT, GAME_EVENTS_CHILD, player)
}

/// A header and one notification id.
fn encode_id(header: FacetHeader, notification: u32) -> Result<Vec<u8>, MessageError> {
    let mut writer = Writer::new();
    header.write(&mut writer);
    Ok(writer.u32(notification).finish())
}

fn decode_id(body: &[u8]) -> Result<(FacetHeader, u32), MessageError> {
    let mut reader = Reader::new(body);
    let header = FacetHeader::read(&mut reader)?;
    let notification = reader.u32()?;
    reader.finish()?;
    Ok((header, notification))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CRC-32 of `enter_sanctuary`.
    const ENTER_SANCTUARY: u32 = 0x6831_69f8;
    /// CRC-32 of `place_camp`.
    const PLACE_CAMP: u32 = 0x0b21_32dc;
    /// CRC-32 of `charmslotrequestssucceeded`.
    const CHARM_SLOT_SUCCEEDED: u32 = 0x1e06_e356;
    /// CRC-32 of `perkname`.
    const PERK_NAME: u32 = 0xc793_6637;
    /// CRC-32 of `itemname`.
    const ITEM_NAME: u32 = 0xf45d_1cce;
    /// A synthetic header.
    const HEADER: FacetHeader = FacetHeader {
        target: 0x0102_0304_0506_0708,
        entity: 0x1112_1314_1516_1718,
    };

    #[test]
    fn the_header_is_the_game_events_child_plus_the_component() {
        let entity = crate::facet_entity(5);
        let header = notification_header(5);
        assert_eq!(header.entity, entity);
        assert_eq!(
            header.target,
            (entity ^ GAME_EVENTS_CHILD).wrapping_add(EVENT_NOTIFICATION_COMPONENT)
        );
    }

    #[test]
    fn a_sync_has_the_retail_layout() {
        let message = SyncNotificationsMsg {
            header: HEADER,
            counts: vec![
                NotificationCount {
                    notification: PLACE_CAMP,
                    count: 1,
                },
                NotificationCount {
                    notification: ENTER_SANCTUARY,
                    count: 36,
                },
            ],
            trailer: SYNC_TRAILER,
        };
        let body = message.encode().expect("encode");
        // Header, count 2, two (id, count) pairs, the trailer: the retail 34-byte form.
        assert_eq!(body.len(), 16 + 1 + 2 * COUNT_LEN + 1);
        assert_eq!(body[16], 2);
        assert_eq!(&body[17..21], PLACE_CAMP.to_be_bytes());
        assert_eq!(body.last(), Some(&SYNC_TRAILER));
        assert_eq!(SyncNotificationsMsg::decode(&body), Ok(message));
        assert!(SyncNotificationsMsg::decode(&body[..body.len() - 1]).is_err());
    }

    #[test]
    fn a_notification_with_replacements_has_the_retail_layout() {
        let message = SendNotificationWithReplacementsMsg {
            header: HEADER,
            notification: CHARM_SLOT_SUCCEEDED,
            replacements: vec![
                Replacement {
                    key: PERK_NAME,
                    text: b"@Gambit4Name".to_vec(),
                },
                Replacement {
                    key: ITEM_NAME,
                    text: b"@2hBowT52_MasterName".to_vec(),
                },
            ],
        };
        let body = message.encode().expect("encode");
        // The retail 63-byte body of these two texts.
        assert_eq!(body.len(), 63);
        assert_eq!(&body[20..26], [0x02, 0xc7, 0x93, 0x66, 0x37, 0x0c]);
        assert_eq!(SendNotificationWithReplacementsMsg::decode(&body), Ok(message));
        let mut long = body;
        long.push(0);
        assert!(SendNotificationWithReplacementsMsg::decode(&long).is_err());
    }

    #[test]
    fn the_single_id_calls_round_trip() {
        let send = SendNotificationMsg {
            header: HEADER,
            notification: PLACE_CAMP,
        };
        let body = send.encode().expect("encode");
        assert_eq!(body.len(), 20);
        assert_eq!(SendNotificationMsg::decode(&body), Ok(send));
        let sent = NotificationSentMsg {
            header: HEADER,
            notification: ENTER_SANCTUARY,
        };
        assert_eq!(
            NotificationSentMsg::decode(&sent.encode().expect("encode")),
            Ok(sent)
        );
        assert_eq!(SendNotificationMsg::to(5, PLACE_CAMP).header, notification_header(5));
    }
}
