//! Execution, undo, recovery and identity marker of the ROM tidying (ADR 0023 §4.5, §4.6, §4.8; design v7.1
//! §3.4.1, which is authoritative). Every change goes through [`Sys`] (fault injection in the tests).
//!
//! Invariants: a file is moved by `hard_link` + `remove_file` (or a folder by one `rename` after an absence
//! check), never copied, never overwritten; a folder is only removed by `remove_dir` (so empty); every operation is
//! announced by a synced journal line; a block without `C` is undone from what is on disk; a journal of another
//! root is never replayed. The OS lock `journal_dir/lock` is held for the whole call.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;

use super::journal::{
    self, Block, Content, JournalWriter, MAX_FORWARD_SIZE, MAX_UNDO_SIZE, MTIME_TOLERANCE_SECS, Op,
    State, same_place,
};
use super::relocate;
use super::sys::Sys;
use super::{
    AbsoluteMoves, TidyBlock, TidyExtraName, TidyOptions, TidyPlan, TidyProgress, TidyReport,
    TidySection, TidyStatus,
};
use crate::scan::mtime_of;
use crate::{LibraryError, TidyRefusal};

/// Journals kept by the purge (only those whose last line is `I` are ever removed beyond them).
pub(super) const KEPT_JOURNALS: usize = 20;

fn io_at(path: &Path, source: io::Error) -> LibraryError {
    LibraryError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn refused(journal: &Path, reason: TidyRefusal) -> LibraryError {
    LibraryError::TidyRefused {
        journal: journal.to_path_buf(),
        reason,
    }
}

/// A journal that could not be written or finished: an I/O error, never an « unreadable journal » (RV3 6).
fn write_error(path: &Path, message: &str) -> LibraryError {
    io_at(path, io::Error::other(message.to_owned()))
}

/// Takes the OS lock of `journal_dir/lock` (released by the system if the process dies).
pub(super) fn lock(journal_dir: &Path) -> Result<File, LibraryError> {
    fs::create_dir_all(journal_dir).map_err(|e| io_at(journal_dir, e))?;
    let path = journal_dir.join("lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| io_at(&path, e))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(fs::TryLockError::WouldBlock) => Err(LibraryError::TidyBusy),
        Err(fs::TryLockError::Error(e)) => Err(io_at(&path, e)),
    }
}

/// What is at a relative path, never following a link (FV0 4): every component is checked.
#[derive(Debug)]
enum Probe {
    Absent,
    File(fs::Metadata),
    Dir,
    /// A component is a link (symlink, junction), or an intermediate component is not a folder.
    Link,
    /// A special file (FIFO, device…).
    Other,
    Error(io::Error),
}

fn probe(root: &Path, rel: &Path) -> Probe {
    let mut at = root.to_path_buf();
    let count = rel.components().count();
    for (i, c) in rel.components().enumerate() {
        at.push(c);
        let meta = match fs::symlink_metadata(&at) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Probe::Absent,
            Err(e) => return Probe::Error(e),
        };
        if meta.file_type().is_symlink() {
            return Probe::Link;
        }
        if i + 1 < count {
            if !meta.is_dir() {
                return Probe::Link;
            }
        } else if meta.is_file() {
            return Probe::File(meta);
        } else if meta.is_dir() {
            return Probe::Dir;
        } else {
            return Probe::Other;
        }
    }
    Probe::Dir
}

/// The parent folder of `rel` exists as a plain folder (the root counts).
fn parent_ok(root: &Path, rel: &Path) -> bool {
    match rel.parent().filter(|p| !p.as_os_str().is_empty()) {
        None => true,
        Some(parent) => matches!(probe(root, parent), Probe::Dir),
    }
}

/// Folder listings read by [`free`], kept while the checks of one block run (FV1a m8: one read per folder, not
/// one per file). Never kept across a change made by the block.
type Listings = HashMap<PathBuf, Vec<OsString>>;

/// An entry whose name folds like `rel`'s in its parent folder (case-insensitive volumes, Unicode forms).
fn existing_folded(root: &Path, rel: &Path, listings: &mut Listings) -> Option<PathBuf> {
    let parent = rel.parent().unwrap_or(Path::new(""));
    let name = rel.file_name()?;
    if !listings.contains_key(parent) {
        let names = fs::read_dir(root.join(parent))
            .ok()?
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect();
        listings.insert(parent.to_path_buf(), names);
    }
    listings
        .get(parent)?
        .iter()
        .find(|n| same_place(Path::new(n), Path::new(name)))
        .map(|n| parent.join(n))
}

/// Free for a new entry: nothing at `rel` and nothing that folds like it; `Err(existing)` otherwise.
fn free(root: &Path, rel: &Path, listings: &mut Listings) -> Result<(), PathBuf> {
    match probe(root, rel) {
        Probe::Absent => match existing_folded(root, rel, listings) {
            Some(existing) => Err(existing),
            None => Ok(()),
        },
        _ => Err(rel.to_path_buf()),
    }
}

/// « Same state as the line »: equal size, mtime within [`MTIME_TOLERANCE_SECS`].
fn same_state(meta: &fs::Metadata, size: u64, mtime: i64) -> bool {
    meta.len() == size && (mtime_of(meta) - mtime).abs() <= MTIME_TOLERANCE_SECS
}

/// Number of names of the file at `path` (Unix: `st_nlink`; Windows: the handle's link count). `None`: unknown
/// (handle impossible to open), so nothing is proven.
#[cfg(unix)]
fn links_of(_path: &Path, meta: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.nlink())
}

#[cfg(windows)]
fn links_of(path: &Path, _meta: &fs::Metadata) -> Option<u64> {
    let handle = winapi_util::Handle::from_path_any(path).ok()?;
    Some(
        winapi_util::file::information(&handle)
            .ok()?
            .number_of_links(),
    )
}

#[cfg(not(any(unix, windows)))]
fn links_of(_path: &Path, _meta: &fs::Metadata) -> Option<u64> {
    None
}

/// First of `rels` (relative to `root`) carrying the Windows « read-only » attribute: the removal of such a name
/// is refused although linking it is allowed (ADR 0023 §4.8, RV3 1). The attribute is never changed. Unix: the
/// removal depends on the folder's rights, which the write classification handles.
#[cfg(windows)]
pub(super) fn first_read_only<'a>(
    root: &Path,
    rels: impl IntoIterator<Item = &'a Path>,
) -> Option<PathBuf> {
    rels.into_iter()
        .find(|rel| fs::symlink_metadata(root.join(rel)).is_ok_and(|m| m.permissions().readonly()))
        .map(Path::to_path_buf)
}

#[cfg(not(windows))]
pub(super) fn first_read_only<'a>(
    _root: &Path,
    _rels: impl IntoIterator<Item = &'a Path>,
) -> Option<PathBuf> {
    None
}

/// `(files, size, newest mtime)` of the regular files below `rel` (not following links); `None` when it holds a
/// link or a special file, or is deeper than `max_depth`.
fn aggregate(root: &Path, rel: &Path, max_depth: usize) -> Option<(u32, u64, i64)> {
    let mut stack = vec![(root.join(rel), 0usize)];
    let (mut files, mut size, mut mtime) = (0u32, 0u64, 0i64);
    while let Some((dir, depth)) = stack.pop() {
        if depth > max_depth {
            return None;
        }
        for entry in fs::read_dir(&dir).ok()? {
            let entry = entry.ok()?;
            let meta = fs::symlink_metadata(entry.path()).ok()?;
            if meta.file_type().is_symlink() {
                return None;
            }
            if meta.is_dir() {
                stack.push((entry.path(), depth + 1));
            } else if meta.is_file() {
                files = files.checked_add(1)?;
                size = size.saturating_add(meta.len());
                mtime = mtime.max(mtime_of(&meta));
            } else {
                return None;
            }
        }
    }
    Some((files, size, mtime))
}

fn depth(path: &Path) -> usize {
    path.components().count()
}

/// Folders from the shallowest to the deepest, ancestors of `rel` (strict), the root excluded.
fn ancestors(rel: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = rel
        .ancestors()
        .skip(1)
        .filter(|a| !a.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();
    out.reverse();
    out
}

fn is_dir_empty(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut it| it.next().is_none())
}

/// One file move of a block to run.
#[derive(Debug, Clone)]
struct FileMove {
    n: u32,
    from: PathBuf,
    to: PathBuf,
    size: u64,
    mtime: i64,
}

#[derive(Debug, Clone)]
struct WholeMove {
    files: u32,
    size: u64,
    mtime: i64,
    from: PathBuf,
    to: PathBuf,
}

/// A block about to be run (forward or undo).
#[derive(Debug, Clone)]
struct Spec {
    unit: u32,
    files: Vec<FileMove>,
    whole: Option<WholeMove>,
    /// `<D>` of an assembled unit (links made under `<D>.emu-rust-part`, renamed at once).
    assembly: Option<PathBuf>,
    /// Folders to remove when emptied by the block, deepest first.
    emptied: Vec<PathBuf>,
}

impl Spec {
    fn pairs(&self) -> Vec<(PathBuf, PathBuf)> {
        let mut out: Vec<(PathBuf, PathBuf)> = self
            .files
            .iter()
            .map(|f| (f.from.clone(), f.to.clone()))
            .collect();
        out.extend(self.whole.iter().map(|w| (w.from.clone(), w.to.clone())));
        out
    }

    fn part(&self) -> Option<PathBuf> {
        let d = self.assembly.as_ref()?;
        let mut name = d.file_name()?.to_os_string();
        name.push(super::plan::PART_SUFFIX);
        Some(d.with_file_name(name))
    }

    /// Where the link of `to` is made: under the assembly folder for an assembled unit.
    fn link_place(&self, to: &Path) -> PathBuf {
        match (&self.assembly, self.part()) {
            (Some(d), Some(part)) => match to.strip_prefix(d) {
                Ok(rest) => part.join(rest),
                Err(_) => to.to_path_buf(),
            },
            _ => to.to_path_buf(),
        }
    }

    /// Folders the block will need, from the shallowest: outside the assembly (`<D>` ancestors, parents of the
    /// destinations), then inside it (named by their final path under `<D>`).
    fn needed_dirs(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut outside = BTreeSet::new();
        let mut inside = BTreeSet::new();
        let targets: Vec<&Path> = match (&self.assembly, &self.whole) {
            (Some(d), _) => {
                outside.extend(ancestors(d));
                for f in &self.files {
                    for a in ancestors(&f.to) {
                        if a.strip_prefix(d).is_ok_and(|r| !r.as_os_str().is_empty()) {
                            inside.insert(a);
                        }
                    }
                }
                Vec::new()
            }
            (None, Some(w)) => vec![w.to.as_path()],
            (None, None) => self.files.iter().map(|f| f.to.as_path()).collect(),
        };
        for t in targets {
            outside.extend(ancestors(t));
        }
        let by_depth = |set: BTreeSet<PathBuf>| {
            let mut v: Vec<PathBuf> = set.into_iter().collect();
            v.sort_by_key(|p| depth(p));
            v
        };
        (by_depth(outside), by_depth(inside))
    }

    /// Upper bound of the block's journal size.
    fn max_journal_len(&self) -> Option<u64> {
        let (outside, inside) = self.needed_dirs();
        let mut ops = vec![Op::Begin(self.unit), Op::Commit(self.unit)];
        ops.extend(outside.into_iter().chain(inside).map(Op::DirCreated));
        ops.extend(self.part().map(Op::Part));
        ops.extend(self.emptied.iter().cloned().map(Op::DirRemoved));
        for f in &self.files {
            ops.push(move_op(f));
            ops.push(Op::MoveDone(f.n));
        }
        if let Some(w) = &self.whole {
            ops.push(whole_op(w));
            ops.push(Op::WholeDone);
        }
        journal::encoded_len(&ops)
    }
}

fn move_op(f: &FileMove) -> Op {
    Op::Move {
        n: f.n,
        size: f.size,
        mtime: f.mtime,
        from: f.from.clone(),
        to: f.to.clone(),
    }
}

fn whole_op(w: &WholeMove) -> Op {
    Op::Whole {
        files: w.files,
        size: w.size,
        mtime: w.mtime,
        from: w.from.clone(),
        to: w.to.clone(),
    }
}

/// How a block ended.
enum Outcome {
    Committed {
        created: u64,
        removed: u64,
    },
    /// Undone, left in place for this reason.
    Left(TidyBlock),
    /// Undone after an I/O error (French message).
    Failed(String),
}

/// The run must stop: the journal could not be written, or undoing a block failed (RV1 I2). The journal stays in
/// progress for the next recovery.
struct Fatal(LibraryError);

pub(super) struct Exec<'a> {
    sys: &'a dyn Sys,
    conn: &'a mut Connection,
    options: &'a TidyOptions,
    root: PathBuf,
    report: TidyReport,
}

impl<'a> Exec<'a> {
    pub(super) fn new(
        sys: &'a dyn Sys,
        conn: &'a mut Connection,
        options: &'a TidyOptions,
    ) -> Self {
        Exec {
            sys,
            conn,
            options,
            root: options.roms_root().to_path_buf(),
            report: TidyReport::default(),
        }
    }

    fn signal(&mut self, rel: &Path, message: &str) {
        self.report
            .errors
            .push((self.root.join(rel), message.to_owned()));
    }

    fn abs(&self, rel: &Path) -> PathBuf {
        self.root.join(rel)
    }

    fn absolute(&self, moves: Vec<(PathBuf, PathBuf)>) -> AbsoluteMoves {
        moves
            .into_iter()
            .map(|(a, b)| (self.abs(&a), self.abs(&b)))
            .collect()
    }

    // -----------------------------------------------------------------------------------------------------------
    // Undoing an open block (recovery rule, identical for the forward and the undo section)

    /// Undoes `block` from what is on disk. Cases that cannot be proven safe are left alone and signalled; an I/O
    /// error is returned (RV1 I2: the caller stops without closing the section).
    fn rollback(&mut self, block: &Block) -> Result<(), LibraryError> {
        let root = self.root.clone();
        let part = block.part().map(Path::to_path_buf);
        let final_dir = block.final_dir();
        let in_part = |rel: &Path| -> Option<PathBuf> {
            let (p, d) = (part.as_ref()?, final_dir.as_ref()?);
            rel.strip_prefix(d).ok().map(|rest| p.join(rest))
        };
        // 1. Folders removed by the block come back (shallowest first: lines were written deepest first).
        for op in block.body.iter().rev() {
            if let Op::DirRemoved(d) = op {
                match probe(&root, d) {
                    Probe::Absent if parent_ok(&root, d) => {
                        self.sys
                            .create_dir(&root.join(d))
                            .map_err(|e| io_at(&root.join(d), e))?;
                    }
                    Probe::Dir => {}
                    Probe::Error(e) => return Err(io_at(&root.join(d), e)),
                    Probe::Absent => self.signal(
                        d,
                        "dossier à recréer : dossier parent absent ou remplacé, laissé tel quel",
                    ),
                    _ => self.signal(d, "dossier à recréer : emplacement occupé, laissé tel quel"),
                }
            }
        }
        // 2-3. Files and folders go back to their source.
        for op in block.body.iter().rev() {
            match op {
                Op::Move {
                    n,
                    size,
                    mtime,
                    from,
                    to,
                } => {
                    let done = block.is_done(&Op::MoveDone(*n));
                    let mut places = vec![to.clone()];
                    places.extend(in_part(to));
                    for place in places {
                        self.return_file(&place, from, *size, *mtime, done)?;
                    }
                }
                Op::Whole {
                    files,
                    size,
                    mtime,
                    from,
                    to,
                } => {
                    let done = block.is_done(&Op::WholeDone);
                    self.return_dir(to, from, (*files, *size, *mtime), done)?;
                }
                _ => {}
            }
        }
        // 4. Cleanup (RV1 I3): created folders deepest first at both places (silent when not empty), then the
        // assembly folder, then `<D>`.
        let mut created: Vec<PathBuf> = block
            .body
            .iter()
            .filter_map(|op| match op {
                Op::DirCreated(d) => Some(d.clone()),
                _ => None,
            })
            .collect();
        created.sort_by_key(|d| std::cmp::Reverse(depth(d)));
        for d in &created {
            let mut places = vec![d.clone()];
            places.extend(in_part(d));
            for place in places {
                self.remove_dir(&place, true)?;
            }
        }
        if let Some(p) = &part {
            self.remove_dir(p, false)?;
        }
        if let Some(d) = &final_dir {
            self.remove_dir(d, false)?;
        }
        // 5. Database: inverse rewrite of what came back, unless a row already holds the source key (I5).
        let back: Vec<(PathBuf, PathBuf)> = block
            .pairs()
            .into_iter()
            .filter(|(from, _)| matches!(probe(&root, from), Probe::File(_) | Probe::Dir))
            .collect();
        if !back.is_empty() {
            relocate::revert(self.conn, self.options.scan_root(), &back)?;
        }
        Ok(())
    }

    /// `sync_dir` of the folder holding `rel` (the root counts).
    fn sync_parent(&self, rel: &Path) -> Result<(), LibraryError> {
        let path = self.abs(rel.parent().unwrap_or(Path::new("")));
        self.sys.sync_dir(&path).map_err(|e| io_at(&path, e))
    }

    fn remove_dir(&mut self, rel: &Path, quiet: bool) -> Result<(), LibraryError> {
        if !matches!(probe(&self.root, rel), Probe::Dir) {
            return Ok(());
        }
        let path = self.abs(rel);
        match self.sys.remove_dir(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::DirectoryNotEmpty || !is_dir_empty(&path) => {
                if !quiet {
                    self.signal(rel, "dossier non vide, laissé en place");
                }
                Ok(())
            }
            Err(e) => Err(io_at(&path, e)),
        }
    }

    /// Recovery of one `M` at `place` (its destination, or the same place in the assembly folder).
    fn return_file(
        &mut self,
        place: &Path,
        source: &Path,
        size: u64,
        mtime: i64,
        done: bool,
    ) -> Result<(), LibraryError> {
        let root = self.root.clone();
        let dest_meta = match probe(&root, place) {
            Probe::Absent => return Ok(()),
            Probe::File(m) => m,
            Probe::Error(e) => return Err(io_at(&root.join(place), e)),
            _ => {
                self.signal(
                    place,
                    "emplacement rangé occupé par un lien ou un dossier : rien n'est touché",
                );
                return Ok(());
            }
        };
        match probe(&root, source) {
            Probe::Absent => {
                if !(done || same_state(&dest_meta, size, mtime)) {
                    self.signal(
                        place,
                        "fichier que le journal ne prouve pas avoir déplacé : laissé en place",
                    );
                    return Ok(());
                }
                if !parent_ok(&root, source) {
                    self.signal(
                        source,
                        "dossier d'origine absent ou remplacé : fichier laissé rangé",
                    );
                    return Ok(());
                }
                let (from, to) = (root.join(place), root.join(source));
                self.sys.hard_link(&from, &to).map_err(|e| io_at(&to, e))?;
                // The name given back durable before the other one goes (FV1a I2).
                self.sync_parent(source)?;
                if self.remove_second_name(&from, &to)? {
                    self.sync_parent(place)?;
                }
                Ok(())
            }
            Probe::File(_) => {
                match self.identical(source, place, size) {
                    Identity::Same => {
                        // The kept name durable before the other one goes (RV3 5).
                        self.sync_parent(source)?;
                        let (path, keep) = (root.join(place), root.join(source));
                        if self.remove_second_name(&path, &keep)? {
                            self.sync_parent(place)?;
                        }
                    }
                    Identity::Different => self.signal(
                        place,
                        "fichier présent à l'origine et à la destination, différent : rien n'est touché",
                    ),
                    Identity::Unproven => self.signal(
                        place,
                        "fichier présent à l'origine et à la destination, identité impossible à vérifier : \
                         rien n'est touché",
                    ),
                }
                Ok(())
            }
            Probe::Error(e) => Err(io_at(&root.join(source), e)),
            _ => {
                self.signal(
                    source,
                    "emplacement d'origine occupé par un lien ou un dossier : rien n'est touché",
                );
                Ok(())
            }
        }
    }

    /// Removes `remove`, a name proven to be a second one of the file also named `keep`. `Ok(false)`: access refused
    /// (read-only attribute, rights) while the two names are still proven the same file: not an error, the name
    /// goes to `TidyReport::extra_names` for the user (RV3 3). Any other failure is returned.
    fn remove_second_name(&mut self, remove: &Path, keep: &Path) -> Result<bool, LibraryError> {
        match self.sys.remove_file(remove) {
            Ok(()) => Ok(true),
            Err(e)
                if is_denied(&e)
                    && distinct_names(remove, keep)
                    && same_file::is_same_file(remove, keep).unwrap_or(false) =>
            {
                self.report.extra_names.push(TidyExtraName {
                    remove: remove.to_path_buf(),
                    keep: keep.to_path_buf(),
                });
                Ok(false)
            }
            Err(e) => Err(io_at(remove, e)),
        }
    }

    /// Two names of one file, proven by the file system (FV1a I3: `same-file`, Unix and Windows, any size): only
    /// then may the destination go. Two equal copies are not enough. It also wants two links on the destination (Unix
    /// and Windows),
    /// read again just before (RV1 I1: one name seen twice is never removed), and the two canonical paths must
    /// differ.
    fn identical(&self, source: &Path, place: &Path, size: u64) -> Identity {
        let (src, dst) = (self.root.join(source), self.root.join(place));
        let (Ok(src_meta), Ok(again)) = (fs::symlink_metadata(&src), fs::symlink_metadata(&dst))
        else {
            return Identity::Unproven;
        };
        if same_place(source, place) || src_meta.len() != size || again.len() != size {
            return Identity::Different;
        }
        match links_of(&dst, &again) {
            Some(n) if n >= 2 => {}
            Some(_) => return Identity::Different,
            None => return Identity::Unproven,
        }
        if !distinct_names(&src, &dst) {
            return Identity::Different;
        }
        // ReFS: the 64-bit file index `same-file` compares is not guaranteed unique there (RV3 5), so Windows also
        // wants both dates exactly equal on the two names.
        #[cfg(windows)]
        {
            let dates = |m: &fs::Metadata| Some((m.created().ok()?, m.modified().ok()?));
            match (dates(&src_meta), dates(&again)) {
                (Some(a), Some(b)) if a == b => {}
                (Some(_), Some(_)) => return Identity::Different,
                _ => return Identity::Unproven,
            }
        }
        match same_file::is_same_file(&src, &dst) {
            Ok(true) => Identity::Same,
            Ok(false) => Identity::Different,
            // Sharing refused (Windows), rights…: nothing proven, nothing removed.
            Err(_) => Identity::Unproven,
        }
    }

    fn return_dir(
        &mut self,
        place: &Path,
        source: &Path,
        line: (u32, u64, i64),
        done: bool,
    ) -> Result<(), LibraryError> {
        let root = self.root.clone();
        match probe(&root, place) {
            Probe::Absent => return Ok(()),
            Probe::Dir => {}
            Probe::Error(e) => return Err(io_at(&root.join(place), e)),
            _ => {
                self.signal(place, "dossier rangé remplacé : rien n'est touché");
                return Ok(());
            }
        }
        match probe(&root, source) {
            Probe::Absent => {
                let same = aggregate(&root, place, self.options.max_depth()).is_some_and(
                    |(files, size, mtime)| {
                        files == line.0
                            && size == line.1
                            && (mtime - line.2).abs() <= MTIME_TOLERANCE_SECS
                    },
                );
                if !(done || same) {
                    self.signal(
                        place,
                        "dossier que le journal ne prouve pas avoir déplacé : laissé en place",
                    );
                    return Ok(());
                }
                if !parent_ok(&root, source) {
                    self.signal(
                        source,
                        "dossier d'origine absent ou remplacé : dossier laissé rangé",
                    );
                    return Ok(());
                }
                let (from, to) = (root.join(place), root.join(source));
                self.sys.rename(&from, &to).map_err(|e| io_at(&to, e))?;
                // Both folders durable before the cleanup removes anything (FV1a I2).
                self.sync_parent(source)?;
                self.sync_parent(place)
            }
            Probe::Error(e) => Err(io_at(&root.join(source), e)),
            _ => {
                self.signal(
                    place,
                    "dossier présent à l'origine et à la destination : rien n'est touché",
                );
                Ok(())
            }
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // Running a block

    /// Runs `spec` as block `B u … C u`. `Err(Fatal)`: stop the run, nothing more is written.
    fn run_block(&mut self, writer: &mut JournalWriter, spec: &Spec) -> Result<Outcome, Fatal> {
        let mut block = Block {
            unit: spec.unit,
            body: Vec::new(),
            committed: false,
        };
        match self.run_steps(writer, spec, &mut block) {
            Ok(outcome) => Ok(outcome),
            // A failed journal write stops at once, nothing undone (FV1a I1): the line may be on disk even though
            // the write reported an error, so only the recovery, reading the journal as it really is, may decide.
            Err(Step::Journal(e)) => Err(Fatal(e)),
            Err(step) => {
                // A failed undo stops everything; otherwise the block is undone and the run goes on.
                if let Err(e) = self.rollback(&block) {
                    return Err(Fatal(e));
                }
                match step {
                    Step::Journal(e) => Err(Fatal(e)),
                    Step::Left(b) => Ok(Outcome::Left(b)),
                    Step::Io(e) => Ok(Outcome::Failed(self.message_of(e))),
                }
            }
        }
    }

    fn write(&mut self, writer: &mut JournalWriter, block: &mut Block, op: Op) -> Result<(), Step> {
        writer
            .append(self.sys, std::slice::from_ref(&op))
            .map_err(Step::Journal)?;
        if !matches!(op, Op::Begin(_) | Op::Commit(_)) {
            block.body.push(op);
        }
        Ok(())
    }

    fn create_dir(
        &mut self,
        writer: &mut JournalWriter,
        block: &mut Block,
        rel: &Path,
        on_disk: &Path,
        created: &mut BTreeSet<PathBuf>,
    ) -> Result<bool, Step> {
        // D+ only when this block sees the folder missing, just before creating it (RV1 I4).
        match probe(&self.root, on_disk) {
            Probe::Dir => return Ok(false),
            Probe::Absent => {}
            _ => {
                return Err(Step::Left(TidyBlock::Conflict {
                    existing: on_disk.to_path_buf(),
                }));
            }
        }
        self.write(writer, block, Op::DirCreated(rel.to_path_buf()))?;
        let path = self.abs(on_disk);
        self.sys
            .create_dir(&path)
            .map_err(|e| self.write_step(&path, e, false))?;
        created.insert(on_disk.parent().map(Path::to_path_buf).unwrap_or_default());
        Ok(true)
    }

    /// `path` relative to the roms root (`.` for the root itself).
    fn rel_of(&self, path: &Path) -> PathBuf {
        match path.strip_prefix(&self.root) {
            Ok(rel) if rel.as_os_str().is_empty() => PathBuf::from("."),
            Ok(rel) => rel.to_path_buf(),
            Err(_) => path.to_path_buf(),
        }
    }

    /// A refused write of `target` (absolute), before any source is removed: the unit is left, or the error kept
    /// (RV3 2: « no hard links » is not « write refused »).
    fn write_step(&self, target: &Path, e: io::Error, link: bool) -> Step {
        match refusal(&e, link) {
            Refusal::OtherVolume => Step::Left(TidyBlock::OtherVolume),
            Refusal::NoHardLinks => Step::Left(TidyBlock::NoHardLinks),
            Refusal::NotWritable => Step::Left(TidyBlock::NotWritable {
                dir: without_part(self.rel_of(target.parent().unwrap_or(target))),
            }),
            Refusal::Exists => Step::Left(TidyBlock::Conflict {
                existing: without_part(self.rel_of(target)),
            }),
            Refusal::Other => Step::Io(io_at(target, e)),
        }
    }

    /// Why the removal of `rel`, a proven second name, was refused.
    fn refused_removal(&self, rel: &Path) -> TidyBlock {
        match first_read_only(&self.root, [rel]) {
            Some(path) => TidyBlock::ReadOnlyFile { path },
            None => TidyBlock::NotWritable {
                dir: self.rel_of(&self.abs(rel.parent().unwrap_or(Path::new("")))),
            },
        }
    }

    fn sync_dirs(&self, dirs: BTreeSet<PathBuf>) -> Result<(), Step> {
        for d in dirs {
            let path = self.abs(&d);
            self.sys
                .sync_dir(&path)
                .map_err(|e| Step::Io(io_at(&path, e)))?;
        }
        Ok(())
    }

    fn run_steps(
        &mut self,
        writer: &mut JournalWriter,
        spec: &Spec,
        block: &mut Block,
    ) -> Result<Outcome, Step> {
        let root = self.root.clone();
        let parent = |p: &Path| p.parent().map(Path::to_path_buf).unwrap_or_default();
        self.write(writer, block, Op::Begin(spec.unit))?;
        let mut created = 0u64;
        // Folders holding a folder made by this block: synced with the destinations (FV1a I2).
        let mut made_in = BTreeSet::new();
        let (outside, inside) = spec.needed_dirs();
        for d in &outside {
            created += u64::from(self.create_dir(writer, block, d, d, &mut made_in)?);
        }
        let part = spec.part();
        if let Some(p) = &part {
            if !matches!(probe(&root, p), Probe::Absent) {
                return Err(Step::Left(TidyBlock::Conflict {
                    existing: p.clone(),
                }));
            }
            self.write(writer, block, Op::Part(p.clone()))?;
            let path = root.join(p);
            self.sys
                .create_dir(&path)
                .map_err(|e| self.write_step(&path, e, false))?;
            made_in.insert(parent(p));
            for d in &inside {
                created += u64::from(self.create_dir(
                    writer,
                    block,
                    d,
                    &spec.link_place(d),
                    &mut made_in,
                )?);
            }
        }
        // Intentions, then the operations, each followed by its synced « done » line.
        for f in &spec.files {
            self.write(writer, block, move_op(f))?;
        }
        for f in &spec.files {
            let (from, to) = (root.join(&f.from), root.join(spec.link_place(&f.to)));
            self.sys
                .hard_link(&from, &to)
                .map_err(|e| self.write_step(&to, e, true))?;
            self.write(writer, block, Op::MoveDone(f.n))?;
        }
        if let Some(w) = &spec.whole {
            free(&root, &w.to, &mut Listings::new())
                .map_err(|existing| Step::Left(TidyBlock::Conflict { existing }))?;
            self.write(writer, block, whole_op(w))?;
            let (from, to) = (root.join(&w.from), root.join(&w.to));
            self.sys
                .rename(&from, &to)
                .map_err(|e| self.write_step(&to, e, false))?;
            self.write(writer, block, Op::WholeDone)?;
        }
        // Destinations durable before any source goes.
        let mut dest_dirs: BTreeSet<PathBuf> = spec
            .files
            .iter()
            .map(|f| parent(&spec.link_place(&f.to)))
            .collect();
        dest_dirs.extend(spec.whole.iter().map(|w| parent(&w.to)));
        dest_dirs.extend(made_in);
        self.sync_dirs(dest_dirs)?;
        if let (Some(p), Some(d)) = (&part, &spec.assembly) {
            free(&root, d, &mut Listings::new())
                .map_err(|existing| Step::Left(TidyBlock::Conflict { existing }))?;
            let (from, to) = (root.join(p), root.join(d));
            self.sys
                .rename(&from, &to)
                .map_err(|e| self.write_step(&to, e, false))?;
            self.sync_dirs(BTreeSet::from([parent(d)]))?;
        }
        for f in &spec.files {
            let path = root.join(&f.from);
            if !distinct_names(&path, &root.join(&f.to)) {
                return Err(Step::Io(io_at(
                    &path,
                    io::Error::other("origine et destination désignent le même nom"),
                )));
            }
            // Proven the same file before the source goes (RV3 6): a link that is not one is never a reason to remove.
            if !matches!(same_file::is_same_file(&path, root.join(&f.to)), Ok(true)) {
                return Err(Step::Io(io_at(
                    &path,
                    io::Error::other("origine et destination ne désignent pas le même fichier"),
                )));
            }
            if let Err(e) = self.sys.remove_file(&path) {
                // Refused removal of a proven name: the unit is undone (its link is the extra name if that fails
                // too), nothing is lost (RV3 3).
                return Err(if is_denied(&e) {
                    Step::Left(self.refused_removal(&f.from))
                } else {
                    Step::Io(io_at(&path, e))
                });
            }
        }
        let mut source_dirs: BTreeSet<PathBuf> =
            spec.files.iter().map(|f| parent(&f.from)).collect();
        source_dirs.extend(spec.whole.iter().map(|w| parent(&w.from)));
        self.sync_dirs(source_dirs)?;
        let mut removed = 0;
        for d in &spec.emptied {
            if matches!(probe(&root, d), Probe::Dir) && is_dir_empty(&root.join(d)) {
                self.write(writer, block, Op::DirRemoved(d.clone()))?;
                let path = root.join(d);
                match self.sys.remove_dir(&path) {
                    Ok(()) => removed += 1,
                    Err(e) if e.kind() == io::ErrorKind::DirectoryNotEmpty => {}
                    // The files are moved: a folder that cannot be removed is left, the unit stays done.
                    Err(e) if is_denied(&e) => {
                        self.signal(d, "dossier vidé laissé en place (suppression refusée)");
                    }
                    Err(e) => return Err(Step::Io(io_at(&path, e))),
                }
            }
        }
        self.sys
            .checkpoint()
            .map_err(|e| Step::Io(io_at(&root, e)))?;
        relocate::run(self.conn, self.options.scan_root(), &spec.pairs()).map_err(Step::Io)?;
        self.sys
            .checkpoint()
            .map_err(|e| Step::Io(io_at(&root, e)))?;
        self.write(writer, block, Op::Commit(spec.unit))?;
        Ok(Outcome::Committed { created, removed })
    }

    /// French message of an I/O error without the absolute path (the report already carries it as the key).
    fn message_of(&self, e: LibraryError) -> String {
        match e {
            LibraryError::Io { path, source } => match path.strip_prefix(&self.root) {
                Ok(rel) if !rel.as_os_str().is_empty() => format!("{} : {source}", rel.display()),
                _ => source.to_string(),
            },
            other => other.to_string(),
        }
    }

    fn record(&mut self, first: &Path, outcome: Outcome) {
        match outcome {
            Outcome::Committed { created, removed } => {
                self.report.dirs_created += created;
                self.report.dirs_removed += removed;
            }
            Outcome::Left(block) => self.report.left.push((self.abs(first), block)),
            Outcome::Failed(message) => self.report.errors.push((self.abs(first), message)),
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // Forward run

    /// Checks of a unit just before its block (FV0 4, ChangedSincePlan); the spec to run.
    fn forward_spec(&self, plan: &TidyPlan, index: usize) -> Result<Spec, TidyBlock> {
        let root = &self.root;
        let unit = &plan.units[index];
        let prints = plan.fingerprints(unit.id).unwrap_or(&[]);
        let pairs: Vec<(&Path, &Path)> = unit
            .moves
            .iter()
            .map(|m| (m.from.as_path(), m.to.as_path()))
            .chain(
                unit.whole_dir
                    .iter()
                    .map(|w| (w.from.as_path(), w.to.as_path())),
            )
            .collect();
        if pairs.iter().any(|(from, to)| same_place(from, to)) {
            return Err(TidyBlock::Conflict {
                existing: pairs[0].0.to_path_buf(),
            });
        }
        for print in prints {
            match probe(root, &print.path) {
                Probe::File(m) if m.len() == print.size && mtime_of(&m) == print.mtime => {}
                Probe::Link => return Err(TidyBlock::Symlink),
                _ => return Err(TidyBlock::ChangedSincePlan),
            }
        }
        let emptied = |sources: &mut dyn Iterator<Item = &Path>| -> Vec<PathBuf> {
            let mut set = BTreeSet::new();
            for s in sources {
                for a in ancestors(s) {
                    let top_system =
                        depth(&a) == 1 && a.to_str().is_some_and(|n| crate::system(n).is_some());
                    if !top_system {
                        set.insert(a);
                    }
                }
            }
            let mut v: Vec<PathBuf> = set.into_iter().collect();
            v.sort_by_key(|p| std::cmp::Reverse(depth(p)));
            v
        };
        if let Some(w) = &unit.whole_dir {
            if !matches!(probe(root, &w.from), Probe::Dir) {
                return Err(TidyBlock::ChangedSincePlan);
            }
            let expected = (
                u32::try_from(prints.len()).unwrap_or(u32::MAX),
                prints.iter().map(|p| p.size).sum::<u64>(),
                prints.iter().map(|p| p.mtime).max().unwrap_or(0),
            );
            if aggregate(root, &w.from, self.options.max_depth()) != Some(expected) {
                return Err(TidyBlock::ChangedSincePlan);
            }
            self.destination_ok(&w.to, &mut Listings::new())?;
            return Ok(Spec {
                unit: unit.id,
                files: Vec::new(),
                whole: Some(WholeMove {
                    files: expected.0,
                    size: expected.1,
                    mtime: expected.2,
                    from: w.from.clone(),
                    to: w.to.clone(),
                }),
                assembly: None,
                emptied: emptied(&mut std::iter::once(w.from.as_path())),
            });
        }
        // Windows: a source with the read-only attribute cannot be removed after its link (RV3 1).
        if let Some(path) = first_read_only(root, unit.moves.iter().map(|m| m.from.as_path())) {
            return Err(TidyBlock::ReadOnlyFile { path });
        }
        let mut files = Vec::new();
        for (n, m) in unit.moves.iter().enumerate() {
            let print = prints
                .iter()
                .find(|p| p.path == m.from)
                .ok_or(TidyBlock::ChangedSincePlan)?;
            files.push(FileMove {
                n: u32::try_from(n).unwrap_or(u32::MAX),
                from: m.from.clone(),
                to: m.to.clone(),
                size: print.size,
                mtime: print.mtime,
            });
        }
        let assembly = plan.assembly_dir(index);
        let spec = Spec {
            unit: unit.id,
            emptied: emptied(&mut files.iter().map(|f| f.from.as_path())),
            files,
            whole: None,
            assembly,
        };
        match (&spec.assembly, spec.part()) {
            (Some(d), Some(part)) => {
                let mut listings = Listings::new();
                self.destination_ok(d, &mut listings)?;
                self.destination_ok(&part, &mut listings)?;
            }
            _ => {
                let mut listings = Listings::new();
                for f in &spec.files {
                    self.destination_ok(&f.to, &mut listings)?;
                }
            }
        }
        Ok(spec)
    }

    /// Every existing component a plain folder, the entry itself free.
    fn destination_ok(&self, rel: &Path, listings: &mut Listings) -> Result<(), TidyBlock> {
        for a in ancestors(rel) {
            match probe(&self.root, &a) {
                Probe::Dir | Probe::Absent => {}
                Probe::Link => return Err(TidyBlock::DestinationNotPlainDir { dir: a }),
                _ => return Err(TidyBlock::Conflict { existing: a }),
            }
        }
        free(&self.root, rel, listings).map_err(|existing| TidyBlock::Conflict { existing })
    }

    pub(super) fn apply(
        mut self,
        plan: &TidyPlan,
        selected: &[u32],
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&TidyProgress),
    ) -> Result<TidyReport, LibraryError> {
        let _lock = lock(self.options.journal_dir())?;
        if plan.roms_root() != self.root {
            return Err(io_at(
                &self.root,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "aperçu fait pour un autre dossier de ROMs",
                ),
            ));
        }
        preflight(self.options.journal_dir())?;
        let chosen: Vec<usize> = (0..plan.units.len())
            .filter(|&i| selected.contains(&plan.units[i].id))
            .collect();
        let total = chosen.len() as u64;
        let mut writer: Option<JournalWriter> = None;
        let mut closed = false;
        for (done, &index) in chosen.iter().enumerate() {
            let unit = &plan.units[index];
            let first = unit
                .whole_dir
                .as_ref()
                .map(|w| w.from.clone())
                .or_else(|| unit.moves.first().map(|m| m.from.clone()))
                .unwrap_or_else(|| PathBuf::from(&unit.title));
            if cancel.load(Ordering::Relaxed) {
                self.report.cancelled = true;
                break;
            }
            progress(&TidyProgress {
                done_units: done as u64,
                total_units: total,
                current: Some(self.abs(&first)),
            });
            let movable = match &unit.status {
                TidyStatus::Sure(_) | TidyStatus::Suggested(_) => unit.system.is_some(),
                TidyStatus::Ambiguous => unit.system.is_some(),
                TidyStatus::Blocked(b) => {
                    self.report.left.push((self.abs(&first), b.clone()));
                    continue;
                }
                TidyStatus::Unknown => false,
            };
            if !movable || (unit.moves.is_empty() && unit.whole_dir.is_none()) {
                self.report.errors.push((
                    self.abs(&first),
                    "aucun système choisi : laissé en place".to_owned(),
                ));
                continue;
            }
            if closed {
                self.report
                    .errors
                    .push((self.abs(&first), JOURNAL_FULL.to_owned()));
                continue;
            }
            let spec = match self.forward_spec(plan, index) {
                Ok(spec) => spec,
                Err(block) => {
                    self.report.left.push((self.abs(&first), block));
                    continue;
                }
            };
            let current = writer.as_ref().map_or_else(
                || journal::header(&self.root).map_or(0, |h| h.len() as u64),
                JournalWriter::len,
            );
            // A path the journal cannot hold leaves this unit only (FV1a m5).
            let Some(len) = spec.max_journal_len() else {
                self.report
                    .errors
                    .push((self.abs(&first), UNJOURNALABLE.to_owned()));
                continue;
            };
            if current + len + 2 > MAX_FORWARD_SIZE {
                closed = true;
                self.report
                    .errors
                    .push((self.abs(&first), JOURNAL_FULL.to_owned()));
                continue;
            }
            if writer.is_none() {
                writer = Some(new_journal(self.sys, self.options)?);
            }
            let Some(w) = writer.as_mut() else { continue };
            match self.run_block(w, &spec) {
                Ok(outcome) => self.record(&first, outcome),
                Err(Fatal(e)) => return Err(e),
            }
        }
        if let Some(mut w) = writer {
            w.append(self.sys, &[Op::End])?;
            self.finish(w.path())?;
        }
        Ok(self.report)
    }

    /// Fills `journal` and `moved` (net list of the journal as written).
    fn finish(&mut self, path: &Path) -> Result<(), LibraryError> {
        let journal = journal::read(path)?.ok_or_else(|| write_error(path, "journal vide"))?;
        self.report.journal = Some(path.to_path_buf());
        self.report.moved = self.absolute(journal.net_moves());
        Ok(())
    }

    // -----------------------------------------------------------------------------------------------------------
    // Undo

    /// The undo block of a committed forward block, or why it stays (FV0 10).
    fn undo_spec(&self, forward: &Block) -> Result<Spec, TidyBlock> {
        let root = &self.root;
        let mut files = Vec::new();
        let mut whole = None;
        let mut listings = Listings::new();
        for op in &forward.body {
            match op {
                Op::Move {
                    n,
                    size,
                    mtime,
                    from,
                    to,
                } => {
                    let meta = match probe(root, to) {
                        Probe::File(m) => m,
                        _ => return Err(TidyBlock::ChangedSincePlan),
                    };
                    // One name only (an unreadable count proves nothing): never undo a file with another link.
                    if links_of(&root.join(to), &meta) != Some(1) {
                        return Err(TidyBlock::ChangedSincePlan);
                    }
                    if let Some(path) = first_read_only(root, [to.as_path()]) {
                        return Err(TidyBlock::ReadOnlyFile { path });
                    }
                    if !same_state(&meta, *size, *mtime) {
                        return Err(TidyBlock::ChangedSincePlan);
                    }
                    free(root, from, &mut listings)
                        .map_err(|existing| TidyBlock::Conflict { existing })?;
                    files.push(FileMove {
                        n: *n,
                        from: to.clone(),
                        to: from.clone(),
                        size: meta.len(),
                        mtime: mtime_of(&meta),
                    });
                }
                Op::Whole {
                    files: count,
                    size,
                    mtime,
                    from,
                    to,
                } => {
                    if !matches!(probe(root, to), Probe::Dir) {
                        return Err(TidyBlock::ChangedSincePlan);
                    }
                    let Some(now) = aggregate(root, to, self.options.max_depth()) else {
                        return Err(TidyBlock::ChangedSincePlan);
                    };
                    if now.0 != *count
                        || now.1 != *size
                        || (now.2 - mtime).abs() > MTIME_TOLERANCE_SECS
                    {
                        return Err(TidyBlock::ChangedSincePlan);
                    }
                    free(root, from, &mut listings)
                        .map_err(|existing| TidyBlock::Conflict { existing })?;
                    whole = Some(WholeMove {
                        files: now.0,
                        size: now.1,
                        mtime: now.2,
                        from: to.clone(),
                        to: from.clone(),
                    });
                }
                _ => {}
            }
        }
        for target in files
            .iter()
            .map(|f| &f.to)
            .chain(whole.iter().map(|w| &w.to))
        {
            for a in ancestors(target) {
                match probe(root, &a) {
                    Probe::Dir | Probe::Absent => {}
                    _ => return Err(TidyBlock::Conflict { existing: a }),
                }
            }
        }
        // Folders the forward block created (and `<D>`), removed when emptied.
        let mut emptied: Vec<PathBuf> = forward
            .body
            .iter()
            .filter_map(|op| match op {
                Op::DirCreated(d) => Some(d.clone()),
                _ => None,
            })
            .chain(forward.final_dir())
            .collect();
        emptied.sort_by_key(|d| std::cmp::Reverse(depth(d)));
        emptied.dedup();
        Ok(Spec {
            unit: forward.unit,
            files,
            whole,
            assembly: None,
            emptied,
        })
    }

    pub(super) fn undo(
        mut self,
        path: &Path,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&TidyProgress),
    ) -> Result<TidyReport, LibraryError> {
        let _lock = lock(self.options.journal_dir())?;
        let entries = journal::load_all(self.options.journal_dir())?;
        // The newest journal (empty files aside) must be the one asked, readable, of this root (FV0 10).
        let Some((newest, content)) = entries
            .into_iter()
            .rev()
            .find(|(_, c)| !matches!(c, Content::Empty))
        else {
            return Err(refused(path, TidyRefusal::NothingToUndo));
        };
        let journal = match content {
            Content::Journal(j) => j,
            Content::Unreadable(e) => return Err(e),
            Content::Empty => return Err(refused(path, TidyRefusal::UnknownJournal)),
        };
        if journal.root != self.root {
            return Err(LibraryError::TidyRootMismatch {
                journal: newest.path,
            });
        }
        if !same_file_path(&newest.path, path) {
            return Err(refused(path, TidyRefusal::NotLatest));
        }
        preflight(self.options.journal_dir())?;
        if !matches!(journal.state, State::Ended | State::EndedMarked) {
            return Err(refused(path, TidyRefusal::AlreadyUndone));
        }
        let mut writer = JournalWriter::append_to(self.sys, &newest.path)?;
        let units: Vec<&Block> = journal
            .forward
            .iter()
            .filter(|b| b.committed)
            .rev()
            .collect();
        let total = units.len() as u64;
        for (done, forward) in units.into_iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                self.report.cancelled = true;
                break;
            }
            let first = forward
                .pairs()
                .first()
                .map(|p| p.1.clone())
                .unwrap_or_default();
            progress(&TidyProgress {
                done_units: done as u64,
                total_units: total,
                current: Some(self.abs(&first)),
            });
            let spec = match self.undo_spec(forward) {
                Ok(spec) => spec,
                Err(block) => {
                    self.report.left.push((self.abs(&first), block));
                    continue;
                }
            };
            let Some(len) = spec.max_journal_len() else {
                self.report
                    .errors
                    .push((self.abs(&first), UNJOURNALABLE.to_owned()));
                continue;
            };
            if writer.len() + len > MAX_UNDO_SIZE {
                self.report
                    .errors
                    .push((self.abs(&first), JOURNAL_FULL.to_owned()));
                break;
            }
            match self.run_block(&mut writer, &spec) {
                Ok(outcome) => self.record(&first, outcome),
                Err(Fatal(e)) => return Err(e),
            }
        }
        writer.append(self.sys, &[Op::Undone])?;
        self.finish(&newest.path)?;
        Ok(self.report)
    }

    // -----------------------------------------------------------------------------------------------------------
    // Recovery

    pub(super) fn recover(mut self) -> Result<Option<TidyReport>, LibraryError> {
        let _lock = lock(self.options.journal_dir())?;
        let mut unreadable = None;
        let mut mismatch = None;
        let mut recovered = false;
        let mut moved = Vec::new();
        for (listed, content) in journal::load_all(self.options.journal_dir())? {
            match content {
                Content::Empty => match self.sys.remove_file(&listed.path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(io_at(&listed.path, e)),
                },
                Content::Unreadable(e) => {
                    unreadable.get_or_insert((listed.path.clone(), e));
                }
                Content::Journal(j) if j.state.in_progress() => {
                    if j.root != self.root {
                        mismatch.get_or_insert(listed.path.clone());
                        continue;
                    }
                    // Open blocks of the section in progress, last first; an I/O error stops before the
                    // closing line (RV1 I2).
                    for block in j.current_section().iter().rev().filter(|b| !b.committed) {
                        self.rollback(block)?;
                    }
                    let (close, section) = if j.state == State::Undoing {
                        (Op::Undone, TidySection::Undo)
                    } else {
                        (Op::End, TidySection::Tidy)
                    };
                    JournalWriter::append_to(self.sys, &listed.path)?.append(self.sys, &[close])?;
                    self.finish(&listed.path)?;
                    self.report.closed.push((listed.path.clone(), section));
                    moved.extend(std::mem::take(&mut self.report.moved));
                    recovered = true;
                }
                Content::Journal(_) => {}
            }
        }
        self.report.moved = moved;
        // What this call closed is reported even when another journal stays refused (RV3 7): those go to the
        // report's errors; without any closed journal the refusal is the result, as before.
        let refusals = unreadable.into_iter().chain(
            mismatch.map(|journal| (journal.clone(), LibraryError::TidyRootMismatch { journal })),
        );
        if recovered {
            for (path, e) in refusals {
                self.report.errors.push((path, e.to_string()));
            }
            return Ok(Some(self.report));
        }
        if let Some((_, e)) = refusals.into_iter().next() {
            return Err(e);
        }
        Ok(None)
    }
}

const UNJOURNALABLE: &str = "chemin impossible à écrire dans le journal : unité laissée en place";

const JOURNAL_FULL: &str =
    "journal de rangement plein : unité laissée en place, relancez le rangement";

/// `rel` with every `<D>.emu-rust-part` component shown as `<D>`: the user never sees the assembly folder.
fn without_part(rel: PathBuf) -> PathBuf {
    rel.components()
        .map(|c| {
            let name = c.as_os_str();
            match name
                .to_str()
                .and_then(|n| n.strip_suffix(super::plan::PART_SUFFIX))
            {
                Some(stem) if !stem.is_empty() => std::ffi::OsString::from(stem),
                _ => name.to_os_string(),
            }
        })
        .collect()
}

/// Why a step of a block failed.
enum Step {
    /// The journal could not be written: the run stops.
    Journal(LibraryError),
    Left(TidyBlock),
    Io(LibraryError),
}

/// Error codes of the OS, by `raw_os_error()` (Rust folds `EPERM` and `EACCES` into `PermissionDenied`); local
/// constants, no dependency (design v7.1 §3.4.2). macOS has the Unix values.
#[cfg(unix)]
mod codes {
    /// `link` refused: no hard links on this file system (`EPERM`; `ENOTSUP` 45 on macOS, `EOPNOTSUPP` 95 on
    /// Linux, e.g. CIFS or FAT).
    #[cfg(target_os = "macos")]
    pub const NO_LINKS: &[i32] = &[1, 45];
    #[cfg(not(target_os = "macos"))]
    pub const NO_LINKS: &[i32] = &[1, 95];
    /// `EPERM`, `EACCES`, `EROFS`.
    pub const DENIED: &[i32] = &[1, 13, 30];
    /// `EXDEV`.
    pub const OTHER_DEVICE: &[i32] = &[18];
    /// `EEXIST`.
    pub const EXISTS: &[i32] = &[17];
}

#[cfg(not(unix))]
mod codes {
    /// `ERROR_INVALID_FUNCTION`, `ERROR_NOT_SUPPORTED`.
    pub const NO_LINKS: &[i32] = &[1, 50];
    /// `ERROR_ACCESS_DENIED`, `ERROR_WRITE_PROTECT`.
    pub const DENIED: &[i32] = &[5, 19];
    /// `ERROR_NOT_SAME_DEVICE`.
    pub const OTHER_DEVICE: &[i32] = &[17];
    /// `ERROR_FILE_EXISTS`, `ERROR_ALREADY_EXISTS`.
    pub const EXISTS: &[i32] = &[80, 183];
}

/// What a refused write meant (design v7.1 §3.4.2, table of point 2).
#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    OtherVolume,
    NoHardLinks,
    NotWritable,
    Exists,
    Other,
}

fn refusal(e: &io::Error, link: bool) -> Refusal {
    use io::ErrorKind as K;
    let raw = e.raw_os_error();
    let has = |set: &[i32]| raw.is_some_and(|r| set.contains(&r));
    if has(codes::OTHER_DEVICE) || e.kind() == K::CrossesDevices {
        Refusal::OtherVolume
    } else if link && (has(codes::NO_LINKS) || e.kind() == K::Unsupported) {
        Refusal::NoHardLinks
    } else if is_denied(e) {
        Refusal::NotWritable
    } else if has(codes::EXISTS) || matches!(e.kind(), K::AlreadyExists | K::DirectoryNotEmpty) {
        Refusal::Exists
    } else {
        Refusal::Other
    }
}

/// « Access refused » (`EACCES`, `EPERM`, `EROFS`, `ERROR_ACCESS_DENIED`, `ERROR_WRITE_PROTECT`): the removal of
/// a name, or a write, the user's rights or attributes forbid.
fn is_denied(e: &io::Error) -> bool {
    match e.raw_os_error() {
        Some(raw) => codes::DENIED.contains(&raw),
        None => matches!(
            e.kind(),
            io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
        ),
    }
}

/// Result of [`Exec::identical`].
enum Identity {
    /// Two names of one file.
    Same,
    Different,
    /// The check could not be made (file not openable).
    Unproven,
}

/// The two canonical paths exist and differ (FV1a I3): guard before removing one name of a file.
fn distinct_names(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a != b)
}

fn same_file_path(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// No journal in progress, none unreadable (`apply_tidy`, `undo_tidy`).
fn preflight(dir: &Path) -> Result<(), LibraryError> {
    for (listed, content) in journal::load_all(dir)? {
        match content {
            Content::Unreadable(e) => return Err(e),
            Content::Journal(j) if j.state.in_progress() => {
                return Err(refused(&listed.path, TidyRefusal::Interrupted));
            }
            _ => {}
        }
    }
    Ok(())
}

/// A new journal named `max(now, newest + 1)` (M3).
fn new_journal(sys: &dyn Sys, options: &TidyOptions) -> Result<JournalWriter, LibraryError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let newest = journal::list(options.journal_dir())?
        .last()
        .map_or(0, |l| l.unix.saturating_add(1));
    JournalWriter::create(
        sys,
        options.journal_dir(),
        options.roms_root(),
        now.max(newest),
    )
}

/// `I` marker and purge (design v7.1 §3.4.1).
pub(super) fn mark(sys: &dyn Sys, options: &TidyOptions, path: &Path) -> Result<(), LibraryError> {
    let _lock = lock(options.journal_dir())?;
    // Only a journal of this folder (the purge below works there).
    let listed = journal::list(options.journal_dir())?;
    if !listed.iter().any(|l| same_file_path(&l.path, path)) {
        return Err(refused(path, TidyRefusal::UnknownJournal));
    }
    let journal = journal::read(path)?.ok_or_else(|| refused(path, TidyRefusal::UnknownJournal))?;
    if journal.root != options.roms_root() {
        return Err(LibraryError::TidyRootMismatch {
            journal: path.to_path_buf(),
        });
    }
    match journal.state {
        State::EndedMarked | State::UndoneMarked => return Ok(()),
        State::Ended | State::Undone => {}
        State::Running | State::Undoing => {
            return Err(refused(path, TidyRefusal::NotClosed));
        }
    }
    JournalWriter::append_to(sys, path)?.append(sys, &[Op::Identities])?;
    let all = journal::list(options.journal_dir())?;
    let old = all.len().saturating_sub(KEPT_JOURNALS);
    for listed in &all[..old] {
        let done = matches!(
            journal::read(&listed.path),
            Ok(Some(j)) if matches!(j.state, State::EndedMarked | State::UndoneMarked)
        );
        if done {
            match sys.remove_file(&listed.path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_at(&listed.path, e)),
            }
        }
    }
    Ok(())
}

/// Closed journals of this root whose identity step is due, oldest first, with their net moves.
pub(super) fn pending(
    options: &TidyOptions,
) -> Result<Vec<(PathBuf, AbsoluteMoves)>, LibraryError> {
    let root = options.roms_root();
    Ok(journal::load_all(options.journal_dir())?
        .into_iter()
        .filter_map(|(listed, content)| match content {
            Content::Journal(j) if j.root == root && j.state.identities_due() => {
                let moved = j
                    .net_moves()
                    .into_iter()
                    .map(|(a, b)| (root.join(a), root.join(b)))
                    .collect();
                Some((listed.path, moved))
            }
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests;
