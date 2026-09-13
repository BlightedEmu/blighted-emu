// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a player's client and this server say to each other after a level loads, in the retail
//! order (tutorial-intro note, section 4).
//!
//! - About [`LEVEL_INFO_AGAIN_MS`] after a `LevelInfoChangedMsg`, the server sends it again; the
//!   client acknowledges the level (5181), and the server sends the phases of its sectors (6310).
//! - In the tutorial, the player's achievements unlock the intro video, which the client plays on
//!   its own. When a video or a cutscene ends, its "watched" achievement is set, which unlocks the
//!   next step of the client's tutorial triggers. The server allows each cutscene the client asks
//!   for (6703 → 6707) and closes it when it ends (6705 → 6709).
//! - When the client asks to leave the tutorial (502), the server shows the loading screen (3951)
//!   and clears the phases (an empty 6310) before the level change.

use std::collections::HashMap;

use nw_content::achievements::AchievementIndexes;
use nw_formats::objectstream::lowercase_crc32;
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
/// The tutorial achievement set at login: the intro video plays only with it.
const INTRO_UNLOCK: &str = "FTUE_Intro2_Cinematic_Unlock";
/// The tutorial achievement set when the client has loaded the tutorial level (retail sets it
/// about 2 s later): the first in-game cutscene (crew member) starts only with it.
const CREW_CUTSCENE_UNLOCK: &str = "FTUE_PCCM_Cinematic_Unlock";
/// The "watched" achievement of each tutorial video and cutscene, by its lowercase name. The
/// client names a video or cutscene by the CRC-32 of that name.
const WATCHED: [(&str, &str); 5] = [
    ("ftue_intro1", "FTUE_Intro1_Cinematic_Watched"),
    ("ftue_intro2", "FTUE_Intro2_Cinematic_Watched"),
    ("ftue_intro3", "FTUE_Intro3_Cinematic_Watched"),
    ("ftue_rez", "FTUE_Rez_Cinematic_Watched"),
    (
        "ftue_01_solesurvivor_confrontingcrewmember_igc",
        "FTUE_PCCM_Cinematic_Watched",
    ),
];
/// The version of the first full achievement state (retail value).
const FIRST_ACHIEVEMENT_VERSION: u64 = 3;

/// The achievement bits of the tutorial flow, from the content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TutorialAchievements {
    bitset_bytes: usize,
    intro_unlock: Option<u32>,
    crew_cutscene_unlock: Option<u32>,
    /// The "watched" bit of each video or cutscene id.
    watched: HashMap<u32, u32>,
}

impl TutorialAchievements {
    /// The bits of `indexes`; an achievement the content does not have is left out.
    pub fn of(indexes: &AchievementIndexes) -> Self {
        Self {
            bitset_bytes: indexes.bitset_bytes(),
            intro_unlock: indexes.index(INTRO_UNLOCK),
            crew_cutscene_unlock: indexes.index(CREW_CUTSCENE_UNLOCK),
            watched: WATCHED
                .iter()
                .filter_map(|(event, achievement)| {
                    Some((lowercase_crc32(event), indexes.index(achievement)?))
                })
                .collect(),
        }
    }
}

/// A player's achievements and the changes not yet sent.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Achievements {
    bits: Vec<u8>,
    version: u64,
    changes: Vec<ByteChange>,
}

impl Achievements {
    /// Sets `bit` and records the changed byte; a set bit or one beyond the bitset changes
    /// nothing.
    fn set(&mut self, bit: u32) {
        if let Some(value) = set_bit(&mut self.bits, bit) {
            self.version += 1;
            self.changes.push(ByteChange {
                byte: bit / u8::BITS,
                version: self.version,
                value,
            });
        }
    }
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
    /// Tutorial players only.
    achievements: Option<Achievements>,
}

impl LevelFlow {
    /// The flow after `level_info` went to the client of entity `entity` at `now_ms`, after
    /// `phasing_sequence` updates. A tutorial player starts with the intro unlocked.
    pub(super) fn after_level_info(
        entity: u64,
        level_info: LevelInfoChangedMsg,
        now_ms: u64,
        phasing_sequence: u8,
        tutorial: Option<&TutorialAchievements>,
    ) -> Self {
        let achievements = tutorial.map(|tutorial| {
            let mut bits = vec![0; tutorial.bitset_bytes];
            if let Some(bit) = tutorial.intro_unlock {
                set_bit(&mut bits, bit);
            }
            Achievements {
                bits,
                version: FIRST_ACHIEVEMENT_VERSION,
                changes: Vec::new(),
            }
        });
        Self {
            entity,
            level_info: Some(level_info),
            level_info_again_at_ms: Some(now_ms.saturating_add(LEVEL_INFO_AGAIN_MS)),
            tutorial: tutorial.is_some(),
            phasing_sequence,
            cutscene: None,
            achievements,
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

    /// The answers to `message` from a client in `client_context`, with `tutorial`'s bits.
    pub(super) fn answer(
        &mut self,
        message: ClientFacetMsg,
        client_context: u8,
        tutorial: &TutorialAchievements,
    ) -> Vec<ServerMessage> {
        match message {
            ClientFacetMsg::AckLevelInfo {
                client_context: acked,
                ..
            } if acked == client_context => {
                if self.tutorial {
                    self.set(tutorial.crew_cutscene_unlock);
                }
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
            ClientFacetMsg::CinematicEnded { cinematic, .. } => {
                self.set(tutorial.watched.get(&cinematic).copied());
                Vec::new()
            }
            ClientFacetMsg::RequestCutscene { cutscene, .. } => {
                self.cutscene = Some(cutscene);
                vec![ServerMessage::CutsceneGranted(CutsceneGrantedMsg {
                    header: self.header(component::PLAYER_CUTSCENE),
                    cutscene,
                })]
            }
            ClientFacetMsg::CutsceneEnded { .. } => {
                if let Some(cutscene) = self.cutscene.take() {
                    self.set(tutorial.watched.get(&cutscene).copied());
                }
                vec![ServerMessage::CutsceneEndFinalized(
                    CutsceneEndFinalizedMsg {
                        header: self.header(component::PLAYER_CUTSCENE),
                    },
                )]
            }
            ClientFacetMsg::AckLevelInfo { .. } | ClientFacetMsg::ReturnToMainWorld { .. } => {
                Vec::new()
            }
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

    /// The whole achievement state for the spawn card; `None` outside the tutorial. Changes
    /// made so far are in it.
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
        let achievements = self.achievements.as_mut()?;
        if achievements.changes.is_empty() {
            return None;
        }
        Some(AchievementState {
            bits: Some(AchievementBits::Changes(std::mem::take(
                &mut achievements.changes,
            ))),
        })
    }

    fn set(&mut self, bit: Option<u32>) {
        if let (Some(achievements), Some(bit)) = (self.achievements.as_mut(), bit) {
            achievements.set(bit);
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
    use nw_messages::{LOGIN_CLIENT_CONTEXT, TUTORIAL_LEVEL};

    use super::*;

    const ENTITY: u64 = 7;
    const NOW: u64 = 1000;
    const PHASE: u64 = 0x8b2c_bbb8_6aa9_9537;
    const INTRO_UNLOCK_BIT: u32 = 14_804;
    const INTRO_WATCHED_BIT: u32 = 14_805;
    const CREW_UNLOCK_BIT: u32 = 15_198;
    const CREW_WATCHED_BIT: u32 = 15_199;

    fn tutorial() -> TutorialAchievements {
        TutorialAchievements::of(&AchievementIndexes::build([
            (INTRO_UNLOCK.to_owned(), INTRO_UNLOCK_BIT),
            (
                "FTUE_Intro2_Cinematic_Watched".to_owned(),
                INTRO_WATCHED_BIT,
            ),
            (CREW_CUTSCENE_UNLOCK.to_owned(), CREW_UNLOCK_BIT),
            ("FTUE_PCCM_Cinematic_Watched".to_owned(), CREW_WATCHED_BIT),
        ]))
    }

    fn flow() -> LevelFlow {
        LevelFlow::after_level_info(
            ENTITY,
            LevelInfoChangedMsg::of_level(TUTORIAL_LEVEL, LOGIN_CLIENT_CONTEXT, NOW, PHASE),
            NOW,
            0,
            Some(&tutorial()),
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
    fn a_tutorial_player_starts_with_only_the_intro_unlocked() {
        let mut flow = flow();
        let first = flow.first_achievements().expect("achievements");
        assert!(bit(&first, INTRO_UNLOCK_BIT));
        assert!(!bit(&first, INTRO_WATCHED_BIT));
        assert!(!bit(&first, CREW_UNLOCK_BIT));
        assert_eq!(flow.take_achievement_changes(), None);
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
        let answers = flow.answer(ack, LOGIN_CLIENT_CONTEXT, &tutorial());
        let [ServerMessage::UpdatePhasing(phasing)] = answers.as_slice() else {
            panic!("one phasing update: {answers:?}");
        };
        assert_eq!(phasing.header, header(component::GRID_TRACKING));
        assert_eq!((phasing.mode, phasing.sequence), (3, 1));
        assert_eq!(phasing.entries[0].phase_id, PHASE);
        let changes = flow.take_achievement_changes().expect("crew unlock");
        assert_eq!(
            changes.bits,
            Some(AchievementBits::Changes(vec![ByteChange {
                byte: CREW_UNLOCK_BIT / 8,
                version: FIRST_ACHIEVEMENT_VERSION + 1,
                value: 1 << (CREW_UNLOCK_BIT % 8),
            }]))
        );
        let other_context = flow.answer(ack, LOGIN_CLIENT_CONTEXT + 1, &tutorial());
        assert!(other_context.is_empty(), "an ack of another level");
    }

    #[test]
    fn videos_and_cutscenes_set_their_watched_bits() {
        let mut flow = flow();
        flow.answer(
            ClientFacetMsg::CinematicEnded {
                header: header(component::PLAYER_CUTSCENE),
                cinematic: lowercase_crc32("ftue_intro2"),
            },
            LOGIN_CLIENT_CONTEXT,
            &tutorial(),
        );
        let crew = lowercase_crc32("ftue_01_solesurvivor_confrontingcrewmember_igc");
        let granted = flow.answer(
            ClientFacetMsg::RequestCutscene {
                header: header(component::PLAYER_CUTSCENE),
                cutscene: crew,
            },
            LOGIN_CLIENT_CONTEXT,
            &tutorial(),
        );
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
            &tutorial(),
        );
        assert!(matches!(
            ended.as_slice(),
            [ServerMessage::CutsceneEndFinalized(_)]
        ));
        let state = flow.first_achievements().expect("achievements");
        assert!(bit(&state, INTRO_WATCHED_BIT));
        assert!(bit(&state, CREW_WATCHED_BIT));
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
