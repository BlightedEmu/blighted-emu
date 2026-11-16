// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool leaderboards`: the leaderboard table ([`nw_content::leaderboard_table`]) from the
//! content bundle, for `status-reporter`. It needs no game install.

use std::path::PathBuf;

use nw_content::ContentBundle;
use nw_content::layout::{ContentLayout, LEADERBOARDS};
use nw_content::leaderboard_table::leaderboard_table;

use crate::error::ToolError;

/// What `leaderboards` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderboardsReport {
    /// The file.
    pub path: PathBuf,
    /// The boards in it.
    pub boards: usize,
}

/// Writes the leaderboard table of `layout`, from its merged bundle.
///
/// # Errors
///
/// Returns [`ToolError`] when the bundle cannot be read or the table cannot be written.
pub fn write_leaderboard_table(layout: &ContentLayout) -> Result<LeaderboardsReport, ToolError> {
    let bundle = ContentBundle::load(layout)?;
    let table = leaderboard_table(&bundle.tables, &bundle.text);
    let path = layout.file(LEADERBOARDS);
    table.write(&path)?;
    Ok(LeaderboardsReport {
        path,
        boards: table.boards.len(),
    })
}

#[cfg(test)]
mod tests {
    use nw_content::layout::BUNDLE;
    use nw_content::leaderboard_table::LeaderboardTable;
    use nw_content::leaderboards::test_rows::board_row;
    use nw_content::table::Table;

    use super::*;

    const BUILD: &str = "25810808";

    #[test]
    fn the_table_holds_the_boards_of_the_bundle() {
        let dir = tempfile::tempdir().expect("dir");
        let mut bundle = ContentBundle {
            client_build: BUILD.to_owned(),
            ..ContentBundle::default()
        };
        bundle.tables.leaderboard_data.push(Table {
            source_path: "leaderboarddata.datasheet".to_owned(),
            unique_id: String::new(),
            rows: vec![nw_content::generated::LeaderboardData {
                rotation: "Season".to_owned(),
                ..board_row("SeasonWarScore", "top-war-score.{world}.{season}")
            }],
        });
        let layout = ContentLayout::new(dir.path(), BUILD);
        bundle.save(&layout.file(BUNDLE)).expect("bundle");
        let report = write_leaderboard_table(&layout).expect("table");
        assert_eq!(report.boards, 1);
        let table = LeaderboardTable::load(&layout).expect("load");
        assert_eq!(table.boards[0].board_id, "SeasonWarScore");
    }
}
