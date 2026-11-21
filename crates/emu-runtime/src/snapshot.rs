//! Stored-zip snapshot of an unpacked game folder (DOS, ADR 0018, design v4.0 §3.2): the core gets
//! `<cache>/<core_id>/<save_name>.zip`, never the user's folder.
//!
//! - zip **stored** (Zip64 for entries of 4 GiB or more), written to a temporary file then renamed, with
//!   `<snapshot>.manifest` = SHA-256 of the sorted list `relative path \0 size \0 mtime`;
//! - entry names: relative, `/`-separated, UTF-8, no empty component, no control character nor backslash (else
//!   [`RuntimeError::UnrepresentableName`]); symbolic links (and junctions) ignored, neither followed nor copied;
//!   anything but plain files and folders ignored (a FIFO would block the walk); depth, entry count and size
//!   bounded **while** walking ([`RuntimeError::SnapshotTooLarge`]);
//! - `cancel` checked between two files and between two chunks of a file ([`RuntimeError::SnapshotCancelled`],
//!   temporary removed);
//! - the source folder is **never opened for writing** (a read-only folder is accepted): files are opened with
//!   `File::open` only, folders are listed with `read_dir` only.

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::content::io_error;
use crate::error::RuntimeError;
use crate::fsname::sanitize_component;
use crate::saves::{FileMode, read_bounded, write_atomic, write_atomic_with};

/// Bounds of one snapshot, enforced during the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotLimits {
    /// Sum of the file sizes.
    pub max_bytes: u64,
    /// Files and folders (zip entries).
    pub max_files: u32,
    /// Folder nesting below the game folder (files at its root are at depth 0, `A/B/F` at depth 2).
    pub max_depth: u32,
}

impl SnapshotLimits {
    /// 4 GiB, 65 535 files, 16 levels.
    pub const DEFAULT: Self = Self {
        max_bytes: 4 << 30,
        max_files: 65_535,
        max_depth: 16,
    };
}

/// State of a snapshot against its folder (manifest comparison; nothing is hashed but the file list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotStatus {
    /// Snapshot and manifest present, manifest equal to the folder's.
    Current,
    /// Snapshot present but the folder changed (or the manifest is missing / unreadable).
    Stale,
    Missing,
}

/// Outcome of [`snapshot_folder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotReport {
    /// Files stored (folders not counted).
    pub files: u32,
    pub bytes: u64,
    /// `false` when the snapshot was already `Current` (nothing written).
    pub rebuilt: bool,
}

/// Longest snapshot file name kept from `name` (before `.zip`), as for game names.
const MAX_NAME_BYTES: usize = 200;
const MANIFEST_HEADER: &str = "emu-rust snapshot 1";
/// A manifest is two short lines: anything longer is not ours (read bound).
const MAX_MANIFEST_BYTES: usize = 256;
/// Copy buffer: also the granularity of `progress` and `cancel` inside a large file.
const CHUNK: usize = 256 * 1024;

/// `<cache_core_dir>/<name>.zip`, `name` sanitized as a single file name, `#` removed (it separates the start
/// program in the path handed to the core). `name` = `GameIdentity::save_name`.
pub fn snapshot_path(cache_core_dir: &Path, name: &str) -> PathBuf {
    let cleaned: String = name.chars().filter(|&c| c != '#').collect();
    let mut stem = sanitize_component(&cleaned, MAX_NAME_BYTES);
    if stem.is_empty() {
        stem = "game".to_owned();
    }
    cache_core_dir.join(format!("{stem}.zip"))
}

/// `<snapshot>.manifest` (the full snapshot file name followed by `.manifest`).
fn manifest_path(snapshot: &Path) -> PathBuf {
    let mut name = snapshot.as_os_str().to_owned();
    name.push(".manifest");
    PathBuf::from(name)
}

/// `Missing` without touching `dir` when there is no snapshot file; otherwise walks `dir` (listing and `stat`
/// only, [`SnapshotLimits::DEFAULT`]) and compares with the manifest. A folder over the default limits is
/// `Stale` (rebuilding reports the limit); an unrepresentable name or an unreadable folder is an error.
pub fn snapshot_status(dir: &Path, snapshot: &Path) -> Result<SnapshotStatus, RuntimeError> {
    match fs::symlink_metadata(snapshot) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Ok(SnapshotStatus::Stale),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(SnapshotStatus::Missing),
        Err(e) => return Err(io_error(snapshot, e)),
    }
    let items = match walk(dir, &SnapshotLimits::DEFAULT) {
        Ok(items) => items,
        Err(RuntimeError::SnapshotTooLarge { .. }) => return Ok(SnapshotStatus::Stale),
        Err(e) => return Err(e),
    };
    Ok(if manifest_matches(snapshot, &items) {
        SnapshotStatus::Current
    } else {
        SnapshotStatus::Stale
    })
}

/// Builds (or keeps, if `Current`) the snapshot of `dir`. `progress(done_bytes, total_bytes)` is called at the
/// start, after every chunk and at the end. On any error (limits, name, I/O, cancellation) nothing replaces an
/// existing snapshot and no temporary file is left.
pub fn snapshot_folder(
    dir: &Path,
    snapshot: &Path,
    limits: &SnapshotLimits,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<SnapshotReport, RuntimeError> {
    let items = walk(dir, limits)?;
    let files = u32::try_from(items.iter().filter(|i| !i.dir).count()).unwrap_or(u32::MAX);
    let total: u64 = items.iter().map(|i| i.size).sum();
    if fs::symlink_metadata(snapshot).is_ok_and(|m| m.is_file())
        && manifest_matches(snapshot, &items)
    {
        progress(total, total);
        return Ok(SnapshotReport {
            files,
            bytes: total,
            rebuilt: false,
        });
    }
    progress(0, total);
    // An error raised inside the zip writer, kept with its own type (the closure must return `io::Error`).
    let mut failure: Option<RuntimeError> = None;
    let written = write_atomic_with(snapshot, FileMode::Default, |file| {
        write_zip(file, snapshot, &items, total, progress, cancel).map_err(|e| {
            let message = e.to_string();
            failure = Some(e);
            io::Error::other(message)
        })
    });
    if let Err(e) = written {
        return Err(failure.unwrap_or_else(|| io_error(snapshot, e)));
    }
    // Snapshot first, manifest last: a crash in between leaves an old or absent manifest, i.e. `Stale`.
    let manifest = manifest_path(snapshot);
    write_atomic(&manifest, &manifest_bytes(&items), FileMode::Default)
        .map_err(|e| io_error(&manifest, e))?;
    tracing::info!(
        files,
        bytes = total,
        "snapshot of {} written to {}",
        dir.display(),
        snapshot.display()
    );
    Ok(SnapshotReport {
        files,
        bytes: total,
        rebuilt: true,
    })
}

/// Identity fingerprint of a game folder (design v4.0 §10 n° 4): total size in bytes and CRC32 of the sorted list
/// `relative path \0 size` (no dates; folders included as `name/`), same walk and bounds as the snapshot.
pub(crate) fn folder_fingerprint(
    dir: &Path,
    limits: &SnapshotLimits,
) -> Result<(u64, u32), RuntimeError> {
    let items = walk(dir, limits)?;
    let mut hasher = crc32fast::Hasher::new();
    let mut total = 0u64;
    for item in &items {
        hasher.update(item.name.as_bytes());
        hasher.update(b"\0");
        hasher.update(item.size.to_string().as_bytes());
        hasher.update(b"\0");
        total += item.size;
    }
    Ok((total, hasher.finalize()))
}

/// One zip entry of the snapshot.
#[derive(Debug)]
struct Item {
    /// Entry name: `/`-separated relative path, folders end with `/`.
    name: String,
    path: PathBuf,
    size: u64,
    /// Nanoseconds since the Unix epoch; `None` for folders or when unknown.
    mtime: Option<u128>,
    dir: bool,
}

/// Lists `dir` recursively under `limits` (listing and `stat` only), sorted by entry name.
fn walk(dir: &Path, limits: &SnapshotLimits) -> Result<Vec<Item>, RuntimeError> {
    let meta = fs::metadata(dir).map_err(|e| io_error(dir, e))?;
    if !meta.is_dir() {
        return Err(io_error(
            dir,
            io::Error::new(io::ErrorKind::InvalidInput, "not a folder"),
        ));
    }
    let too_large = |limit: u64| RuntimeError::SnapshotTooLarge { limit };
    let mut items = Vec::new();
    let mut bytes = 0u64;
    let mut pending: Vec<(PathBuf, String, u32)> = vec![(dir.to_path_buf(), String::new(), 0)];
    while let Some((folder, prefix, depth)) = pending.pop() {
        for entry in fs::read_dir(&folder).map_err(|e| io_error(&folder, e))? {
            let entry = entry.map_err(|e| io_error(&folder, e))?;
            let path = entry.path();
            // `DirEntry::file_type` does not follow links (a Windows junction is a link too).
            let kind = entry.file_type().map_err(|e| io_error(&path, e))?;
            if kind.is_symlink() || !(kind.is_dir() || kind.is_file()) {
                continue;
            }
            let file_name = entry.file_name();
            let Some(component) = entry_component(&file_name) else {
                return Err(RuntimeError::UnrepresentableName(path));
            };
            if items.len() >= limits.max_files as usize {
                return Err(too_large(u64::from(limits.max_files)));
            }
            if kind.is_dir() {
                if depth >= limits.max_depth {
                    return Err(too_large(u64::from(limits.max_depth)));
                }
                let name = format!("{prefix}{component}/");
                pending.push((path.clone(), name.clone(), depth + 1));
                items.push(Item {
                    name,
                    path,
                    size: 0,
                    mtime: None,
                    dir: true,
                });
            } else {
                let meta = entry.metadata().map_err(|e| io_error(&path, e))?;
                bytes = bytes.saturating_add(meta.len());
                if bytes > limits.max_bytes {
                    return Err(too_large(limits.max_bytes));
                }
                items.push(Item {
                    name: format!("{prefix}{component}"),
                    path,
                    size: meta.len(),
                    mtime: meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos()),
                    dir: false,
                });
            }
        }
    }
    items.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

/// A file name usable as one component of a zip entry name: UTF-8, not empty, no control character, no `/` nor
/// `\` (zip readers, dosbox_pure included, may take a backslash for a separator).
fn entry_component(name: &std::ffi::OsStr) -> Option<&str> {
    let name = name.to_str()?;
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\');
    (!bad).then_some(name)
}

fn manifest_bytes(items: &[Item]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    for item in items {
        hasher.update(item.name.as_bytes());
        hasher.update(b"\0");
        hasher.update(item.size.to_string().as_bytes());
        hasher.update(b"\0");
        match item.mtime {
            Some(ns) => hasher.update(ns.to_string().as_bytes()),
            None => hasher.update(b"-"),
        }
        hasher.update(b"\0");
    }
    let mut out = String::with_capacity(MANIFEST_HEADER.len() + 66);
    out.push_str(MANIFEST_HEADER);
    out.push('\n');
    for byte in hasher.finalize() {
        out.push_str(&format!("{byte:02x}"));
    }
    out.push('\n');
    out.into_bytes()
}

fn manifest_matches(snapshot: &Path, items: &[Item]) -> bool {
    let manifest = manifest_path(snapshot);
    match fs::symlink_metadata(&manifest) {
        Ok(meta) if meta.is_file() => {}
        _ => return false,
    }
    read_bounded(&manifest, MAX_MANIFEST_BYTES).is_ok_and(|bytes| bytes == manifest_bytes(items))
}

fn write_zip(
    file: &mut File,
    snapshot: &Path,
    items: &[Item],
    total: u64,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), RuntimeError> {
    let archive_error = |message: String| RuntimeError::Archive {
        path: snapshot.to_path_buf(),
        message,
    };
    let mut zip = ZipWriter::new(BufWriter::with_capacity(CHUNK, file));
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    for item in items {
        if cancel.load(Ordering::Relaxed) {
            return Err(RuntimeError::SnapshotCancelled);
        }
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .last_modified_time(dos_time(item.mtime));
        if item.dir {
            zip.add_directory(item.name.as_str(), options)
                .map_err(|e| archive_error(e.to_string()))?;
            continue;
        }
        let mut input = File::open(&item.path).map_err(|e| io_error(&item.path, e))?;
        zip.start_file(
            item.name.as_str(),
            options.large_file(item.size >= u64::from(u32::MAX)),
        )
        .map_err(|e| archive_error(e.to_string()))?;
        // Exactly the size listed by the walk (what the manifest records): a file that shrank meanwhile is an
        // error, bytes appended meanwhile are left out (the next status is then `Stale`).
        let mut left = item.size;
        while left > 0 {
            if cancel.load(Ordering::Relaxed) {
                return Err(RuntimeError::SnapshotCancelled);
            }
            let want = usize::try_from(left).map_or(CHUNK, |l| l.min(CHUNK));
            let n = match input.read(&mut buf[..want]) {
                Ok(0) => {
                    return Err(io_error(
                        &item.path,
                        io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "file shrank during the snapshot",
                        ),
                    ));
                }
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(io_error(&item.path, e)),
            };
            zip.write_all(&buf[..n])
                .map_err(|e| archive_error(e.to_string()))?;
            left -= n as u64;
            done += n as u64;
            progress(done, total);
        }
    }
    let mut out = zip.finish().map_err(|e| archive_error(e.to_string()))?;
    out.flush().map_err(|e| archive_error(e.to_string()))?;
    progress(total, total);
    Ok(())
}

/// MS-DOS date of a modification time (UTC; DOS has no time zone). Clamped to the zip range 1980-2107; unknown
/// or folder ⇒ 1980-01-01 00:00.
fn dos_time(mtime_ns: Option<u128>) -> DateTime {
    let Some(ns) = mtime_ns else {
        return DateTime::default();
    };
    let secs = i64::try_from(ns / 1_000_000_000).unwrap_or(i64::MAX);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    if year > 2107 {
        return DateTime::from_date_and_time(2107, 12, 31, 23, 59, 58).unwrap_or_default();
    }
    let Some(year) = u16::try_from(year).ok().filter(|&y| y >= 1980) else {
        return DateTime::default();
    };
    // Bounded by construction: hour < 24, minute < 60, second < 60.
    DateTime::from_date_and_time(
        year,
        month,
        day,
        (rem / 3600) as u8,
        (rem % 3600 / 60) as u8,
        (rem % 60) as u8,
    )
    .unwrap_or_default()
}

/// Proleptic Gregorian (year, month, day) of a day count since 1970-01-01 (H. Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Validates `start` (design v4.0 §3.2) and returns the name to append after `#`: the 8.3 name in upper case.
/// Non-empty, at most 12 characters, `NAME.EXT` with a 1-8 character name and an `exe` / `com` / `bat` extension,
/// printable ASCII without space nor `/ \ # :` (nor the other characters DOS refuses: `* ? < > | "`), and a
/// plain file of that name (case ignored) at the root of `dir`. Anything else: [`RuntimeError::InvalidStart`].
pub(crate) fn validate_start(dir: &Path, start: &str) -> Result<String, RuntimeError> {
    let invalid = || RuntimeError::InvalidStart(start.to_owned());
    let printable = start.bytes().all(|b| {
        b.is_ascii_graphic()
            && !matches!(
                b,
                b'/' | b'\\' | b'#' | b':' | b'*' | b'?' | b'<' | b'>' | b'|' | b'"'
            )
    });
    if start.is_empty() || start.len() > 12 || !printable {
        return Err(invalid());
    }
    let Some((base, ext)) = start.split_once('.') else {
        return Err(invalid());
    };
    if base.is_empty()
        || base.len() > 8
        || !["exe", "com", "bat"]
            .iter()
            .any(|e| ext.eq_ignore_ascii_case(e))
    {
        return Err(invalid());
    }
    let read = fs::read_dir(dir).map_err(|e| io_error(dir, e))?;
    for entry in read {
        let entry = entry.map_err(|e| io_error(dir, e))?;
        let is_file = entry.file_type().is_ok_and(|t| t.is_file());
        if is_file
            && entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(start))
        {
            return Ok(start.to_ascii_uppercase());
        }
    }
    Err(invalid())
}

#[cfg(test)]
mod tests;
