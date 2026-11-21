//! DOS identification against `libretro-database/dat/DOS.dat` (design v4.0 §3.4): CRC32 + size + name of the
//! files of a game (zip central directory, or root `exe com bat ima img` of a folder, or the image alone). Pure,
//! no file system.

use std::collections::HashMap;

/// One file of a DOS game as seen by the scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DosFile {
    /// File name (zip entry base name or folder root name); compared case-insensitively with `rom_name`.
    pub name: String,
    pub crc32: u32,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DosMatch {
    /// DAT game name with the most matching lines.
    Unique(String),
    /// Several games tie (shared tools such as `INSTALL.EXE`): not identified, fall back to the name.
    Ambiguous,
    None,
}

/// Last component of a DAT `rom_name` (some DATs keep a folder prefix).
fn base_name(rom_name: &str) -> &str {
    rom_name.rsplit(['/', '\\']).next().unwrap_or(rom_name)
}

/// `lookup(crc32, size)` returns the `(game, rom_name)` pairs of the `(dos, libretro)` DAT (`dat_entry` columns,
/// schema v1 / v4). A DAT line counts when CRC32, size and name (case ignored) match; a file counts once per game.
pub(crate) fn dos_candidates(
    files: &[DosFile],
    lookup: &dyn Fn(u32, u64) -> Vec<(String, String)>,
) -> DosMatch {
    let mut score: HashMap<String, u32> = HashMap::new();
    for file in files {
        let mut games: Vec<String> = lookup(file.crc32, file.size)
            .into_iter()
            .filter(|(_, rom)| {
                rom.eq_ignore_ascii_case(&file.name)
                    || base_name(rom).eq_ignore_ascii_case(&file.name)
            })
            .map(|(game, _)| game)
            .collect();
        games.sort_unstable();
        games.dedup();
        for game in games {
            *score.entry(game).or_default() += 1;
        }
    }
    let Some(best) = score.values().copied().max() else {
        return DosMatch::None;
    };
    let mut top = score.into_iter().filter(|(_, n)| *n == best);
    match (top.next(), top.next()) {
        (Some((game, _)), None) => DosMatch::Unique(game),
        _ => DosMatch::Ambiguous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, crc32: u32, size: u64) -> DosFile {
        DosFile {
            name: name.to_owned(),
            crc32,
            size,
        }
    }

    fn dat() -> impl Fn(u32, u64) -> Vec<(String, String)> {
        |crc, size| {
            let rows: &[(u32, u64, &str, &str)] = &[
                (1, 10, "Alpha", "ALPHA.EXE"),
                (2, 20, "Alpha", "ALPHA.DAT"),
                (3, 30, "Alpha", "INSTALL.EXE"),
                (3, 30, "Beta", "INSTALL.EXE"),
                (4, 40, "Beta", "sub/BETA.EXE"),
            ];
            rows.iter()
                .filter(|r| r.0 == crc && r.1 == size)
                .map(|r| (r.2.to_owned(), r.3.to_owned()))
                .collect()
        }
    }

    #[test]
    fn most_matching_lines_win_and_names_ignore_case() {
        let files = [
            file("alpha.exe", 1, 10),
            file("Alpha.Dat", 2, 20),
            file("INSTALL.EXE", 3, 30),
        ];
        assert_eq!(
            dos_candidates(&files, &dat()),
            DosMatch::Unique("Alpha".into())
        );
        // A DAT name with a folder prefix matches the base name.
        assert_eq!(
            dos_candidates(&[file("BETA.EXE", 4, 40)], &dat()),
            DosMatch::Unique("Beta".into())
        );
    }

    #[test]
    fn shared_tools_tie_and_mismatches_do_not_count() {
        assert_eq!(
            dos_candidates(&[file("INSTALL.EXE", 3, 30)], &dat()),
            DosMatch::Ambiguous
        );
        // Right CRC and size, wrong name; right name, wrong size.
        assert_eq!(
            dos_candidates(&[file("OTHER.EXE", 1, 10)], &dat()),
            DosMatch::None
        );
        assert_eq!(
            dos_candidates(&[file("ALPHA.EXE", 1, 11)], &dat()),
            DosMatch::None
        );
        assert_eq!(dos_candidates(&[], &dat()), DosMatch::None);
    }

    #[test]
    fn duplicate_lines_of_one_game_count_once_per_file() {
        let twice = |crc: u32, _: u64| {
            if crc == 1 {
                vec![
                    ("A".to_owned(), "X.EXE".to_owned()),
                    ("A".to_owned(), "X.EXE".to_owned()),
                    ("B".to_owned(), "X.EXE".to_owned()),
                ]
            } else {
                Vec::new()
            }
        };
        assert_eq!(
            dos_candidates(&[file("X.EXE", 1, 1)], &twice),
            DosMatch::Ambiguous
        );
    }
}
