//! DOS identification against `libretro-database/dat/DOS.dat` (design v4.0 §3.4): CRC32 + size + name of the
//! files of a game (zip central directory, or root `exe com bat ima img` of a folder, or the image alone). Pure,
//! no file system. Filled and tested by DB1.

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

/// `lookup(crc32, size)` returns the `(game, rom_name)` pairs of the `(dos, libretro)` DAT (`dat_entry` columns,
/// schema v1 / v4). A DAT line counts when CRC32, size and name (case ignored) match.
pub(crate) fn dos_candidates(
    files: &[DosFile],
    lookup: &dyn Fn(u32, u64) -> Vec<(String, String)>,
) -> DosMatch {
    let _ = (files, lookup);
    todo!("DB1: count matching lines per game; best unique, tie = Ambiguous")
}
