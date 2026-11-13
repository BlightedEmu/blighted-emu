// SPDX-License-Identifier: AGPL-3.0-or-later
//! `content-tool claim-plots`: the claim plot file ([`nw_world::claim_plots`]) from the game
//! paks: every `.dynamicslice` under [`CLAIM_PLOT_FOLDER`] that has a land claim component.

use std::path::{Path, PathBuf};

use nw_world::claim_plots::{CLAIM_PLOT_FOLDER, ClaimPlotSlice, ClaimPlots, claim_plots_path};

use crate::error::ToolError;
use crate::source::GameSource;
use crate::world::stream_of;

/// The pak path's suffix of a dynamic slice.
const DYNAMIC_SLICE_SUFFIX: &str = ".dynamicslice";

/// What `claim-plots` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimPlotsReport {
    /// The file.
    pub path: PathBuf,
    /// The dynamic slices of the claim plot folder.
    pub slices: usize,
    /// The claim plots among them.
    pub plots: usize,
}

/// Whether pak path `path` is a dynamic slice of the claim plot folder.
fn is_plot_folder_slice(path: &str) -> bool {
    path.starts_with(CLAIM_PLOT_FOLDER) && path.ends_with(DYNAMIC_SLICE_SUFFIX)
}

/// Writes the claim plot file of `client_build` into `out_dir` from `source`.
///
/// # Errors
///
/// Returns [`ToolError`] when the paks or a slice cannot be read, or the file cannot be
/// written.
pub fn build_claim_plots(
    source: &GameSource,
    client_build: &str,
    out_dir: &Path,
) -> Result<ClaimPlotsReport, ToolError> {
    let mut slices = 0;
    let mut plots = Vec::new();
    source.for_each_entry(is_plot_folder_slice, |path, bytes| {
        slices += 1;
        let root = stream_of(path, &bytes)?;
        plots.extend(ClaimPlotSlice::of(path, &root));
        Ok::<(), ToolError>(())
    })?;
    let table = ClaimPlots::of(plots);
    let path = claim_plots_path(out_dir, client_build);
    table.write(&path)?;
    Ok(ClaimPlotsReport {
        path,
        slices,
        plots: table.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_dynamic_slices_of_the_claim_plot_folder_are_read() {
        assert!(is_plot_folder_slice(
            "slices/darkness/zone_capitalplot_windswardshores.dynamicslice"
        ));
        assert!(!is_plot_folder_slice(
            "coatgen/5daf0528/zone_capitalplot_shatteredmountain_1253122726.dynamicslice"
        ));
        assert!(!is_plot_folder_slice(
            "slices/darkness/zone_capitalplot_windswardshores.slice"
        ));
    }
}
