//! Folder scan (ADR 0014 §1): walk, hash (zip entries included), identify, store.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use rusqlite::{Connection, params};
use zip::ZipArchive;

use crate::db::{
    Identified, NameIndex, dat_match, db_error, extension_of, identification_text, identify,
    identify_dos, load_media, sha1_from, to_i64,
};
use crate::dos::DosFile;
use crate::media::{parse_m3u, strip_media_marker};
use crate::systems::{Attribution, attribute, systems};
use crate::{
    DISC_EXTENSIONS, DiscFormat, Identification, LibraryError, MAX_HASH_SIZE, RomHashes,
    SystemInfo, SystemKind, cue_references, disc_info, group_media, hash_reader,
    systems_for_extension,
};

/// Folders are walked without following directory symlinks. A path component equal to a system `id`
/// (`roms/gb/…`) decides between systems sharing an extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    pub roots: Vec<PathBuf>,
    pub max_depth: usize,
    /// Files (or zip entries) above this size are recorded but not hashed.
    pub max_file_size: u64,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            max_depth: 8,
            max_file_size: crate::MAX_HASH_SIZE,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub files_seen: u64,
    pub hashed: u64,
    pub identified: u64,
    pub current: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub added: u64,
    pub updated: u64,
    /// Unchanged path, size and mtime: not hashed again.
    pub unchanged: u64,
    pub removed: u64,
    /// Empty files (0 byte) and archives holding only empty entries: not listed, nothing to launch.
    pub ignored_empty: u64,
    /// Files of a shared extension (`.ipf`, `.m3u`…) that no rule attributes to a system (design v3.0 §3.5.2):
    /// not listed as games, reported with the advice to move them under `roms/<id>/`. Their paths are in
    /// `unclassified_paths`.
    pub unclassified: u64,
    /// At most `MAX_UNCLASSIFIED_PATHS` of them, in scan order.
    pub unclassified_paths: Vec<PathBuf>,
    /// Roots that did not exist: their games are kept (unmounted drive), not removed.
    pub missing_roots: Vec<PathBuf>,
    /// Unreadable files and archives (path + French message), the scan goes on.
    pub errors: Vec<(PathBuf, String)>,
    /// Executables directly in a `folder_games` root (`roms/dos/`): not games (design v4.0 §3.4).
    pub ignored_loose_executables: u64,
    /// Files referenced by a `.cue` and hidden from every system (CD tracks).
    pub hidden_tracks: u64,
    /// Archives holding images of two different computers (`.dsk` and `.d64`): ignored (design v5.0 §3.4).
    pub ignored_mixed_archives: u64,
}

/// Bound of [`ScanReport::unclassified_paths`] (the count itself is not bounded).
pub const MAX_UNCLASSIFIED_PATHS: usize = 200;

/// Archives listing more entries are refused before the central directory is parsed (same bound as emu-runtime).
const MAX_ARCHIVE_ENTRIES: u64 = 4096;
/// Twice the hash limit: a stored entry right at the limit, plus headers, still fits (same rule as emu-runtime).
const MAX_ARCHIVE_SIZE: u64 = 2 * MAX_HASH_SIZE;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// Files hashed before their results are written. Hashing happens outside any transaction; the write transaction
/// only stores the results, so the UI connection is never blocked for the duration of a hash.
const BATCH: usize = 128;
/// Results are also written at least this often, so a cancellation keeps the work of a slow drive.
const BATCH_TIME: Duration = Duration::from_secs(2);

struct Found {
    path: PathBuf,
    /// UTF-8 form of `path` (stored in the base).
    key: String,
    /// Path components (root folder name included) checked against system ids.
    folders: Vec<String>,
    file_size: u64,
    mtime: i64,
    /// Ordered disk images of a multi-disk game (a `(Disk N of M)` group or a `.m3u`); empty otherwise.
    media: Vec<String>,
    /// An unpacked game folder (`path` is the folder; `file_size` / `mtime` summarise its root).
    is_dir: bool,
}

struct Existing {
    id: i64,
    file_size: i64,
    mtime: i64,
    sha1: Option<[u8; 20]>,
    entry: Option<String>,
    system: String,
    pinned: bool,
    /// Stored identification name (`sha1`, `crc32_size`, `name`, `unknown`).
    identification: String,
    media: Vec<String>,
    is_dir: bool,
}

/// How the system of an analysed file is decided once the DATs can be read (design v3.0 §3.5.2).
enum Rule {
    /// Identification among `candidates`, the first one when nothing matches. `pinned`: the folder rule chose.
    Identify {
        candidates: Vec<&'static SystemInfo>,
        pinned: bool,
    },
    /// Shared extension: a DAT hash match in exactly one candidate, else unclassified.
    ByDat(Vec<&'static SystemInfo>),
    /// `.m3u`: the system of its entries, else the folder rule of the playlist, else unclassified.
    Playlist {
        entries: Vec<EntryCheck>,
        folder: Option<&'static SystemInfo>,
    },
    /// Game of a `folder_games` system (DOS): identified from the lines of its file list, else by name.
    /// `files` is empty when only the name counts (disc images, big images).
    Dos {
        files: Vec<DosFile>,
        system: &'static SystemInfo,
        pinned: bool,
    },
}

/// One image listed by a `.m3u`, hashed only when the DATs may be needed to attribute it.
struct EntryCheck {
    attribution: Attribution,
    hashes: Option<RomHashes>,
}

struct Analysis {
    entry: Option<String>,
    size: u64,
    hashes: Option<RomHashes>,
    rule: Rule,
    stem: String,
}

enum Failure {
    Cancelled,
    /// Archive whose only recognised entries are empty: ignored, not an error.
    Empty,
    /// No system takes the file without a DAT match (folder-only extension outside its folder).
    Unclassified,
    /// Archive with images of two computers: ignored and counted, not an error.
    MixedArchive,
    /// French message reported in `ScanReport::errors`.
    Message(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Message(message)
    }
}

fn is_cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

/// Fails every read once `cancel` is set: hashing a large file (or a zip entry) stops within one buffer.
struct CancelRead<'c, R> {
    inner: R,
    cancel: &'c AtomicBool,
}

impl<R: Read> Read for CancelRead<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if is_cancelled(self.cancel) {
            // Not `Interrupted`: `hash_reader` retries those.
            return Err(io::Error::other("scan annulé"));
        }
        self.inner.read(buf)
    }
}

impl<R: Seek> Seek for CancelRead<'_, R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// Opens `path` and checks that it is still a regular file: it may have been replaced since the walk. On Unix the
/// open is non-blocking, so a FIFO swapped in does not hang the scan (no effect on regular files).
fn open_regular(path: &Path) -> Result<File, String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("pas un fichier ordinaire".to_owned());
    }
    Ok(file)
}

fn mtime_of(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Rebuilds the root from its components: stored paths (the game keys) then have one spelling per file whatever
/// the root's spelling (`/` on Windows, `.` components, trailing separator). Without it, a root typed
/// `C:\x\gb/roms` stores `C:\x\gb/roms\a.gb`, which a scan from `C:\x\gb` sees as another file.
fn normalize_root(root: &Path) -> PathBuf {
    root.components().collect()
}

fn lower_ext(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_ascii_lowercase())
}

fn is_candidate(path: &Path) -> bool {
    lower_ext(path).is_some_and(|e| e == "zip" || !systems_for_extension(&e).is_empty())
}

/// Extensions of the programs that make a folder a DOS game.
const EXECUTABLE_EXTENSIONS: &[&str] = &["exe", "com", "bat"];
/// Disc-like images of `dos` never hashed (like `DISC_EXTENSIONS`): identified by name.
const NAME_ONLY_EXTENSIONS: &[&str] = &["ins", "vhd", "jrc"];
/// Files of a game folder (or a lone image) hashed for the DOS DAT: bounds of design v4.0 §3.4.
const MAX_DOS_FILES: usize = 64;
const MAX_DOS_FILE_SIZE: u64 = 16 * 1024 * 1024;

fn is_executable(path: &Path) -> bool {
    lower_ext(path).is_some_and(|e| EXECUTABLE_EXTENSIONS.contains(&e.as_str()))
}

fn is_m3u(ext: &str) -> bool {
    ext == "m3u" || ext == "m3u8"
}

/// Path components of `dir` below `base` (the parent of the scan root, so the root's own name is included).
fn folders_of(base: &Path, dir: &Path) -> Vec<String> {
    dir.strip_prefix(base)
        .map(|rel| {
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The `folder_games` system whose id is a component of `folders` (any position).
fn folder_games_system(folders: &[String]) -> Option<&'static SystemInfo> {
    systems()
        .iter()
        .find(|s| s.folder_games && folders.contains(&s.id))
}

/// `Some(system)` when `folders` (those of a directory) end strictly below a `folder_games` folder: the
/// directory is then a game when it holds an executable.
fn game_folder_system(folders: &[String]) -> Option<&'static SystemInfo> {
    let (_, parents) = folders.split_last()?;
    folder_games_system(parents)
}

fn entries_have_executable(entries: &[fs::DirEntry]) -> bool {
    entries.iter().any(|e| {
        // Links are ignored (not followed), like the runtime snapshot.
        is_executable(&e.path()) && e.file_type().is_ok_and(|t| t.is_file())
    })
}

struct Walk<'a> {
    report: &'a mut ScanReport,
    /// Prefixes whose stored games are kept even if not seen (missing root, unreadable folder).
    keep: &'a mut Vec<PathBuf>,
    seen: &'a mut HashSet<String>,
    found: &'a mut Vec<Found>,
}

impl Walk<'_> {
    /// Records the folder `dir` as one game of `system` (its files are not walked). `entries` are its entries.
    fn game_folder(
        &mut self,
        dir: &Path,
        dir_folders: &[String],
        system: &'static SystemInfo,
        entries: &[fs::DirEntry],
    ) {
        let Some(key) = dir.to_str().map(str::to_owned) else {
            tracing::warn!("non UTF-8 path ignored: {}", dir.display());
            self.report
                .errors
                .push((dir.to_path_buf(), "chemin non UTF-8 ignoré".to_owned()));
            return;
        };
        // The folder has no size of its own: the sum of the files of its root and the newest mtime stand for it,
        // which is enough to notice a replaced executable or data file (not a change deeper down).
        let mut file_size = 0u64;
        let mut mtime = fs::metadata(dir).map_or(0, |m| mtime_of(&m));
        for entry in entries {
            if entry.file_type().is_ok_and(|t| t.is_file())
                && let Ok(meta) = entry.metadata()
            {
                file_size = file_size.saturating_add(meta.len());
                mtime = mtime.max(mtime_of(&meta));
            }
        }
        if file_size == 0 {
            self.report.ignored_empty += 1;
            return;
        }
        if !self.seen.insert(key.clone()) {
            return;
        }
        debug_assert!(system.folder_games);
        self.found.push(Found {
            path: dir.to_path_buf(),
            key,
            // Like a file: the folders above it.
            folders: dir_folders[..dir_folders.len().saturating_sub(1)].to_vec(),
            file_size,
            mtime,
            media: Vec::new(),
            is_dir: true,
        });
    }

    fn root(
        &mut self,
        root: &Path,
        max_depth: usize,
        cancel: &AtomicBool,
    ) -> Result<(), LibraryError> {
        let root = &normalize_root(root);
        // The root itself may be a symlink (a mounted drive): it is the only followed directory link.
        if !fs::metadata(root).is_ok_and(|m| m.is_dir()) {
            self.report.missing_roots.push(root.to_path_buf());
            self.keep.push(root.to_path_buf());
            return Ok(());
        }
        let base = root.parent().unwrap_or(root);
        let mut stack = vec![(root.to_path_buf(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if is_cancelled(cancel) {
                return Err(LibraryError::Cancelled);
            }
            let entries = match fs::read_dir(&dir) {
                Ok(it) => it,
                Err(e) => {
                    self.report.errors.push((dir.clone(), e.to_string()));
                    self.keep.push(dir);
                    continue;
                }
            };
            let mut entries: Vec<_> = entries
                .filter_map(|e| match e {
                    Ok(e) => Some(e),
                    Err(err) => {
                        self.report.errors.push((dir.clone(), err.to_string()));
                        self.keep.push(dir.clone());
                        None
                    }
                })
                .collect();
            entries.sort_by_key(|e| e.file_name());
            let dir_folders = folders_of(base, &dir);
            if let Some(system) = game_folder_system(&dir_folders)
                && entries_have_executable(&entries)
            {
                self.game_folder(&dir, &dir_folders, system, &entries);
                continue;
            }
            let games_root = dir_folders
                .last()
                .is_some_and(|last| systems().iter().any(|s| s.folder_games && s.id == *last));
            let mut subdirs = Vec::new();
            for entry in entries {
                let path = entry.path();
                let Ok(kind) = entry.file_type() else {
                    self.keep.push(path);
                    continue;
                };
                let meta = if kind.is_dir() {
                    if depth < max_depth {
                        subdirs.push(path);
                    }
                    continue;
                } else if !is_candidate(&path) {
                    continue;
                } else if kind.is_symlink() {
                    // File links are followed; directory links never are (loops, escaping the root).
                    match fs::metadata(&path) {
                        Ok(m) if m.is_file() => m,
                        _ => continue,
                    }
                } else if kind.is_file() {
                    match entry.metadata() {
                        Ok(m) => m,
                        Err(e) => {
                            self.report.errors.push((path.clone(), e.to_string()));
                            self.keep.push(path);
                            continue;
                        }
                    }
                } else {
                    continue;
                };
                // Executables are games only as the root of a game folder: loose ones in the root of a
                // `folder_games` folder are counted, anywhere else they are not ours.
                if is_executable(&path) {
                    if games_root {
                        self.report.ignored_loose_executables += 1;
                    }
                    continue;
                }
                let Some(key) = path.to_str().map(str::to_owned) else {
                    tracing::warn!("non UTF-8 path ignored: {}", path.display());
                    self.report
                        .errors
                        .push((path, "chemin non UTF-8 ignoré".to_owned()));
                    continue;
                };
                if meta.len() == 0 {
                    self.report.ignored_empty += 1;
                    continue;
                }
                if !self.seen.insert(key.clone()) {
                    continue;
                }
                let folders = path
                    .parent()
                    .and_then(|p| p.strip_prefix(base).ok())
                    .map(|rel| {
                        rel.components()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                self.found.push(Found {
                    file_size: meta.len(),
                    mtime: mtime_of(&meta),
                    path,
                    key,
                    folders,
                    media: Vec::new(),
                    is_dir: false,
                });
            }
            // Reversed so the stack pops them in name order.
            stack.extend(subdirs.into_iter().rev().map(|d| (d, depth + 1)));
        }
        Ok(())
    }
}

/// Total entry count announced by the end-of-central-directory record (ZIP64 record if the 16-bit field is
/// saturated), read before `ZipArchive::new` allocates for it. `None` when no record is found.
fn declared_entries(file: &mut File, size: u64) -> io::Result<Option<u64>> {
    const EOCD_LEN: u64 = 22;
    const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const LOCATOR_LEN: u64 = 20;
    let window = size.min(EOCD_LEN + 65535);
    let start = size - window;
    file.seek(SeekFrom::Start(start))?;
    let mut tail = vec![0u8; window as usize];
    file.read_exact(&mut tail)?;
    let Some(pos) = tail
        .windows(EOCD_SIG.len())
        .rposition(|w| w == EOCD_SIG)
        .filter(|&p| p + EOCD_LEN as usize <= tail.len())
    else {
        return Ok(None);
    };
    let total = u64::from(u16::from_le_bytes([tail[pos + 10], tail[pos + 11]]));
    if total != 0xFFFF {
        return Ok(Some(total));
    }
    let eocd_at = start + pos as u64;
    if eocd_at < LOCATOR_LEN {
        return Ok(Some(total));
    }
    let mut locator = [0u8; LOCATOR_LEN as usize];
    file.seek(SeekFrom::Start(eocd_at - LOCATOR_LEN))?;
    file.read_exact(&mut locator)?;
    if locator[..4] != [0x50, 0x4b, 0x06, 0x07] {
        return Ok(Some(total));
    }
    let mut offset = [0u8; 8];
    offset.copy_from_slice(&locator[8..16]);
    let offset = u64::from_le_bytes(offset);
    let mut record = [0u8; 56];
    if offset
        .checked_add(record.len() as u64)
        .is_none_or(|end| end > size)
    {
        return Ok(Some(total));
    }
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut record)?;
    if record[..4] != [0x50, 0x4b, 0x06, 0x06] {
        return Ok(Some(total));
    }
    let mut n = [0u8; 8];
    n.copy_from_slice(&record[32..40]);
    Ok(Some(u64::from_le_bytes(n)))
}

/// Last component of an entry name (`/` or `\` separated), control characters replaced; `None` when empty,
/// `.`, `..` or too long. Only used to pick and display the entry: never joined to a path.
fn entry_file_name(raw: &[u8]) -> Option<String> {
    let base = raw.rsplit(|&b| b == b'/' || b == b'\\').next()?;
    let name: String = String::from_utf8_lossy(base)
        .chars()
        .map(|c| if c.is_control() { '_' } else { c })
        .collect();
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." || trimmed.len() > 200 {
        return None;
    }
    Some(trimmed.to_owned())
}

fn hash_error_message(e: &LibraryError) -> String {
    match e {
        LibraryError::Io { source, .. } => source.to_string(),
        LibraryError::TooLarge { .. } => "taille supérieure à la limite".to_owned(),
        other => other.to_string(),
    }
}

/// Largest `.m3u` read (a playlist is a few lines).
const MAX_M3U_SIZE: u64 = 64 * 1024;

/// Existing images listed by a `.m3u` (paths as the core reads them), in playlist order.
fn read_m3u(path: &Path) -> Result<Vec<String>, String> {
    let file = open_regular(path)?;
    let mut text = Vec::new();
    file.take(MAX_M3U_SIZE + 1)
        .read_to_end(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() as u64 > MAX_M3U_SIZE {
        return Err("playlist .m3u trop grosse".to_owned());
    }
    let text = String::from_utf8(text).map_err(|_| "playlist .m3u non UTF-8".to_owned())?;
    let dir = path.parent().unwrap_or(Path::new(""));
    let listed = parse_m3u(&text, dir)?;
    let images: Vec<String> = listed
        .iter()
        .filter(|p| lower_ext(p).is_none_or(|e| !is_m3u(&e)))
        .filter(|p| fs::metadata(p).is_ok_and(|m| m.is_file()))
        .filter_map(|p| p.to_str().map(str::to_owned))
        .collect();
    if images.is_empty() {
        return Err("playlist .m3u sans image présente".to_owned());
    }
    Ok(images)
}

/// Key used to match a `.m3u` entry with a scanned file: NTFS and APFS ignore case, so `DISK1.ST` names `disk1.st`.
fn norm_key(key: &str) -> String {
    if cfg!(windows) {
        // `.m3u` entries joined to the folder may mix `/` and `\`: compare with one separator, case-insensitively.
        key.replace('/', "\\").to_lowercase()
    } else if cfg!(target_os = "macos") {
        key.to_lowercase()
    } else {
        key.to_owned()
    }
}

/// Multi-disk games (design v2.0 §3.4): the files of a valid `.m3u` are listed under the playlist, and the
/// disks of a `(Disk N of M)` group of a computer system under their first disk. The other files of these
/// games are removed from `found` (and `seen`, so a row stored by an earlier scan goes away).
fn resolve_media(found: &mut Vec<Found>, seen: &mut HashSet<String>, report: &mut ScanReport) {
    let mut hidden: HashSet<String> = HashSet::new();
    let mut by_norm: HashMap<String, Vec<String>> = HashMap::new();
    for f in found.iter() {
        by_norm
            .entry(norm_key(&f.key))
            .or_default()
            .push(f.key.clone());
    }
    hide_disc_tracks(found, &by_norm, &mut hidden, report);
    for f in found.iter_mut() {
        if !lower_ext(&f.path).is_some_and(|e| is_m3u(&e)) {
            continue;
        }
        match read_m3u(&f.path) {
            Ok(images) => {
                for image in &images {
                    if let Some(keys) = by_norm.get(&norm_key(image)) {
                        hidden.extend(keys.iter().cloned());
                    }
                }
                f.media = images;
            }
            Err(message) => {
                tracing::warn!("cannot read playlist {}: {message}", f.path.display());
                report.errors.push((f.path.clone(), message));
                hidden.insert(f.key.clone());
            }
        }
    }
    let by_path: HashMap<PathBuf, usize> = found
        .iter()
        .enumerate()
        .filter(|(_, f)| !hidden.contains(&f.key))
        .filter(|(_, f)| {
            lower_ext(&f.path).is_some_and(|e| {
                let systems = systems_for_extension(&e);
                !systems.is_empty() && systems.iter().all(|s| s.kind == SystemKind::Computer)
            })
        })
        .filter(|(_, f)| lower_ext(&f.path).is_some_and(|e| !is_m3u(&e)))
        .map(|(i, f)| (f.path.clone(), i))
        .collect();
    let files: Vec<PathBuf> = found
        .iter()
        .filter(|f| by_path.contains_key(&f.path))
        .map(|f| f.path.clone())
        .collect();
    let (groups, _) = group_media(&files);
    for group in groups {
        let keys: Vec<String> = group
            .members
            .iter()
            .filter_map(|m| m.to_str().map(str::to_owned))
            .collect();
        let Some(&first) = group.members.first().and_then(|m| by_path.get(m)) else {
            continue;
        };
        hidden.extend(keys.iter().skip(1).cloned());
        found[first].media = keys;
    }
    for f in found.iter().filter(|f| hidden.contains(&f.key)) {
        seen.remove(&f.key);
    }
    found.retain(|f| !hidden.contains(&f.key));
}

/// Tracks of a disc are not games (design v2.1 Mega CD §3.2): the files named by a `.cue` are hidden (any
/// extension of the catalog, `.cue` excepted), and a `.bin` left alone whose header is a Sega CD one is a track
/// without its `.cue`, not a Mega Drive cartridge.
fn hide_disc_tracks(
    found: &[Found],
    by_norm: &HashMap<String, Vec<String>>,
    hidden: &mut HashSet<String>,
    report: &mut ScanReport,
) {
    for f in found {
        if lower_ext(&f.path).as_deref() != Some("cue") {
            continue;
        }
        match cue_references(&f.path) {
            Ok(tracks) => {
                for track in &tracks {
                    if lower_ext(track).as_deref() == Some("cue") {
                        continue;
                    }
                    if let Some(keys) = track.to_str().and_then(|t| by_norm.get(&norm_key(t))) {
                        for key in keys {
                            if hidden.insert(key.clone()) {
                                report.hidden_tracks += 1;
                            }
                        }
                    }
                }
            }
            // The `.cue` stays listed: the core reads it itself and decides.
            Err(e) => {
                let message = hash_error_message(&e);
                tracing::warn!("cannot read cue sheet {}: {message}", f.path.display());
                report.errors.push((f.path.clone(), message));
            }
        }
    }
    for f in found {
        if lower_ext(&f.path).as_deref() != Some("bin") || hidden.contains(&f.key) {
            continue;
        }
        if let Ok(Some(info)) = disc_info(&f.path)
            && info.format == DiscFormat::SegaCd
        {
            report.errors.push((
                f.path.clone(),
                "piste de CD sans fichier .cue : ignorée".to_owned(),
            ));
            hidden.insert(f.key.clone());
        }
    }
}

fn header_of(candidates: &[&SystemInfo]) -> Option<crate::HeaderRule> {
    candidates.iter().find_map(|s| s.header)
}

fn rule_of(attribution: Attribution) -> Result<Rule, Failure> {
    match attribution {
        Attribution::Folder(s) => Ok(Rule::Identify {
            candidates: vec![s],
            pinned: true,
        }),
        Attribution::Candidates(candidates) => Ok(Rule::Identify {
            candidates,
            pinned: false,
        }),
        Attribution::ByDat(candidates) => Ok(Rule::ByDat(candidates)),
        Attribution::Unclassified => Err(Failure::Unclassified),
    }
}

/// The attribution needs the DATs (a shared extension, or consoles sharing one).
fn needs_hashes(attribution: &Attribution) -> bool {
    match attribution {
        Attribution::ByDat(_) => true,
        Attribution::Candidates(c) => c.len() > 1,
        Attribution::Folder(_) | Attribution::Unclassified => false,
    }
}

fn is_playlist(found: &Found) -> bool {
    !found.media.is_empty() && lower_ext(&found.path).is_some_and(|e| is_m3u(&e))
}

/// Folders of a playlist entry: those of the playlist, plus the sub-folders between it and the entry.
fn entry_folders(found: &Found, entry: &Path) -> Vec<String> {
    let mut folders = found.folders.clone();
    if let (Some(dir), Some(parent)) = (found.path.parent(), entry.parent())
        && let Ok(rel) = parent.strip_prefix(dir)
    {
        folders.extend(
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned()),
        );
    }
    folders
}

/// The system accepting `.m3u` named by a folder of the playlist.
fn playlist_folder(found: &Found) -> Option<&'static SystemInfo> {
    match attribute(&lower_ext(&found.path)?, &found.folders) {
        Attribution::Folder(s) => Some(s),
        _ => None,
    }
}

/// An unchanged file is not hashed again unless the attribution rules now give another answer (the roots
/// changed, a v1 row whose migrated flag was approximate, a 2.0 `.ipf` outside any folder): the stored system
/// and flag must match the rules.
fn pin_agrees(old: &Existing, found: &Found) -> bool {
    if found.is_dir != old.is_dir {
        return false;
    }
    if found.is_dir {
        return old.pinned
            && folder_games_system(&found.folders).is_some_and(|s| s.id == old.system);
    }
    if is_playlist(found) {
        return playlist_agrees(old, found);
    }
    let Some(ext) = extension_of(&found.path, old.entry.as_deref()) else {
        return true;
    };
    if ext == "zip" {
        // A DOS archive (no launched entry): only the folder rule can have changed.
        return crate::system(&old.system).is_some_and(|s| s.folder_games)
            && old.pinned
                == folder_games_system(&found.folders).is_some_and(|s| s.id == old.system);
    }
    let accepts = |c: &[&SystemInfo]| c.iter().any(|s| s.id == old.system);
    match attribute(&ext, &found.folders) {
        Attribution::Folder(s) => old.pinned && old.system == s.id,
        Attribution::Candidates(c) => !old.pinned && accepts(&c),
        // Kept only while the DATs still attribute it (a DAT import re-identifies and sets `unknown` otherwise).
        Attribution::ByDat(c) => {
            !old.pinned
                && accepts(&c)
                && matches!(old.identification.as_str(), "sha1" | "crc32_size")
        }
        // A disc outside a folder, kept as Mega CD by its header at the last scan.
        Attribution::Unclassified => {
            !old.pinned
                && ext != "chd"
                && DISC_EXTENSIONS.contains(&ext.as_str())
                && old.system == "megacd"
        }
    }
}

/// Without hashing: entries needing the DATs always send the playlist to a new analysis.
fn playlist_agrees(old: &Existing, found: &Found) -> bool {
    let mut systems: Vec<&str> = Vec::new();
    for entry in &found.media {
        let path = Path::new(entry);
        let Some(ext) = lower_ext(path) else {
            continue;
        };
        let attribution = attribute(&ext, &entry_folders(found, path));
        if needs_hashes(&attribution) {
            return false;
        }
        if let Attribution::Folder(s) = &attribution {
            systems.push(&s.id);
        } else if let Attribution::Candidates(c) = &attribution {
            systems.extend(c.first().map(|s| s.id.as_str()));
        }
    }
    systems.sort_unstable();
    systems.dedup();
    let folder = playlist_folder(found);
    let system = match systems.as_slice() {
        [one] => *one,
        _ => match folder {
            Some(f) => f.id.as_str(),
            None => return false,
        },
    };
    old.system == system && old.pinned == folder.is_some_and(|f| f.id == system)
}

/// The `folder_games` system of the catalog (DOS): where `.dosz` and zips holding programs go.
fn dos_system() -> Option<&'static SystemInfo> {
    systems().iter().find(|s| s.folder_games)
}

fn analyze_zip(found: &Found, max: u64, cancel: &AtomicBool) -> Result<Analysis, Failure> {
    // Under a DOS folder, or a `.dosz`: a DOS game whatever the archive holds (design v4.0 §3.4).
    let folder_dos = folder_games_system(&found.folders);
    let dosz = lower_ext(&found.path).as_deref() == Some("dosz");
    // The content of a DOS archive is never read: only its central directory, so no size limit.
    if found.file_size > MAX_ARCHIVE_SIZE && folder_dos.is_none() && !dosz {
        return Err("archive trop grosse".to_owned().into());
    }
    let mut file = open_regular(&found.path)?;
    match declared_entries(&mut file, found.file_size) {
        Ok(Some(n)) if n > MAX_ARCHIVE_ENTRIES => {
            return Err(format!("archive illisible : trop d'entrées ({n})").into());
        }
        Ok(_) => {}
        Err(e) => return Err(e.to_string().into()),
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(BufReader::new(CancelRead {
        inner: file,
        cancel,
    }))
    .map_err(|e| format!("archive illisible : {e}"))?;
    if archive.len() as u64 > MAX_ARCHIVE_ENTRIES {
        return Err("archive illisible : trop d'entrées".to_owned().into());
    }
    // First file entry with a catalog extension, as emu-runtime picks the first accepted entry.
    let mut chosen = None;
    let mut saw_empty = false;
    let mut saw_program = false;
    let mut saw_disc = false;
    let mut dos_files = Vec::new();
    // Computers owning, alone, the extension of a non-empty entry (`.dsk` = cpc, `.d64` = c64).
    let mut computers: HashSet<&'static str> = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index_data(index)
            .map_err(|e| format!("archive illisible : {e}"))?;
        if !entry.is_file() {
            continue;
        }
        let Some(name) = entry_file_name(entry.name_raw()) else {
            continue;
        };
        if entry.size() == 0 {
            saw_empty = true;
        } else {
            dos_files.push(DosFile {
                name: name.clone(),
                crc32: entry.crc32(),
                size: entry.size(),
            });
        }
        let Some(ext) = lower_ext(Path::new(&name)) else {
            continue;
        };
        saw_program |= EXECUTABLE_EXTENSIONS.contains(&ext.as_str());
        saw_disc |= DISC_EXTENSIONS.contains(&ext.as_str());
        let candidates = systems_for_extension(&ext);
        if let [only] = candidates.as_slice()
            && only.kind == SystemKind::Computer
            && entry.size() > 0
        {
            computers.insert(only.id.as_str());
        }
        if chosen.is_some() {
            // Kept walking to see the programs and discs listed after the chosen entry.
            continue;
        }
        if !candidates.is_empty() {
            if entry.size() == 0 {
                continue;
            }
            chosen = Some((index, name, entry.size(), candidates));
        }
    }
    if let Some(system) = folder_dos.or_else(|| (dosz || saw_program).then(dos_system).flatten()) {
        if dos_files.is_empty() {
            return Err(if saw_empty {
                Failure::Empty
            } else {
                "aucun jeu reconnu dans l'archive".to_owned().into()
            });
        }
        return Ok(Analysis {
            entry: None,
            size: found.file_size,
            hashes: None,
            rule: Rule::Dos {
                files: dos_files,
                system,
                pinned: folder_dos.is_some(),
            },
            stem: stem_of(&found.path),
        });
    }
    if computers.len() > 1 {
        return Err(Failure::MixedArchive);
    }
    if saw_disc {
        return Err(
            "Les CD ne se lisent pas dans un zip : décompressez-le ou convertissez-le en CHD."
                .to_owned()
                .into(),
        );
    }
    let Some((index, name, declared, _)) = chosen else {
        return Err(if saw_empty {
            Failure::Empty
        } else {
            "aucun jeu reconnu dans l'archive".to_owned().into()
        });
    };
    let Some(ext) = lower_ext(Path::new(&name)) else {
        return Err("extension inconnue".to_owned().into());
    };
    let rule = rule_of(attribute(&ext, &found.folders))?;
    let hashes = if declared > max {
        None
    } else {
        let entry = archive
            .by_index(index)
            .map_err(|e| format!("archive illisible : {e}"))?;
        // `hash_reader` fails past `declared` bytes: a lying entry (zip bomb) is never read further.
        let reader = entry.take(declared.saturating_add(1));
        Some(
            hash_reader(reader, declared, header_of(rule_systems(&rule)), max)
                .map_err(|e| format!("archive illisible : {}", hash_error_message(&e)))?,
        )
    };
    Ok(Analysis {
        entry: Some(name),
        size: declared,
        hashes,
        rule,
        stem: stem_of(&found.path),
    })
}

fn rule_systems(rule: &Rule) -> &[&'static SystemInfo] {
    match rule {
        Rule::Identify { candidates, .. } | Rule::ByDat(candidates) => candidates,
        Rule::Playlist { .. } => &[],
        Rule::Dos { system, .. } => std::slice::from_ref(system),
    }
}

/// Hashes of a file of `size` bytes (`None` above `max`).
fn hash_file(
    path: &Path,
    size: u64,
    header: Option<crate::HeaderRule>,
    max: u64,
    cancel: &AtomicBool,
) -> Result<Option<RomHashes>, String> {
    if size > max {
        return Ok(None);
    }
    let file = open_regular(path)?;
    hash_reader(
        CancelRead {
            inner: file,
            cancel,
        },
        size,
        header,
        max,
    )
    .map(Some)
    .map_err(|e| hash_error_message(&e))
}

/// The entries of a playlist are hashed only when their attribution needs the DATs; an unreadable entry is
/// left unattributed (the playlist then follows its folder).
fn playlist_rule(found: &Found, max: u64, cancel: &AtomicBool) -> Rule {
    let entries = found
        .media
        .iter()
        .filter_map(|entry| {
            let path = Path::new(entry);
            let attribution = attribute(&lower_ext(path)?, &entry_folders(found, path));
            let hashes = if needs_hashes(&attribution) {
                let size = fs::metadata(path).map(|m| m.len()).ok()?;
                let header = match &attribution {
                    Attribution::ByDat(c) | Attribution::Candidates(c) => header_of(c),
                    _ => None,
                };
                hash_file(path, size, header, max, cancel).ok().flatten()
            } else {
                None
            };
            Some(EntryCheck {
                attribution,
                hashes,
            })
        })
        .collect();
    Rule::Playlist {
        entries,
        folder: playlist_folder(found),
    }
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn analyze(found: &Found, max: u64, cancel: &AtomicBool) -> Result<Analysis, Failure> {
    let result = analyze_file(found, max, cancel);
    // A read failed by `CancelRead` surfaces as an I/O or archive error: the flag tells them apart.
    if result.is_err() && is_cancelled(cancel) {
        return Err(Failure::Cancelled);
    }
    result
}

fn analyze_file(found: &Found, max: u64, cancel: &AtomicBool) -> Result<Analysis, Failure> {
    if found.is_dir {
        return analyze_dir(found, max, cancel);
    }
    let Some(ext) = lower_ext(&found.path) else {
        return Err("extension inconnue".to_owned().into());
    };
    if ext == "zip" || ext == "dosz" {
        return analyze_zip(found, max, cancel);
    }
    // Disc images first (never hashed, probed only), before the generic attribution of design v3.0 §3.5.2.
    if DISC_EXTENSIONS.contains(&ext.as_str()) {
        return analyze_disc(found, &ext);
    }
    let mut rule = if is_playlist(found) {
        playlist_rule(found, max, cancel)
    } else {
        rule_of(attribute(&ext, &found.folders))?
    };
    // A lone DOS file: identified by its name and, for a small image, by its CRC32 (design v4.0 §3.4).
    let dos = match &rule {
        Rule::Identify { candidates, pinned }
            if candidates.len() == 1 && candidates[0].folder_games =>
        {
            Some((candidates[0], *pinned))
        }
        _ => None,
    };
    let limit = if dos.is_some() {
        max.min(MAX_DOS_FILE_SIZE)
    } else {
        max
    };
    let hashes = if NAME_ONLY_EXTENSIONS.contains(&ext.as_str()) {
        None
    } else {
        hash_file(
            &found.path,
            found.file_size,
            header_of(rule_systems(&rule)),
            limit,
            cancel,
        )?
    };
    if let Some((system, pinned)) = dos {
        let files = hashes
            .iter()
            .filter_map(|h| {
                Some(DosFile {
                    name: found.path.file_name()?.to_string_lossy().into_owned(),
                    crc32: h.full.crc32,
                    size: h.full.size,
                })
            })
            .collect();
        rule = Rule::Dos {
            files,
            system,
            pinned,
        };
    }
    Ok(Analysis {
        entry: None,
        size: found.file_size,
        hashes,
        rule,
        stem: stem_of(&found.path),
    })
}

/// A game folder: the DOS DAT is matched on the programs and data files of its root (at most `MAX_DOS_FILES`
/// files of at most `MAX_DOS_FILE_SIZE` bytes, their CRC32 computed here).
fn analyze_dir(found: &Found, max: u64, cancel: &AtomicBool) -> Result<Analysis, Failure> {
    let Some(system) = folder_games_system(&found.folders) else {
        return Err("dossier hors d'un dossier de jeux".to_owned().into());
    };
    let mut names: Vec<(std::ffi::OsString, PathBuf)> = fs::read_dir(&found.path)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| (e.file_name(), e.path()))
        .filter(|(_, path)| {
            is_executable(path) || lower_ext(path).is_some_and(|e| e == "ima" || e == "img")
        })
        .collect();
    names.sort();
    let mut files = Vec::new();
    for (name, path) in names {
        if files.len() >= MAX_DOS_FILES {
            break;
        }
        if is_cancelled(cancel) {
            return Err(Failure::Cancelled);
        }
        let Some(size) = fs::symlink_metadata(&path)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len())
        else {
            continue;
        };
        if size == 0 || size > MAX_DOS_FILE_SIZE {
            continue;
        }
        if let Some(h) = hash_file(&path, size, None, max.min(MAX_DOS_FILE_SIZE), cancel)? {
            files.push(DosFile {
                name: name.to_string_lossy().into_owned(),
                crc32: h.full.crc32,
                size,
            });
        }
    }
    Ok(Analysis {
        entry: None,
        size: found.file_size,
        hashes: None,
        rule: Rule::Dos {
            files,
            system,
            pinned: true,
        },
        stem: found
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    })
}

/// Disc images are handed to the core in place: never hashed (hundreds of MiB), only probed (< 4 KiB read).
/// Under a system folder the folder decides. Elsewhere only a Sega CD header classifies a `.cue` / `.iso`
/// (design v4.0 §10 #7: the extensions are shared with `dos`), anything else is unclassified.
fn analyze_disc(found: &Found, ext: &str) -> Result<Analysis, Failure> {
    let rule = match attribute(ext, &found.folders) {
        Attribution::Folder(s) if s.folder_games => Rule::Dos {
            files: Vec::new(),
            system: s,
            pinned: true,
        },
        Attribution::Folder(s) => Rule::Identify {
            candidates: vec![s],
            pinned: true,
        },
        _ => {
            // A `.chd` is never read; an unreadable `.cue` is reported once, by `hide_disc_tracks`.
            let info = if ext == "chd" {
                None
            } else {
                disc_info(&found.path).ok().flatten()
            };
            let system = info.and_then(|i| match i.format {
                DiscFormat::SegaCd => crate::system("megacd"),
            });
            match system {
                Some(s) => Rule::Identify {
                    candidates: vec![s],
                    pinned: false,
                },
                None => return Err(Failure::Unclassified),
            }
        }
    };
    Ok(Analysis {
        entry: None,
        size: found.file_size,
        hashes: None,
        rule,
        stem: stem_of(&found.path),
    })
}

fn load_existing(conn: &Connection) -> Result<HashMap<String, Existing>, LibraryError> {
    let mut media = load_media(conn, None)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, path, file_size, mtime_unix, sha1, entry, system, pinned, identification, is_dir
             FROM game",
        )
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(1)?,
            Existing {
                id: r.get(0)?,
                file_size: r.get(2)?,
                mtime: r.get(3)?,
                sha1: sha1_from(r.get(4)?),
                entry: r.get(5)?,
                system: r.get(6)?,
                pinned: r.get::<_, i64>(7)? != 0,
                identification: r.get(8)?,
                media: Vec::new(),
                is_dir: r.get::<_, i64>(9)? != 0,
            },
        ))
    })
    .map_err(db_error)?
    .collect::<Result<HashMap<String, Existing>, _>>()
    .map_err(db_error)
    .map(|mut all| {
        for old in all.values_mut() {
            old.media = media
                .remove(&old.id)
                .unwrap_or_default()
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
        }
        all
    })
}

/// Replaces the disk list of the game stored at `key`.
fn store_media(conn: &Connection, key: &str, media: &[String]) -> Result<(), LibraryError> {
    let id: i64 = conn
        .query_row("SELECT id FROM game WHERE path = ?1", [key], |r| r.get(0))
        .map_err(db_error)?;
    conn.execute("DELETE FROM game_media WHERE game_id = ?1", [id])
        .map_err(db_error)?;
    for (idx, path) in media.iter().enumerate() {
        // A disk belongs to one game: it leaves the game that listed it before.
        conn.execute("DELETE FROM game_media WHERE path = ?1", [path])
            .map_err(db_error)?;
        conn.execute(
            "INSERT INTO game_media (game_id, idx, path) VALUES (?1, ?2, ?3)",
            params![id, to_i64(idx as u64), path],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

/// The single system given by the classified entries of a playlist (design v3.0 §3.5.2).
fn playlist_system(
    conn: &Connection,
    names: &mut NameIndex,
    entries: &[EntryCheck],
) -> Result<Option<&'static SystemInfo>, LibraryError> {
    let mut systems: Vec<&'static SystemInfo> = Vec::new();
    for e in entries {
        let system = match &e.attribution {
            Attribution::Folder(s) => Some(*s),
            Attribution::Candidates(c) if c.len() == 1 => Some(c[0]),
            Attribution::Candidates(c) => {
                let id = identify(conn, names, c, e.hashes.as_ref(), "")?;
                c.iter().copied().find(|s| s.id == id.system)
            }
            Attribution::ByDat(c) => dat_match(conn, names, c, e.hashes.as_ref())?
                .and_then(|id| c.iter().copied().find(|s| s.id == id.system)),
            Attribution::Unclassified => None,
        };
        if let Some(s) = system
            && !systems.iter().any(|k| k.id == s.id)
        {
            systems.push(s);
        }
    }
    Ok(match systems.as_slice() {
        [one] => Some(*one),
        _ => None,
    })
}

/// Identification and folder flag of an analysed file; `None` when it is unclassified.
fn resolve(
    conn: &Connection,
    names: &mut NameIndex,
    analysis: &Analysis,
) -> Result<Option<(Identified, bool)>, LibraryError> {
    let hashes = analysis.hashes.as_ref();
    Ok(match &analysis.rule {
        Rule::Identify { candidates, pinned } => Some((
            identify(conn, names, candidates, hashes, &analysis.stem)?,
            *pinned,
        )),
        Rule::ByDat(candidates) => {
            dat_match(conn, names, candidates, hashes)?.map(|id| (id, false))
        }
        Rule::Dos {
            files,
            system,
            pinned,
        } => Some((
            identify_dos(conn, names, system, files, &analysis.stem)?,
            *pinned,
        )),
        Rule::Playlist { entries, folder } => {
            let system = match playlist_system(conn, names, entries)? {
                Some(s) => Some(s),
                None => *folder,
            };
            match system {
                Some(s) => Some((
                    identify(conn, names, &[s], hashes, &analysis.stem)?,
                    folder.is_some_and(|f| f.id == s.id),
                )),
                None => None,
            }
        }
    })
}

enum Stored {
    Identified,
    Unknown,
    /// Not stored; a row of an earlier scan is removed.
    Unclassified,
}

/// Inserts or updates the row of `found`.
fn store(
    conn: &Connection,
    names: &mut NameIndex,
    found: &Found,
    analysis: &Analysis,
    existing: Option<&Existing>,
) -> Result<Stored, LibraryError> {
    let Some((id, pinned)) = resolve(conn, names, analysis)? else {
        conn.execute("DELETE FROM game WHERE path = ?1", [&found.key])
            .map_err(db_error)?;
        return Ok(Stored::Unclassified);
    };
    let title = if found.media.is_empty() {
        id.title
    } else {
        // The DAT names the first disk `Game (Disk 1 of 2)`: the game is the whole set.
        strip_media_marker(&id.title)
    };
    let full = analysis.hashes.map(|h| h.full);
    let headerless = analysis.hashes.and_then(|h| h.headerless);
    conn.execute(
        "INSERT INTO game (path, entry, system, size, file_size, mtime_unix, crc32, sha1, crc32_nh, sha1_nh,
                           title, identification, pinned, is_dir)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT(path) DO UPDATE SET entry = excluded.entry, system = excluded.system, size = excluded.size,
             file_size = excluded.file_size, mtime_unix = excluded.mtime_unix, crc32 = excluded.crc32,
             sha1 = excluded.sha1, crc32_nh = excluded.crc32_nh, sha1_nh = excluded.sha1_nh,
             title = excluded.title, identification = excluded.identification, pinned = excluded.pinned,
             is_dir = excluded.is_dir",
        params![
            found.key,
            analysis.entry,
            id.system,
            to_i64(analysis.size),
            to_i64(found.file_size),
            found.mtime,
            full.map(|h| i64::from(h.crc32)),
            full.as_ref().map(|h| &h.sha1[..]),
            headerless.map(|h| i64::from(h.crc32)),
            headerless.as_ref().map(|h| &h.sha1[..]),
            title,
            identification_text(id.how),
            pinned,
            found.is_dir,
        ],
    )
    .map_err(db_error)?;
    store_media(conn, &found.key, &found.media)?;
    // Covers are cached by content hash: a different content needs a new lookup.
    if let Some(old) = existing
        && old.sha1 != full.map(|h| h.sha1)
    {
        conn.execute("DELETE FROM cover WHERE game = ?1", [old.id])
            .map_err(db_error)?;
    }
    Ok(if id.how == Identification::Unknown {
        Stored::Unknown
    } else {
        Stored::Identified
    })
}

/// Writes the analysed files of `pending` in one short transaction, then empties it.
fn store_batch(
    conn: &mut Connection,
    names: &mut NameIndex,
    existing: &HashMap<String, Existing>,
    pending: &mut Vec<(usize, &Found, Analysis)>,
    state: &mut ScanProgress,
    report: &mut ScanReport,
    unclassified: &mut Vec<(usize, PathBuf)>,
) -> Result<(), LibraryError> {
    if pending.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction().map_err(db_error)?;
    let mut added = 0;
    let mut updated = 0;
    let mut skipped = Vec::new();
    for (index, f, analysis) in pending.iter() {
        let old = existing.get(&f.key);
        match store(&tx, names, f, analysis, old)? {
            Stored::Unclassified => {
                skipped.push((*index, f.path.clone()));
                continue;
            }
            Stored::Identified => state.identified += 1,
            Stored::Unknown => {}
        }
        if old.is_some() {
            updated += 1;
        } else {
            added += 1;
        }
    }
    tx.commit().map_err(db_error)?;
    // Counted after the commit: a failed batch reports nothing as stored.
    report.added += added;
    report.updated += updated;
    unclassified.extend(skipped);
    pending.clear();
    Ok(())
}

pub(crate) fn run(
    conn: &mut Connection,
    options: &ScanOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<ScanReport, LibraryError> {
    let mut report = ScanReport::default();
    let mut keep = Vec::new();
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    {
        let mut walk = Walk {
            report: &mut report,
            keep: &mut keep,
            seen: &mut seen,
            found: &mut found,
        };
        for root in &options.roots {
            walk.root(root, options.max_depth, cancel)?;
        }
    }

    resolve_media(&mut found, &mut seen, &mut report);
    let existing = load_existing(conn)?;
    let mut names = NameIndex::default();
    let mut state = ScanProgress::default();
    let mut last_emit: Option<Instant> = None;
    let mut pending: Vec<(usize, &Found, Analysis)> = Vec::new();
    // (scan order, path): sorted at the end, files rejected before hashing and after a DAT lookup mixed.
    let mut unclassified: Vec<(usize, PathBuf)> = Vec::new();
    let mut batch_start = Instant::now();
    let mut cancelled = false;
    for (index, f) in found.iter().enumerate() {
        if is_cancelled(cancel) {
            cancelled = true;
            break;
        }
        state.files_seen += 1;
        state.current = Some(f.path.clone());
        let old = existing.get(&f.key);
        if old.is_some_and(|o| {
            o.file_size == to_i64(f.file_size)
                && o.mtime == f.mtime
                && o.media == f.media
                && pin_agrees(o, f)
        }) {
            report.unchanged += 1;
        } else {
            // No transaction is open here: a slow read never holds the write lock.
            match analyze(f, options.max_file_size, cancel) {
                Ok(analysis) => {
                    if analysis.hashes.is_some() {
                        state.hashed += 1;
                    }
                    pending.push((index, f, analysis));
                }
                Err(Failure::Cancelled) => {
                    cancelled = true;
                    break;
                }
                Err(Failure::Unclassified) => {
                    unclassified.push((index, f.path.clone()));
                    seen.remove(&f.key);
                }
                Err(Failure::MixedArchive) => {
                    report.ignored_mixed_archives += 1;
                    seen.remove(&f.key);
                }
                Err(Failure::Empty) => {
                    report.ignored_empty += 1;
                    // Not seen: a row stored by an earlier scan is removed.
                    seen.remove(&f.key);
                }
                Err(Failure::Message(message)) => {
                    tracing::warn!("cannot scan {}: {message}", f.path.display());
                    report.errors.push((f.path.clone(), message));
                }
            }
        }
        if pending.len() >= BATCH || (!pending.is_empty() && batch_start.elapsed() >= BATCH_TIME) {
            store_batch(
                conn,
                &mut names,
                &existing,
                &mut pending,
                &mut state,
                &mut report,
                &mut unclassified,
            )?;
            batch_start = Instant::now();
        }
        if last_emit.is_none_or(|t| t.elapsed() >= PROGRESS_INTERVAL) {
            last_emit = Some(Instant::now());
            progress(&state);
        }
    }
    // Work done before a cancellation is kept: the next scan skips it as unchanged.
    store_batch(
        conn,
        &mut names,
        &existing,
        &mut pending,
        &mut state,
        &mut report,
        &mut unclassified,
    )?;
    if cancelled || is_cancelled(cancel) {
        return Err(LibraryError::Cancelled);
    }

    let tx = conn.transaction().map_err(db_error)?;
    for (path, old) in &existing {
        if seen.contains(path) || keep.iter().any(|k| Path::new(path).starts_with(k)) {
            continue;
        }
        report.removed += tx
            .execute("DELETE FROM game WHERE id = ?1", [old.id])
            .map_err(db_error)? as u64;
    }
    tx.commit().map_err(db_error)?;
    unclassified.sort_by_key(|(index, _)| *index);
    report.unclassified = unclassified.len() as u64;
    report.unclassified_paths = unclassified
        .into_iter()
        .take(MAX_UNCLASSIFIED_PATHS)
        .map(|(_, path)| path)
        .collect();
    state.current = None;
    progress(&state);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_names() {
        assert_eq!(
            entry_file_name(b"dir/sub/Game.gb").as_deref(),
            Some("Game.gb")
        );
        assert_eq!(
            entry_file_name(b"..\\..\\evil.gb").as_deref(),
            Some("evil.gb")
        );
        assert_eq!(entry_file_name(b"/abs/x.nes").as_deref(), Some("x.nes"));
        assert_eq!(entry_file_name(b"a\x01b.gb").as_deref(), Some("a_b.gb"));
        assert_eq!(entry_file_name(b"dir/.."), None);
        assert_eq!(entry_file_name(b"dir/"), None);
        assert_eq!(entry_file_name(&[b'a'; 300]), None);
    }

    #[cfg(unix)]
    #[test]
    fn fifo_is_not_opened_as_a_rom() {
        let tmp = tempfile::tempdir().unwrap();
        let fifo = tmp.path().join("swapped.gb");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .is_ok_and(|s| s.success());
        if !made {
            eprintln!("mkfifo unavailable: skipped");
            return;
        }
        // Without O_NONBLOCK this open would wait for a writer forever.
        assert_eq!(open_regular(&fifo).unwrap_err(), "pas un fichier ordinaire");
        let file = tmp.path().join("ok.gb");
        fs::write(&file, b"x").unwrap();
        assert!(open_regular(&file).is_ok());
    }

    #[test]
    fn cancellation_stops_a_hash_in_progress() {
        /// An endless file whose first read is the moment the user presses "cancel".
        struct Endless<'c> {
            cancel: &'c AtomicBool,
            reads: usize,
        }
        impl Read for Endless<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                self.reads += 1;
                self.cancel.store(true, Ordering::Relaxed);
                buf.fill(0);
                Ok(buf.len())
            }
        }
        let cancel = AtomicBool::new(false);
        let mut inner = Endless {
            cancel: &cancel,
            reads: 0,
        };
        let reader = CancelRead {
            inner: &mut inner,
            cancel: &cancel,
        };
        assert!(hash_reader(reader, MAX_HASH_SIZE, None, MAX_HASH_SIZE).is_err());
        assert_eq!(inner.reads, 1);

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("big.gb");
        fs::write(&path, vec![0u8; 1 << 20]).unwrap();
        let found = Found {
            key: path.to_str().unwrap().to_owned(),
            path,
            folders: Vec::new(),
            file_size: 1 << 20,
            mtime: 0,
            media: Vec::new(),
            is_dir: false,
        };
        let cancel = AtomicBool::new(true);
        assert!(matches!(
            analyze(&found, MAX_HASH_SIZE, &cancel),
            Err(Failure::Cancelled)
        ));
        assert!(analyze(&found, MAX_HASH_SIZE, &AtomicBool::new(false)).is_ok());
    }

    #[test]
    fn folder_rule() {
        let pinned = |ext: &str, folders: &[&str]| match rule_of(attribute(
            ext,
            &folders.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )) {
            Ok(Rule::Identify { candidates, pinned }) => (candidates[0].id.clone(), pinned),
            _ => panic!("{ext}"),
        };
        assert_eq!(pinned("gb", &["roms", "gb"]), ("gb".into(), true));
        assert!(!pinned("gb", &["roms"]).1);
        // A folder naming a system that does not accept the extension does not pin.
        assert_eq!(pinned("sfc", &["gb"]), ("snes".into(), false));
    }

    #[test]
    fn root_spelling_is_normalized() {
        let canonical: PathBuf = ["base", "gb", "roms"].iter().collect();
        for spelling in ["base/gb/roms", "base/./gb/roms/", "base//gb/roms"] {
            assert_eq!(
                normalize_root(Path::new(spelling)).to_str(),
                canonical.to_str(),
                "{spelling}"
            );
        }
    }

    mod megacd {
        use super::*;
        use crate::{Game, Library};

        fn write(path: &Path, data: &[u8]) {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, data).unwrap();
        }

        fn sector0(region: u8) -> Vec<u8> {
            let mut s = vec![0u8; 2048];
            s[..16].copy_from_slice(b"SEGADISCSYSTEM  ");
            s[0x20B] = region;
            s
        }

        /// A MODE1/2352 data track: sector 0 with its sync and header, then `extra` zero bytes.
        fn raw_track(region: u8, extra: usize) -> Vec<u8> {
            let mut s = vec![0x00u8];
            s.extend([0xFF; 10]);
            s.extend([0x00, 0x00, 0x02, 0x00, 0x01]);
            s.extend(sector0(region));
            s.resize(2352 + extra, 0);
            s
        }

        fn cartridge() -> Vec<u8> {
            let mut rom: Vec<u8> = (0..8192u32).map(|i| (i * 13 % 251) as u8).collect();
            rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
            rom
        }

        /// Stored (uncompressed) zip of `entries`.
        fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
            let mut out = Vec::new();
            let mut central = Vec::new();
            for (name, data) in entries {
                let offset = out.len() as u32;
                let mut common = Vec::new();
                for v in [20u16, 0, 0, 0, 0] {
                    common.extend_from_slice(&v.to_le_bytes());
                }
                common.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
                common.extend_from_slice(&(data.len() as u32).to_le_bytes());
                common.extend_from_slice(&(data.len() as u32).to_le_bytes());
                common.extend_from_slice(&(name.len() as u16).to_le_bytes());
                common.extend_from_slice(&0u16.to_le_bytes());
                out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
                out.extend_from_slice(&common);
                out.extend_from_slice(name.as_bytes());
                out.extend_from_slice(data);
                central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
                central.extend_from_slice(&20u16.to_le_bytes());
                central.extend_from_slice(&common);
                // Comment length, disk number, internal and external attributes.
                central.extend_from_slice(&[0u8; 10]);
                central.extend_from_slice(&offset.to_le_bytes());
                central.extend_from_slice(name.as_bytes());
            }
            let cd = out.len() as u32;
            out.extend_from_slice(&central);
            out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
            out.extend_from_slice(&[0u8; 4]);
            out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
            out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
            out.extend_from_slice(&(central.len() as u32).to_le_bytes());
            out.extend_from_slice(&cd.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out
        }

        fn scan(lib: &mut Library, root: &Path) -> (ScanReport, ScanProgress) {
            let mut last = ScanProgress::default();
            let opts = ScanOptions {
                roots: vec![root.to_path_buf()],
                ..Default::default()
            };
            let report = lib
                .scan(&opts, &AtomicBool::new(false), &mut |p| {
                    last = p.clone();
                })
                .unwrap();
            (report, last)
        }

        fn names(games: &[Game]) -> Vec<(String, String)> {
            let mut v: Vec<_> = games
                .iter()
                .map(|g| {
                    (
                        g.system.clone(),
                        g.path.file_name().unwrap().to_string_lossy().into_owned(),
                    )
                })
                .collect();
            v.sort();
            v
        }

        fn has_error(report: &ScanReport, file: &str, needle: &str) -> bool {
            report
                .errors
                .iter()
                .any(|(p, m)| p.file_name().is_some_and(|n| n == file) && m.contains(needle))
        }

        #[test]
        fn cue_with_bin_tracks_is_one_unhashed_game() {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("roms/megacd/Dune (Europe)");
            write(
                &dir.join("Dune (Europe) (Track 01).bin"),
                &raw_track(0x64, 4096),
            );
            write(&dir.join("Dune (Europe) (Track 02).bin"), &[1u8; 4096]);
            write(&dir.join("Dune (Europe) (Track 03).bin"), &[2u8; 4096]);
            write(
                &dir.join("Dune (Europe).cue"),
                b"FILE \"Dune (Europe) (Track 01).bin\" BINARY\r\n  TRACK 01 MODE1/2352\r\n\
                  FILE \"Dune (Europe) (Track 02).bin\" BINARY\r\n  TRACK 02 AUDIO\r\n\
                  FILE \"Dune (Europe) (Track 03).bin\" BINARY\r\n  TRACK 03 AUDIO\r\n",
            );
            let mut lib = Library::open_in_memory().unwrap();
            let (report, progress) = scan(&mut lib, &tmp.path().join("roms"));
            assert!(report.errors.is_empty(), "{report:?}");
            let games = lib.games().unwrap();
            assert_eq!(
                names(&games),
                [("megacd".to_owned(), "Dune (Europe).cue".to_owned())]
            );
            let g = &games[0];
            assert_eq!((g.sha1, g.crc32), (None, None));
            assert_eq!(g.title, "Dune (Europe)");
            assert!(g.media.is_empty());
            assert_eq!(progress.hashed, 0);
            let (again, _) = scan(&mut lib, &tmp.path().join("roms"));
            assert_eq!((again.unchanged, again.added, again.removed), (1, 0, 0));
        }

        #[test]
        fn cue_iso_wav_like_240p_outside_a_megacd_folder() {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("roms/240p");
            write(&dir.join("240p_SegaCD_USA.iso"), &sector0(0x7A));
            write(&dir.join("240p_SegaCD_Track-02.wav"), &[0u8; 512]);
            write(&dir.join("240p_SegaCD_Track-03.wav"), &[0u8; 512]);
            write(
                &dir.join("240pSuite_USA.cue"),
                b"FILE \"240p_SegaCD_USA.iso\" BINARY\n  TRACK 01 MODE1/2048\n    INDEX 01 00:00:00\n    POSTGAP 00:02:00\n\
                  FILE \"240p_SegaCD_Track-02.wav\" WAVE\n  TRACK 02 AUDIO\n    PREGAP 00:02:00\n    INDEX 01 00:00:00\n\
                  FILE \"240p_SegaCD_Track-03.wav\" WAVE\n  TRACK 03 AUDIO\n    INDEX 01 00:00:00\n",
            );
            let mut lib = Library::open_in_memory().unwrap();
            let (report, _) = scan(&mut lib, &tmp.path().join("roms"));
            assert!(report.errors.is_empty(), "{report:?}");
            assert_eq!(
                names(&lib.games().unwrap()),
                [("megacd".to_owned(), "240pSuite_USA.cue".to_owned())]
            );
        }

        #[test]
        fn orphan_sega_cd_bin_is_ignored_and_cartridges_are_unchanged() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("roms");
            write(&root.join("Lost Track 01.bin"), &raw_track(0xA1, 0));
            write(&root.join("megadrive/Sonic.bin"), &cartridge());
            write(&root.join("Other.md"), &cartridge()[..4096]);
            let mut lib = Library::open_in_memory().unwrap();
            let (report, _) = scan(&mut lib, &root);
            assert!(
                has_error(
                    &report,
                    "Lost Track 01.bin",
                    "piste de CD sans fichier .cue"
                ),
                "{report:?}"
            );
            let games = lib.games().unwrap();
            assert_eq!(
                names(&games),
                [
                    ("megadrive".to_owned(), "Other.md".to_owned()),
                    ("megadrive".to_owned(), "Sonic.bin".to_owned()),
                ]
            );
            assert!(games.iter().all(|g| g.sha1.is_some()));
        }

        #[test]
        fn tracks_stored_by_a_2_0_scan_are_removed() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("roms");
            write(&root.join("Game (Track 1).bin"), &raw_track(0x64, 0));
            write(&root.join("Game (Track 2).bin"), &[3u8; 2352]);
            let mut lib = Library::open_in_memory().unwrap();
            scan(&mut lib, &root);
            // Track 2 alone is not a Sega CD header: a Mega Drive "game" until its `.cue` appears.
            assert_eq!(names(&lib.games().unwrap()).len(), 1);
            write(
                &root.join("Game.cue"),
                b"FILE \"Game (Track 1).bin\" BINARY\nFILE \"Game (Track 2).bin\" BINARY\n",
            );
            let (report, _) = scan(&mut lib, &root);
            assert_eq!(report.removed, 1, "{report:?}");
            assert_eq!(
                names(&lib.games().unwrap()),
                [("megacd".to_owned(), "Game.cue".to_owned())]
            );
        }

        #[test]
        fn headerless_iso_and_cue_need_the_megacd_folder() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("roms");
            write(&root.join("pc/Linux.iso"), &[0x20u8; 4096]);
            write(&root.join("pc/Audio.cue"), b"FILE \"a.wav\" WAVE\n");
            write(&root.join("pc/a.wav"), &[0u8; 64]);
            write(&root.join("megacd/Odd.iso"), &[0x20u8; 4096]);
            write(&root.join("megacd/Odd.cue"), b"FILE \"gone.bin\" BINARY\n");
            write(&root.join("any/Game.chd"), b"MComprHD");
            write(&root.join("any/Real (Japan).iso"), &sector0(0xA1));
            let mut lib = Library::open_in_memory().unwrap();
            let (report, _) = scan(&mut lib, &root);
            assert!(report.errors.is_empty(), "{report:?}");
            assert_eq!(report.ignored_empty, 0);
            // Design v4.0 §10 #7: outside a system folder a `.chd` (never read), a headerless `.iso` or a `.cue`
            // is unclassified, no longer silently ignored or listed.
            assert_eq!(report.unclassified, 3, "{report:?}");
            assert!(
                report
                    .unclassified_paths
                    .iter()
                    .any(|p| p.ends_with("Game.chd"))
            );
            assert_eq!(
                names(&lib.games().unwrap()),
                [
                    ("megacd".to_owned(), "Odd.cue".to_owned()),
                    ("megacd".to_owned(), "Odd.iso".to_owned()),
                    ("megacd".to_owned(), "Real (Japan).iso".to_owned()),
                ]
            );
        }

        #[test]
        fn unreadable_cue_is_reported_and_stays_listed_in_its_folder() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("roms");
            write(&root.join("megacd/Big.cue"), &vec![b' '; 64 * 1024 + 1]);
            // Outside a folder nothing classifies it (no header to probe): reported, unclassified.
            write(&root.join("Lost.cue"), &vec![b' '; 64 * 1024 + 1]);
            let mut lib = Library::open_in_memory().unwrap();
            let (report, _) = scan(&mut lib, &root);
            assert_eq!(report.errors.len(), 2, "{report:?}");
            assert_eq!(
                names(&lib.games().unwrap()),
                [("megacd".to_owned(), "Big.cue".to_owned())]
            );
            assert_eq!(report.unclassified, 1);
        }

        #[test]
        fn hostile_cue_never_hides_files_outside_its_folder() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("roms");
            write(&root.join("megadrive/Sonic.bin"), &cartridge());
            write(
                &root.join("megacd/x/Evil.cue"),
                b"FILE \"../../megadrive/Sonic.bin\" BINARY\nFILE \"/etc/passwd\" BINARY\n",
            );
            let mut lib = Library::open_in_memory().unwrap();
            scan(&mut lib, &root);
            assert_eq!(
                names(&lib.games().unwrap()),
                [
                    ("megacd".to_owned(), "Evil.cue".to_owned()),
                    ("megadrive".to_owned(), "Sonic.bin".to_owned()),
                ]
            );
        }

        #[test]
        fn zipped_discs_are_refused() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("roms");
            // The `.bin` comes first: the refusal must not depend on entry order.
            write(
                &root.join("Disc.zip"),
                &zip_of(&[
                    ("Disc (Track 1).bin", &raw_track(0x64, 0)),
                    ("Disc.cue", b"FILE \"Disc (Track 1).bin\" BINARY\n"),
                ]),
            );
            write(
                &root.join("Iso.zip"),
                &zip_of(&[("Iso.iso", &sector0(0x64))]),
            );
            write(
                &root.join("Cart.zip"),
                &zip_of(&[("Cart.md", &cartridge())]),
            );
            let mut lib = Library::open_in_memory().unwrap();
            let (report, _) = scan(&mut lib, &root);
            for zip in ["Disc.zip", "Iso.zip"] {
                assert!(
                    has_error(&report, zip, "Les CD ne se lisent pas dans un zip"),
                    "{report:?}"
                );
            }
            assert_eq!(
                names(&lib.games().unwrap()),
                [("megadrive".to_owned(), "Cart.zip".to_owned())]
            );
        }

        #[test]
        fn large_track_is_never_read() {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("roms/megacd");
            write(&dir.join("Big (Track 1).bin"), &raw_track(0x64, 0));
            // Sparse 600 MiB audio track: hidden by the cue, so neither hashed nor probed.
            let big = dir.join("Big (Track 2).bin");
            File::create(&big)
                .unwrap()
                .set_len(600 * 1024 * 1024)
                .unwrap();
            write(
                &dir.join("Big.cue"),
                b"FILE \"Big (Track 1).bin\" BINARY\nFILE \"Big (Track 2).bin\" BINARY\n",
            );
            let mut lib = Library::open_in_memory().unwrap();
            let start = Instant::now();
            let (_, progress) = scan(&mut lib, &tmp.path().join("roms"));
            assert_eq!(progress.hashed, 0);
            assert!(start.elapsed() < Duration::from_secs(5));
            assert_eq!(names(&lib.games().unwrap()).len(), 1);
        }

        #[cfg(any(windows, target_os = "macos"))]
        #[test]
        fn cue_names_match_tracks_case_insensitively() {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("roms/megacd");
            write(&dir.join("game (track 1).bin"), &raw_track(0x64, 0));
            write(
                &dir.join("Game.cue"),
                b"FILE \"GAME (TRACK 1).BIN\" BINARY\n",
            );
            let mut lib = Library::open_in_memory().unwrap();
            let (report, _) = scan(&mut lib, &tmp.path().join("roms"));
            assert!(report.errors.is_empty(), "{report:?}");
            assert_eq!(names(&lib.games().unwrap()).len(), 1);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_root_with_forward_slashes_is_normalized() {
        for spelling in [r"C:\base\gb/roms", "C:/base/gb/roms", r"C:\base\gb\roms\"] {
            assert_eq!(
                normalize_root(Path::new(spelling)).to_str(),
                Some(r"C:\base\gb\roms"),
                "{spelling}"
            );
        }
    }
}
