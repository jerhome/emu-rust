//! ROM tidying of `<data>/roms` (ADR 0023 §4, design v7.1 §3.4): read-only plan, then moves by indivisible units,
//! journal written before acting, never overwrites, never renames, never deletes user data, undo and recovery.
//! Only the roms root is tidied (never `[library] roots`). The scan classifier is reused, not duplicated.

mod journal;
mod plan;
mod relocate;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::db::{GameId, Library};
use crate::error::LibraryError;
use crate::scan::{ScanOptions, ScanProgress};

#[cfg_attr(
    not(test),
    allow(unused_imports, reason = "used by the tidy execution (FB1b)")
)]
pub(crate) use journal::{JournalWriter, Op};
pub(crate) use plan::Fingerprint;

/// Bounds and paths of a tidying run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TidyOptions {
    /// Canonical: file system work, journal header and identity registries (`relocate_identities`) use it.
    roms_root: PathBuf,
    /// The root as given, rebuilt by the scan's `normalize_root`: the spelling of the database keys (the scan
    /// stores `normalize_root(root)`-based paths, not canonical ones: `\\?\` on Windows, links elsewhere). Used
    /// for `known_game` lookups and [`Library::relocate`]. The UI gives the same path as `ScanOptions::roots`.
    scan_root: PathBuf,
    journal_dir: PathBuf,
    max_depth: usize,
    max_file_size: u64,
    /// Lower-case file names of known firmware (BIOS): such a file is never a unit, it stays in place.
    firmware_names: Vec<String>,
}

impl TidyOptions {
    /// Canonicalizes `roms_root` (must exist and be a directory) and keeps its scan spelling for the database;
    /// scan defaults for the bounds. `journal_dir` (`<data>/rangement`) is created by the first run, not here.
    pub fn new(roms_root: &Path, journal_dir: &Path) -> Result<Self, LibraryError> {
        let io = |source| LibraryError::Io {
            path: roms_root.to_path_buf(),
            source,
        };
        let canonical = std::fs::canonicalize(roms_root).map_err(io)?;
        if !canonical.is_dir() {
            return Err(io(std::io::Error::from(std::io::ErrorKind::NotADirectory)));
        }
        let defaults = ScanOptions::default();
        Ok(Self {
            roms_root: canonical,
            scan_root: roms_root.components().collect(),
            journal_dir: journal_dir.to_path_buf(),
            max_depth: defaults.max_depth,
            max_file_size: defaults.max_file_size,
            firmware_names: Vec::new(),
        })
    }

    /// File names of the known firmware (`firmware.toml`, `required_firmware` of emu-assets: this crate does not
    /// know them). A file of such a name (case-insensitive, any folder; a path keeps its last component) is never
    /// planned as a game: it stays in place and is listed in [`TidyPlan::ignored_paths`], for « Importer un BIOS… ».
    pub fn with_firmware_names<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.firmware_names = names
            .into_iter()
            .filter_map(|n| {
                let n = n.as_ref().replace('\\', "/");
                n.rsplit('/').next().map(str::to_lowercase)
            })
            .filter(|n| !n.is_empty())
            .collect();
        self
    }

    pub fn roms_root(&self) -> &Path {
        &self.roms_root
    }

    /// The root as the scan spells it (`normalize_root` of the given path): prefix of the database keys.
    pub(crate) fn scan_root(&self) -> &Path {
        &self.scan_root
    }

    pub(crate) fn journal_dir(&self) -> &Path {
        &self.journal_dir
    }

    pub(crate) fn max_depth(&self) -> usize {
        self.max_depth
    }

    pub(crate) fn max_file_size(&self) -> u64 {
        self.max_file_size
    }

    pub(crate) fn is_firmware_name(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        self.firmware_names.contains(&name)
    }
}

/// Why a unit's system is known (ADR 0023 §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TidyReason {
    /// Already a row of the library: the system of the row.
    Library,
    /// Header probe (`disc_info`, `probe_bin`, iNES, Mega Drive / 32X…).
    Header,
    /// CRC32 / SHA1 (or serial, track 01) in an imported DAT.
    DatHash,
    /// Extensions of the entries of a `.zip`.
    ArchiveContent,
    /// Name present in exactly one DAT (`Suggested`).
    DatName,
    /// Extension of exactly one system.
    Extension,
    /// Game folder with an executable at its root (`Suggested`, indivisible).
    DosFolder,
}

/// Why a unit stays where it is (shown in the preview; nothing is moved for it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TidyBlock {
    /// Destination taken by a different file (case-insensitive, on disk or in the plan).
    Conflict { existing: PathBuf },
    /// Destination taken by the same file (size and CRC32).
    Duplicate { existing: PathBuf },
    /// `.cue` / `.m3u` reference that does not exist.
    MissingReference { name: String },
    /// Absolute reference, or outside the folder of its `.cue` / `.m3u`.
    OutsideReference { name: String },
    /// Track named by two `.cue`: both units are blocked.
    SharedTrack { name: String },
    /// `.bin` tracks without `.cue` that cannot be grouped.
    UngroupedTracks,
    /// The unit is (or holds) a symbolic link.
    Symlink,
    /// A CD image inside a zip (refused since 2.1).
    DiscInArchive,
    /// `roms/<id>` exists but is a link, a junction or not a directory.
    DestinationNotPlainDir { dir: PathBuf },
    /// `roms/<id>` or the source is on another volume: no copy across volumes in 7.1.
    OtherVolume,
    /// File system without hard links (FAT, exFAT).
    NoHardLinks,
    /// The database stores text paths.
    NonUtf8Path,
    /// Destination longer than 240 characters (Windows).
    PathTooLong,
    /// A folder to create is named after a system id (it would change the folder rule of the scan).
    FolderNamedLikeSystem { name: String },
    /// A file of the unit changed (path, size or mtime) between the preview and the run.
    ChangedSincePlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TidyStatus {
    /// Checked by default.
    Sure(TidyReason),
    /// Shown unchecked, with the reason.
    Suggested(TidyReason),
    /// Several systems possible: `TidyUnit::candidates` lists the user's choices.
    Ambiguous,
    /// No system: stays in place.
    Unknown,
    Blocked(TidyBlock),
}

/// One file move, both paths relative to the roms root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TidyMove {
    pub from: PathBuf,
    pub to: PathBuf,
    pub size: u64,
}

/// Indivisible group of files moved together (ADR 0023 §4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TidyUnit {
    /// Unique in its plan; the selection given to [`Library::apply_tidy`].
    pub id: u32,
    pub title: String,
    /// System id of the destination (`psx`), once known.
    pub system: Option<String>,
    /// Systems the user may choose (ambiguous unit, or another system for a known game).
    pub candidates: Vec<String>,
    pub status: TidyStatus,
    /// Empty until a system is known.
    pub moves: Vec<TidyMove>,
    /// Unit moved by one directory rename (DOS game folder, folder holding only this unit).
    pub whole_dir: Option<TidyMove>,
    /// Lower-case extension of the primary file (bulk choice by extension); empty for a game folder.
    pub extension: String,
    /// Already a row of the library.
    pub known_game: Option<GameId>,
    /// `(system of the row, chosen system)` of a known game sent to another system (warned: saves stay behind).
    pub system_change: Option<(String, String)>,
}

/// Result of [`Library::plan_tidy`]; never persisted.
///
/// Paths of [`TidyMove`], [`TidyBlock::Conflict`] / [`TidyBlock::Duplicate`] (`existing`),
/// [`TidyBlock::DestinationNotPlainDir`] and `ignored_paths` are relative to the canonical roms root. A unit of
/// status `Ambiguous` whose `system` is set was resolved by [`TidyPlan::choose_system`]: it may be selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TidyPlan {
    pub units: Vec<TidyUnit>,
    /// Items directly under the folder of their system (`roms/<id>/…`), never analysed nor moved.
    pub already_tidy: u64,
    /// Files that are not games (texts, images, executables outside a game folder, empty files…), left in place.
    pub ignored: u64,
    /// The first [`MAX_IGNORED_PATHS`] of them, in walk order: the UI checks them against the known BIOS md5
    /// (« Importer un BIOS… »), which this crate does not know.
    pub ignored_paths: Vec<PathBuf>,
    /// One entry per unit (same index): files as analysed, layout and status before the destination checks.
    meta: Vec<plan::UnitMeta>,
    context: plan::PlanContext,
}

/// Bound of [`TidyPlan::ignored_paths`] (the count itself is not bounded).
pub const MAX_IGNORED_PATHS: usize = 2000;

impl TidyPlan {
    /// Resolves an `Ambiguous` unit (or overrides a known game: `system_change` is then set) to one of its
    /// candidates: fills `moves`, re-runs the collision and blocking checks.
    pub fn choose_system(&mut self, unit: u32, system: &str) -> Result<(), LibraryError> {
        self.set_system(unit, system)?;
        self.place_all();
        Ok(())
    }

    /// Same for every still `Ambiguous` unit of `extension` that lists `system`; returns how many were resolved.
    pub fn choose_system_for_extension(&mut self, extension: &str, system: &str) -> u32 {
        let extension = extension.trim_start_matches('.').to_ascii_lowercase();
        let ids: Vec<u32> = self
            .units
            .iter()
            .filter(|u| {
                u.status == TidyStatus::Ambiguous
                    && u.system.is_none()
                    && u.extension == extension
                    && u.candidates.iter().any(|c| c == system)
            })
            .map(|u| u.id)
            .collect();
        let mut resolved = 0;
        for id in ids {
            if self.set_system(id, system).is_ok() {
                resolved += 1;
            }
        }
        if resolved > 0 {
            self.place_all();
        }
        resolved
    }

    /// Canonical roms root the plan was made for ([`Library::apply_tidy`] refuses another one).
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "used by the tidy execution (FB1b)")
    )]
    pub(crate) fn roms_root(&self) -> &Path {
        &self.context.root
    }

    /// `(path relative to the root, size, mtime)` of every file of `unit` as analysed (primary file first; every
    /// file below a game folder), rechecked just before the unit is moved.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "used by the tidy execution (FB1b)")
    )]
    pub(crate) fn fingerprints(&self, unit: u32) -> Option<&[Fingerprint]> {
        let index = self.units.iter().position(|u| u.id == unit)?;
        Some(&self.meta[index].files)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TidyProgress {
    pub done_units: u64,
    pub total_units: u64,
    pub current: Option<PathBuf>,
}

/// `(from, to)` absolute paths under the canonical roms root: [`TidyReport::moved`], the moves of a journal
/// ([`Library::pending_tidy_identities`]), the input of `emu_runtime::relocate_identities`.
pub type AbsoluteMoves = Vec<(PathBuf, PathBuf)>;

/// Outcome of [`Library::apply_tidy`], [`Library::undo_tidy`] and [`Library::recover_tidy`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TidyReport {
    pub journal: Option<PathBuf>,
    /// `(from, to)` absolute, under the canonical root: input of `emu_runtime::relocate_identities`.
    pub moved: AbsoluteMoves,
    pub left: Vec<(PathBuf, TidyBlock)>,
    /// French messages.
    pub errors: Vec<(PathBuf, String)>,
    pub dirs_created: u64,
    pub dirs_removed: u64,
    pub cancelled: bool,
}

/// One tidy journal (`<journal_dir>/journal-<unix>.txt`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TidyJournalInfo {
    pub path: PathBuf,
    pub created_unix: i64,
    pub moved: u64,
    /// `U` line: undone.
    pub undone: bool,
    /// `E` line: run finished (or recovered).
    pub complete: bool,
    /// `I` line: identity registries rewritten.
    pub identities_done: bool,
}

/// Creates `roms/<id>/` for every system of the catalog, moving nothing. Returns the created ones only. The roms
/// root itself is created when missing; an existing entry of that name (folder, file or link) is left alone.
pub fn create_system_folders(roms_root: &Path) -> Result<Vec<PathBuf>, LibraryError> {
    let io = |path: &Path, source| LibraryError::Io {
        path: path.to_path_buf(),
        source,
    };
    std::fs::create_dir_all(roms_root).map_err(|e| io(roms_root, e))?;
    let mut created = Vec::new();
    for system in crate::systems() {
        let dir = roms_root.join(&system.id);
        if std::fs::symlink_metadata(&dir).is_ok() {
            continue;
        }
        match std::fs::create_dir(&dir) {
            Ok(()) => created.push(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io(&dir, e)),
        }
    }
    Ok(created)
}

/// Newest journal, if any (« Annuler le dernier rangement » is offered when it is complete and not undone).
pub fn last_tidy(options: &TidyOptions) -> Result<Option<TidyJournalInfo>, LibraryError> {
    let Some((path, created_unix)) = journal::list(options.journal_dir())?.pop() else {
        return Ok(None);
    };
    let journal = journal::read(&path)?;
    Ok(Some(journal.info(path, created_unix)))
}

impl Library {
    /// Read-only analysis of the roms root (nothing is written, not even the database).
    pub fn plan_tidy(
        &self,
        options: &TidyOptions,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&ScanProgress),
    ) -> Result<TidyPlan, LibraryError> {
        plan::run(&self.conn, options, cancel, progress)
    }

    /// Moves the `selected` units (ids of `Sure` / `Suggested` / resolved units). Journal first, never overwrites.
    pub fn apply_tidy(
        &mut self,
        options: &TidyOptions,
        plan: &TidyPlan,
        selected: &[u32],
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&TidyProgress),
    ) -> Result<TidyReport, LibraryError> {
        let _ = (options, plan, selected, cancel, progress);
        todo!("FB1b: execution")
    }

    /// Replays `journal` backwards, unit by unit (ADR 0023 §4.6).
    pub fn undo_tidy(
        &mut self,
        options: &TidyOptions,
        journal: &Path,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&TidyProgress),
    ) -> Result<TidyReport, LibraryError> {
        let _ = (options, journal, cancel, progress);
        todo!("FB1b: undo")
    }

    /// Rolls back every started, uncommitted unit of an interrupted run and closes its journal. `None`: nothing.
    pub fn recover_tidy(
        &mut self,
        options: &TidyOptions,
    ) -> Result<Option<TidyReport>, LibraryError> {
        let _ = options;
        todo!("FB1b: recovery")
    }

    /// Complete journals whose identity step is not marked done, with the moves to replay (oldest first).
    pub fn pending_tidy_identities(
        &self,
        options: &TidyOptions,
    ) -> Result<Vec<(PathBuf, AbsoluteMoves)>, LibraryError> {
        let _ = options;
        todo!("FB1b: pending identity steps")
    }

    /// Appends the « identities done » marker; called by the UI after `emu_runtime::relocate_identities` succeeded.
    pub fn mark_tidy_identities(
        &mut self,
        options: &TidyOptions,
        journal: &Path,
    ) -> Result<(), LibraryError> {
        let _ = (options, journal);
        todo!("FB1b: I marker")
    }

    /// One transaction: `game.path`, `game_media.path`, `game.pinned`. A path under a moved folder follows it.
    /// `roms_root` is spelled as in `ScanOptions::roots` (database keys are built from it with the scan's
    /// `normalize_root`); `moves` are relative to it, like [`TidyMove`].
    /// Keys are built by the scan's function; a stale row at a destination (file gone) is removed, a live one is
    /// an error (transaction rolled back). Never creates a row.
    ///
    /// Destination row that is not one of the moved rows: stale (removed) when its file is absent, or when the
    /// source of the move is gone (the file was moved: that row duplicates it); live (error) when both the source
    /// and the destination exist — the call does not match the disk. `pinned` follows the folder rule of the scan
    /// at the new place, so the next scan finds the row unchanged. Returns the number of `game` rows rewritten.
    pub fn relocate(
        &mut self,
        roms_root: &Path,
        moves: &[(PathBuf, PathBuf)],
    ) -> Result<u64, LibraryError> {
        relocate::run(&mut self.conn, roms_root, moves)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod options_tests {
    use super::*;

    #[test]
    fn options_canonicalize_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let roms = dir.path().join("roms");
        std::fs::create_dir(&roms).unwrap();
        let spelled = dir.path().join("roms/./");
        let options = TidyOptions::new(&spelled, &dir.path().join("rangement")).unwrap();
        assert_eq!(options.roms_root(), std::fs::canonicalize(&roms).unwrap());
        assert!(TidyOptions::new(&dir.path().join("absent"), dir.path()).is_err());
    }

    #[test]
    fn plan_of_an_empty_root_is_empty_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let roms = dir.path().join("roms");
        std::fs::create_dir(&roms).unwrap();
        let journals = dir.path().join("rangement");
        let library = Library::open_in_memory().unwrap();
        let options = TidyOptions::new(&roms, &journals).unwrap();
        let plan = library
            .plan_tidy(&options, &AtomicBool::new(false), &mut |_| {})
            .unwrap();
        assert!(plan.units.is_empty());
        assert!(!journals.exists());
        assert_eq!(std::fs::read_dir(&roms).unwrap().count(), 0);
    }
}
