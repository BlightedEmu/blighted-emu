// SPDX-License-Identifier: GPL-3.0-or-later
//! Finds the paths the launch options need on this PC: the Steam install, its libraries, the
//! patched game copy and the installed Proton builds. The options window offers what it finds;
//! the player can still type any path.

use std::path::{Path, PathBuf};

use crate::game::PATCHED_EXE_NAME;

/// Steam's library list, under the Steam install.
const LIBRARY_LIST: [&str; 2] = ["steamapps", "libraryfolders.vdf"];
/// The key of a library folder in the library list.
const LIBRARY_PATH_KEY: &str = "\"path\"";
/// Folder of installed games in a library.
const GAMES_DIR: [&str; 2] = ["steamapps", "common"];
/// The game folder in a library, and its executable folder.
const GAME_DIR: &str = "New World";
const GAME_BIN_DIR: &str = "Bin64";
/// Folder of community Proton builds (for example GE-Proton) under the Steam install.
const COMPAT_TOOLS_DIR: &str = "compatibilitytools.d";
/// The Proton program in a Proton build folder.
const PROTON_PROGRAM: &str = "proton";
/// The quote and escape characters of Steam's text format.
const VDF_QUOTE: char = '"';
const VDF_ESCAPED_BACKSLASH: &str = "\\\\";
const BACKSLASH: &str = "\\";

/// Home folder variable, and the Steam install folders under it (native, old link, Flatpak).
#[cfg(not(windows))]
const HOME_ENV: &str = "HOME";
#[cfg(not(windows))]
const STEAM_HOME_DIRS: [&str; 3] = [
    ".local/share/Steam",
    ".steam/steam",
    ".var/app/com.valvesoftware.Steam/data/Steam",
];
/// Program files variable on Windows, and the Steam folder under it.
#[cfg(windows)]
const PROGRAM_FILES_ENV: &str = "ProgramFiles(x86)";
#[cfg(windows)]
const STEAM_PROGRAM_DIR: &str = "Steam";

/// One Proton build found on this PC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtonBuild {
    /// The build folder name, for example `GE-Proton10-12`.
    pub name: String,
    /// The `proton` program.
    pub program: PathBuf,
}

/// What a scan of this PC found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// The first Steam install folder that has a library list.
    pub steam_install: Option<PathBuf>,
    /// The patched game copy in a Steam library.
    pub game_exe: Option<PathBuf>,
    /// The Proton builds, by name.
    pub protons: Vec<ProtonBuild>,
}

/// The Steam install folders to try, in order, from environment variables.
pub fn steam_install_candidates(lookup: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    #[cfg(not(windows))]
    {
        lookup(HOME_ENV)
            .filter(|home| !home.is_empty())
            .map(|home| {
                STEAM_HOME_DIRS
                    .iter()
                    .map(|dir| Path::new(&home).join(dir))
                    .collect()
            })
            .unwrap_or_default()
    }
    #[cfg(windows)]
    {
        lookup(PROGRAM_FILES_ENV)
            .filter(|dir| !dir.is_empty())
            .map(|dir| vec![Path::new(&dir).join(STEAM_PROGRAM_DIR)])
            .unwrap_or_default()
    }
}

fn library_list(steam_install: &Path) -> PathBuf {
    LIBRARY_LIST
        .iter()
        .fold(steam_install.to_path_buf(), |path, part| path.join(part))
}

/// The library folders named in the text of Steam's `libraryfolders.vdf`.
pub fn parse_library_paths(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix(LIBRARY_PATH_KEY))
        .filter_map(|rest| {
            let value = rest
                .trim()
                .strip_prefix(VDF_QUOTE)?
                .strip_suffix(VDF_QUOTE)?;
            Some(PathBuf::from(
                value.replace(VDF_ESCAPED_BACKSLASH, BACKSLASH),
            ))
        })
        .collect()
}

/// The Steam libraries: the install folder itself, then each folder of its library list.
pub fn library_folders(steam_install: &Path) -> Vec<PathBuf> {
    let mut folders = vec![steam_install.to_path_buf()];
    if let Ok(text) = std::fs::read_to_string(library_list(steam_install)) {
        for folder in parse_library_paths(&text) {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
    }
    folders
}

/// The patched game copy's path inside a library.
pub fn game_exe_in(library: &Path) -> PathBuf {
    GAMES_DIR
        .iter()
        .fold(library.to_path_buf(), |path, part| path.join(part))
        .join(GAME_DIR)
        .join(GAME_BIN_DIR)
        .join(PATCHED_EXE_NAME)
}

/// The Proton builds in the sub-folders of `dir` that hold a `proton` program.
fn protons_in(dir: &Path) -> Vec<ProtonBuild> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let program = entry.path().join(PROTON_PROGRAM);
            program.is_file().then(|| ProtonBuild {
                name: entry.file_name().to_string_lossy().into_owned(),
                program,
            })
        })
        .collect()
}

/// The Proton builds of the Steam install and its libraries, sorted by name.
pub fn proton_builds(steam_install: &Path, libraries: &[PathBuf]) -> Vec<ProtonBuild> {
    let mut builds = protons_in(&steam_install.join(COMPAT_TOOLS_DIR));
    for library in libraries {
        let games = GAMES_DIR
            .iter()
            .fold(library.clone(), |path, part| path.join(part));
        builds.extend(protons_in(&games));
    }
    builds.sort_by(|a, b| a.name.cmp(&b.name));
    builds.dedup_by(|a, b| a.program == b.program);
    builds
}

/// Scans this PC: the first Steam install with a library list, its libraries, the patched game
/// copy and the Proton builds.
pub fn scan(lookup: impl Fn(&str) -> Option<String>) -> Found {
    let Some(steam_install) = steam_install_candidates(lookup)
        .into_iter()
        .find(|candidate| library_list(candidate).is_file())
    else {
        return Found::default();
    };
    let libraries = library_folders(&steam_install);
    let game_exe = libraries
        .iter()
        .map(|library| game_exe_in(library))
        .find(|exe| exe.is_file());
    let protons = proton_builds(&steam_install, &libraries);
    Found {
        steam_install: Some(steam_install),
        game_exe,
        protons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_paths_are_read_from_the_library_list() {
        let text = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/p/.local/share/Steam"
		"label"		""
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}
"#;
        assert_eq!(
            parse_library_paths(text),
            [
                PathBuf::from("/home/p/.local/share/Steam"),
                PathBuf::from("D:\\SteamLibrary")
            ]
        );
    }

    #[test]
    fn a_scan_finds_the_libraries_the_game_and_the_proton_builds() {
        let root = tempfile::tempdir().expect("dir");
        let steam = root.path().join("Steam");
        let library = root.path().join("Library");
        let list = library_list(&steam);
        std::fs::create_dir_all(list.parent().expect("parent")).expect("steamapps");
        std::fs::write(&list, format!("\"path\"\t\t\"{}\"\n", library.display()))
            .expect("library list");
        let exe = game_exe_in(&library);
        std::fs::create_dir_all(exe.parent().expect("parent")).expect("game");
        std::fs::write(&exe, "").expect("exe");
        let ge = steam.join(COMPAT_TOOLS_DIR).join("GE-Proton10-12");
        let valve = library.join("steamapps/common/Proton 10.0");
        let runtime = library.join("steamapps/common/Proton EasyAntiCheat Runtime");
        for dir in [&ge, &valve, &runtime] {
            std::fs::create_dir_all(dir).expect("tool dir");
        }
        for dir in [&ge, &valve] {
            std::fs::write(dir.join(PROTON_PROGRAM), "").expect("proton");
        }

        assert_eq!(library_folders(&steam), [steam.clone(), library.clone()]);
        let builds = proton_builds(&steam, &library_folders(&steam));
        assert_eq!(
            builds,
            [
                ProtonBuild {
                    name: "GE-Proton10-12".to_owned(),
                    program: ge.join(PROTON_PROGRAM),
                },
                ProtonBuild {
                    name: "Proton 10.0".to_owned(),
                    program: valve.join(PROTON_PROGRAM),
                },
            ]
        );
        #[cfg(not(windows))]
        {
            let home = root.path().to_string_lossy().into_owned();
            let native = root.path().join(STEAM_HOME_DIRS[0]);
            std::fs::create_dir_all(native.parent().expect("parent")).expect("share");
            std::fs::rename(&steam, &native).expect("move steam");
            let found = scan(|name| (name == HOME_ENV).then(|| home.clone()));
            assert_eq!(found.steam_install, Some(native));
            assert_eq!(found.game_exe, Some(exe));
            assert_eq!(found.protons.len(), builds.len());
        }
    }

    #[test]
    fn no_steam_install_finds_nothing() {
        assert_eq!(scan(|_| None), Found::default());
    }
}
