// SPDX-License-Identifier: AGPL-3.0-or-later
//! The musical performance rules of the game data (run 15 music study): the song book
//! (`javelindata_songbook`), its sheets (`javelindata_songbooksheets`), the performance rewards
//! (`javelindata_musicalperformancerewards`), the result grades (`javelindata_ranking`), the
//! points of a judged note (`javelindata_inputscoring`) and the instrument slots
//! (`javelindata_instrumentslots`). Every id goes on the wire as the CRC-32 of its lowercase
//! form (Observed: songs, sheets and rewards of six retail sessions).

use std::collections::HashMap;

use nw_formats::objectstream::lowercase_crc32;

use crate::Tables;

/// The categorical progression of music.
pub const MUSIC_TRADESKILL: &str = "Musician";
/// Separates the achievements of a `Pages` cell.
const PAGE_SEPARATOR: char = ',';
/// The `InputType` names of `MusicalScoring`.
const PERFECT: &str = "Perfect";
const MISSED: &str = "Missed";
/// The client enum `PaperdollSlotTypes` names and values of the instrument slots
/// (`FUN_145cac380`); an instrument slot row names its slot as `instrument-<name>-slot`.
const INSTRUMENT_SLOTS: [(&str, u32); 5] = [
    ("instrument-flute-slot", 39),
    ("instrument-guitar-slot", 40),
    ("instrument-mandolin-slot", 41),
    ("instrument-uprightbass-slot", 42),
    ("instrument-drums-slot", 43),
];

/// One song (`SongBookData`).
#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    /// `SongId`.
    pub id: String,
    /// `Duration`, s: the length the client sends with its start (4865).
    pub duration_s: f64,
    /// `Slot01` to `Slot05`: the sheets of the song's parts, in column order.
    pub sheets: Vec<String>,
    /// `CompletionGameEvent`: the event a finished song fires (`SongCompletedEasy`).
    pub completion_event: String,
}

impl Song {
    /// The 1-based sheet column of sheet `sheet` (the `packedPerformerSheetIndices` value;
    /// Observed: `Part2` of `PoundHammerEasy` is 2); `None` for a sheet of another song.
    pub fn sheet_column(&self, sheet: &str) -> Option<u16> {
        let index = self
            .sheets
            .iter()
            .position(|known| known.eq_ignore_ascii_case(sheet))?;
        u16::try_from(index + 1).ok()
    }
}

/// One sheet (`SongBookSheets`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sheet {
    /// `SheetId`.
    pub id: String,
    /// `Instrument`: the instrument slot row it is played on.
    pub instrument: String,
    /// `Pages`: the achievements that unlock its pages.
    pub pages: Vec<String>,
}

/// One performance reward (`MusicalPerformanceRewards`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformanceReward {
    /// `RewardId`.
    pub id: String,
    /// `Rank`: the lowest `Musician` rank that may pick it.
    pub rank: u16,
    /// The game event of each grade: `GameEventId_Rank_Amazing`, `_Great`, `_Okay`, `_Bad`.
    events: [String; 4],
}

impl PerformanceReward {
    /// The game event of grade `grade` (its index in [`MusicRules::grade`] order: 0 the best).
    pub fn event(&self, grade: usize) -> Option<&str> {
        self.events.get(grade).map(String::as_str)
    }
}

/// One result grade (`MusicalRanking`).
#[derive(Debug, Clone, PartialEq)]
pub struct Grade {
    /// `Grade`.
    pub name: String,
    /// `MinimumScore`: the lowest performance score of the grade.
    pub minimum_score: f64,
    /// `RankGameEventId`.
    pub event: Option<String>,
    /// `MusicalScoreMult`.
    pub score_multiplier: f64,
}

/// The points of a judged note (`MusicalScoring`): a note played on time (`Perfect`) and a
/// missed one (`Missed`). The client's `Great`, a note a little off, needs the chart's note times,
/// which are not in the data the server reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NotePoints {
    /// `Perfect`.
    pub perfect: u32,
    /// `Missed`.
    pub missed: u32,
}

/// The music rules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MusicRules {
    songs: HashMap<u32, Song>,
    sheets: HashMap<u32, Sheet>,
    rewards: HashMap<u32, PerformanceReward>,
    /// The grades, the best first.
    grades: Vec<Grade>,
    /// The points of a judged note.
    pub points: NotePoints,
    /// The paperdoll slot of each instrument slot row, by its lowercase name.
    instruments: HashMap<String, u32>,
}

impl MusicRules {
    /// The music rules of `tables`.
    pub fn from_tables(tables: &Tables) -> Self {
        let songs = tables
            .song_book_data
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| {
                let sheets = [
                    Some(&row.slot01),
                    row.slot02.as_ref(),
                    row.slot03.as_ref(),
                    row.slot04.as_ref(),
                    row.slot05.as_ref(),
                ]
                .into_iter()
                .flatten()
                .filter(|sheet| !sheet.is_empty())
                .cloned()
                .collect();
                (
                    lowercase_crc32(&row.song_id),
                    Song {
                        id: row.song_id.clone(),
                        duration_s: row.duration,
                        sheets,
                        completion_event: row.completion_game_event.clone(),
                    },
                )
            })
            .collect();
        let sheets = tables
            .song_book_sheets
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| {
                (
                    lowercase_crc32(&row.sheet_id),
                    Sheet {
                        id: row.sheet_id.clone(),
                        instrument: row.instrument.clone(),
                        pages: row
                            .pages
                            .as_deref()
                            .unwrap_or_default()
                            .split(PAGE_SEPARATOR)
                            .map(str::trim)
                            .filter(|page| !page.is_empty())
                            .map(str::to_owned)
                            .collect(),
                    },
                )
            })
            .collect();
        let rewards = tables
            .musical_performance_rewards
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| {
                (
                    lowercase_crc32(&row.reward_id),
                    PerformanceReward {
                        id: row.reward_id.clone(),
                        rank: u16::try_from(row.rank.max(0)).unwrap_or(u16::MAX),
                        events: [
                            row.game_event_id_rank_amazing.clone(),
                            row.game_event_id_rank_great.clone(),
                            row.game_event_id_rank_okay.clone(),
                            row.game_event_id_rank_bad.clone(),
                        ],
                    },
                )
            })
            .collect();
        let mut grades: Vec<Grade> = tables
            .musical_ranking
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| Grade {
                name: row.grade.clone(),
                minimum_score: row.minimum_score,
                event: row.rank_game_event_id.clone().filter(|event| !event.is_empty()),
                score_multiplier: row.musical_score_mult,
            })
            .collect();
        grades.sort_by(|best, worse| worse.minimum_score.total_cmp(&best.minimum_score));
        let points_of = |name: &str| {
            tables
                .musical_scoring
                .iter()
                .flat_map(|table| &table.rows)
                .find(|row| row.input_type.eq_ignore_ascii_case(name))
                .map_or(0, |row| u32::try_from(row.points.max(0)).unwrap_or(u32::MAX))
        };
        let instruments = tables
            .musical_instrument_slot
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| {
                let slot = INSTRUMENT_SLOTS
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(&row.paperdoll_id))?
                    .1;
                Some((row.paperdoll_id.to_lowercase(), slot))
            })
            .collect();
        Self {
            songs,
            sheets,
            rewards,
            grades,
            points: NotePoints {
                perfect: points_of(PERFECT),
                missed: points_of(MISSED),
            },
            instruments,
        }
    }

    /// The song whose lowercase id has CRC-32 `crc`.
    pub fn song(&self, crc: u32) -> Option<&Song> {
        self.songs.get(&crc)
    }

    /// The sheet whose lowercase id has CRC-32 `crc`.
    pub fn sheet(&self, crc: u32) -> Option<&Sheet> {
        self.sheets.get(&crc)
    }

    /// The reward whose lowercase id has CRC-32 `crc`.
    pub fn reward(&self, crc: u32) -> Option<&PerformanceReward> {
        self.rewards.get(&crc)
    }

    /// The grade of performance score `score` (0 to 1) and its index (0 the best); `None`
    /// without grades.
    pub fn grade(&self, score: f64) -> Option<(usize, &Grade)> {
        self.grades
            .iter()
            .enumerate()
            .find(|(_, grade)| score >= grade.minimum_score)
            .or_else(|| self.grades.iter().enumerate().last())
    }

    /// The paperdoll slot of the instrument slot row `instrument`.
    pub fn instrument_slot(&self, instrument: &str) -> Option<u32> {
        self.instruments.get(&instrument.to_lowercase()).copied()
    }

    /// The achievements that unlock song pages: every sheet's `Pages`, sorted, once each.
    pub fn page_achievements(&self) -> Vec<&str> {
        let mut pages: Vec<&str> = self
            .sheets
            .values()
            .flat_map(|sheet| sheet.pages.iter().map(String::as_str))
            .collect();
        pages.sort_unstable();
        pages.dedup();
        pages
    }

    /// Every instrument paperdoll slot.
    pub fn instrument_slots(&self) -> impl Iterator<Item = u32> + '_ {
        self.instruments.values().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::{
        MusicalInstrumentSlot, MusicalPerformanceRewards, MusicalRanking, MusicalScoring,
        SongBookData, SongBookSheets,
    };
    use crate::table::Table;

    fn table<T>(rows: Vec<T>) -> Vec<Table<T>> {
        vec![Table {
            source_path: String::new(),
            unique_id: String::new(),
            rows,
        }]
    }

    /// Rows of build 25810808.
    fn tables() -> Tables {
        let song: SongBookData = serde_json::from_value(serde_json::json!({
            "song_id": "PoundHammerEasy",
            "music_title": "@songtitle",
            "performance": "Easy",
            "filter_type": "DefaultPerformance",
            "note_pattern": "Novice",
            "sound_bank": "MXMI",
            "slot01": "PoundHammer_Guitar_Part1_Easy",
            "slot02": "PoundHammer_Guitar_Part2_Easy",
            "crowd": "Crowd",
            "tempo": 120,
            "scroll_speed": 2.5,
            "duration": 56.536,
            "hit_zone": 0.222_222_2,
            "perfect_zone": 0.083_333_34,
            "dance_timeline": "Performance_Dance",
            "completion_game_event": "SongCompletedEasy",
            "highest_rank_achievement_id": "MP_Perfect_PoundHammerEasy",
        }))
        .expect("song row");
        let ranking = |grade: &str, minimum: f64, event: Option<&str>, mult: f64| MusicalRanking {
            grade: grade.to_owned(),
            minimum_score: minimum,
            rank_game_event_id: event.map(str::to_owned),
            musical_score_mult: mult,
        };
        Tables {
            song_book_data: table(vec![song]),
            song_book_sheets: table(vec![SongBookSheets {
                sheet_id: "PoundHammer_Guitar_Part2_Easy".to_owned(),
                instrument: "instrument-guitar-slot".to_owned(),
                sheet_name: None,
                pages: Some(
                    "MP_PoundHammer_Guitar_Part2_Page1, MP_PoundHammer_Guitar_Part2_Page2"
                        .to_owned(),
                ),
                audio_event: String::new(),
                error_event: String::new(),
                timeline: String::new(),
            }]),
            musical_performance_rewards: table(vec![MusicalPerformanceRewards {
                reward_id: "MusicBuff_03".to_owned(),
                name: String::new(),
                description: String::new(),
                rank: 100,
                filter_type: "DefaultPerformance".to_owned(),
                game_event_id_rank_amazing: "MusicReward_Amazing_Buff_03".to_owned(),
                game_event_id_rank_great: "MusicReward_Great_Buff_03".to_owned(),
                game_event_id_rank_okay: "MusicReward_Okay_Buff_03".to_owned(),
                game_event_id_rank_bad: "MusicReward_Bad_Buff_03".to_owned(),
            }]),
            musical_ranking: table(vec![
                ranking("Bad", 0.0, None, 1.0),
                ranking("Amazing", 0.95, Some("MusicPerformance_Amazing_Result"), 2.0),
                ranking("Great", 0.75, None, 1.6),
            ]),
            musical_scoring: table(vec![
                MusicalScoring {
                    input_type: "Perfect".to_owned(),
                    points: 10,
                },
                MusicalScoring {
                    input_type: "Great".to_owned(),
                    points: 7,
                },
            ]),
            musical_instrument_slot: table(vec![MusicalInstrumentSlot {
                paperdoll_id: "instrument-guitar-slot".to_owned(),
                name: String::new(),
                clef_icon_path: String::new(),
            }]),
            ..Tables::default()
        }
    }

    #[test]
    fn the_rows_read_by_their_wire_ids() {
        let rules = MusicRules::from_tables(&tables());
        let song = rules.song(0x16d9_564e).expect("retail 4867 PoundHammerEasy");
        assert_eq!(song.completion_event, "SongCompletedEasy");
        assert!((song.duration_s - 56.536).abs() < f64::EPSILON);
        assert_eq!(song.sheet_column("poundhammer_guitar_part2_easy"), Some(2));
        assert_eq!(song.sheet_column("Other"), None);
        let sheet = rules.sheet(0x7742_927a).expect("retail 4879 sheet");
        assert_eq!(rules.instrument_slot(&sheet.instrument), Some(40));
        let reward = rules.reward(0xb908_2ee0).expect("retail 5022 MusicBuff_03");
        assert_eq!(reward.rank, 100);
        assert_eq!(reward.event(0), Some("MusicReward_Amazing_Buff_03"));
        assert_eq!(
            rules.points,
            NotePoints {
                perfect: 10,
                missed: 0,
            }
        );
        assert_eq!(rules.instrument_slots().collect::<Vec<_>>(), [40]);
        assert_eq!(
            rules.page_achievements(),
            [
                "MP_PoundHammer_Guitar_Part2_Page1",
                "MP_PoundHammer_Guitar_Part2_Page2"
            ]
        );
    }

    #[test]
    fn a_score_gets_the_best_grade_it_reaches() {
        let rules = MusicRules::from_tables(&tables());
        let (index, amazing) = rules.grade(0.990_476).expect("grade");
        assert_eq!((index, amazing.name.as_str()), (0, "Amazing"));
        assert!((amazing.score_multiplier - 2.0).abs() < f64::EPSILON);
        assert_eq!(rules.grade(0.8).map(|(index, _)| index), Some(1));
        assert_eq!(rules.grade(0.1).map(|(_, grade)| grade.name.as_str()), Some("Bad"));
        assert_eq!(MusicRules::default().grade(1.0), None);
    }
}
