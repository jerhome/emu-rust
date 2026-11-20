//! Identity of working copies (`<saves>/<core_id>/<game>[ (n)].disks/`, see `media.rs`). A manifest in the folder
//! records, for each copy, the source it was made from, so that two games with the same name, or a source
//! replaced by another version, never reuse (and keep writing into) the progress held by another source's copy.
//!
//! Source identity = content: size + CRC32 of the whole source file (for a zip entry: of the zip, plus the entry
//! name). The canonical path and the mtime are recorded too: when path, size and mtime are all unchanged the
//! recorded CRC32 is trusted and the file is not read (a normal launch only `stat`s its sources); otherwise the
//! source is hashed once. A moved or touched source with the same content keeps its copies; a source whose
//! content changed gets new ones. CRC32 is not collision resistant, which is fine here: the threat is an honest
//! mix-up between two images, not a forged one.

use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use zip::ZipArchive;

use crate::content::{self, ArchiveEntry, Limits, io_error};
use crate::error::RuntimeError;
use crate::media::{MAX_MEDIA_IMAGES, accepts_ext, has_ext, too_many};
use crate::saves::{FileMode, read_bounded, write_atomic_with};

/// Hidden file in each working-copy folder.
pub(crate) const MANIFEST_NAME: &str = ".emu-rust-disks";
const HEADER: &[u8] = b"emu-rust working copies 1";
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
/// Set entries (≤ 32) + disks added from the menu (the core's list holds at most 32 images, so this bound is never
/// reached in practice); anything longer was not written by us.
pub(crate) const MAX_MANIFEST_ENTRIES: usize = 4 * MAX_MEDIA_IMAGES;

/// Why a copy is in the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    /// Part of the disk set the folder was prepared for.
    Set,
    /// Added from the disk menu during a session; reused when the same source is added again.
    Added,
}

/// One working copy and the identity of its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestEntry {
    pub origin: Origin,
    /// File name in the folder (a `disk_file_name`, checked when read back).
    pub copy: String,
    /// Zip entry the copy was extracted from.
    pub entry: Option<String>,
    path: Vec<u8>,
    size: u64,
    mtime: Option<u128>,
    crc: u32,
}

impl ManifestEntry {
    pub fn new(
        origin: Origin,
        copy: String,
        entry: Option<String>,
        source: &mut Source,
    ) -> Result<Self, RuntimeError> {
        let crc = source.crc()?;
        Ok(Self {
            origin,
            copy,
            entry,
            path: source.key.clone(),
            size: source.size,
            mtime: source.mtime,
            crc,
        })
    }

    /// « Reprendre » (2.1): an orphan copy becomes an added disk without a recorded source. Such an entry never
    /// matches a source (`Source::matches`), so it is only reached through the disk menu.
    pub fn adopted(copy: String, size: u64) -> Self {
        Self {
            origin: Origin::Added,
            copy,
            entry: None,
            path: Vec::new(),
            size,
            mtime: None,
            crc: 0,
        }
    }

    /// Recorded source path; `None` for an adopted orphan.
    pub fn source_path(&self) -> Option<PathBuf> {
        (!self.path.is_empty()).then(|| path_from_key(&self.path))
    }

    /// Size and CRC32 of the recorded source (rule 3 of the game identity, `identity.rs`).
    pub fn source_size_crc(&self) -> (u64, u32) {
        (self.size, self.crc)
    }

    /// Raw path key of the recorded source (empty for an adopted orphan).
    pub fn source_key(&self) -> &[u8] {
        &self.path
    }

    /// Records the current path / mtime of a source found identical, so the next launch takes the fast path.
    pub fn refresh(&mut self, source: &Source) {
        self.path.clone_from(&source.key);
        self.size = source.size;
        self.mtime = source.mtime;
        if let Some(crc) = source.crc {
            self.crc = crc;
        }
    }
}

/// A user file a copy is made from (only ever read).
#[derive(Debug)]
pub(crate) struct Source {
    pub path: PathBuf,
    key: Vec<u8>,
    pub size: u64,
    mtime: Option<u128>,
    crc: Option<u32>,
    max_read: u64,
}

impl Source {
    pub fn open(path: &Path, max_read: u64) -> Result<Self, RuntimeError> {
        let meta = fs::metadata(path).map_err(|e| io_error(path, e))?;
        if !meta.is_file() {
            return Err(RuntimeError::InvalidMediaSet(format!(
                "{} is not a file",
                path.display()
            )));
        }
        let canonical = fs::canonicalize(path)
            .or_else(|_| std::path::absolute(path))
            .unwrap_or_else(|_| path.to_path_buf());
        Ok(Self {
            path: path.to_path_buf(),
            key: path_key(&canonical),
            size: meta.len(),
            mtime: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos()),
            crc: None,
            max_read,
        })
    }

    fn crc(&mut self) -> Result<u32, RuntimeError> {
        if let Some(crc) = self.crc {
            return Ok(crc);
        }
        let file = File::open(&self.path).map_err(|e| io_error(&self.path, e))?;
        let mut reader = file.take(self.max_read);
        let mut hasher = crc32fast::Hasher::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => hasher.update(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(io_error(&self.path, e)),
            }
        }
        let crc = hasher.finalize();
        self.crc = Some(crc);
        Ok(crc)
    }

    /// Same content as the source recorded in `entry` (see the module docs for when the file is read).
    fn matches(&mut self, entry: &ManifestEntry) -> Result<bool, RuntimeError> {
        // An adopted orphan has no source: never reused for one, whatever its CRC field says.
        if entry.size != self.size || entry.path.is_empty() {
            return Ok(false);
        }
        if entry.path == self.key && entry.mtime.is_some() && entry.mtime == self.mtime {
            return Ok(true);
        }
        Ok(self.crc()? == entry.crc)
    }
}

/// Bytes of a (canonical) path as recorded in manifests: exact on Unix, UTF-8 (lossy) elsewhere.
#[cfg(unix)]
pub(crate) fn path_key(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
pub(crate) fn path_key(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

/// Inverse of [`path_key`].
#[cfg(unix)]
pub(crate) fn path_from_key(key: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(key))
}

#[cfg(not(unix))]
pub(crate) fn path_from_key(key: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(key).into_owned())
}

/// One image to stage: a whole source file, or one entry of a zip source.
pub(crate) struct PlanItem {
    pub src: usize,
    pub entry: Option<ArchiveEntry>,
}

/// Sources of a set, opened and listed (zip entries) before anything is written.
pub(crate) struct Plan {
    pub sources: Vec<Source>,
    pub archives: Vec<Option<ZipArchive<BufReader<File>>>>,
    pub items: Vec<PlanItem>,
}

pub(crate) fn zip_entry_accepted(exts: &[String], name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, ext)) => !ext.eq_ignore_ascii_case("m3u") && accepts_ext(exts, ext),
        None => false,
    }
}

impl Plan {
    /// Same rules as `Stage::add_source` / `Stage::add_zip`: a zip the core cannot read gives up to `max_per_zip`
    /// accepted entries (more than [`MAX_MEDIA_IMAGES`] in a set is an error).
    pub fn new(
        paths: &[PathBuf],
        exts: &[String],
        limits: Limits,
        max_per_zip: usize,
    ) -> Result<Self, RuntimeError> {
        let mut plan = Self {
            sources: Vec::new(),
            archives: Vec::new(),
            items: Vec::new(),
        };
        for path in paths {
            let src = plan.sources.len();
            if has_ext(path, "zip") && !accepts_ext(exts, "zip") {
                let mut archive = content::open_archive(path, limits)?;
                let room = MAX_MEDIA_IMAGES
                    .saturating_sub(plan.items.len())
                    .min(max_per_zip);
                let entries = content::accepted_entries(
                    &mut archive,
                    path,
                    |name| zip_entry_accepted(exts, name),
                    // One more than allowed, to report an oversized set instead of silently dropping disks.
                    room.saturating_add(1),
                )?;
                if entries.is_empty() {
                    return Err(RuntimeError::NoContentInArchive(path.clone()));
                }
                if entries.len() > room && max_per_zip == MAX_MEDIA_IMAGES {
                    return Err(too_many());
                }
                plan.sources
                    .push(Source::open(path, limits.max_archive_size)?);
                plan.archives.push(Some(archive));
                plan.items
                    .extend(entries.into_iter().take(room).map(|entry| PlanItem {
                        src,
                        entry: Some(entry),
                    }));
            } else {
                if plan.items.len() >= MAX_MEDIA_IMAGES {
                    return Err(too_many());
                }
                let source = Source::open(path, limits.max_size)?;
                if source.size > limits.max_size {
                    return Err(RuntimeError::ContentTooLarge {
                        path: path.clone(),
                        size: source.size,
                    });
                }
                plan.sources.push(source);
                plan.archives.push(None);
                plan.items.push(PlanItem { src, entry: None });
            }
        }
        Ok(plan)
    }

    fn item_matches(&mut self, i: usize, entry: &ManifestEntry) -> Result<bool, RuntimeError> {
        let item = &self.items[i];
        if entry.entry.as_deref() != item.entry.as_ref().map(|e| e.name.as_str()) {
            return Ok(false);
        }
        self.sources[item.src].matches(entry)
    }

    /// A present set entry of the folder was recorded for a source that has not changed since (same path, size
    /// and mtime): the folder is almost certainly this set's, and checking it costs no file read.
    pub fn quick_match(&self, entries: &[ManifestEntry], present: &[bool]) -> bool {
        entries.iter().zip(present).any(|(e, &p)| {
            p && e.origin == Origin::Set
                && e.mtime.is_some()
                && self.items.iter().any(|it| {
                    let s = &self.sources[it.src];
                    e.entry.as_deref() == it.entry.as_ref().map(|a| a.name.as_str())
                        && e.path == s.key
                        && e.size == s.size
                        && e.mtime == s.mtime
                })
        })
    }

    /// One of the sources is at the path recorded by a set entry of the folder.
    pub fn shares_path(&self, entries: &[ManifestEntry], present: &[bool]) -> bool {
        entries.iter().zip(present).any(|(e, &p)| {
            p && e.origin == Origin::Set && self.sources.iter().any(|s| s.key == e.path)
        })
    }
}

/// Manifest entry holding the copy of each plan item. `None` when a recorded copy of the folder's set (still
/// present) has no identical source here: the folder belongs to other files and must not be used.
/// Matching order: same path first (keeps two identical disks of one set on their own copies), then same
/// content, then a disk previously added from the menu (it joins the set).
pub(crate) fn assign(
    entries: &[ManifestEntry],
    present: &[bool],
    plan: &mut Plan,
) -> Result<Option<Vec<Option<usize>>>, RuntimeError> {
    let mut taken = vec![false; entries.len()];
    let mut out = vec![None; plan.items.len()];
    for pass in 0..3 {
        for i in 0..plan.items.len() {
            if out[i].is_some() {
                continue;
            }
            for (j, entry) in entries.iter().enumerate() {
                if taken[j] || !present[j] {
                    continue;
                }
                let eligible = match pass {
                    0 => {
                        entry.origin == Origin::Set
                            && entry.path == plan.sources[plan.items[i].src].key
                    }
                    1 => entry.origin == Origin::Set,
                    _ => entry.origin == Origin::Added,
                };
                if eligible && plan.item_matches(i, entry)? {
                    taken[j] = true;
                    out[i] = Some(j);
                    break;
                }
            }
        }
    }
    let orphan = entries
        .iter()
        .enumerate()
        .any(|(j, e)| e.origin == Origin::Set && present[j] && !taken[j]);
    Ok((!orphan).then_some(out))
}

/// A previously added disk (`Origin::Added`, copy present, not `excluded`) made from the same source as item 0.
pub(crate) fn find_added(
    entries: &[ManifestEntry],
    present: &[bool],
    plan: &mut Plan,
    excluded: impl Fn(&ManifestEntry) -> bool,
) -> Result<Option<usize>, RuntimeError> {
    let mut fallback = None;
    for (j, entry) in entries.iter().enumerate() {
        if entry.origin != Origin::Added || !present[j] || excluded(entry) {
            continue;
        }
        if plan.item_matches(0, entry)? {
            if entry.path == plan.sources[plan.items[0].src].key {
                return Ok(Some(j));
            }
            fallback.get_or_insert(j);
        }
    }
    Ok(fallback)
}

/// What is at a working-copy folder candidate.
pub(crate) enum DirState {
    Absent,
    /// A file or a link: never used (nor followed).
    NotDir,
    /// A folder made before manifests existed: adopted by the first set prepared into it.
    Legacy,
    /// A manifest we cannot read: the folder is left alone.
    Unreadable,
    Manifest(Vec<ManifestEntry>),
}

pub(crate) fn inspect(
    dir: &Path,
    is_copy_name: fn(&str) -> bool,
) -> Result<DirState, RuntimeError> {
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Ok(DirState::NotDir),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(DirState::Absent),
        Err(e) => return Err(io_error(dir, e)),
    }
    let path = dir.join(MANIFEST_NAME);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Ok(unreadable(&path)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(DirState::Legacy),
        Err(e) => return Err(io_error(&path, e)),
    }
    let bytes = read_bounded(&path, MAX_MANIFEST_BYTES).map_err(|e| io_error(&path, e))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Ok(unreadable(&path));
    }
    Ok(decode(&bytes, is_copy_name).map_or_else(|| unreadable(&path), DirState::Manifest))
}

fn unreadable(path: &Path) -> DirState {
    tracing::warn!(
        "{} is not a valid manifest: folder left alone",
        path.display()
    );
    DirState::Unreadable
}

/// Copy of each entry is a regular file (a link or a folder in its place holds no progress of ours).
pub(crate) fn present(dir: &Path, entries: &[ManifestEntry]) -> Result<Vec<bool>, RuntimeError> {
    entries
        .iter()
        .map(|e| is_regular_file(&dir.join(&e.copy)))
        .collect()
}

pub(crate) fn is_regular_file(path: &Path) -> Result<bool, RuntimeError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.is_file()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io_error(path, e)),
    }
}

/// Atomic, and only when the content changes.
pub(crate) fn write(dir: &Path, entries: &[ManifestEntry]) -> Result<(), RuntimeError> {
    let path = dir.join(MANIFEST_NAME);
    let bytes = encode(entries);
    if read_bounded(&path, bytes.len()).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    write_atomic_with(&path, FileMode::Default, |f| {
        io::Write::write_all(f, &bytes)
    })
    .map_err(|e| io_error(&path, e))
}

// Format: a header line, then one line per copy, tab-separated:
// `set|added`, copy, zip entry (empty if none), size, mtime in ns (`-` if unknown), crc32 (8 hex), source path.
// Text fields escape `%`, control bytes and DEL as `%XX`; the path is kept byte-exact (Unix paths need not be UTF-8).

pub(crate) fn encode(entries: &[ManifestEntry]) -> Vec<u8> {
    let mut out = HEADER.to_vec();
    out.push(b'\n');
    for e in entries {
        out.extend_from_slice(match e.origin {
            Origin::Set => b"set",
            Origin::Added => b"added",
        });
        out.push(b'\t');
        escape(e.copy.as_bytes(), &mut out);
        out.push(b'\t');
        escape(e.entry.as_deref().unwrap_or("").as_bytes(), &mut out);
        let mtime = e.mtime.map_or_else(|| "-".to_owned(), |m| m.to_string());
        out.extend_from_slice(format!("\t{}\t{mtime}\t{:08x}\t", e.size, e.crc).as_bytes());
        escape(&e.path, &mut out);
        out.push(b'\n');
    }
    out
}

pub(crate) fn escape(bytes: &[u8], out: &mut Vec<u8>) {
    for &b in bytes {
        if b < 0x20 || b == 0x7f || b == b'%' {
            out.extend_from_slice(format!("%{b:02X}").as_bytes());
        } else {
            out.push(b);
        }
    }
}

pub(crate) fn unescape(field: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(field.len());
    let mut i = 0;
    while i < field.len() {
        if field[i] == b'%' {
            let hex = std::str::from_utf8(field.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(field[i]);
            i += 1;
        }
    }
    Some(out)
}

pub(crate) fn decode(bytes: &[u8], is_copy_name: fn(&str) -> bool) -> Option<Vec<ManifestEntry>> {
    let body = bytes.strip_suffix(b"\n")?;
    let mut lines = body.split(|&b| b == b'\n');
    if lines.next()? != HEADER {
        return None;
    }
    let mut out = Vec::new();
    for line in lines {
        if out.len() >= MAX_MANIFEST_ENTRIES {
            return None;
        }
        let fields: Vec<&[u8]> = line.split(|&b| b == b'\t').collect();
        let [origin, copy, entry, size, mtime, crc, path] = fields.as_slice() else {
            return None;
        };
        let origin = match *origin {
            b"set" => Origin::Set,
            b"added" => Origin::Added,
            _ => return None,
        };
        let copy = String::from_utf8(unescape(copy)?).ok()?;
        // The generated playlist is `<game>.m3u`: no copy may take an `.m3u` name, nor repeat a name (case-insensitive
        // filesystems would make both entries the same file).
        if !is_copy_name(&copy)
            || copy == MANIFEST_NAME
            || has_ext(Path::new(&copy), "m3u")
            || out
                .iter()
                .any(|e: &ManifestEntry| e.copy.eq_ignore_ascii_case(&copy))
        {
            return None;
        }
        let entry = String::from_utf8(unescape(entry)?).ok()?;
        let text = |f: &[u8]| std::str::from_utf8(f).ok().map(str::to_owned);
        let mtime = match *mtime {
            b"-" => None,
            m => Some(text(m)?.parse().ok()?),
        };
        if crc.len() != 8 {
            return None;
        }
        out.push(ManifestEntry {
            origin,
            copy,
            entry: (!entry.is_empty()).then_some(entry),
            path: unescape(path)?,
            size: text(size)?.parse().ok()?,
            mtime,
            crc: u32::from_str_radix(&text(crc)?, 16).ok()?,
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn any_name(name: &str) -> bool {
        !name.contains(['/', '\\'])
    }

    #[test]
    fn manifest_round_trip_with_hostile_bytes() {
        let entries = vec![
            ManifestEntry {
                origin: Origin::Set,
                copy: "Disk 1.st".into(),
                entry: None,
                path: b"/roms/a\tb\nc%d\xff".to_vec(),
                size: 737_280,
                mtime: Some(1_700_000_000_123_456_789),
                crc: 0xdead_beef,
            },
            ManifestEntry {
                origin: Origin::Added,
                copy: "02 x.msa".into(),
                entry: Some("inner.msa".into()),
                path: b"C:\\roms\\x.zip".to_vec(),
                size: 1,
                mtime: None,
                crc: 0,
            },
        ];
        let bytes = encode(&entries);
        assert_eq!(decode(&bytes, any_name), Some(entries));
        assert_eq!(decode(&encode(&[]), any_name), Some(Vec::new()));
    }

    #[test]
    fn malformed_manifests_are_refused() {
        let ok = "emu-rust working copies 1\nset\ta.st\t\t1\t-\t00000001\t/a\n";
        assert!(decode(ok.as_bytes(), any_name).is_some());
        for bad in [
            "",
            "emu-rust working copies 2\n",
            "emu-rust working copies 1\nset\ta.st\t\t1\t-\t00000001\n",
            "emu-rust working copies 1\nset\t../x\t\t1\t-\t00000001\t/a\n",
            "emu-rust working copies 1\nset\t.emu-rust-disks\t\t1\t-\t00000001\t/a\n",
            "emu-rust working copies 1\nother\ta.st\t\t1\t-\t00000001\t/a\n",
            "emu-rust working copies 1\nset\ta.st\t\tx\t-\t00000001\t/a\n",
            "emu-rust working copies 1\nset\ta.st\t\t1\t-\t1\t/a\n",
            "emu-rust working copies 1\nset\ta%2\t\t1\t-\t00000001\t/a\n",
            "emu-rust working copies 1\nset\ta.st\t\t1\t-\t00000001\t/a",
            "emu-rust working copies 1\nset\ta.st\t\t1\t-\t00000001\t/a\nset\tA.ST\t\t1\t-\t00000001\t/b\n",
            "emu-rust working copies 1\nset\tGame.M3U\t\t1\t-\t00000001\t/a\n",
        ] {
            assert_eq!(decode(bad.as_bytes(), any_name), None, "{bad:?}");
        }
    }

    #[test]
    fn adopted_entries_have_no_source_and_never_match_one() {
        let adopted = ManifestEntry::adopted("02 X.st".into(), 4);
        assert_eq!(adopted.source_path(), None);
        let bytes = encode(std::slice::from_ref(&adopted));
        assert_eq!(decode(&bytes, any_name), Some(vec![adopted.clone()]));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x.st");
        fs::write(&path, b"abcd").unwrap();
        let mut source = Source::open(&path, 1024).unwrap();
        // Same size, and a CRC field that happens to match: still not this source's copy.
        let mut forged = adopted;
        forged.crc = source.crc().unwrap();
        assert!(!source.matches(&forged).unwrap());
    }
}
