// SPDX-License-Identifier: AGPL-3.0-or-later
//! The game event log of each player: the 497 entry (`GameEventComponentReplicatedState`,
//! [`nw_messages::game_events`]) that retail sends for each game event reward it grants, and the
//! shared reward of a game mode's end event ([`SimWorld::give_mode_event`]).
//!
//! Observed (retail ledgers, `gev` over every capture the flag, Outpost Rush, arena and catacombs
//! ledger and `MSQ5-lv25`):
//! - One entry for each granted event: kills (`KillEvent`, `CatacombsKillEvent_*`), gathers
//!   (`Gather_*`), salvages (`SalvageCoinGrant*`) and the end events of the game modes
//!   (`CaptureTheFlag_T*`, `OutpostRush_*`, `Arena3v3_*`, `CatacombsKillEvent_Boss_NN`).
//! - The list is a ring of [`MAX_GAME_EVENTS`]: the first entry after the arrival on a sim goes as
//!   the full list of that one entry (`CATACOMBS5` 124.5 s, `CTF01` 82.1 s), each later one as a
//!   change at the next index (1 to 9, then 0 again: `CATACOMBS5` 188.0 s index 9, 193.8 s
//!   index 0). The first state holds only the empty daily bonuses
//!   ([`nw_messages::player_components`]), so each sim starts an empty log.
//! - Entries of one tick go as one change list: the first with the version, the others with 0
//!   (`CATACOMBS5` 1674.6 s indexes 3 and 4, `catacombshigh` 1075.3 s indexes 9, 0, 1, 2, 3).
//! - The version is the version of the player's other lists of that tick (`arena` 159.741 s: the
//!   911 change and the 497 entry both 7965); ours is the player's list version.
//!
//! The fields of an entry ([`GrantedEvent::entry`]), Observed against the content rows: `value_a`
//! the coins (`SalvageCoinGrantT2` 25, kills `KillCurrency` x the creature type's `Currency`,
//! `CaptureTheFlag_T4` 25,000), `standing` the territory standing before the territory's
//! multiplier (kills 5, 15, 2), `value_f` the azoth, `value_g` the season experience, `value_h`
//! the umbral shards, the rewards the given items (item 0 word, `value_a` `u32::MAX`, `value_b`
//! 51 on every retail item), `territory` the territory of the player (0 in an instance),
//! `amount_a` 1.0 for a kill of one player (2 players in a group 0.7, 3 players 0.6: the
//! catacombs ledgers; this server gives a kill only to its killer) and 1.0 for the gathers,
//! salvages and mode events, `amount_b` 1.0. The flag (name Unknown) is true on the mode end
//! events that pay coins (11 of 11 capture the flag and Outpost Rush ends; false on the 2 arena
//! wins without coins) and false on kills, gathers and salvages (Observed on 94 % of 28,418
//! retail kills; its meaning on the others is Unknown).

use bevy_ecs::entity::Entity;
use nw_content::rewards::{EventGrant, Reward};
use nw_formats::objectstream::lowercase_crc32;
use nw_messages::{
    BundleMember, FullList, GAME_EVENT_COMPONENT, GameEventEntry, GameEventState, ListChange,
    MAX_GAME_EVENTS, MemberState, ReplicatedList, RewardItem,
};

use super::quest_objects::grant;
use super::seasons::{SEASON_PASS, SeasonEvent};
use super::status_effects::GainKind;
use super::tradeskill::{AZOTH_CURRENCY, ONE_FIRING};
use super::{Player, SimWorld, unix_ns};
use crate::objectives::Event;

/// The share of a kill or event of one player, and `amount_b` (Observed: 1.0).
pub(super) const WHOLE_SHARE: f32 = 1.0;
/// The territory of an entry outside every territory (Observed: 0 in every instance).
const NO_TERRITORY: u16 = 0;
/// The fixed fields of every retail reward item of an entry (Observed, names Unknown).
const ITEM_WORD: u16 = 0;
const ITEM_VALUE_A: u32 = u32::MAX;
const ITEM_VALUE_B: u32 = 51;
/// The categorical progressions of the PvP experience and the umbral shards (Observed: the 911
/// entries `PvP_XP` +1,350 and `UmbralCurrency` to its cap 100,000 at `CTF01` 1222.234 s).
pub(super) const PVP_XP: &str = "PvP_XP";
pub(super) const UMBRAL_CURRENCY: &str = "UmbralCurrency";

/// What one granted game event gave, for its entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct GrantedEvent {
    /// The event id.
    pub(super) event: String,
    /// `amount_a`: the player's share of it.
    pub(super) share: f32,
    /// The coins.
    pub(super) coins: u64,
    /// Whether it is a mode end event that pays coins (the entry's flag).
    pub(super) mode_coins: bool,
    /// The territory standing before the territory's multiplier.
    pub(super) standing: f64,
    /// The azoth.
    pub(super) azoth: f64,
    /// The season experience.
    pub(super) seasons_xp: f64,
    /// The umbral shards.
    pub(super) umbral: f64,
    /// The items given (item id, quantity).
    pub(super) items: Vec<(String, u32)>,
}

/// A whole amount of a reward for a `u32` field, rounded.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to the target range first"
)]
fn whole(raw: f64) -> u32 {
    raw.round().clamp(0.0, f64::from(u32::MAX)) as u32
}

impl GrantedEvent {
    /// The plain grant of `event`: a whole share and nothing else yet.
    pub(super) fn of(event: &str) -> Self {
        Self {
            event: event.to_owned(),
            share: WHOLE_SHARE,
            ..Self::default()
        }
    }

    /// Its entry at `time_ns` in territory `territory` (module note).
    pub(super) fn entry(&self, time_ns: u64, territory: u16) -> GameEventEntry {
        GameEventEntry {
            time_ns,
            event: lowercase_crc32(&self.event),
            amount_a: self.share,
            amount_b: WHOLE_SHARE,
            value_a: u32::try_from(self.coins).unwrap_or(u32::MAX),
            flag: self.mode_coins,
            standing: whole(self.standing),
            value_c: 0,
            value_d: 0,
            rewards: self
                .items
                .iter()
                .map(|(item, quantity)| RewardItem {
                    item: lowercase_crc32(item),
                    word: ITEM_WORD,
                    quantity: *quantity,
                    flag_a: false,
                    value_a: ITEM_VALUE_A,
                    value_b: ITEM_VALUE_B,
                    flag_b: false,
                    flag_c: false,
                    list_a: Vec::new(),
                    list_b: Vec::new(),
                    flag_d: false,
                    list_c: Vec::new(),
                })
                .collect(),
            byte_a: 0,
            byte_b: 0,
            value_f: whole(self.azoth),
            territory,
            value_g: whole(self.seasons_xp),
            value_h: whole(self.umbral),
        }
    }
}

/// The ring of a player's last entries on this sim and what its client has not had yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct GameEventLog {
    entries: Vec<GameEventEntry>,
    /// The index the next entry takes.
    next: usize,
    /// Whether the client had the list once (its full form).
    sent: bool,
    /// The indexes changed since the last member, in order.
    changed: Vec<usize>,
}

impl GameEventLog {
    /// Puts `entry` at the ring's next index.
    pub(super) fn record(&mut self, entry: GameEventEntry) {
        let index = self.next;
        match self.entries.get_mut(index) {
            Some(slot) => *slot = entry,
            None => self.entries.push(entry),
        }
        self.next = (index + 1) % MAX_GAME_EVENTS;
        if !self.changed.contains(&index) {
            self.changed.push(index);
        }
    }

    /// Whether entries wait for the client.
    pub(super) fn has_change(&self) -> bool {
        !self.changed.is_empty()
    }

    /// The member of the waiting entries at list version `version`: the full list the first
    /// time, the changed indexes after (module note); `None` when nothing waits.
    pub(super) fn take_member(&mut self, version: u64) -> Option<BundleMember> {
        let changed = std::mem::take(&mut self.changed);
        if changed.is_empty() {
            return None;
        }
        let events = if std::mem::replace(&mut self.sent, true) {
            ReplicatedList::Changes(
                changed
                    .into_iter()
                    .enumerate()
                    .filter_map(|(position, index)| {
                        Some(ListChange {
                            index: u32::try_from(index).ok()?,
                            version: if position == 0 { version } else { 0 },
                            value: self.entries.get(index)?.clone(),
                        })
                    })
                    .collect(),
            )
        } else {
            ReplicatedList::Full(FullList {
                version: Some(version),
                elements: self.entries.clone(),
            })
        };
        Some(BundleMember {
            component: GAME_EVENT_COMPONENT,
            state: MemberState::GameEvents(GameEventState {
                game_events: Some(events),
                daily_bonuses_used: None,
            }),
        })
    }
}

impl SimWorld {
    /// Logs `granted` for player `number` at the server time, in the territory it is in.
    pub(super) fn log_game_event(&mut self, number: u64, granted: &GrantedEvent) {
        let territory = self
            .current_territory(number)
            .and_then(|id| id.parse().ok())
            .unwrap_or(NO_TERRITORY);
        let time_ns = unix_ns(self.server_ms);
        let Some(entity) = self.player_by_number(number) else {
            return;
        };
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            player.game_events.record(granted.entry(time_ns, territory));
        }
    }

    /// Player `entity` (`number`) gets the end event `event` of a game mode
    /// ([`nw_content::game_mode_rewards`]): its coins and character experience, its `PvP_XP`,
    /// faction reputation and tokens (each under the player's held effects), azoth, umbral
    /// shards and season experience (each to its cap), its loot table's items, its entry in the
    /// log, and the event counts for the player's quests and season stats. An event the content
    /// does not hold gives nothing (logged).
    pub(super) fn give_mode_event(&mut self, entity: Entity, number: u64, event: &str) {
        let content = std::sync::Arc::clone(&self.content);
        let Some(reward) = content.mode_rewards.get(event) else {
            tracing::warn!(player = number, event, "end event not in the content");
            return;
        };
        if let Some(mut player) = self.world.get_mut::<Player>(entity) {
            let wallet = Reward {
                coins: reward.coins,
                experience: reward.experience,
                ..Reward::default()
            };
            grant(&mut player, &wallet, &content.gameplay);
        }
        let gains = [
            (PVP_XP, GainKind::Progression, reward.pvp_xp),
            (AZOTH_CURRENCY, GainKind::Azoth, reward.azoth),
            (UMBRAL_CURRENCY, GainKind::Progression, reward.umbral),
            (SEASON_PASS, GainKind::Progression, reward.seasons_xp),
        ];
        for (id, kind, points) in gains.into_iter().filter(|(_, _, points)| *points > 0.0) {
            self.gain_categorical(number, event, id, kind, points, ONE_FIRING);
        }
        let faction = EventGrant {
            faction_reputation: reward.faction_reputation,
            faction_tokens: reward.faction_tokens,
            ..EventGrant::default()
        };
        self.gain_faction(number, event, &faction, ONE_FIRING);
        let items = reward
            .item
            .clone()
            .map(|item| self.give_loot_listed(number, vec![item], self.server_ms))
            .unwrap_or_default();
        let granted = GrantedEvent {
            coins: reward.coins,
            mode_coins: reward.coins > 0,
            azoth: reward.azoth,
            seasons_xp: reward.seasons_xp,
            umbral: reward.umbral,
            items,
            ..GrantedEvent::of(event)
        };
        self.log_game_event(number, &granted);
        self.quest_event(number, Event::GameEvent(event), self.server_ms);
        self.season_event(entity, SeasonEvent::GameEvent { id: event });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic event time: ns since 1970-01-01.
    const TIME_NS: u64 = 1_790_409_530_342_236_500;
    const FODDER: &str = "CatacombsKillEvent_Fodder";
    const SOULS: &str = "Catacombs_Currency_Souls";
    /// The territory of a retail `MSQ5-lv25` kill.
    const TERRITORY: u16 = 12;

    fn fodder() -> GameEventEntry {
        GrantedEvent {
            items: vec![(SOULS.to_owned(), 4)],
            ..GrantedEvent::of(FODDER)
        }
        .entry(TIME_NS, NO_TERRITORY)
    }

    #[test]
    fn an_entry_carries_the_grant_as_retail() {
        // Retail `CATACOMBS5` 167.2 s: a fodder kill, souls x4 with the fixed item fields.
        let entry = fodder();
        assert_eq!(entry.event, lowercase_crc32(FODDER));
        assert_eq!(entry.rewards.len(), 1);
        let item = &entry.rewards[0];
        assert_eq!(
            (item.quantity, item.word, item.value_a, item.value_b),
            (4, 0, u32::MAX, 51)
        );
        assert_eq!((entry.value_a, entry.value_f, entry.value_h), (0, 0, 0));
        // Retail `CTF01` 1222.234 s: `CaptureTheFlag_T4`.
        let win = GrantedEvent {
            coins: 25_000,
            mode_coins: true,
            azoth: 125.0,
            seasons_xp: 15.0,
            umbral: 150.0,
            ..GrantedEvent::of("CaptureTheFlag_T4")
        }
        .entry(TIME_NS, NO_TERRITORY);
        assert_eq!(
            (win.value_a, win.flag, win.value_f, win.value_g, win.value_h),
            (25_000, true, 125, 15, 150)
        );
        // Retail `MSQ5-lv25`: a `Solo+` kill, 540 coins, standing 15, azoth 10, territory 12.
        let kill = GrantedEvent {
            coins: 540,
            standing: 15.0,
            azoth: 10.0,
            ..GrantedEvent::of("KillEvent")
        }
        .entry(TIME_NS, TERRITORY);
        assert_eq!(
            (kill.value_a, kill.flag, kill.standing, kill.value_f, kill.territory),
            (540, false, 15, 10, TERRITORY)
        );
        assert!((kill.amount_a - WHOLE_SHARE).abs() < f32::EPSILON);
    }

    #[test]
    fn the_log_sends_the_full_list_first_then_the_ring_indexes() {
        let mut log = GameEventLog::default();
        assert!(log.take_member(1).is_none(), "nothing waits");
        log.record(fodder());
        let full = log.take_member(1267).expect("the first entry");
        let MemberState::GameEvents(state) = &full.state else {
            panic!("a game event member");
        };
        assert_eq!(full.component, GAME_EVENT_COMPONENT);
        assert_eq!(
            state.game_events,
            Some(ReplicatedList::Full(FullList {
                version: Some(1267),
                elements: vec![fodder()],
            }))
        );
        // Retail `CATACOMBS5`: indexes 1 to 9 one by one, then 0 again.
        let mut indexes = Vec::new();
        for version in 0..MAX_GAME_EVENTS as u64 {
            log.record(fodder());
            let member = log.take_member(version + 2).expect("an entry");
            let MemberState::GameEvents(GameEventState {
                game_events: Some(ReplicatedList::Changes(changes)),
                ..
            }) = member.state
            else {
                panic!("changes after the first entry");
            };
            assert_eq!(changes.len(), 1);
            assert_eq!(changes[0].version, version + 2);
            indexes.push(changes[0].index);
        }
        assert_eq!(indexes, [1, 2, 3, 4, 5, 6, 7, 8, 9, 0]);
        // Two in one tick: one list, the second with version 0 (`CATACOMBS5` 1674.6 s).
        log.record(fodder());
        log.record(fodder());
        assert!(log.has_change());
        let member = log.take_member(47_514).expect("two entries");
        let MemberState::GameEvents(GameEventState {
            game_events: Some(ReplicatedList::Changes(changes)),
            ..
        }) = member.state
        else {
            panic!("changes");
        };
        let sent: Vec<(u32, u64)> = changes
            .iter()
            .map(|change| (change.index, change.version))
            .collect();
        assert_eq!(sent, [(1, 47_514), (2, 0)]);
        assert!(!log.has_change());
    }
}
