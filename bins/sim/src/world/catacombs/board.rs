// SPDX-License-Identifier: AGPL-3.0-or-later
//! The war board of a catacombs run: the manifests 3530 and 2968 and the stats 839
//! ([`nw_messages::warboard_stats`]).
//!
//! Observed (retail `CATACOMBS`, `CATACOMBS3`, `CATACOMBS5`; every push carries the header
//! `d56a5012fd7adbdbce27d68aba595a83`):
//!
//! - At the run's start every participant gets the full manifest (sync 2 with two players) and
//!   an 839 whose own part is empty and that gives every participant all twelve catacombs slots
//!   at 0 (`CATACOMBS5` 80.949 s, 65 ms before the run's values).
//! - Slots 44 to 55 (`CatacombsWarboardStatDefinitions`, in its row order): `BossesKilled`,
//!   `NamedEnemiesKilled`, `ChestsOpened`, `Extracted`, `SilversExtracted`, `CrownsExtracted`,
//!   `ChestsOpenedSilvers`, `ChestsOpenedCrowns`, `BossesSilvers`, `BossesCrowns`,
//!   `NamedEnemiesSilvers`, `NamedEnemiesCrowns`.
//! - In the run a chest, a named room or a boss raises its count slot for every participant;
//!   the next push on a half-second boundary names only the changed slots, for the receiver
//!   and for every changed participant from the highest index (`CATACOMBS5` 157.113 s pool,
//!   157.558 s board; every push time ends in .05 or .55 s).
//! - A late joiner raises the sync by one: the others get the 2968 of the joiner and an 839
//!   naming it with no slot (`CATACOMBS5` 902.057 s, 902.557 s). Its counts start at 0
//!   (984.049 s: 1 chest against the others' 3).
//! - At the run's end every participant's `Extracted` (the extracted players), the paid
//!   silvers and crowns and the run's chest, boss and named silvers and crowns change; slots
//!   still 0 stay out (`CATACOMBS5` 1675.555 s: 3, 741, 2, 132, 300, 1, 186; `CATACOMBS`
//!   948.904 s: 44, 100, 124).
//! - A participant who leaves is named once with the left bit (`CATACOMBS5` 1687.557 s).
//!
//! Inferred: a late joiner itself gets the full manifest and every participant's slots (no
//! capture shows a joiner's own view); the sync counts the participants ever joined.

use nw_messages::FacetHeader;
use nw_messages::social::CharacterRef;
use nw_messages::warboard_stats::{ParticipantStats, TeamManifest, WarboardStats};
use nw_messages::{RAW_ID_LEN, ServerMessage, WarServerMsg};

use super::super::SimWorld;

/// The header of every catacombs board push (Observed, module text).
const BOARD_HEADER: FacetHeader = FacetHeader {
    target: 0xd56a_5012_fd7a_dbdb,
    entity: 0xce27_d68a_ba59_5a83,
};
/// The first catacombs slot and the count of them.
const FIRST_SLOT: u8 = 44;
const STATS: usize = 12;
/// The catacombs slots (module text).
pub(super) const BOSSES_KILLED: u8 = 44;
pub(super) const NAMED_ENEMIES_KILLED: u8 = 45;
pub(super) const CHESTS_OPENED: u8 = 46;
pub(super) const EXTRACTED: u8 = 47;
pub(super) const SILVERS_EXTRACTED: u8 = 48;
pub(super) const CROWNS_EXTRACTED: u8 = 49;
pub(super) const CHESTS_OPENED_SILVERS: u8 = 50;
pub(super) const CHESTS_OPENED_CROWNS: u8 = 51;
pub(super) const BOSSES_SILVERS: u8 = 52;
pub(super) const BOSSES_CROWNS: u8 = 53;
pub(super) const NAMED_ENEMIES_SILVERS: u8 = 54;
pub(super) const NAMED_ENEMIES_CROWNS: u8 = 55;
/// The board pushes on these boundaries of the server clock, ms.
const FLUSH_MS: u64 = 500;

/// One participant on the board, by its index (its place in the list).
#[derive(Debug, Clone, PartialEq, Eq)]
struct BoardMember {
    number: u64,
    character: [u8; RAW_ID_LEN],
    /// Its slots now and as last pushed.
    values: [u32; STATS],
    sent: [u32; STATS],
    /// It left, and that was pushed.
    left: bool,
    left_sent: bool,
}

/// The board of one run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RunBoard {
    members: Vec<BoardMember>,
    /// The manifest's sync.
    sync: u16,
    /// The run started and the board went out.
    started: bool,
    /// When the next push is due, server ms.
    flush_ms: Option<u64>,
}

/// The pushes for one player: its number and the messages.
pub(super) type BoardPushes = Vec<(u64, Vec<WarServerMsg>)>;

/// The slot `slot`'s place in a member's values.
fn place(slot: u8) -> usize {
    usize::from(slot - FIRST_SLOT)
}

/// Every slot of `values`, zeros included.
fn all_slots(values: &[u32; STATS]) -> ParticipantStats {
    ParticipantStats {
        left: false,
        slots: (FIRST_SLOT..).zip(values.iter().copied()).collect(),
    }
}

/// The 839 of `stats`.
fn stats_push(stats: &WarboardStats) -> Option<WarServerMsg> {
    match stats.encode_payload() {
        Ok(payload) => Some(WarServerMsg::UpdateWarboardStats {
            header: BOARD_HEADER,
            payload,
        }),
        Err(error) => {
            tracing::warn!(%error, "catacombs board stats not sent");
            None
        }
    }
}

/// The 3530 (`full`) or 2968 of `manifest`.
fn manifest_push(manifest: &TeamManifest, full: bool) -> Option<WarServerMsg> {
    match manifest.encode_payload() {
        Ok(payload) if full => Some(WarServerMsg::UpdateFullWarboardManifest {
            header: BOARD_HEADER,
            payload,
        }),
        Ok(payload) => Some(WarServerMsg::UpdateWarboardManifest {
            header: BOARD_HEADER,
            payload,
        }),
        Err(error) => {
            tracing::warn!(%error, "catacombs board manifest not sent");
            None
        }
    }
}

/// `index` as a board index (a run holds three players).
fn board_index(index: usize) -> u8 {
    u8::try_from(index).unwrap_or(u8::MAX)
}

impl RunBoard {
    /// The index and member of player `number`.
    fn member(&self, number: u64) -> Option<usize> {
        self.members.iter().position(|member| member.number == number)
    }

    /// The members still on the board, with their index.
    fn active(&self) -> impl DoubleEndedIterator<Item = (usize, &BoardMember)> {
        self.members
            .iter()
            .enumerate()
            .filter(|(_, member)| !member.left)
    }

    /// The full manifest.
    fn manifest(&self) -> TeamManifest {
        TeamManifest {
            sync: self.sync,
            participants: self
                .members
                .iter()
                .enumerate()
                .map(|(index, member)| (CharacterRef(Some(member.character)), board_index(index)))
                .collect(),
        }
    }

    /// The 839 that gives `receiver` every active participant's slots, its own part empty.
    fn full_stats(&self, receiver: usize) -> WarboardStats {
        WarboardStats {
            sync: self.sync,
            local_index: board_index(receiver),
            local: ParticipantStats::default(),
            participants: self
                .active()
                .rev()
                .map(|(index, member)| (board_index(index), all_slots(&member.values)))
                .collect(),
        }
    }

    /// The run started with `participants` (number, character): the board and its first
    /// pushes to each of them.
    pub(super) fn start(&mut self, participants: &[(u64, [u8; RAW_ID_LEN])]) -> BoardPushes {
        self.members = participants
            .iter()
            .map(|(number, character)| BoardMember {
                number: *number,
                character: *character,
                values: [0; STATS],
                sent: [0; STATS],
                left: false,
                left_sent: false,
            })
            .collect();
        self.sync = u16::try_from(self.members.len()).unwrap_or(u16::MAX);
        self.started = true;
        let manifest = manifest_push(&self.manifest(), true);
        self.active()
            .map(|(index, member)| {
                let pushes = manifest
                    .clone()
                    .into_iter()
                    .chain(stats_push(&self.full_stats(index)))
                    .collect();
                (member.number, pushes)
            })
            .collect()
    }

    /// Whether the run's board went out.
    pub(super) const fn started(&self) -> bool {
        self.started
    }

    /// Player `number` with `character` joined the started run: its index goes to the others,
    /// and it gets the whole board (module text). Nothing before the start or for a member.
    pub(super) fn join(&mut self, number: u64, character: [u8; RAW_ID_LEN]) -> BoardPushes {
        if !self.started || self.member(number).is_some() {
            return Vec::new();
        }
        let index = self.members.len();
        self.members.push(BoardMember {
            number,
            character,
            values: [0; STATS],
            sent: [0; STATS],
            left: false,
            left_sent: false,
        });
        self.sync = self.sync.saturating_add(1);
        let joined = manifest_push(
            &TeamManifest {
                sync: self.sync,
                participants: vec![(CharacterRef(Some(character)), board_index(index))],
            },
            false,
        );
        let mut pushes: BoardPushes = self
            .active()
            .filter(|(other, _)| *other != index)
            .map(|(other, member)| {
                let named = WarboardStats {
                    sync: self.sync,
                    local_index: board_index(other),
                    local: ParticipantStats::default(),
                    participants: vec![(board_index(index), ParticipantStats::default())],
                };
                let messages = joined.clone().into_iter().chain(stats_push(&named)).collect();
                (member.number, messages)
            })
            .collect();
        let own = manifest_push(&self.manifest(), true)
            .into_iter()
            .chain(stats_push(&self.full_stats(index)))
            .collect();
        pushes.push((number, own));
        pushes
    }

    /// Player `number` left the board.
    pub(super) fn leave(&mut self, number: u64, server_ms: u64) {
        if let Some(index) = self.member(number) {
            self.members[index].left = true;
            self.due(server_ms);
        }
    }

    /// Every participant's `slot` rises by one.
    pub(super) fn count(&mut self, slot: u8, server_ms: u64) {
        for member in self.members.iter_mut().filter(|member| !member.left) {
            member.values[place(slot)] = member.values[place(slot)].saturating_add(1);
        }
        self.due(server_ms);
    }

    /// Every participant's slots `values` (slot, value) are set.
    pub(super) fn set(&mut self, values: &[(u8, u32)], server_ms: u64) {
        for member in self.members.iter_mut().filter(|member| !member.left) {
            for (slot, value) in values {
                member.values[place(*slot)] = *value;
            }
        }
        self.due(server_ms);
    }

    /// The next push is due on the next boundary after `server_ms`.
    fn due(&mut self, server_ms: u64) {
        if self.started && self.flush_ms.is_none() {
            self.flush_ms = Some((server_ms / FLUSH_MS + 1) * FLUSH_MS);
        }
    }

    /// The pushes due at `server_ms`: per active participant, its changed slots and every
    /// changed participant's (module text).
    pub(super) fn flush(&mut self, server_ms: u64) -> Vec<(u64, WarServerMsg)> {
        if self.flush_ms.is_none_or(|due| server_ms < due) {
            return Vec::new();
        }
        self.flush_ms = None;
        let changed: Vec<(usize, ParticipantStats)> = self
            .members
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(index, member)| {
                let slots: std::collections::BTreeMap<u8, u32> = (FIRST_SLOT..)
                    .zip(member.values.iter().zip(&member.sent))
                    .filter(|(_, (now, sent))| now != sent)
                    .map(|(slot, (now, _))| (slot, *now))
                    .collect();
                let left = member.left && !member.left_sent;
                (!slots.is_empty() || left).then_some((index, ParticipantStats { left, slots }))
            })
            .collect();
        for (index, _) in &changed {
            let member = &mut self.members[*index];
            member.sent = member.values;
            member.left_sent |= member.left;
        }
        if changed.is_empty() {
            return Vec::new();
        }
        let participants: Vec<(u8, ParticipantStats)> = changed
            .iter()
            .map(|(index, stats)| (board_index(*index), stats.clone()))
            .collect();
        self.active()
            .filter_map(|(index, member)| {
                let local = changed
                    .iter()
                    .find(|(changed, _)| *changed == index)
                    .map(|(_, stats)| stats.clone())
                    .unwrap_or_default();
                let stats = WarboardStats {
                    sync: self.sync,
                    local_index: board_index(index),
                    local,
                    participants: participants.clone(),
                };
                Some((member.number, stats_push(&stats)?))
            })
            .collect()
    }
}

impl SimWorld {
    /// Sends each participant of this sim's catacombs run its board pushes `pushes`.
    pub(super) fn send_catacomb_board(&mut self, pushes: BoardPushes) {
        for (number, messages) in pushes {
            let Some(entity) = self.catacomb_run().and_then(|run| {
                run.progress
                    .participants
                    .iter()
                    .find(|participant| participant.number == number)
                    .map(|participant| participant.entity)
            }) else {
                continue;
            };
            let messages: Vec<ServerMessage> = messages.into_iter().map(ServerMessage::War).collect();
            self.send_to(entity, &messages);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START_MS: u64 = 80_949;

    fn character(byte: u8) -> [u8; RAW_ID_LEN] {
        [byte; RAW_ID_LEN]
    }

    /// The body of a push, as the client gets it.
    fn body(message: &WarServerMsg) -> String {
        hex::encode(message.encode())
    }

    /// The stats of a push.
    fn stats_of(message: &WarServerMsg) -> WarboardStats {
        let WarServerMsg::UpdateWarboardStats { payload, .. } = message else {
            panic!("not an 839: {message:?}");
        };
        WarboardStats::decode_payload(payload).expect("stats")
    }

    /// Retail `CATACOMBS5` 80.949 s: two players get the manifest and every slot at 0, byte for
    /// byte (the receiver at index 1).
    #[test]
    fn the_start_pushes_the_manifest_and_every_slot_at_zero_as_retail() {
        let mut board = RunBoard::default();
        let pushes = board.start(&[(7, character(1)), (8, character(2))]);
        assert_eq!(pushes.len(), 2);
        let (number, messages) = &pushes[1];
        assert_eq!(*number, 8);
        assert_eq!(messages.len(), 2);
        assert!(body(&messages[0]).starts_with("d56a5012fd7adbdbce27d68aba595a8300020002"));
        assert_eq!(
            body(&messages[1]),
            "d56a5012fd7adbdbce27d68aba595a8300020100000201ff0000000000e0ff0100000000000000000000000000ff0000000000e0ff01000000000000000000000000"
        );
    }

    /// Retail `CATACOMBS5` 157.113 s and 157.558 s: a chest raises every participant's count;
    /// the push waits for the next half second and names the changed slot only.
    #[test]
    fn a_count_goes_out_on_the_next_half_second_with_the_changed_slot_only() {
        let mut board = RunBoard::default();
        board.start(&[(7, character(1)), (8, character(2))]);
        board.count(CHESTS_OPENED, 157_113);
        assert!(board.flush(157_400).is_empty(), "not yet");
        let pushes = board.flush(157_500);
        assert_eq!(pushes.len(), 2);
        assert_eq!(
            body(&pushes[1].1),
            "d56a5012fd7adbdbce27d68aba595a83000201fc00000000004001000201fc0000000000400100fc00000000004001"
        );
        assert!(board.flush(158_000).is_empty(), "nothing new");
    }

    /// Retail `CATACOMBS5` 902.057 s to 984.049 s: a late joiner raises the sync, the others
    /// get its 2968 and an 839 naming it; its count starts at 0.
    #[test]
    fn a_late_joiner_raises_the_sync_and_counts_from_zero() {
        let mut board = RunBoard::default();
        board.start(&[(7, character(1)), (8, character(2))]);
        board.count(CHESTS_OPENED, START_MS);
        board.count(CHESTS_OPENED, START_MS);
        board.flush(START_MS + FLUSH_MS);
        let pushes = board.join(9, character(3));
        assert_eq!(pushes.len(), 3);
        let to_other = &pushes[1].1;
        assert_eq!(
            body(&to_other[0]),
            format!(
                "d56a5012fd7adbdbce27d68aba595a830003000105{}02000000000000",
                hex::encode(character(3))
            )
        );
        assert_eq!(
            body(&to_other[1]),
            "d56a5012fd7adbdbce27d68aba595a830003010000010200"
        );
        let own = stats_of(&pushes[2].1[1]);
        assert_eq!(own.local_index, 2);
        assert_eq!(own.participants.len(), 3);
        assert_eq!(own.participants[1].1.slots[&CHESTS_OPENED], 2);
        board.count(CHESTS_OPENED, 983_910);
        let pushes = board.flush(984_000);
        let seen = stats_of(&pushes[1].1);
        assert_eq!(seen.local.slots[&CHESTS_OPENED], 3);
        assert_eq!(seen.participants[0], (2, all_one(CHESTS_OPENED)));
        assert!(board.join(9, character(3)).is_empty(), "once");
    }

    fn all_one(slot: u8) -> ParticipantStats {
        ParticipantStats {
            left: false,
            slots: [(slot, 1)].into_iter().collect(),
        }
    }

    /// Retail `CATACOMBS5` 1675.555 s and 1687.557 s: the end's slots go out (zeros stay out);
    /// a leaver is named once with the left bit and gets nothing more.
    #[test]
    fn the_end_sets_the_totals_and_a_leaver_is_named_once() {
        let mut board = RunBoard::default();
        board.start(&[(7, character(1)), (8, character(2))]);
        board.set(
            &[
                (EXTRACTED, 2),
                (SILVERS_EXTRACTED, 741),
                (CHESTS_OPENED_CROWNS, 0),
            ],
            1_674_550,
        );
        let pushes = board.flush(1_675_000);
        let end = stats_of(&pushes[0].1);
        assert_eq!(
            end.local.slots,
            [(EXTRACTED, 2), (SILVERS_EXTRACTED, 741)].into_iter().collect()
        );
        board.leave(7, 1_687_101);
        let pushes = board.flush(1_687_500);
        assert_eq!(pushes.len(), 1, "the leaver gets nothing");
        assert_eq!(pushes[0].0, 8);
        assert_eq!(
            body(&pushes[0].1),
            "d56a5012fd7adbdbce27d68aba595a830002010000010001"
        );
        board.count(CHESTS_OPENED, 1_688_000);
        let pushes = board.flush(1_688_500);
        assert_eq!(stats_of(&pushes[0].1).participants.len(), 1, "the leaver is out");
    }
}
