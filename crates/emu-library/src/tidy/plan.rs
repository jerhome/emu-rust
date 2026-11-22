//! Read-only analysis of the roms root (ADR 0023 §4.2 to §4.4): own walk (no link is ever followed), indivisible
//! units, classification by the scan's classifier with the folder rule neutralised, destinations and blocking
//! checks. Nothing is written: neither the disk nor the database (SELECT only).

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension};

use super::journal::{fold_path, same_place};
use super::{
    MAX_IGNORED_PATHS, TidyBlock, TidyMove, TidyOptions, TidyPlan, TidyReason, TidyStatus, TidyUnit,
};
use crate::db::{DiscIndex, NameIndex, db_error, identify};
use crate::disc::cue_file_names;
use crate::media::{parse_m3u, strip_media_marker};
use crate::scan::{
    self, AUDIO_TRACK_ALONE, Failure, Found, MAX_CARTRIDGE_BIN, Rule, SEGA_TRACK_ALONE,
    ZIPPED_DISC, audio_track_name, is_candidate, is_console_disc, is_executable, is_m3u, lower_ext,
    mtime_of, needs_hashes, norm_key, normalize_root, probe_cached, track_format,
};
use crate::systems::Attribution;
use crate::{
    BinKind, GameId, Identification, LibraryError, ScanProgress, SystemInfo, SystemKind,
    group_media, systems_for_extension,
};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// Windows limit of a destination path (ADR 0023 §4.2), counted without the `\\?\` prefix.
const MAX_WINDOWS_PATH: usize = 240;
/// Suffix of the assembly folder of a unit being moved (`<dest>.emu-rust-part`): never analysed.
pub(crate) const PART_SUFFIX: &str = ".emu-rust-part";
/// Same bound as the scan's `.m3u` reader.
const MAX_M3U_SIZE: u64 = 64 * 1024;

/// One file of a unit as analysed: path relative to the canonical roms root, size and mtime (seconds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fingerprint {
    pub(crate) path: PathBuf,
    pub(crate) size: u64,
    pub(crate) mtime: i64,
}

/// Plan-wide facts used again by [`TidyPlan::choose_system`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlanContext {
    /// Canonical roms root.
    pub(super) root: PathBuf,
    /// Device of the root (Unix): a destination or a source on another device is blocked.
    root_dev: Option<u64>,
    /// Files above it are never hashed for a collision: `Conflict`, not `Duplicate` (FV0 I6).
    pub(super) max_file_size: u64,
}

/// CRC32 of the files compared for collisions, by `(path, size, mtime)` (FV0 I6): `choose_system` re-runs every
/// check, a file is hashed once per plan. Not part of the plan's identity (always equal).
#[derive(Debug, Clone, Default)]
pub(super) struct CrcCache(HashMap<(PathBuf, u64, i64), Option<u32>>);

impl PartialEq for CrcCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for CrcCache {}

impl CrcCache {
    /// Same size and CRC32 (ADR 0023 §4.4); a file above `max` is never read (different).
    fn same_file(&mut self, a: &Path, b: &Path, max: u64) -> bool {
        match (fs::symlink_metadata(a), fs::symlink_metadata(b)) {
            (Ok(ma), Ok(mb))
                if ma.is_file() && mb.is_file() && ma.len() == mb.len() && ma.len() <= max =>
            {
                let ca = self.crc(a, &ma);
                ca.is_some() && ca == self.crc(b, &mb)
            }
            _ => false,
        }
    }

    fn crc(&mut self, path: &Path, meta: &fs::Metadata) -> Option<u32> {
        *self
            .0
            .entry((path.to_path_buf(), meta.len(), mtime_of(meta)))
            .or_insert_with(|| crc_of(path))
    }
}

/// How the files of a unit are laid out at the destination.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Layout {
    /// Every file directly under `roms/<id>/` (single file, `.sbi`, disks of a `(Disk N of M)` group).
    Flat,
    /// Under `roms/<id>/<name>/`, keeping the paths relative to `base` (the folder of the `.cue`, `.m3u`, group
    /// or game folder). `whole`: `base` holds nothing but the unit and is moved by one rename, under its own name.
    Folder {
        name: String,
        base: PathBuf,
        whole: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UnitMeta {
    pub(super) files: Vec<Fingerprint>,
    layout: Layout,
    /// Status given by the analysis, before the destination checks.
    base: TidyStatus,
    /// `choose_system` is allowed (ambiguous unit, or known game with other candidates).
    choosable: bool,
    /// System set by the user: its destination is claimed after the analysed units.
    chosen: bool,
    /// System of the library row (known game).
    row_system: Option<String>,
    /// A file of the unit is on another device than the root (Unix).
    other_volume: bool,
}

// ---------------------------------------------------------------------------------------------------------------
// Walk

struct WalkFile {
    rel: PathBuf,
    abs: PathBuf,
    size: u64,
    mtime: i64,
    dev: Option<u64>,
}

struct GameDir {
    rel: PathBuf,
    files: Vec<WalkFile>,
    block: Option<TidyBlock>,
}

#[derive(Default)]
struct Walked {
    /// Regular files with a UTF-8 path, outside the system folders and the game folders, in name order.
    files: Vec<WalkFile>,
    /// Folders holding an executable at their root (DOS games), with every file below them.
    game_dirs: Vec<GameDir>,
    /// Entries reported as blocked units: links, non-UTF-8 paths (relative path, block).
    blocked: Vec<(PathBuf, TidyBlock)>,
    /// Entries that are neither in `files` nor in `game_dirs` (links, special files, unreadable or too deep
    /// folders, leftovers): a folder holding one of them is never moved whole.
    others: Vec<PathBuf>,
    already_tidy: u64,
    ignored: u64,
    ignored_paths: Vec<PathBuf>,
}

impl Walked {
    fn ignore(&mut self, rel: PathBuf) {
        self.ignored += 1;
        if self.ignored_paths.len() < MAX_IGNORED_PATHS {
            self.ignored_paths.push(rel);
        }
    }
}

#[cfg(unix)]
fn dev_of(meta: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.dev())
}

#[cfg(not(unix))]
fn dev_of(_meta: &fs::Metadata) -> Option<u64> {
    None
}

fn io_error(path: &Path, source: std::io::Error) -> LibraryError {
    LibraryError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn sorted_entries(dir: &Path) -> std::io::Result<Vec<fs::DirEntry>> {
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

fn is_system_id(name: &str) -> bool {
    crate::system(name).is_some()
}

fn walk(options: &TidyOptions, cancel: &AtomicBool) -> Result<Walked, LibraryError> {
    let (root, max_depth) = (options.roms_root(), options.max_depth());
    let mut out = Walked::default();
    let mut stack = vec![(PathBuf::new(), 0usize)];
    while let Some((rel, depth)) = stack.pop() {
        if scan::is_cancelled(cancel) {
            return Err(LibraryError::Cancelled);
        }
        let abs = root.join(&rel);
        let entries = match sorted_entries(&abs) {
            Ok(e) => e,
            Err(e) if rel.as_os_str().is_empty() => return Err(io_error(&abs, e)),
            Err(e) => {
                tracing::warn!("tidy: unreadable folder {}: {e}", abs.display());
                out.others.push(rel);
                continue;
            }
        };
        // The root is never a game folder: its executables are loose files.
        if !rel.as_os_str().is_empty() && has_executable(&entries) {
            out.game_dirs.push(game_dir(root, rel, max_depth));
            continue;
        }
        let mut subdirs = Vec::new();
        for entry in entries {
            let name = entry.file_name();
            let child = rel.join(&name);
            let Ok(kind) = entry.file_type() else {
                out.others.push(child);
                continue;
            };
            let top = rel.as_os_str().is_empty();
            if kind.is_symlink() {
                // A linked `roms/<id>` is a destination matter (`DestinationNotPlainDir`), not a unit.
                if top && name.to_str().is_some_and(is_system_id) {
                    continue;
                }
                let to_dir = fs::metadata(abs.join(&name)).is_ok_and(|m| m.is_dir());
                if to_dir || is_candidate(&child) {
                    out.blocked.push((child.clone(), TidyBlock::Symlink));
                } else {
                    out.ignore(child.clone());
                }
                out.others.push(child);
                continue;
            }
            if name.to_str().is_none() {
                if kind.is_dir() || is_candidate(&child) {
                    out.blocked.push((child.clone(), TidyBlock::NonUtf8Path));
                } else {
                    out.ignore(child.clone());
                }
                out.others.push(child);
                continue;
            }
            if kind.is_dir() {
                let name = name.to_string_lossy();
                if top && is_system_id(&name) {
                    out.already_tidy += count_items(&abs.join(&*name));
                } else if name.ends_with(PART_SUFFIX) || depth >= max_depth {
                    out.others.push(child);
                } else {
                    subdirs.push(child);
                }
                continue;
            }
            if !kind.is_file() {
                out.others.push(child);
                continue;
            }
            if options.is_firmware_name(&name.to_string_lossy()) {
                // A BIOS dropped here by mistake: signalled by the UI, never moved as a game (ADR 0023 §4.2).
                out.ignore(child.clone());
                out.others.push(child);
                continue;
            }
            match entry.metadata() {
                Ok(meta) => out.files.push(WalkFile {
                    abs: abs.join(&name),
                    rel: child,
                    size: meta.len(),
                    mtime: mtime_of(&meta),
                    dev: dev_of(&meta),
                }),
                Err(e) => {
                    tracing::warn!("tidy: unreadable file {}: {e}", child.display());
                    out.others.push(child);
                }
            }
        }
        stack.extend(subdirs.into_iter().rev().map(|d| (d, depth + 1)));
    }
    Ok(out)
}

fn has_executable(entries: &[fs::DirEntry]) -> bool {
    entries
        .iter()
        .any(|e| is_executable(&e.path()) && e.file_type().is_ok_and(|t| t.is_file()))
}

/// Entries directly under an already tidy `roms/<id>/` (one per game, roughly), dot files excepted.
fn count_items(dir: &Path) -> u64 {
    fs::read_dir(dir).map_or(0, |it| {
        it.filter_map(Result::ok)
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .count() as u64
    })
}

/// Every file below a game folder (it moves whole); a link or a non-UTF-8 name inside blocks it.
fn game_dir(root: &Path, rel: PathBuf, max_depth: usize) -> GameDir {
    let mut dir = GameDir {
        rel,
        files: Vec::new(),
        block: None,
    };
    if dir.rel.to_str().is_none() {
        dir.block = Some(TidyBlock::NonUtf8Path);
    }
    let mut stack = vec![(dir.rel.clone(), 0usize)];
    while let Some((rel, depth)) = stack.pop() {
        let Ok(entries) = sorted_entries(&root.join(&rel)) else {
            continue;
        };
        let mut subdirs = Vec::new();
        for entry in entries {
            let child = rel.join(entry.file_name());
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                dir.block.get_or_insert(TidyBlock::Symlink);
                continue;
            }
            if entry.file_name().to_str().is_none() {
                dir.block.get_or_insert(TidyBlock::NonUtf8Path);
                continue;
            }
            if kind.is_dir() {
                if depth < max_depth {
                    subdirs.push(child);
                }
            } else if kind.is_file()
                && let Ok(meta) = entry.metadata()
            {
                dir.files.push(WalkFile {
                    abs: root.join(&child),
                    rel: child,
                    size: meta.len(),
                    mtime: mtime_of(&meta),
                    dev: dev_of(&meta),
                });
            }
        }
        stack.extend(subdirs.into_iter().rev().map(|d| (d, depth + 1)));
    }
    dir
}

// ---------------------------------------------------------------------------------------------------------------
// Units

#[derive(Debug, Clone, PartialEq, Eq)]
enum DraftKind {
    /// One file (and its `.sbi`), or a `.cue` with its tracks.
    File,
    /// A `.bin` PlayStation / 3DO data track without `.cue`, with its audio tracks.
    Track,
    /// `(Disk N of M)` / `(Disc N)` group: the media, in disk order (absolute paths).
    Group(Vec<String>),
    /// `.m3u` and the images it lists (absolute paths).
    Playlist(Vec<String>),
    /// Game folder (index into `Walked::game_dirs`).
    GameDir(usize),
    /// Link or non-UTF-8 entry.
    Odd(PathBuf),
}

#[derive(Debug, Clone)]
struct Draft {
    /// Indexes into `Walked::files`, primary file first.
    files: Vec<usize>,
    kind: DraftKind,
    layout: Layout,
    block: Option<TidyBlock>,
}

impl Draft {
    fn new(file: usize, kind: DraftKind) -> Self {
        Draft {
            files: vec![file],
            kind,
            layout: Layout::Flat,
            block: None,
        }
    }

    fn block(&mut self, block: TidyBlock) {
        self.block.get_or_insert(block);
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn parent_rel(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// `Name (Track 01)` and `Name` → `Name`: the title shared by the tracks of one disc.
fn track_title(path: &Path) -> String {
    let stem = stem(path);
    // ASCII lower-casing keeps byte offsets.
    let lower = stem.to_ascii_lowercase();
    match lower.rfind("(track ") {
        Some(at) if lower[at..].ends_with(')') => stem[..at].trim_end().to_owned(),
        _ => stem.trim_end().to_owned(),
    }
}

fn track_radical(path: &Path) -> String {
    track_title(path).to_lowercase()
}

struct Builder<'w> {
    walked: &'w Walked,
    root: &'w Path,
    drafts: Vec<Option<Draft>>,
    owner: Vec<Option<usize>>,
    by_key: HashMap<String, usize>,
    blocked_keys: HashMap<String, TidyBlock>,
}

impl<'w> Builder<'w> {
    fn new(walked: &'w Walked, root: &'w Path) -> Self {
        let by_key = walked
            .files
            .iter()
            .enumerate()
            .filter_map(|(i, f)| Some((norm_key(f.abs.to_str()?), i)))
            .collect();
        let blocked_keys = walked
            .blocked
            .iter()
            .filter_map(|(rel, b)| Some((norm_key(root.join(rel).to_str()?), b.clone())))
            .collect();
        Builder {
            walked,
            root,
            drafts: Vec::new(),
            owner: vec![None; walked.files.len()],
            by_key,
            blocked_keys,
        }
    }

    fn ext(&self, file: usize) -> String {
        lower_ext(&self.walked.files[file].rel).unwrap_or_default()
    }

    fn push(&mut self, draft: Draft) -> usize {
        let index = self.drafts.len();
        for &f in &draft.files {
            self.owner[f].get_or_insert(index);
        }
        self.drafts.push(Some(draft));
        index
    }

    /// Gives `file` to `draft`; a file already owned by another unit blocks both (`SharedTrack`).
    fn claim(&mut self, draft: usize, file: usize) {
        match self.owner[file] {
            Some(other) if other != draft => {
                let name = file_name(&self.walked.files[file].rel);
                for d in [other, draft] {
                    if let Some(d) = self.drafts[d].as_mut() {
                        d.block(TidyBlock::SharedTrack { name: name.clone() });
                    }
                }
            }
            Some(_) => {}
            None => {
                self.owner[file] = Some(draft);
                if let Some(d) = self.drafts[draft].as_mut()
                    && !d.files.contains(&file)
                {
                    d.files.push(file);
                }
            }
        }
    }

    /// Looks a reference up: the file index, or why the unit is blocked.
    fn reference(&self, path: &Path, name: &str) -> Result<usize, TidyBlock> {
        let key = path.to_str().map(norm_key);
        if let Some(&i) = key.as_ref().and_then(|k| self.by_key.get(k)) {
            return Ok(i);
        }
        if let Some(b) = key.as_ref().and_then(|k| self.blocked_keys.get(k)) {
            return Err(b.clone());
        }
        let name = name.to_owned();
        // Present but out of reach (inside a system folder, a game folder, a linked folder): never moved.
        if fs::symlink_metadata(path).is_ok() {
            Err(TidyBlock::OutsideReference { name })
        } else {
            Err(TidyBlock::MissingReference { name })
        }
    }

    /// `Name.sbi` next to a `.chd` / `.pbp` / `.iso` / `.cue` of the same stem (PlayStation subchannel data).
    fn attach_sbi(&mut self, draft: usize, primary: usize) {
        let f = &self.walked.files[primary];
        let wanted = norm_key(&format!("{}.sbi", stem(&f.rel)));
        let dir = parent_rel(&f.rel);
        let sbi = self.walked.files.iter().position(|o| {
            parent_rel(&o.rel) == dir
                && lower_ext(&o.rel).as_deref() == Some("sbi")
                && norm_key(&file_name(&o.rel)) == wanted
        });
        if let Some(sbi) = sbi
            && self.owner[sbi].is_none()
        {
            self.claim(draft, sbi);
        }
    }

    fn cues(&mut self) {
        for i in 0..self.walked.files.len() {
            if self.ext(i) != "cue" || self.owner[i].is_some() {
                continue;
            }
            let cue = &self.walked.files[i];
            let mut draft = Draft::new(i, DraftKind::File);
            draft.layout = Layout::Folder {
                name: stem(&cue.rel),
                base: parent_rel(&cue.rel),
                whole: false,
            };
            let names = cue_file_names(&cue.abs);
            let dir = cue.abs.parent().unwrap_or(self.root).to_path_buf();
            let d = self.push(draft);
            match names {
                Ok(names) => {
                    for (name, plain) in names {
                        if !plain {
                            self.block(d, TidyBlock::OutsideReference { name });
                            continue;
                        }
                        let path = dir.join(&name);
                        if lower_ext(&path).as_deref() == Some("cue") {
                            continue;
                        }
                        match self.reference(&path, &name) {
                            Ok(track) => self.claim(d, track),
                            Err(b) => self.block(d, b),
                        }
                    }
                }
                Err(e) => tracing::warn!("tidy: unreadable cue sheet {}: {e}", cue.rel.display()),
            }
            self.attach_sbi(d, i);
        }
    }

    fn block(&mut self, draft: usize, block: TidyBlock) {
        if let Some(d) = self.drafts[draft].as_mut() {
            d.block(block);
        }
    }

    fn playlists(&mut self) {
        for i in 0..self.walked.files.len() {
            if !is_m3u(&self.ext(i)) || self.owner[i].is_some() {
                continue;
            }
            let m3u = &self.walked.files[i];
            let Some(entries) = read_playlist(&m3u.abs) else {
                // Unreadable playlist: a plain file (the scan reports it, the classification decides).
                continue;
            };
            let dir = m3u.abs.parent().unwrap_or(self.root).to_path_buf();
            let mut draft = Draft::new(i, DraftKind::Playlist(Vec::new()));
            draft.layout = Layout::Folder {
                name: stem(&m3u.rel),
                base: parent_rel(&m3u.rel),
                whole: false,
            };
            let d = self.push(draft);
            let mut media = Vec::new();
            match entries {
                Err(name) => self.block(d, TidyBlock::OutsideReference { name }),
                Ok(entries) => {
                    for entry in entries {
                        let name = entry
                            .strip_prefix(&dir)
                            .unwrap_or(&entry)
                            .display()
                            .to_string();
                        if !entry.starts_with(&dir) {
                            self.block(d, TidyBlock::OutsideReference { name });
                            continue;
                        }
                        if lower_ext(&entry).is_some_and(|e| is_m3u(&e)) {
                            continue;
                        }
                        let file = match self.reference(&entry, &name) {
                            Ok(f) => f,
                            Err(b) => {
                                self.block(d, b);
                                continue;
                            }
                        };
                        media.extend(entry.to_str().map(str::to_owned));
                        self.absorb(d, file);
                    }
                }
            }
            if let Some(Some(draft)) = self.drafts.get_mut(d) {
                draft.kind = DraftKind::Playlist(media);
            }
        }
    }

    /// A playlist entry: a `.cue` brings its whole unit (tracks, `.sbi`, blocks); an image its `.sbi`.
    fn absorb(&mut self, draft: usize, file: usize) {
        if let Some(cue_draft) = self.owner[file].filter(|&o| o != draft)
            && self.ext(file) == "cue"
            && self.drafts[cue_draft]
                .as_ref()
                .is_some_and(|c| c.files.first() == Some(&file))
        {
            let Some(cue) = self.drafts[cue_draft].take() else {
                return;
            };
            for f in &cue.files {
                self.owner[*f] = None;
            }
            for f in cue.files {
                self.claim(draft, f);
            }
            if let Some(b) = cue.block {
                self.block(draft, b);
            }
            return;
        }
        self.claim(draft, file);
        if matches!(self.ext(file).as_str(), "chd" | "pbp" | "iso") {
            self.attach_sbi(draft, file);
        }
    }

    /// `.bin` files no `.cue` names: PlayStation / 3DO data tracks with their audio tracks; anything else that
    /// cannot be a Mega Drive / 32X ROM is blocked (`UngroupedTracks`).
    fn tracks(&mut self) {
        let bins: Vec<usize> = (0..self.walked.files.len())
            .filter(|&i| self.ext(i) == "bin" && self.owner[i].is_none())
            .collect();
        for &i in &bins {
            if self.owner[i].is_some() {
                continue;
            }
            let file = &self.walked.files[i];
            let is_data = probe_cached(&file.abs)
                .map(|p| p.kind)
                .and_then(track_format)
                .is_some();
            if !is_data {
                continue;
            }
            let radical = track_radical(&file.rel);
            let dir = parent_rel(&file.rel);
            let audio: Vec<usize> = bins
                .iter()
                .copied()
                .filter(|&j| {
                    let o = &self.walked.files[j];
                    j != i
                        && self.owner[j].is_none()
                        && parent_rel(&o.rel) == dir
                        && audio_track_name(&o.rel)
                        && track_radical(&o.rel) == radical
                })
                .collect();
            let mut draft = Draft::new(i, DraftKind::Track);
            if !audio.is_empty() {
                draft.layout = Layout::Folder {
                    name: track_title(&file.rel),
                    base: dir,
                    whole: false,
                };
            }
            let d = self.push(draft);
            for j in audio {
                self.claim(d, j);
            }
        }
        for &i in &bins {
            if self.owner[i].is_some() {
                continue;
            }
            let file = &self.walked.files[i];
            let probe = probe_cached(&file.abs);
            let ungrouped = probe.is_some_and(|p| p.kind == BinKind::SegaCdTrack)
                || audio_track_name(&file.rel)
                || (file.size > MAX_CARTRIDGE_BIN && !probe.is_some_and(|p| p.sega_header));
            if ungrouped {
                let mut draft = Draft::new(i, DraftKind::File);
                draft.block(TidyBlock::UngroupedTracks);
                self.push(draft);
            }
        }
    }

    fn game_dirs(&mut self) {
        for (index, dir) in self.walked.game_dirs.iter().enumerate() {
            let name = file_name(&dir.rel);
            self.drafts.push(Some(Draft {
                files: Vec::new(),
                kind: DraftKind::GameDir(index),
                layout: Layout::Folder {
                    name,
                    base: dir.rel.clone(),
                    whole: true,
                },
                block: dir.block.clone(),
            }));
        }
    }

    /// Every other game file is a unit of its own (with its `.sbi`); the rest is ignored.
    fn singles(&mut self) -> Vec<usize> {
        let mut ignored = Vec::new();
        for i in 0..self.walked.files.len() {
            if self.owner[i].is_some() {
                continue;
            }
            let f = &self.walked.files[i];
            let ext = self.ext(i);
            if !is_candidate(&f.rel) || is_executable(&f.rel) || f.size == 0 || ext == "sbi" {
                // `.sbi` files are claimed by their image below, when it exists.
                if ext != "sbi" {
                    ignored.push(i);
                }
                continue;
            }
            let d = self.push(Draft::new(i, DraftKind::File));
            if matches!(ext.as_str(), "chd" | "pbp" | "iso") {
                self.attach_sbi(d, i);
            }
        }
        ignored.extend(
            (0..self.walked.files.len())
                .filter(|&i| self.ext(i) == "sbi" && self.owner[i].is_none()),
        );
        ignored.sort_unstable();
        ignored
    }

    /// `(Disk N of M)` groups of a computer (flat) and `(Disc N)` groups of a disc system grouping its discs (one
    /// folder for the whole group; design: otherwise the scan would see one game per disc), like `resolve_media`.
    fn groups(&mut self) {
        let mut primaries: Vec<PathBuf> = Vec::new();
        let mut draft_of: HashMap<PathBuf, (usize, bool)> = HashMap::new();
        for (d, draft) in self.drafts.iter().enumerate() {
            let Some(draft) = draft else { continue };
            if !matches!(draft.kind, DraftKind::File | DraftKind::Track) || draft.block.is_some() {
                continue;
            }
            let primary = draft.files[0];
            let file = &self.walked.files[primary];
            let ext = self.ext(primary);
            if is_m3u(&ext) {
                continue;
            }
            let computer = draft.layout == Layout::Flat && draft.files.len() == 1 && {
                let systems = systems_for_extension(&ext);
                !systems.is_empty() && systems.iter().all(|s| s.kind == SystemKind::Computer)
            };
            let disc = !computer && is_console_disc(&found_of(file, Vec::new(), false));
            if computer || disc {
                primaries.push(file.abs.clone());
                draft_of.insert(file.abs.clone(), (d, disc));
            }
        }
        let (groups, _) = group_media(&primaries);
        for group in groups {
            let members: Vec<(usize, bool)> = group
                .members
                .iter()
                .filter_map(|m| draft_of.get(m).copied())
                .collect();
            let Some(&(first, disc)) = members.first() else {
                continue;
            };
            let mut files = Vec::new();
            for (d, _) in &members {
                if let Some(draft) = self.drafts[*d].take() {
                    files.extend(draft.files);
                }
            }
            let base = parent_rel(&self.walked.files[files[0]].rel);
            let media = group
                .members
                .iter()
                .filter_map(|m| m.to_str().map(str::to_owned))
                .collect();
            let layout = if disc {
                Layout::Folder {
                    name: group.name.clone(),
                    base,
                    whole: false,
                }
            } else {
                Layout::Flat
            };
            for f in &files {
                self.owner[*f] = Some(first);
            }
            self.drafts[first] = Some(Draft {
                files,
                kind: DraftKind::Group(media),
                layout,
                block: None,
            });
        }
    }

    fn odd(&mut self) {
        for (rel, block) in &self.walked.blocked {
            self.drafts.push(Some(Draft {
                files: Vec::new(),
                kind: DraftKind::Odd(rel.clone()),
                layout: Layout::Flat,
                block: Some(block.clone()),
            }));
        }
    }
}

/// Entries of a `.m3u` (absolute), `Err(entry)` for a refused one (`..`, drive prefix), `None` if unreadable.
fn read_playlist(path: &Path) -> Option<Result<Vec<PathBuf>, String>> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_M3U_SIZE + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_M3U_SIZE {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    let dir = path.parent().unwrap_or(Path::new(""));
    Some(parse_m3u(&text, dir).map_err(|message| {
        message
            .rsplit_once(" : ")
            .map_or(message.clone(), |(_, entry)| entry.to_owned())
    }))
}

fn found_of(file: &WalkFile, media: Vec<String>, is_dir: bool) -> Found {
    Found {
        path: file.abs.clone(),
        key: file.abs.to_string_lossy().into_owned(),
        folders: Vec::new(),
        file_size: file.size,
        mtime: file.mtime,
        media,
        is_dir,
    }
}

/// Every unit's draft once grouping is done, in walk order of their primary entry.
fn drafts(walked: &Walked, root: &Path) -> (Vec<Draft>, Vec<usize>) {
    let mut b = Builder::new(walked, root);
    b.cues();
    b.playlists();
    b.tracks();
    b.game_dirs();
    let ignored = b.singles();
    b.groups();
    b.odd();
    let mut drafts: Vec<Draft> = b.drafts.into_iter().flatten().collect();
    let key = |d: &Draft| match &d.kind {
        DraftKind::GameDir(i) => walked.game_dirs[*i].rel.clone(),
        DraftKind::Odd(rel) => rel.clone(),
        _ => walked.files[d.files[0]].rel.clone(),
    };
    drafts.sort_by_cached_key(key);
    for d in &mut drafts {
        whole_folder(d, walked);
    }
    (drafts, ignored)
}

/// A folder unit whose folder holds nothing else (no other file, link, special file, game folder, leftover) is
/// moved by one rename, under the folder's own name.
fn whole_folder(draft: &mut Draft, walked: &Walked) {
    let Layout::Folder { name, base, whole } = &mut draft.layout else {
        return;
    };
    if *whole || base.as_os_str().is_empty() {
        return;
    }
    let own: HashSet<usize> = draft.files.iter().copied().collect();
    let only_ours = walked
        .files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.rel.starts_with(&*base))
        .all(|(i, _)| own.contains(&i))
        && !walked.others.iter().any(|o| o.starts_with(&*base))
        && !walked.game_dirs.iter().any(|g| g.rel.starts_with(&*base));
    if only_ours {
        *name = file_name(base);
        *whole = true;
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Classification

struct Classified {
    system: Option<String>,
    status: TidyStatus,
    candidates: Vec<String>,
    title: Option<String>,
}

impl Classified {
    fn sure(system: &str, reason: TidyReason, title: String) -> Self {
        Classified {
            system: Some(system.to_owned()),
            status: TidyStatus::Sure(reason),
            candidates: Vec::new(),
            title: Some(title),
        }
    }

    fn suggested(system: &str, reason: TidyReason, title: String) -> Self {
        Classified {
            status: TidyStatus::Suggested(reason),
            ..Classified::sure(system, reason, title)
        }
    }

    fn ambiguous(candidates: Vec<String>) -> Self {
        if candidates.is_empty() {
            return Classified::status(TidyStatus::Unknown);
        }
        Classified {
            system: None,
            status: TidyStatus::Ambiguous,
            candidates,
            title: None,
        }
    }

    fn status(status: TidyStatus) -> Self {
        Classified {
            system: None,
            status,
            candidates: Vec::new(),
            title: None,
        }
    }
}

fn ids(systems: &[&SystemInfo]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in systems {
        if !out.contains(&s.id) {
            out.push(s.id.clone());
        }
    }
    out
}

struct Classifier<'c> {
    conn: &'c Connection,
    names: NameIndex,
    discs: DiscIndex,
    max: u64,
}

impl Classifier<'_> {
    /// Candidates whose DAT names the stem (ADR 0023 §4.3 rule 4): `Suggested(DatName)` when exactly one.
    fn by_name(
        &mut self,
        candidates: &[&'static SystemInfo],
        stem: &str,
        candidate_ids: Vec<String>,
    ) -> Result<Classified, LibraryError> {
        let mut hits = Vec::new();
        for c in candidates {
            let id = identify(self.conn, &mut self.names, &[c], None, stem)?;
            if id.how == Identification::Name {
                hits.push(id);
            }
        }
        Ok(match hits.as_slice() {
            [one] => Classified::suggested(&one.system, TidyReason::DatName, one.title.clone()),
            _ => Classified::ambiguous(candidate_ids),
        })
    }

    fn classify(
        &mut self,
        found: &Found,
        kind: &DraftKind,
        cancel: &AtomicBool,
    ) -> Result<Option<(Classified, bool)>, LibraryError> {
        let ext = if found.is_dir {
            String::new()
        } else {
            lower_ext(&found.path).unwrap_or_default()
        };
        let analysis = match scan::analyze(found, self.max, cancel, &self.discs) {
            Ok(a) => a,
            Err(Failure::Cancelled) => return Err(LibraryError::Cancelled),
            Err(Failure::Empty) => return Ok(None),
            Err(Failure::MixedArchive) => {
                return Ok(Some((Classified::status(TidyStatus::Unknown), false)));
            }
            Err(Failure::Unclassified) => {
                // Folder-only extension outside its folder (`.chd`, `.cue`, `.hdf`…): the candidates are every system
                // accepting it, folder-only ones included; a single one is rule 5 (extension of one system).
                let all = systems_for_extension(&ext);
                let c = match all.as_slice() {
                    [one] => Classified::sure(&one.id, TidyReason::Extension, stem(&found.path)),
                    _ => Classified::ambiguous(ids(&all)),
                };
                return Ok(Some((c, false)));
            }
            Err(Failure::Message(m)) => {
                let status = if m == ZIPPED_DISC {
                    TidyStatus::Blocked(TidyBlock::DiscInArchive)
                } else if m == SEGA_TRACK_ALONE || m == AUDIO_TRACK_ALONE {
                    TidyStatus::Blocked(TidyBlock::UngroupedTracks)
                } else {
                    tracing::warn!("tidy: cannot analyse {}: {m}", found.path.display());
                    TidyStatus::Unknown
                };
                return Ok(Some((Classified::status(status), false)));
            }
        };
        let hashed = analysis.hashes.is_some();
        let resolved = scan::resolve(self.conn, &mut self.names, &analysis)?;
        let by_dat =
            |how: Identification| matches!(how, Identification::Sha1 | Identification::Crc32Size);
        let grouped = matches!(kind, DraftKind::Group(_) | DraftKind::Playlist(_));
        let title = |t: String| if grouped { strip_media_marker(&t) } else { t };
        let classified = match &analysis.rule {
            Rule::Dos { system, .. } => {
                let title = resolved.map_or_else(|| analysis.stem.clone(), |(id, _)| id.title);
                if found.is_dir {
                    Classified::suggested(&system.id, TidyReason::DosFolder, title)
                } else if ext == "zip" {
                    Classified::suggested(&system.id, TidyReason::ArchiveContent, title)
                } else {
                    Classified::sure(&system.id, TidyReason::Extension, title)
                }
            }
            Rule::Disc {
                system, matched, ..
            } => {
                let title =
                    title(resolved.map_or_else(|| analysis.stem.clone(), |(id, _)| id.title));
                match matched {
                    Some((_, Identification::Crc32Size)) => {
                        Classified::sure(&system.id, TidyReason::DatHash, title)
                    }
                    // A `.chd` is never read: its system came from its name in exactly one Redump DAT.
                    _ if ext == "chd" => {
                        Classified::suggested(&system.id, TidyReason::DatName, title)
                    }
                    _ if ext == "pbp" => Classified::sure(&system.id, TidyReason::Extension, title),
                    _ => Classified::sure(&system.id, TidyReason::Header, title),
                }
            }
            Rule::Identify { candidates, .. } => {
                let Some((id, _)) = resolved else {
                    return Ok(Some((Classified::status(TidyStatus::Unknown), hashed)));
                };
                if by_dat(id.how) {
                    Classified::sure(&id.system, TidyReason::DatHash, title(id.title))
                } else if candidates.len() > 1 {
                    let stem = analysis.stem.clone();
                    let candidate_ids = if ext == "zip" {
                        ids(candidates)
                    } else {
                        ids(&systems_for_extension(&ext))
                    };
                    self.by_name(candidates, &stem, candidate_ids)?
                } else {
                    let reason = if ext == "zip" {
                        TidyReason::ArchiveContent
                    } else if ext == "bin" && !self.bin_header(found) {
                        // `.bin` is shared by BIOS files and stray tracks: without a Sega header, only suggested.
                        return Ok(Some((
                            Classified::suggested(
                                &id.system,
                                TidyReason::Extension,
                                title(id.title),
                            ),
                            hashed,
                        )));
                    } else if ext == "bin" {
                        TidyReason::Header
                    } else {
                        TidyReason::Extension
                    };
                    Classified::sure(&id.system, reason, title(id.title))
                }
            }
            Rule::ByDat(candidates) => match resolved {
                Some((id, _)) => Classified::sure(&id.system, TidyReason::DatHash, title(id.title)),
                None => {
                    let stem = analysis.stem.clone();
                    let candidate_ids = if ext == "zip" {
                        ids(candidates)
                    } else {
                        ids(&systems_for_extension(&ext))
                    };
                    self.by_name(candidates, &stem, candidate_ids)?
                }
            },
            Rule::Playlist { entries, .. } => match resolved {
                Some((id, _)) => {
                    let reason = if entries.iter().any(|e| needs_hashes(&e.attribution)) {
                        TidyReason::DatHash
                    } else if found.media.iter().any(|m| {
                        lower_ext(Path::new(m))
                            .is_some_and(|e| scan::is_disc_image(&e) || e == "bin")
                    }) {
                        TidyReason::Header
                    } else {
                        TidyReason::Extension
                    };
                    Classified::sure(&id.system, reason, title(id.title))
                }
                None => Classified::ambiguous(playlist_candidates(found, entries)),
            },
        };
        Ok(Some((classified, hashed)))
    }

    fn bin_header(&self, found: &Found) -> bool {
        probe_cached(&found.path).is_some_and(|p| p.sega_header || p.kind == BinKind::Sega32x)
    }
}

/// Systems accepting `.m3u` and the extension of every entry (all `.m3u` systems when none does).
fn playlist_candidates(found: &Found, entries: &[scan::EntryCheck]) -> Vec<String> {
    let all = systems_for_extension("m3u");
    // A disc entry attributed by its header names its system: the playlist goes with it.
    let headed: Vec<&SystemInfo> = entries
        .iter()
        .filter_map(|e| match &e.attribution {
            Attribution::Candidates(c) if c.len() == 1 => Some(c[0]),
            _ => None,
        })
        .collect();
    if !headed.is_empty() {
        return ids(&headed);
    }
    let fitting: Vec<&SystemInfo> = all
        .iter()
        .copied()
        .filter(|s| {
            found
                .media
                .iter()
                .all(|m| lower_ext(Path::new(m)).is_some_and(|e| s.extensions.contains(&e)))
        })
        .collect();
    if fitting.is_empty() {
        ids(&all)
    } else {
        ids(&fitting)
    }
}

/// `(id, system, title)` of the library row stored at `key`.
fn known_row(conn: &Connection, key: &str) -> Result<Option<(i64, String, String)>, LibraryError> {
    conn.prepare_cached("SELECT id, system, title FROM game WHERE path = ?1")
        .map_err(db_error)?
        .query_row([key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()
        .map_err(db_error)
}

// ---------------------------------------------------------------------------------------------------------------
// Plan

pub(super) fn run(
    conn: &Connection,
    options: &TidyOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<TidyPlan, LibraryError> {
    scan::clear_probes();
    let root = options.roms_root();
    let root_dev = fs::metadata(root).ok().as_ref().and_then(dev_of);
    let walked = walk(options, cancel)?;
    let (drafts, ignored) = drafts(&walked, root);
    let mut classifier = Classifier {
        conn,
        names: NameIndex::default(),
        discs: DiscIndex::load(conn)?,
        max: options.max_file_size(),
    };
    let scan_root = normalize_root(options.scan_root());
    let mut state = ScanProgress::default();
    let mut last_emit: Option<Instant> = None;
    let mut units = Vec::new();
    let mut meta = Vec::new();
    let mut ignored_extra = Vec::new();
    for draft in drafts {
        if scan::is_cancelled(cancel) {
            return Err(LibraryError::Cancelled);
        }
        let files: Vec<&WalkFile> = match &draft.kind {
            DraftKind::GameDir(i) => walked.game_dirs[*i].files.iter().collect(),
            DraftKind::Odd(_) => Vec::new(),
            _ => draft.files.iter().map(|&i| &walked.files[i]).collect(),
        };
        let (rel, primary_abs) = match &draft.kind {
            DraftKind::GameDir(i) => {
                let rel = walked.game_dirs[*i].rel.clone();
                let abs = root.join(&rel);
                (rel, abs)
            }
            DraftKind::Odd(rel) => (rel.clone(), root.join(rel)),
            _ => (files[0].rel.clone(), files[0].abs.clone()),
        };
        state.files_seen += 1;
        state.current = Some(primary_abs.clone());
        let extension = match &draft.kind {
            DraftKind::GameDir(_) => String::new(),
            _ => lower_ext(&rel).unwrap_or_default(),
        };
        let default_title = match &draft.kind {
            DraftKind::GameDir(_) | DraftKind::Odd(_) => file_name(&rel),
            DraftKind::Group(_) => match &draft.layout {
                Layout::Folder { name, .. } => name.clone(),
                Layout::Flat => strip_media_marker(&stem(&rel)),
            },
            _ => stem(&rel),
        };
        let mut unit = TidyUnit {
            id: units.len() as u32,
            title: default_title,
            system: None,
            candidates: Vec::new(),
            status: TidyStatus::Unknown,
            moves: Vec::new(),
            whole_dir: None,
            extension: extension.clone(),
            known_game: None,
            system_change: None,
        };
        let mut m = UnitMeta {
            files: files
                .iter()
                .map(|f| Fingerprint {
                    path: f.rel.clone(),
                    size: f.size,
                    mtime: f.mtime,
                })
                .collect(),
            layout: draft.layout.clone(),
            base: TidyStatus::Unknown,
            choosable: false,
            chosen: false,
            row_system: None,
            other_volume: root_dev.is_some() && files.iter().any(|f| f.dev != root_dev),
        };
        if let Some(block) = draft.block.clone() {
            m.base = TidyStatus::Blocked(block);
        } else if let Some((id, system, title)) = scan_root
            .join(&rel)
            .to_str()
            .map(|key| known_row(conn, key))
            .transpose()?
            .flatten()
        {
            // Known game: the system of its row (its saves live under that system's core).
            unit.known_game = Some(GameId(id));
            unit.title = title;
            unit.candidates = systems_for_extension(&extension)
                .iter()
                .filter(|s| s.id != system)
                .map(|s| s.id.clone())
                .collect();
            m.choosable = !unit.candidates.is_empty();
            m.base = TidyStatus::Sure(TidyReason::Library);
            unit.system = Some(system.clone());
            m.row_system = Some(system);
            state.identified += 1;
        } else {
            let (found, kind) = match &draft.kind {
                DraftKind::GameDir(i) => {
                    let dir = &walked.game_dirs[*i];
                    let root_files: Vec<&WalkFile> = dir
                        .files
                        .iter()
                        .filter(|f| parent_rel(&f.rel) == dir.rel)
                        .collect();
                    let mut found = Found {
                        path: primary_abs.clone(),
                        key: primary_abs.to_string_lossy().into_owned(),
                        // Analysed like a folder of `roms/dos/` (design §10.1): the folder rule is the only way in.
                        folders: vec![dos_id()],
                        file_size: root_files.iter().map(|f| f.size).sum(),
                        mtime: root_files.iter().map(|f| f.mtime).max().unwrap_or(0),
                        media: Vec::new(),
                        is_dir: true,
                    };
                    if found.file_size == 0 {
                        found.file_size = 1;
                    }
                    (found, draft.kind.clone())
                }
                DraftKind::Group(media) | DraftKind::Playlist(media) => {
                    (found_of(files[0], media.clone(), false), draft.kind.clone())
                }
                _ => (found_of(files[0], Vec::new(), false), draft.kind.clone()),
            };
            match classifier.classify(&found, &kind, cancel)? {
                None => {
                    ignored_extra.push(rel.clone());
                    continue;
                }
                Some((c, hashed)) => {
                    if hashed {
                        state.hashed += 1;
                    }
                    if matches!(c.status, TidyStatus::Sure(_) | TidyStatus::Suggested(_)) {
                        state.identified += 1;
                    }
                    m.choosable = c.status == TidyStatus::Ambiguous;
                    m.base = c.status;
                    unit.system = c.system;
                    unit.candidates = c.candidates;
                    if let Some(t) = c.title.filter(|t| !t.is_empty()) {
                        unit.title = t;
                    }
                }
            }
        }
        units.push(unit);
        meta.push(m);
        if last_emit.is_none_or(|t| t.elapsed() >= PROGRESS_INTERVAL) {
            last_emit = Some(Instant::now());
            progress(&state);
        }
    }
    let mut ignored_paths: Vec<PathBuf> = walked.ignored_paths.clone();
    let mut ignored_count = walked.ignored;
    for i in ignored {
        ignored_count += 1;
        if ignored_paths.len() < MAX_IGNORED_PATHS {
            ignored_paths.push(walked.files[i].rel.clone());
        }
    }
    for rel in ignored_extra {
        ignored_count += 1;
        if ignored_paths.len() < MAX_IGNORED_PATHS {
            ignored_paths.push(rel);
        }
    }
    let mut plan = TidyPlan {
        units,
        already_tidy: walked.already_tidy,
        ignored: ignored_count,
        ignored_paths,
        meta,
        context: PlanContext {
            root: root.to_path_buf(),
            root_dev,
            max_file_size: options.max_file_size(),
        },
        crc_cache: CrcCache::default(),
    };
    plan.place_all();
    state.current = None;
    progress(&state);
    Ok(plan)
}

fn dos_id() -> String {
    crate::systems()
        .iter()
        .find(|s| s.folder_games)
        .map_or_else(|| "dos".to_owned(), |s| s.id.clone())
}

// ---------------------------------------------------------------------------------------------------------------
// Destinations and checks

/// Destination paths already taken by the plan, lower-cased (`/`-joined): `(unit, first file of the claimant)`.
type Claims = HashMap<String, (u32, Option<Fingerprint>)>;

/// The fold of the journal's « same place » rule (RV1 I1): one function for both.
fn claim_key(path: &Path) -> String {
    fold_path(path)
}

/// Lower-cased names of the folders read during one placement pass.
#[derive(Default)]
struct Listings(HashMap<PathBuf, Option<HashMap<String, OsString>>>);

impl Listings {
    /// The on-disk entry of `rel` (case-insensitive), as a path relative to the root.
    fn existing(&mut self, root: &Path, rel: &Path) -> Option<PathBuf> {
        let parent = parent_rel(rel);
        let names = self.0.entry(parent.clone()).or_insert_with(|| {
            fs::read_dir(root.join(&parent)).ok().map(|it| {
                it.filter_map(Result::ok)
                    .map(|e| (fold_path(Path::new(&e.file_name())), e.file_name()))
                    .collect()
            })
        });
        let wanted = fold_path(Path::new(rel.file_name()?));
        names
            .as_ref()?
            .get(&wanted)
            .map(|actual| parent.join(actual))
    }
}

pub(super) fn crc_of(path: &Path) -> Option<u32> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hasher.finalize())
}

fn destinations(meta: &UnitMeta, system: &str) -> (Vec<TidyMove>, Option<TidyMove>) {
    let sys = PathBuf::from(system);
    match &meta.layout {
        Layout::Flat => (
            meta.files
                .iter()
                .map(|f| TidyMove {
                    from: f.path.clone(),
                    to: sys.join(f.path.file_name().unwrap_or_default()),
                    size: f.size,
                })
                .collect(),
            None,
        ),
        Layout::Folder { name, base, whole } => {
            let folder = sys.join(name);
            let moves = meta
                .files
                .iter()
                .map(|f| TidyMove {
                    from: f.path.clone(),
                    to: folder.join(f.path.strip_prefix(base).unwrap_or(&f.path)),
                    size: f.size,
                })
                .collect();
            let whole = whole.then(|| TidyMove {
                from: base.clone(),
                to: folder.clone(),
                size: meta.files.iter().map(|f| f.size).sum(),
            });
            (moves, whole)
        }
    }
}

/// Characters of the destination as Windows sees it (`\\?\` prefix not counted).
fn windows_length(root: &Path, to: &Path) -> usize {
    let root = root.to_string_lossy();
    let root = root.strip_prefix(r"\\?\").unwrap_or(&root);
    root.chars().count() + 1 + to.to_string_lossy().chars().count()
}

impl TidyPlan {
    /// `<D>` of a unit assembled under `<D>.emu-rust-part` (folder layout, not moved whole).
    pub(super) fn assembly_dir(&self, index: usize) -> Option<PathBuf> {
        match (&self.meta[index].layout, &self.units[index].system) {
            (
                Layout::Folder {
                    name, whole: false, ..
                },
                Some(system),
            ) => Some(PathBuf::from(system).join(name)),
            _ => None,
        }
    }

    pub(super) fn set_system(&mut self, unit: u32, system: &str) -> Result<(), LibraryError> {
        let refused = || LibraryError::TidyChoice {
            unit,
            system: system.to_owned(),
        };
        let index = self
            .units
            .iter()
            .position(|u| u.id == unit)
            .ok_or_else(refused)?;
        let (u, m) = (&mut self.units[index], &mut self.meta[index]);
        let row = m.row_system.clone();
        if !m.choosable
            || !(u.candidates.iter().any(|c| c == system) || row.as_deref() == Some(system))
        {
            return Err(refused());
        }
        u.system = Some(system.to_owned());
        match row {
            Some(row) => {
                u.system_change = (row != system).then(|| (row, system.to_owned()));
                m.chosen = u.system_change.is_some();
            }
            None => m.chosen = true,
        }
        Ok(())
    }

    /// Recomputes every destination and check: the analysed units claim their destinations first (walk order),
    /// then the units whose system the user chose, so a choice never blocks an already planned unit.
    pub(super) fn place_all(&mut self) {
        let mut claims = Claims::new();
        let mut listings = Listings::default();
        for chosen in [false, true] {
            for index in 0..self.units.len() {
                if self.meta[index].chosen == chosen {
                    place(
                        &mut self.units[index],
                        &self.meta[index],
                        &self.context,
                        &mut claims,
                        &mut listings,
                        &mut self.crc_cache,
                    );
                }
            }
        }
    }
}

fn place(
    unit: &mut TidyUnit,
    meta: &UnitMeta,
    context: &PlanContext,
    claims: &mut Claims,
    listings: &mut Listings,
    crcs: &mut CrcCache,
) {
    unit.moves.clear();
    unit.whole_dir = None;
    unit.status = meta.base.clone();
    let movable = matches!(
        meta.base,
        TidyStatus::Sure(_) | TidyStatus::Suggested(_) | TidyStatus::Ambiguous
    );
    let Some(system) = unit.system.clone().filter(|_| movable) else {
        return;
    };
    let (moves, whole) = destinations(meta, &system);
    unit.moves = moves;
    unit.whole_dir = whole;
    if let Some(block) = check(unit, meta, context, claims, listings, crcs, &system) {
        unit.status = TidyStatus::Blocked(block);
    }
}

fn check(
    unit: &TidyUnit,
    meta: &UnitMeta,
    context: &PlanContext,
    claims: &mut Claims,
    listings: &mut Listings,
    crcs: &mut CrcCache,
    system: &str,
) -> Option<TidyBlock> {
    let root = &context.root;
    let max = context.max_file_size;
    let mut same_file = |a: &Path, b: &Path| crcs.same_file(a, b, max);
    if meta.other_volume {
        return Some(TidyBlock::OtherVolume);
    }
    match fs::symlink_metadata(root.join(system)) {
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
            return Some(TidyBlock::DestinationNotPlainDir {
                dir: PathBuf::from(system),
            });
        }
        Ok(m) if context.root_dev.is_some() && dev_of(&m) != context.root_dev => {
            return Some(TidyBlock::OtherVolume);
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            return Some(TidyBlock::DestinationNotPlainDir {
                dir: PathBuf::from(system),
            });
        }
    }
    // Folders this unit would create below `roms/<id>/`: a system id among them would pin its files elsewhere.
    let created = unit
        .whole_dir
        .iter()
        .map(|w| w.to.as_path())
        .chain(unit.moves.iter().filter_map(|m| m.to.parent()));
    for folder in created {
        for c in folder.components().skip(1) {
            let name = c.as_os_str().to_string_lossy();
            if is_system_id(&name) {
                return Some(TidyBlock::FolderNamedLikeSystem {
                    name: name.into_owned(),
                });
            }
        }
    }
    if cfg!(windows)
        && unit
            .moves
            .iter()
            .any(|m| windows_length(root, &m.to) > MAX_WINDOWS_PATH)
    {
        return Some(TidyBlock::PathTooLong);
    }
    // A move that only changes case or Unicode form names one file on a case-insensitive volume: the journal
    // refuses it (RV1 I1), the plan never produces it.
    if let Some(same) = unit
        .moves
        .iter()
        .chain(unit.whole_dir.iter())
        .find(|m| same_place(&m.from, &m.to))
    {
        return Some(TidyBlock::Conflict {
            existing: same.from.clone(),
        });
    }
    let folder = match &meta.layout {
        Layout::Folder { name, .. } => Some(PathBuf::from(system).join(name)),
        Layout::Flat => None,
    };
    let targets: Vec<(PathBuf, Option<&TidyMove>)> = match &folder {
        Some(f) => vec![(f.clone(), None)],
        None => unit.moves.iter().map(|m| (m.to.clone(), Some(m))).collect(),
    };
    for (target, mv) in &targets {
        if let Some(existing) = listings.existing(root, target) {
            let duplicate = match mv {
                Some(mv) => same_file(&root.join(&mv.from), &root.join(&existing)),
                None => {
                    !unit.moves.is_empty()
                        && unit.moves.iter().all(|m| {
                            let inner = m.to.strip_prefix(target).unwrap_or(&m.to);
                            same_file(&root.join(&m.from), &root.join(&existing).join(inner))
                        })
                }
            };
            return Some(if duplicate {
                TidyBlock::Duplicate { existing }
            } else {
                TidyBlock::Conflict { existing }
            });
        }
        if let Some((_, first)) = claims.get(&claim_key(target)) {
            let duplicate = match (mv, first) {
                (Some(mv), Some(first)) => {
                    targets.len() == 1 && same_file(&root.join(&mv.from), &root.join(&first.path))
                }
                _ => false,
            };
            let existing = target.clone();
            return Some(if duplicate {
                TidyBlock::Duplicate { existing }
            } else {
                TidyBlock::Conflict { existing }
            });
        }
    }
    for (target, _) in targets {
        claims.insert(claim_key(&target), (unit.id, meta.files.first().cloned()));
    }
    None
}

/// Components of a relative path that the plan produces: never absolute, never `..`.
#[cfg(test)]
pub(super) fn is_plain_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}
