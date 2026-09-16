// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a player's client and this server say to each other after a level loads, in the retail
//! order (tutorial-intro note, section 4).
//!
//! - About [`LEVEL_INFO_AGAIN_MS`] after a `LevelInfoChangedMsg`, the server sends it again; the
//!   client acknowledges the level (5181), and the server sends the phases of its sectors (6310).
//! - Every player starts with the expansion and the achievement of its archetype pick. In the
//!   tutorial, the quest's task tree sets the achievements that unlock the client's videos and
//!   cutscenes ([`crate::objectives`]). The server allows each cutscene the client asks for
//!   (6703 → 6707) and closes it when it ends (6705 → 6709).
//! - When the client asks to leave the tutorial (502), the server shows the loading screen (3951)
//!   and clears the phases (an empty 6310) before the level change.

use std::collections::HashMap;

use nw_content::achievements::AchievementIndexes;
use nw_messages::facet::component;
use nw_messages::{
    AchievementBits, AchievementState, ByteChange, ClientFacetMsg, CutsceneEndFinalizedMsg,
    CutsceneGrantedMsg, FacetHeader, FullList, LevelInfoChangedMsg, PhasingEntry, ServerMessage,
    TeleportWithLoadingScreenMsg, UpdatePhasingMsg,
};

/// Time from a `LevelInfoChangedMsg` to the same message again (5.0 s in the retail tutorial and
/// open world).
pub(super) const LEVEL_INFO_AGAIN_MS: u64 = 5000;
/// The 6310 byte of unknown meaning, by level (retail values).
const TUTORIAL_PHASING_MODE: u8 = 3;
const OPEN_WORLD_PHASING_MODE: u8 = 2;
const EMPTY_PHASING_MODE: u8 = 0;
/// Bytes of the first achievement bitset that build 25810808 retail servers send (live capture of
/// 2026-09-12). A bit beyond it grows the bitset (see [`Achievements::set`]).
const BITSET_BYTES: usize = 1910;
/// The version of every entry of a change after the first (retail: one version per change).
const LATER_ENTRY_VERSION: u64 = 0;
/// The achievement of the Rise of the Angry Earth expansion; every account of this server has it
/// (retail sets it 5 s after login).
const EXPANSION_UNLOCK: &str = "Expansion_2023_Unlock";
/// The version of the first full achievement state (retail value).
const FIRST_ACHIEVEMENT_VERSION: u64 = 3;

/// The achievement bits a player starts with, and the bit of every achievement id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AchievementRules {
    indexes: AchievementIndexes,
    expansion: Option<u32>,
    /// The bit of each archetype pick, by backstory id.
    archetypes: HashMap<String, u32>,
}

impl AchievementRules {
    /// The bit of achievement `id`.
    pub(super) fn bit(&self, id: &str) -> Option<u32> {
        self.indexes.index(id)
    }

    /// The bit of the expansion every player has from the start.
    pub(super) const fn expansion(&self) -> Option<u32> {
        self.expansion
    }

    /// The bits of `indexes`, with the achievement of each archetype pick in `archetype_unlocks`
    /// (backstory id, achievement id); an achievement the content does not have is left out.
    pub fn of(indexes: &AchievementIndexes, archetype_unlocks: &[(String, String)]) -> Self {
        Self {
            expansion: indexes.index(EXPANSION_UNLOCK),
            archetypes: archetype_unlocks
                .iter()
                .filter_map(|(backstory, achievement)| {
                    Some((backstory.clone(), indexes.index(achievement)?))
                })
                .collect(),
            indexes: indexes.clone(),
        }
    }
}

/// A player's achievements and the bytes changed since the last sent change.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Achievements {
    bits: Vec<u8>,
    version: u64,
    /// Changed bytes not sent yet (byte index, new value), in the order they first changed.
    changes: Vec<(u32, u8)>,
}

impl Achievements {
    /// Sets `bit` and records the changed bytes. A bit beyond the bitset first grows it with
    /// zero bytes up to the bit's byte, each a changed byte (retail: the door bit 15317 came as
    /// bytes 1911 to 1914 of a 1911-byte bitset, the first three 0). A set bit changes nothing.
    fn set(&mut self, bit: u32) {
        let byte = bit / u8::BITS;
        let first_new = self.bits.len();
        // A byte index fits `usize`.
        let end = byte as usize + 1;
        if end > first_new {
            self.bits.resize(end, 0);
            for new in first_new..end - 1 {
                // The index fits `u32`: it is below `byte`.
                self.note(new as u32, 0);
            }
        }
        if let Some(value) = set_bit(&mut self.bits, bit) {
            self.note(byte, value);
        }
    }

    /// Clears `bit` and records the changed byte; a clear bit or one beyond the bitset changes
    /// nothing.
    fn clear(&mut self, bit: u32) {
        if let Some(value) = clear_bit(&mut self.bits, bit) {
            self.note(bit / u8::BITS, value);
        }
    }

    /// Notes the new `value` of byte `byte`.
    fn note(&mut self, byte: u32, value: u8) {
        match self
            .changes
            .iter_mut()
            .find(|(changed, _)| *changed == byte)
        {
            Some(change) => change.1 = value,
            None => self.changes.push((byte, value)),
        }
    }

    /// The changed bytes as one change: the next version on its first entry, 0 on the others
    /// (retail 142262, 175193, 196810); `None` without changes.
    fn take_changes(&mut self) -> Option<Vec<ByteChange>> {
        if self.changes.is_empty() {
            return None;
        }
        self.version += 1;
        let version = self.version;
        Some(
            std::mem::take(&mut self.changes)
                .into_iter()
                .enumerate()
                .map(|(index, (byte, value))| ByteChange {
                    byte,
                    version: if index == 0 {
                        version
                    } else {
                        LATER_ENTRY_VERSION
                    },
                    value,
                })
                .collect(),
        )
    }
}

/// Clears `bit` of `bits`; the new byte, or `None` when nothing changed.
fn clear_bit(bits: &mut [u8], bit: u32) -> Option<u8> {
    let value = bits.get_mut((bit / u8::BITS) as usize)?;
    let new = *value & !(1 << (bit % u8::BITS));
    (new != *value).then(|| {
        *value = new;
        new
    })
}

/// Sets `bit` of `bits`; the new byte, or `None` when nothing changed.
fn set_bit(bits: &mut [u8], bit: u32) -> Option<u8> {
    let value = bits.get_mut((bit / u8::BITS) as usize)?;
    let new = *value | 1 << (bit % u8::BITS);
    (new != *value).then(|| {
        *value = new;
        new
    })
}

/// The flow of one player in its current level.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LevelFlow {
    /// The entity number, for the facet headers.
    entity: u64,
    /// The last `LevelInfoChangedMsg`; `None` for a player resumed without one.
    level_info: Option<LevelInfoChangedMsg>,
    /// When to send it again.
    level_info_again_at_ms: Option<u64>,
    tutorial: bool,
    /// The sequence of the last 6310.
    phasing_sequence: u8,
    /// The cutscene the client was allowed to play.
    cutscene: Option<u32>,
    /// `None` for a player taken without a level change (its client has them).
    achievements: Option<Achievements>,
}

impl LevelFlow {
    /// The flow after `level_info` went to the client of entity `entity` at `now_ms`, after
    /// `phasing_sequence` updates. Every player starts with the expansion and the achievement of
    /// its archetype pick (`backstory`).
    pub(super) fn after_level_info(
        entity: u64,
        level_info: LevelInfoChangedMsg,
        now_ms: u64,
        phasing_sequence: u8,
        rules: &AchievementRules,
        tutorial: bool,
        backstory: Option<&str>,
    ) -> Self {
        let mut bits = vec![0; BITSET_BYTES];
        let archetype = backstory.and_then(|backstory| rules.archetypes.get(backstory).copied());
        for bit in [rules.expansion, archetype].into_iter().flatten() {
            set_bit(&mut bits, bit);
        }
        Self {
            entity,
            level_info: Some(level_info),
            level_info_again_at_ms: Some(now_ms.saturating_add(LEVEL_INFO_AGAIN_MS)),
            tutorial,
            phasing_sequence,
            cutscene: None,
            achievements: Some(Achievements {
                bits,
                version: FIRST_ACHIEVEMENT_VERSION,
                changes: Vec::new(),
            }),
        }
    }

    /// The flow of a player that a sim took without a level change: nothing is pending.
    pub(super) const fn resumed(entity: u64, phasing_sequence: u8) -> Self {
        Self {
            entity,
            level_info: None,
            level_info_again_at_ms: None,
            tutorial: false,
            phasing_sequence,
            cutscene: None,
            achievements: None,
        }
    }

    /// The phase id of the level's private instance (the tutorial); `None` without a level change
    /// or a phase.
    pub(super) fn phase_id(&self) -> Option<u64> {
        let level_info = self.level_info.as_ref()?;
        level_info.phasing.first().map(|entry| entry.phase_id)
    }

    /// The sequence of the last phasing update; a level change goes on from it.
    pub(super) const fn phasing_sequence(&self) -> u8 {
        self.phasing_sequence
    }

    /// The `LevelInfoChangedMsg` to send again at `now_ms`, once.
    pub(super) fn level_info_due(&mut self, now_ms: u64) -> Option<ServerMessage> {
        let due = self.level_info_again_at_ms.is_some_and(|at| now_ms >= at);
        if !due {
            return None;
        }
        self.level_info_again_at_ms = None;
        self.level_info.clone().map(ServerMessage::LevelInfoChanged)
    }

    /// The cutscene the client plays, when one was allowed.
    pub(super) const fn cutscene(&self) -> Option<u32> {
        self.cutscene
    }

    /// The answers to `message` from a client in `client_context`.
    pub(super) fn answer(
        &mut self,
        message: ClientFacetMsg,
        client_context: u8,
    ) -> Vec<ServerMessage> {
        match message {
            ClientFacetMsg::AckLevelInfo {
                client_context: acked,
                ..
            } if acked == client_context => {
                let mode = if self.tutorial {
                    TUTORIAL_PHASING_MODE
                } else {
                    OPEN_WORLD_PHASING_MODE
                };
                let entries = self
                    .level_info
                    .as_ref()
                    .map(|level| level.phasing.clone())
                    .unwrap_or_default();
                vec![self.phasing(entries, mode)]
            }
            ClientFacetMsg::RequestCutscene { cutscene, .. } => {
                self.cutscene = Some(cutscene);
                vec![ServerMessage::CutsceneGranted(CutsceneGrantedMsg {
                    header: self.header(component::PLAYER_CUTSCENE),
                    cutscene,
                })]
            }
            ClientFacetMsg::CutsceneEnded { .. } => {
                self.cutscene = None;
                vec![ServerMessage::CutsceneEndFinalized(
                    CutsceneEndFinalizedMsg {
                        header: self.header(component::PLAYER_CUTSCENE),
                    },
                )]
            }
            ClientFacetMsg::AckLevelInfo { .. }
            | ClientFacetMsg::CinematicStarted { .. }
            | ClientFacetMsg::CinematicEnded { .. }
            | ClientFacetMsg::QteCompleted { .. }
            | ClientFacetMsg::UiScreenOpened { .. }
            | ClientFacetMsg::TriggerAreaPassthrough { .. }
            | ClientFacetMsg::TakeAllItems { .. }
            | ClientFacetMsg::EquipItem { .. }
            | ClientFacetMsg::ReturnToMainWorld { .. } => Vec::new(),
        }
    }

    /// The messages when the player leaves this level: the loading screen and no phases.
    pub(super) fn leave(&mut self) -> Vec<ServerMessage> {
        vec![
            ServerMessage::TeleportWithLoadingScreen(TeleportWithLoadingScreenMsg {
                header: self.header(component::PLAYER),
                flag: true,
            }),
            self.phasing(Vec::new(), EMPTY_PHASING_MODE),
        ]
    }

    /// The whole achievement state for the spawn card; `None` for a player taken without a level
    /// change. Changes made so far are in it.
    pub(super) fn first_achievements(&mut self) -> Option<AchievementState> {
        let achievements = self.achievements.as_mut()?;
        achievements.changes.clear();
        Some(AchievementState {
            bits: Some(AchievementBits::Full(FullList {
                version: Some(achievements.version),
                elements: achievements.bits.clone(),
            })),
        })
    }

    /// The achievement changes not sent yet, as one state; `None` without changes.
    pub(super) fn take_achievement_changes(&mut self) -> Option<AchievementState> {
        let changes = self.achievements.as_mut()?.take_changes()?;
        Some(AchievementState {
            bits: Some(AchievementBits::Changes(changes)),
        })
    }

    /// Sets achievement `id` of `rules`; an id the content does not have changes nothing.
    pub(super) fn set_achievement(&mut self, rules: &AchievementRules, id: &str) {
        if let (Some(achievements), Some(bit)) =
            (self.achievements.as_mut(), rules.indexes.index(id))
        {
            achievements.set(bit);
        }
    }

    /// Clears achievement `id` (a quest's `TaskLockAchievement`).
    pub(super) fn clear_achievement(&mut self, rules: &AchievementRules, id: &str) {
        if let (Some(achievements), Some(bit)) =
            (self.achievements.as_mut(), rules.indexes.index(id))
        {
            achievements.clear(bit);
        }
    }

    fn phasing(&mut self, entries: Vec<PhasingEntry>, mode: u8) -> ServerMessage {
        self.phasing_sequence = self.phasing_sequence.wrapping_add(1);
        ServerMessage::UpdatePhasing(UpdatePhasingMsg {
            header: self.header(component::GRID_TRACKING),
            entries,
            mode,
            sequence: self.phasing_sequence,
        })
    }

    const fn header(&self, component: u64) -> FacetHeader {
        FacetHeader::of(component, self.entity)
    }
}

#[cfg(test)]
mod tests {
    use nw_formats::objectstream::lowercase_crc32;
    use nw_messages::{LOGIN_CLIENT_CONTEXT, TUTORIAL_LEVEL};

    use super::*;

    const ENTITY: u64 = 7;
    const NOW: u64 = 1000;
    const PHASE: u64 = 0x8b2c_bbb8_6aa9_9537;
    const INTRO_UNLOCK: &str = "FTUE_Intro2_Cinematic_Unlock";
    const INTRO_UNLOCK_BIT: u32 = 14_804;

    const EXPANSION_BIT: u32 = 11_979;
    const WARMONGER_BIT: u32 = 14_878;
    const WARMONGER: &str = "Archetype_Destroyer";

    fn tutorial() -> AchievementRules {
        let indexes = AchievementIndexes::build([
            (EXPANSION_UNLOCK.to_owned(), EXPANSION_BIT),
            ("Archetype_Warmonger".to_owned(), WARMONGER_BIT),
            (INTRO_UNLOCK.to_owned(), INTRO_UNLOCK_BIT),
        ]);
        AchievementRules::of(
            &indexes,
            &[(WARMONGER.to_owned(), "Archetype_Warmonger".to_owned())],
        )
    }

    fn flow() -> LevelFlow {
        LevelFlow::after_level_info(
            ENTITY,
            LevelInfoChangedMsg::of_level(TUTORIAL_LEVEL, LOGIN_CLIENT_CONTEXT, NOW, PHASE),
            NOW,
            0,
            &tutorial(),
            true,
            Some(WARMONGER),
        )
    }

    fn header(component: u64) -> FacetHeader {
        FacetHeader::of(component, ENTITY)
    }

    fn bit(state: &AchievementState, bit: u32) -> bool {
        match &state.bits {
            Some(AchievementBits::Full(list)) => {
                list.elements[(bit / 8) as usize] & 1 << (bit % 8) != 0
            }
            _ => false,
        }
    }

    #[test]
    fn a_player_starts_with_the_expansion_and_its_archetype() {
        let mut flow = flow();
        let first = flow.first_achievements().expect("achievements");
        let Some(AchievementBits::Full(list)) = &first.bits else {
            panic!("a full bitset");
        };
        assert_eq!(list.elements.len(), BITSET_BYTES);
        assert!(bit(&first, EXPANSION_BIT));
        assert!(bit(&first, WARMONGER_BIT));
        assert!(!bit(&first, INTRO_UNLOCK_BIT));
        assert_eq!(flow.take_achievement_changes(), None);
    }

    #[test]
    fn a_bit_beyond_the_bitset_grows_it_as_retail() {
        // Retail 175193: the door bit 15317 on a 1911-byte bitset came as bytes 1911 to 1914,
        // the first with the version, the bits of 1911 to 1913 still 0.
        const DOOR_BIT: u32 = 15_317;
        const DOOR_BYTE: u32 = 1914;
        let mut achievements = Achievements {
            bits: vec![0; 1911],
            version: FIRST_ACHIEVEMENT_VERSION,
            changes: Vec::new(),
        };
        achievements.set(DOOR_BIT);
        let changes = achievements.take_changes().expect("the door");
        let entries: Vec<(u32, u64, u8)> = changes
            .iter()
            .map(|change| (change.byte, change.version, change.value))
            .collect();
        assert_eq!(
            entries,
            [
                (1911, FIRST_ACHIEVEMENT_VERSION + 1, 0),
                (1912, LATER_ENTRY_VERSION, 0),
                (1913, LATER_ENTRY_VERSION, 0),
                (DOOR_BYTE, LATER_ENTRY_VERSION, 0x20),
            ]
        );
        assert_eq!(achievements.bits.len(), DOOR_BYTE as usize + 1);
        achievements.set(DOOR_BIT);
        assert_eq!(achievements.take_changes(), None, "set once");
        achievements.clear(DOOR_BIT + 1_000);
        assert_eq!(
            achievements.take_changes(),
            None,
            "a clear bit past the end"
        );
    }

    #[test]
    fn a_set_achievement_is_one_change() {
        let mut flow = flow();
        flow.first_achievements();
        flow.set_achievement(&tutorial(), INTRO_UNLOCK);
        flow.set_achievement(&tutorial(), INTRO_UNLOCK);
        flow.set_achievement(&tutorial(), "Nothing");
        let changes = flow.take_achievement_changes().expect("intro unlock");
        assert_eq!(
            changes.bits,
            Some(AchievementBits::Changes(vec![ByteChange {
                byte: INTRO_UNLOCK_BIT / 8,
                version: FIRST_ACHIEVEMENT_VERSION + 1,
                value: 1 << (INTRO_UNLOCK_BIT % 8),
            }]))
        );
    }

    #[test]
    fn the_level_info_goes_again_once_and_its_ack_gets_the_phases() {
        let mut flow = flow();
        assert_eq!(flow.level_info_due(NOW + LEVEL_INFO_AGAIN_MS - 1), None);
        assert!(matches!(
            flow.level_info_due(NOW + LEVEL_INFO_AGAIN_MS),
            Some(ServerMessage::LevelInfoChanged(_))
        ));
        assert_eq!(flow.level_info_due(NOW + 2 * LEVEL_INFO_AGAIN_MS), None);
        let ack = ClientFacetMsg::AckLevelInfo {
            header: header(component::PLAYER),
            client_context: LOGIN_CLIENT_CONTEXT,
        };
        let answers = flow.answer(ack, LOGIN_CLIENT_CONTEXT);
        let [ServerMessage::UpdatePhasing(phasing)] = answers.as_slice() else {
            panic!("one phasing update: {answers:?}");
        };
        assert_eq!(phasing.header, header(component::GRID_TRACKING));
        assert_eq!((phasing.mode, phasing.sequence), (3, 1));
        assert_eq!(phasing.entries[0].phase_id, PHASE);
        let other_context = flow.answer(ack, LOGIN_CLIENT_CONTEXT + 1);
        assert!(other_context.is_empty(), "an ack of another level");
    }

    #[test]
    fn a_cutscene_is_granted_then_closed() {
        let mut flow = flow();
        let crew = lowercase_crc32("ftue_01_solesurvivor_confrontingcrewmember_igc");
        let granted = flow.answer(
            ClientFacetMsg::RequestCutscene {
                header: header(component::PLAYER_CUTSCENE),
                cutscene: crew,
            },
            LOGIN_CLIENT_CONTEXT,
        );
        assert_eq!(flow.cutscene(), Some(crew));
        assert_eq!(
            granted,
            vec![ServerMessage::CutsceneGranted(CutsceneGrantedMsg {
                header: header(component::PLAYER_CUTSCENE),
                cutscene: crew,
            })]
        );
        let ended = flow.answer(
            ClientFacetMsg::CutsceneEnded {
                header: header(component::PLAYER_CUTSCENE),
            },
            LOGIN_CLIENT_CONTEXT,
        );
        assert!(matches!(
            ended.as_slice(),
            [ServerMessage::CutsceneEndFinalized(_)]
        ));
        assert_eq!(flow.cutscene(), None);
    }

    #[test]
    fn leaving_shows_the_loading_screen_and_clears_the_phases() {
        let mut flow = flow();
        let messages = flow.leave();
        let [
            ServerMessage::TeleportWithLoadingScreen(teleport),
            ServerMessage::UpdatePhasing(empty),
        ] = messages.as_slice()
        else {
            panic!("teleport and empty phasing: {messages:?}");
        };
        assert_eq!(teleport.header, header(component::PLAYER));
        assert!(empty.entries.is_empty());
        assert_eq!((empty.mode, empty.sequence), (EMPTY_PHASING_MODE, 1));
        assert_eq!(flow.phasing_sequence(), 1);
    }

    #[test]
    fn video_ids_are_the_retail_crcs() {
        const RETAIL_INTRO2: u32 = 0x4b85_43cd;
        const RETAIL_CREW: u32 = 0xa81b_d1af;
        assert_eq!(lowercase_crc32("ftue_intro2"), RETAIL_INTRO2);
        assert_eq!(
            lowercase_crc32("ftue_01_solesurvivor_confrontingcrewmember_igc"),
            RETAIL_CREW
        );
    }
}
