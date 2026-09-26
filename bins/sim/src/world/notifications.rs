// SPDX-License-Identifier: AGPL-3.0-or-later
//! Event notification counts (conversation-notify note, section 2). Retail keeps per character
//! how often each notification was shown and sends the whole map (3236 `SyncNotifications`): in
//! the login burst, before the level info, when the character has any; after each notification
//! the server sends (3233, 3234), with its count one higher; and after each one the client
//! reports it showed itself (3239 `NotificationSent`, 67 of 67 answered in 18 to 90 ms). The
//! counts save with the progression (world migration 0022).

use nw_bus::ConnectionRef;
use nw_messages::{NotificationCount, ServerMessage, SyncNotificationsMsg};

use super::{Player, SimWorld, send_now};

/// How many times each notification was shown to a player, in the order it first was.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ShownNotifications {
    counts: Vec<NotificationCount>,
}

impl ShownNotifications {
    /// The counts a join or a handoff carried: (notification, times shown).
    pub(super) fn of(carried: &[(u32, u32)]) -> Self {
        Self {
            counts: carried
                .iter()
                .map(|&(notification, count)| NotificationCount {
                    notification,
                    count,
                })
                .collect(),
        }
    }

    /// Counts one more showing of `notification`: the whole map after it.
    pub(super) fn shown(&mut self, notification: u32) -> Vec<NotificationCount> {
        match self
            .counts
            .iter_mut()
            .find(|count| count.notification == notification)
        {
            Some(count) => count.count = count.count.saturating_add(1),
            None => self.counts.push(NotificationCount {
                notification,
                count: 1,
            }),
        }
        self.counts.clone()
    }

    /// The counts as a save and a handoff carry them, ascending by notification.
    pub(super) fn carried(&self) -> Vec<(u32, u32)> {
        let mut carried: Vec<(u32, u32)> = self
            .counts
            .iter()
            .map(|count| (count.notification, count.count))
            .collect();
        carried.sort_unstable();
        carried
    }

    /// The login map of player `number`: `None` when it was shown none (retail sends a new
    /// character no 3236).
    pub(super) fn login_sync(&self, number: u64) -> Option<ServerMessage> {
        (!self.counts.is_empty()).then(|| {
            ServerMessage::SyncNotifications(SyncNotificationsMsg::to(number, self.counts.clone()))
        })
    }
}

impl SimWorld {
    /// The client of `connection` showed `notification` itself (3239): one more showing, and the
    /// whole map back (3236).
    pub fn notification_sent(&mut self, connection: &ConnectionRef, notification: u32) {
        let Some(&entity) = self.by_connection.get(connection) else {
            return;
        };
        let Some(sync) = self
            .world
            .get_mut::<Player>(entity)
            .map(|mut player| player.notification_sync(notification))
        else {
            return;
        };
        send_now(&mut self.combat_messages, connection, &[sync]);
    }
}

impl Player {
    /// Counts one more showing of `notification`: the 3236 with the whole map.
    pub(super) fn notification_sync(&mut self, notification: u32) -> ServerMessage {
        let counts = self.notifications.shown(notification);
        ServerMessage::SyncNotifications(SyncNotificationsMsg::to(self.number, counts))
    }
}

#[cfg(test)]
mod tests {
    use nw_formats::objectstream::lowercase_crc32;

    use super::*;

    /// The retail CRC of `CharmSlotRequestsSucceeded` (notifications note).
    const RETAIL_SLOTTED: u32 = 0x1e06_e356;

    #[test]
    fn a_count_rises_per_showing_and_the_whole_map_goes() {
        let mut shown = ShownNotifications::default();
        assert_eq!(
            shown.shown(RETAIL_SLOTTED),
            [NotificationCount {
                notification: RETAIL_SLOTTED,
                count: 1,
            }]
        );
        shown.shown(lowercase_crc32("Enter_Sanctuary"));
        let counts = shown.shown(RETAIL_SLOTTED);
        assert_eq!(counts.len(), 2);
        assert_eq!(counts[0].count, 2);
        assert_eq!(counts[1].count, 1);
    }

    #[test]
    fn saved_counts_come_back_and_go_on_rising() {
        let sanctuary = lowercase_crc32("Enter_Sanctuary");
        let mut carried = vec![(sanctuary, 1), (RETAIL_SLOTTED, 11)];
        carried.sort_unstable();
        let mut shown = ShownNotifications::of(&carried);
        assert_eq!(shown.carried(), carried);
        shown.shown(RETAIL_SLOTTED);
        let mut expected = vec![(sanctuary, 1), (RETAIL_SLOTTED, 12)];
        expected.sort_unstable();
        assert_eq!(shown.carried(), expected);
        assert!(shown.login_sync(1).is_some());
        assert!(
            ShownNotifications::default().login_sync(1).is_none(),
            "a new character gets no map"
        );
    }
}
