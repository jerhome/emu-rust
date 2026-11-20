//! Removable media of computer systems (ADR 0015, `docs/design/v2.0-atari-st.md` §3.2): multi-disk sets,
//! working copies, generated `.m3u`, disk swapping during a session.
//!
//! Safety rules (reviewed): the user's files are only ever opened for reading; every write goes to
//! `<saves>/<core_id>/<game>[ (n)].disks/` (`WorkingCopy`) or to a private temporary directory (`Direct`), through
//! `write_atomic_with`. Zip entries are untrusted: names reduced to a sanitized base name, sizes bounded by the
//! `content.rs` limits.

use std::collections::HashSet;
use std::fs;
use std::io::{self, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use emu_libretro::{DiskControl, DiskControlKind};
use tempfile::TempDir;
use zip::ZipArchive;

use crate::content::{self, ArchiveEntry, Limits, io_error};
use crate::emulator::Emulator;
use crate::error::RuntimeError;
use crate::fsname::sanitize_component;
use crate::saves::{FileMode, game_name_from_str, read_bounded, write_atomic_with};
use crate::workcopy::{self, DirState, MANIFEST_NAME, ManifestEntry, Origin, Plan};

/// Most images accepted in one set (generated playlist, zip with several disks, disk menu additions).
/// Same value as `emu_library::MAX_MEDIA_IMAGES`.
pub const MAX_MEDIA_IMAGES: usize = 32;

/// A user `.m3u` larger than this is refused (same bound as the library scan).
const MAX_PLAYLIST_BYTES: usize = 64 * 1024;

/// What to boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentSpec {
    /// One file, as in 1.0 (zip extraction rules of `content.rs` unchanged). The file is handed to the core as
    /// is: use `Media` with `WorkingCopy` for cores that write into their content.
    File(PathBuf),
    /// Ordered disk set: [`prepare_media`] runs after the core is loaded (it needs the extensions the binary
    /// really accepts, which may differ from its `.info`), then `PreparedMedia::content` is loaded; the other
    /// images are reachable through [`Disks`]. The game name (saves, states) is `MediaSet::name`.
    Media { set: MediaSet, policy: MediaPolicy },
    /// `retro_load_game(NULL)` (GEM desktop); needs `SET_SUPPORT_NO_GAME`. Game name [`NO_GAME_NAME`].
    NoGame,
}

/// Game name (saves, states, working copies of disks added from the menu) of a `ContentSpec::NoGame` session.
pub const NO_GAME_NAME: &str = "no-game";

/// Disk images of one game, in insertion order, as grouped by the library (or one user `.m3u`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSet {
    /// Game name (ADR 0011 naming): working-copy folder and generated playlist are named after it.
    pub name: String,
    /// Plain images or `.zip` archives holding images; at most [`MAX_MEDIA_IMAGES`]. May be empty when
    /// `playlist` is set. A single `.m3u` here counts as `playlist`.
    pub images: Vec<PathBuf>,
    /// A user `.m3u`: used as is when the policy is `Direct` (its entries are never rewritten); under
    /// `WorkingCopy` its entries are copied and a new playlist is generated. Takes precedence over `images`.
    pub playlist: Option<PathBuf>,
}

/// Whether the user's files may be handed to the core (from `emu-assets/data/cores.toml`, `media = …`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaPolicy {
    /// The core never writes to content files (or keeps its own copies, e.g. hatariB): pass them through;
    /// zips the core cannot read are extracted to a private temporary directory as in 1.0.
    Direct,
    /// The core writes into disk images in place (hatari): copy them once into
    /// `<saves>/<core_id>/<game>.disks/` and only ever hand the copies to the core.
    WorkingCopy,
}

/// Result of [`prepare_media`].
#[derive(Debug, Clone)]
pub struct PreparedMedia {
    /// File given to `retro_load_game`: the single image, the user playlist, or a generated `.m3u`.
    pub content: PathBuf,
    /// Images actually seen by the core, in order (working copies when `WorkingCopy`).
    pub images: Vec<PathBuf>,
    /// `WorkingCopy` only: `<saves>/<core_id>/<game>.disks`, or `<game> (n).disks` when the first folder holds
    /// copies of other files (see [`WorkDirNotice`]). This is the folder [`reset_work_dir`] must be given.
    pub work_dir: Option<PathBuf>,
    /// Set when this preparation had to start new copies in a suffixed folder instead of `<game>.disks`
    /// (only at that launch; the next ones reuse the new folder silently). The UI should tell the player.
    pub notice: Option<WorkDirNotice>,
    /// `Direct` only: private directory holding extracted zip entries and the generated playlist. Removed when
    /// the last clone is dropped, so keep this value alive as long as the core may read the files.
    pub(crate) temp_dir: Option<Arc<TempDir>>,
}

/// Why new working copies were started in a suffixed folder ([`PreparedMedia::notice`]). The older copies, and
/// the progress they hold, are never modified nor deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkDirNotice {
    /// A source file of this set has another content than when its copies were made (image replaced, e.g. by a
    /// fixed dump), or the set lost a disk: the progress made on the previous copies stays in `previous`.
    SourceChanged { previous: PathBuf },
    /// `<game>.disks` holds the copies of other files with the same game name (`Jeu1/disk1.st`, `Jeu2/disk1.st`).
    SameName,
}

impl PartialEq for PreparedMedia {
    fn eq(&self, other: &Self) -> bool {
        self.content == other.content
            && self.images == other.images
            && self.work_dir == other.work_dir
            && self.notice == other.notice
    }
}

impl Eq for PreparedMedia {}

impl PreparedMedia {
    /// Private temporary directory of a `Direct` set, if one was needed.
    pub fn temp_dir(&self) -> Option<&Path> {
        self.temp_dir.as_deref().map(TempDir::path)
    }
}

/// `<saves>/<core_id>/<game>.disks` for the sanitized game name.
fn work_dir_for(saves_core_dir: &Path, game: &str) -> PathBuf {
    work_dir_candidate(saves_core_dir, &game_name_from_str(game), 1)
}

/// Most `<game> (n).disks` folders tried for one game name.
const MAX_WORK_DIRS: u32 = 64;

/// `n`-th working-copy folder of an already sanitized game name: `<game>.disks`, then `<game> (n).disks`.
fn work_dir_candidate(saves_core_dir: &Path, game: &str, n: u32) -> PathBuf {
    if n <= 1 {
        saves_core_dir.join(format!("{game}.disks"))
    } else {
        saves_core_dir.join(format!("{game} ({n}).disks"))
    }
}

/// Copies / extracts / writes the playlist as the policy requires. Never modifies the files of `set`.
///
/// `saves_core_dir` = `<saves>/<core_id>`. Working copies are created only when absent (an existing copy holds
/// the player's progress and is reused); every write is atomic (`write_atomic`).
///
/// Working-copy folder (`workcopy.rs` for the identity of a source): `<game>.disks`, `<game> (2).disks`, … are
/// examined in order and the first one whose manifest records copies of this set's sources is used; a folder
/// is compatible when each copy recorded there for a set (and still present) has an identical source in this
/// set (new disks of the set are simply added). Otherwise the copies are started in the first free candidate
/// and [`PreparedMedia::notice`] says why: progress is never overwritten nor handed to another game. A folder
/// without manifest (made by an earlier version) is adopted as is by the first set prepared into it, which
/// then records its sources: its origin cannot be known, so this is the earlier behaviour, once. A link or a
/// file in place of a folder, an unreadable manifest, or a link in place of a copy is never used. Zip entries are read with the
/// limits of `content.rs` and only entries whose extension the core accepts are kept (a zip is passed whole when
/// the core accepts `zip`); entry names are never used as paths (sanitised with `fsname`). A generated `.m3u`
/// uses names relative to its own folder (`WorkingCopy`) or absolute paths (`Direct`, private temp directory).
pub fn prepare_media(
    set: &MediaSet,
    policy: MediaPolicy,
    accepted_extensions: &[String],
    saves_core_dir: &Path,
) -> Result<PreparedMedia, RuntimeError> {
    prepare_media_with(
        set,
        policy,
        accepted_extensions,
        saves_core_dir,
        Limits::DEFAULT,
    )
}

pub(crate) fn prepare_media_with(
    set: &MediaSet,
    policy: MediaPolicy,
    exts: &[String],
    saves_core_dir: &Path,
    limits: Limits,
) -> Result<PreparedMedia, RuntimeError> {
    let game = game_name_from_str(&set.name);
    let playlist = set
        .playlist
        .clone()
        .or_else(|| match set.images.as_slice() {
            [only] if has_ext(only, "m3u") => Some(only.clone()),
            _ => None,
        });
    if playlist.is_none() && set.images.is_empty() {
        return Err(RuntimeError::InvalidMediaSet("no disk image".into()));
    }
    if set.images.len() > MAX_MEDIA_IMAGES {
        return Err(too_many());
    }

    if policy == MediaPolicy::Direct
        && let Some(playlist) = playlist
    {
        // Handed over untouched; the entries are only listed for the disk menu.
        let images = parse_playlist(&playlist).unwrap_or_else(|e| {
            tracing::warn!("cannot list the disks of {}: {e}", playlist.display());
            Vec::new()
        });
        return Ok(PreparedMedia {
            content: playlist,
            images,
            work_dir: None,
            notice: None,
            temp_dir: None,
        });
    }

    let sources = match &playlist {
        Some(playlist) => parse_playlist(playlist)?,
        None => set.images.clone(),
    };
    if policy == MediaPolicy::WorkingCopy {
        return prepare_working_copies(&game, &sources, exts, saves_core_dir, limits);
    }
    let mut stage = Stage::new(policy, exts, limits, PathBuf::new())?;
    for source in &sources {
        stage.add_source(source)?;
    }
    let content = match stage.images.as_slice() {
        [only] => only.clone(),
        _ => stage.write_playlist(&game)?,
    };
    Ok(PreparedMedia {
        content,
        images: stage.images,
        work_dir: None,
        notice: None,
        temp_dir: stage.temp_dir,
    })
}

/// Folder chosen for a set, with what its manifest says.
struct ChosenDir {
    dir: PathBuf,
    /// No manifest yet (earlier version): existing copies are adopted by name.
    legacy: bool,
    entries: Vec<ManifestEntry>,
    present: Vec<bool>,
    /// Entry of `entries` holding the copy of each plan item.
    assignment: Vec<Option<usize>>,
    notice: Option<WorkDirNotice>,
}

fn choose_work_dir(
    saves_core_dir: &Path,
    game: &str,
    plan: &mut Plan,
) -> Result<ChosenDir, RuntimeError> {
    let mut free: Option<u32> = None;
    let mut changed = None;
    // Folders whose manifest does not obviously belong to this set: checked (possibly hashing sources) only
    // after the quick matches, so a game kept in `(n).disks` does not cost a full CRC at each launch.
    let mut deferred: Vec<(PathBuf, Vec<ManifestEntry>, Vec<bool>)> = Vec::new();
    let found = |dir: PathBuf, entries, present, assignment| ChosenDir {
        dir,
        legacy: false,
        entries,
        present,
        assignment,
        notice: None,
    };
    // Tries one manifest folder: `Some` when it holds this set's copies.
    let try_folder = |dir: PathBuf,
                      entries: Vec<ManifestEntry>,
                      present: Vec<bool>,
                      plan: &mut Plan,
                      changed: &mut Option<PathBuf>|
     -> Result<Option<ChosenDir>, RuntimeError> {
        match workcopy::assign(&entries, &present, plan)? {
            Some(assignment) => Ok(Some(found(dir, entries, present, assignment))),
            None => {
                if changed.is_none() && plan.shares_path(&entries, &present) {
                    *changed = Some(dir);
                }
                Ok(None)
            }
        }
    };
    let mut legacy = None;
    for n in 1..=MAX_WORK_DIRS {
        let dir = work_dir_candidate(saves_core_dir, game, n);
        match workcopy::inspect(&dir, is_disk_file_name)? {
            DirState::Absent => {
                free.get_or_insert(n);
            }
            DirState::NotDir | DirState::Unreadable => {}
            DirState::Legacy => {
                legacy = Some(dir);
                break;
            }
            DirState::Manifest(entries) => {
                let mut present = workcopy::present(&dir, &entries)?;
                // A copy named like a file we generate in the folder is ignored, never handed to the core.
                let playlist = format!("{game}.m3u").to_lowercase();
                for (p, e) in present.iter_mut().zip(&entries) {
                    *p &= e.copy.to_lowercase() != playlist;
                }
                if plan.quick_match(&entries, &present) {
                    if let Some(c) = try_folder(dir, entries, present, plan, &mut changed)? {
                        return Ok(c);
                    }
                } else {
                    deferred.push((dir, entries, present));
                }
            }
        }
    }
    for (dir, entries, present) in deferred {
        if let Some(c) = try_folder(dir, entries, present, plan, &mut changed)? {
            return Ok(c);
        }
    }
    if let Some(dir) = legacy {
        return Ok(ChosenDir {
            dir,
            legacy: true,
            entries: Vec::new(),
            present: Vec::new(),
            assignment: Vec::new(),
            notice: None,
        });
    }
    let Some(first_free) = free else {
        return Err(RuntimeError::InvalidMediaSet(format!(
            "{MAX_WORK_DIRS} working-copy folders already exist for `{game}`"
        )));
    };
    // Exclusive creation: another instance (a homonymous game) that took the folder since the scan is skipped.
    // Two instances of the very same game starting together can still share a folder (not guarded).
    fs::create_dir_all(saves_core_dir).map_err(|e| io_error(saves_core_dir, e))?;
    let mut created = None;
    for n in first_free..=MAX_WORK_DIRS {
        let dir = work_dir_candidate(saves_core_dir, game, n);
        match fs::create_dir(&dir) {
            Ok(()) => {
                created = Some((n, dir));
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io_error(&dir, e)),
        }
    }
    let Some((n, dir)) = created else {
        return Err(RuntimeError::InvalidMediaSet(format!(
            "{MAX_WORK_DIRS} working-copy folders already exist for `{game}`"
        )));
    };
    // Only a folder pushed aside by another one is news; the first folder of a game (or one reset) is not.
    let notice = (n > 1).then_some(match changed {
        Some(previous) => WorkDirNotice::SourceChanged { previous },
        None => WorkDirNotice::SameName,
    });
    if let Some(notice) = &notice {
        tracing::warn!(?notice, "new working copies in {}", dir.display());
    }
    Ok(ChosenDir {
        dir,
        legacy: false,
        entries: Vec::new(),
        present: Vec::new(),
        assignment: Vec::new(),
        notice,
    })
}

fn prepare_working_copies(
    game: &str,
    sources: &[PathBuf],
    exts: &[String],
    saves_core_dir: &Path,
    limits: Limits,
) -> Result<PreparedMedia, RuntimeError> {
    let mut plan = Plan::new(sources, exts, limits, MAX_MEDIA_IMAGES)?;
    let chosen = choose_work_dir(saves_core_dir, game, &mut plan)?;
    let mut stage = Stage::new(MediaPolicy::WorkingCopy, exts, limits, chosen.dir.clone())?;
    // Files of other sources stay out of reach: only a pre-manifest folder adopts copies by name.
    stage.fresh = !chosen.legacy;
    stage.used.insert(MANIFEST_NAME.to_lowercase());
    stage.used.insert(format!("{game}.m3u").to_lowercase());
    for (entry, &present) in chosen.entries.iter().zip(&chosen.present) {
        if present {
            stage.used.insert(entry.copy.to_lowercase());
        }
    }
    let mut recorded = Vec::with_capacity(plan.items.len());
    let mut consumed = vec![false; chosen.entries.len()];
    for i in 0..plan.items.len() {
        let src = plan.items[i].src;
        match chosen.assignment.get(i).copied().flatten() {
            Some(j) => {
                consumed[j] = true;
                let mut entry = chosen.entries[j].clone();
                entry.origin = Origin::Set;
                entry.refresh(&plan.sources[src]);
                stage.push(chosen.dir.join(&entry.copy))?;
                recorded.push(entry);
            }
            None => {
                let copy = stage.materialize(&mut plan, i)?;
                let name = plan.items[i].entry.as_ref().map(|e| e.name.clone());
                recorded.push(ManifestEntry::new(
                    Origin::Set,
                    copy_name(&copy),
                    name,
                    &mut plan.sources[src],
                )?);
            }
        }
    }
    // Disks added from the menu stay available to later sessions (the newest within the manifest's bound, which
    // the core's 32-image list keeps out of reach).
    let kept: Vec<&ManifestEntry> = chosen
        .entries
        .iter()
        .enumerate()
        .filter(|&(j, e)| !consumed[j] && chosen.present[j] && e.origin == Origin::Added)
        .map(|(_, e)| e)
        .collect();
    let room = workcopy::MAX_MANIFEST_ENTRIES.saturating_sub(recorded.len());
    let skip = kept.len().saturating_sub(room);
    recorded.extend(kept.into_iter().skip(skip).cloned());
    let content = match stage.images.as_slice() {
        [only] => only.clone(),
        _ => stage.write_playlist(game)?,
    };
    workcopy::write(&chosen.dir, &recorded)?;
    Ok(PreparedMedia {
        content,
        images: stage.images,
        work_dir: Some(chosen.dir),
        notice: chosen.notice,
        temp_dir: None,
    })
}

fn copy_name(copy: &Path) -> String {
    copy.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// A name `disk_file_name` could have produced (manifests are read back from disk).
pub(crate) fn is_disk_file_name(name: &str) -> bool {
    disk_file_name(name) == name
}

pub(crate) fn too_many() -> RuntimeError {
    RuntimeError::InvalidMediaSet(format!("more than {MAX_MEDIA_IMAGES} disk images"))
}

pub(crate) fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

pub(crate) fn accepts_ext(exts: &[String], ext: &str) -> bool {
    exts.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

/// Single safe file name for a disk copy; a leading `#` would turn a playlist line into a comment.
fn disk_file_name(raw: &str) -> String {
    let mut name = sanitize_component(raw, 200);
    if name.starts_with('#') {
        name.replace_range(..1, "_");
    }
    if name.is_empty() {
        "disk".to_owned()
    } else {
        name
    }
}

/// Destination of the images of one set (or of one disk added from the menu).
struct Stage<'a> {
    policy: MediaPolicy,
    exts: &'a [String],
    limits: Limits,
    work_dir: PathBuf,
    temp_dir: Option<Arc<TempDir>>,
    images: Vec<PathBuf>,
    /// Lower-cased names already given in the destination folder (case-insensitive filesystems).
    used: HashSet<String>,
    /// Disk added during a session: never adopt a file already on disk (it may be another source's working copy).
    fresh: bool,
}

impl<'a> Stage<'a> {
    fn new(
        policy: MediaPolicy,
        exts: &'a [String],
        limits: Limits,
        work_dir: PathBuf,
    ) -> Result<Self, RuntimeError> {
        if policy == MediaPolicy::WorkingCopy {
            fs::create_dir_all(&work_dir).map_err(|e| io_error(&work_dir, e))?;
        }
        Ok(Self {
            policy,
            exts,
            limits,
            work_dir,
            temp_dir: None,
            images: Vec::new(),
            used: HashSet::new(),
            fresh: false,
        })
    }

    /// Folder receiving copies and extracted entries.
    fn dest_dir(&mut self) -> Result<PathBuf, RuntimeError> {
        match self.policy {
            MediaPolicy::WorkingCopy => Ok(self.work_dir.clone()),
            MediaPolicy::Direct => {
                if self.temp_dir.is_none() {
                    self.temp_dir = Some(Arc::new(content::private_temp_dir()?));
                }
                Ok(self
                    .temp_dir
                    .as_deref()
                    .map(|d| d.path().to_path_buf())
                    .unwrap_or_default())
            }
        }
    }

    /// Unique destination for `name`; a clash (two `Disk 1.st` from different folders) gets a numeric prefix
    /// derived from the position, so the same set maps to the same files at every launch.
    ///
    /// An existing file is adopted only when its size is `size` (a legacy folder may hold another game's disk).
    fn dest_path(&mut self, name: &str, size: u64) -> Result<PathBuf, RuntimeError> {
        let dir = self.dest_dir()?;
        let mut candidate = name.to_owned();
        let mut n = self.images.len() + 1;
        loop {
            let free = !self.used.contains(&candidate.to_lowercase())
                && match fs::symlink_metadata(dir.join(&candidate)) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => true,
                    Err(e) => return Err(io_error(&dir.join(&candidate), e)),
                    // An existing copy is adopted (never during a session, never a link or a folder).
                    Ok(meta) => meta.is_file() && !self.fresh && meta.len() == size,
                };
            if free {
                self.used.insert(candidate.to_lowercase());
                break;
            }
            candidate = disk_file_name(&format!("{n:02} {name}"));
            n += 1;
        }
        Ok(dir.join(candidate))
    }

    fn push(&mut self, image: PathBuf) -> Result<(), RuntimeError> {
        if self.images.len() >= MAX_MEDIA_IMAGES {
            return Err(too_many());
        }
        self.images.push(image);
        Ok(())
    }

    fn add_source(&mut self, source: &Path) -> Result<(), RuntimeError> {
        if has_ext(source, "zip") && !accepts_ext(self.exts, "zip") {
            return self.add_zip(source, MAX_MEDIA_IMAGES);
        }
        match self.policy {
            MediaPolicy::Direct => {
                // Absolute so that a generated playlist in the temp directory still finds it.
                let path = std::path::absolute(source).map_err(|e| io_error(source, e))?;
                self.push(path)
            }
            MediaPolicy::WorkingCopy => self.copy_plain(source).map(drop),
        }
    }

    /// Working copy of a whole file; an existing regular file at the chosen name is adopted (`!fresh` only).
    fn copy_plain(&mut self, source: &Path) -> Result<PathBuf, RuntimeError> {
        let name = disk_file_name(&source.file_name().unwrap_or_default().to_string_lossy());
        let size = fs::metadata(source).map_err(|e| io_error(source, e))?.len();
        let dest = self.dest_path(&name, size)?;
        if !exists(&dest)? {
            copy_file(source, &dest, self.limits.max_size)?;
            tracing::info!("working copy of {} created", source.display());
        }
        self.push(dest.clone())?;
        Ok(dest)
    }

    /// Copy (or extraction) of one planned image.
    fn materialize(&mut self, plan: &mut Plan, i: usize) -> Result<PathBuf, RuntimeError> {
        let src = plan.items[i].src;
        let source = plan.sources[src].path.clone();
        match (plan.items[i].entry.clone(), plan.archives[src].as_mut()) {
            (Some(entry), Some(archive)) => self.extract_entry(archive, &source, &entry),
            _ => self.copy_plain(&source),
        }
    }

    fn extract_entry(
        &mut self,
        archive: &mut ZipArchive<BufReader<fs::File>>,
        zip: &Path,
        entry: &ArchiveEntry,
    ) -> Result<PathBuf, RuntimeError> {
        let dest = self.dest_path(&disk_file_name(&entry.name), entry.declared)?;
        if self.policy == MediaPolicy::Direct || !exists(&dest)? {
            let mut failure = None;
            let written = write_atomic_with(&dest, FileMode::Default, |file| {
                content::copy_entry(archive, zip, entry, self.limits.max_size, file, &dest)
                    .map(drop)
                    .map_err(|e| {
                        failure = Some(e);
                        io::Error::other("zip entry not extracted")
                    })
            });
            if let Some(e) = failure {
                return Err(e);
            }
            written.map_err(|e| io_error(&dest, e))?;
            tracing::info!(entry = %entry.name, "disk image extracted from {}", zip.display());
        }
        self.push(dest.clone())?;
        Ok(dest)
    }

    /// Extracts up to `max` accepted entries (archive order). A playlist inside the archive is ignored.
    fn add_zip(&mut self, zip: &Path, max: usize) -> Result<(), RuntimeError> {
        let mut archive = content::open_archive(zip, self.limits)?;
        let exts = self.exts;
        let room = MAX_MEDIA_IMAGES.saturating_sub(self.images.len()).min(max);
        let entries = content::accepted_entries(
            &mut archive,
            zip,
            |name| workcopy::zip_entry_accepted(exts, name),
            // One more than allowed, to report an oversized set instead of silently dropping disks.
            room.saturating_add(1),
        )?;
        if entries.is_empty() {
            return Err(RuntimeError::NoContentInArchive(zip.to_path_buf()));
        }
        if entries.len() > room && max == MAX_MEDIA_IMAGES {
            return Err(too_many());
        }
        for entry in entries.iter().take(room) {
            self.extract_entry(&mut archive, zip, entry)?;
        }
        Ok(())
    }

    /// `<game>.m3u` next to the copies (relative names) or in the temp directory (absolute paths). Rewritten only
    /// when its content changes.
    fn write_playlist(&mut self, game: &str) -> Result<PathBuf, RuntimeError> {
        let dir = self.dest_dir()?;
        let path = dir.join(format!("{game}.m3u"));
        let mut text = Vec::new();
        for image in &self.images {
            let entry = match self.policy {
                MediaPolicy::WorkingCopy => image.strip_prefix(&dir).unwrap_or(image),
                MediaPolicy::Direct => image.as_path(),
            };
            let bytes = path_bytes(entry)?;
            if bytes.iter().any(|&b| b == b'\n' || b == b'\r') || bytes.first() == Some(&b'#') {
                return Err(RuntimeError::InvalidMediaSet(format!(
                    "{} cannot be listed in a playlist",
                    image.display()
                )));
            }
            text.extend_from_slice(&bytes);
            text.push(b'\n');
        }
        let unchanged = read_bounded(&path, text.len()).is_ok_and(|old| old == text);
        if !unchanged {
            write_atomic_with(&path, FileMode::Default, |f| io::Write::write_all(f, &text))
                .map_err(|e| io_error(&path, e))?;
        }
        Ok(path)
    }
}

fn exists(path: &Path) -> Result<bool, RuntimeError> {
    // `symlink_metadata`: a dangling link still counts as present (never written through).
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io_error(path, e)),
    }
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Result<Vec<u8>, RuntimeError> {
    use std::os::unix::ffi::OsStrExt;
    Ok(path.as_os_str().as_bytes().to_vec())
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Result<Vec<u8>, RuntimeError> {
    // libretro reads playlists as UTF-8 on Windows.
    path.to_str()
        .map(|s| s.as_bytes().to_vec())
        .ok_or_else(|| RuntimeError::InvalidMediaSet(format!("non UTF-8 path {}", path.display())))
}

/// Streams `source` into `dest` atomically; `source` is only read.
fn copy_file(source: &Path, dest: &Path, max: u64) -> Result<(), RuntimeError> {
    let mut input = fs::File::open(source).map_err(|e| io_error(source, e))?;
    let meta = input.metadata().map_err(|e| io_error(source, e))?;
    if !meta.is_file() {
        return Err(RuntimeError::InvalidMediaSet(format!(
            "{} is not a file",
            source.display()
        )));
    }
    if meta.len() > max {
        return Err(RuntimeError::ContentTooLarge {
            path: source.to_path_buf(),
            size: meta.len(),
        });
    }
    let mut read_error = None;
    let mut grown = None;
    let written = write_atomic_with(dest, FileMode::Default, |out| {
        // The file may grow after `metadata`: one byte past the limit detects it.
        let mut limited = (&mut input).take(max + 1);
        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = match limited.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    let kind = e.kind();
                    read_error = Some(e);
                    return Err(io::Error::new(kind, "source not readable"));
                }
            };
            io::Write::write_all(out, &buf[..n])?;
            total += n as u64;
        }
        if total > max {
            grown = Some(total);
            return Err(io::Error::other("source too large"));
        }
        Ok(())
    });
    if let Some(e) = read_error {
        return Err(io_error(source, e));
    }
    if let Some(size) = grown {
        return Err(RuntimeError::ContentTooLarge {
            path: source.to_path_buf(),
            size,
        });
    }
    written.map_err(|e| io_error(dest, e))
}

/// Entries of a user `.m3u` (≤ 64 KiB, ≤ [`MAX_MEDIA_IMAGES`] entries): blank lines and `#` lines skipped, a
/// `|label` suffix dropped, paths absolute or relative to the playlist's folder; `..`, and drive or root prefixes
/// on a relative path, are refused, as are non-UTF-8 files and control characters inside an entry. Backslashes are separators on every platform (playlists made on Windows).
pub(crate) fn parse_playlist(playlist: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
    let bytes = read_bounded(playlist, MAX_PLAYLIST_BYTES).map_err(|e| io_error(playlist, e))?;
    if bytes.len() > MAX_PLAYLIST_BYTES {
        return Err(RuntimeError::InvalidMediaSet(format!(
            "{} is larger than {MAX_PLAYLIST_BYTES} bytes",
            playlist.display()
        )));
    }
    let base = playlist.parent().unwrap_or(Path::new(""));
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        RuntimeError::InvalidMediaSet(format!("{} is not UTF-8", playlist.display()))
    })?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = line.split('|').next().unwrap_or("").trim();
        if entry.is_empty() {
            continue;
        }
        if entry.chars().any(char::is_control) {
            return Err(RuntimeError::InvalidMediaSet(format!(
                "{}: an entry contains a control character",
                playlist.display()
            )));
        }
        let entry = playlist_entry_path(entry);
        let refuse = |why: &str| {
            RuntimeError::InvalidMediaSet(format!(
                "{}: entry `{}` {why}",
                playlist.display(),
                entry.display()
            ))
        };
        if entry.components().any(|c| c == Component::ParentDir) {
            return Err(refuse("goes up with `..`"));
        }
        let path = if entry.is_absolute() {
            entry
        } else {
            if entry
                .components()
                .any(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
            {
                return Err(refuse("has a drive or root prefix"));
            }
            base.join(entry)
        };
        if out.len() >= MAX_MEDIA_IMAGES {
            return Err(too_many());
        }
        out.push(path);
    }
    if out.is_empty() {
        return Err(RuntimeError::InvalidMediaSet(format!(
            "{} lists no disk",
            playlist.display()
        )));
    }
    Ok(out)
}

fn playlist_entry_path(entry: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(entry)
    } else {
        PathBuf::from(entry.replace('\\', "/"))
    }
}

/// Deletes `<saves_core_dir>/<game>.disks` (menu « Réinitialiser les disquettes »); the core must not be running
/// that game. Returns `false` when there was nothing to delete. A game whose copies live in a suffixed
/// `<game> (n).disks` folder ([`PreparedMedia::work_dir`]) must be reset with [`reset_work_dir`]: this function
/// would delete the copies of the other game owning `<game>.disks`.
pub fn reset_working_copies(saves_core_dir: &Path, game: &str) -> Result<bool, RuntimeError> {
    remove_work_dir(&work_dir_for(saves_core_dir, game))
}

/// Deletes one working-copy folder, normally [`PreparedMedia::work_dir`] of the game (the core must not be
/// running it). Refused unless `work_dir` is a `*.disks` entry directly inside `saves_core_dir`. Returns `false`
/// when there was nothing to delete.
pub fn reset_work_dir(saves_core_dir: &Path, work_dir: &Path) -> Result<bool, RuntimeError> {
    let named_disks = work_dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.len() > ".disks".len() && n.ends_with(".disks"));
    if work_dir.parent() != Some(saves_core_dir) || !named_disks {
        return Err(RuntimeError::InvalidMediaSet(format!(
            "{} is not a working-copy folder of {}",
            work_dir.display(),
            saves_core_dir.display()
        )));
    }
    remove_work_dir(work_dir)
}

fn remove_work_dir(dir: &Path) -> Result<bool, RuntimeError> {
    let meta = match fs::symlink_metadata(dir) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(io_error(dir, e)),
    };
    // A link in place of the folder: remove the link, never what it points to (`remove_dir_all` does not
    // follow links inside the folder either).
    let removed = if meta.is_dir() {
        fs::remove_dir_all(dir)
    } else {
        remove_link(dir)
    };
    removed.map_err(|e| io_error(dir, e))?;
    tracing::info!("working copies removed: {}", dir.display());
    Ok(true)
}

/// Removes a file or a link without following it.
fn remove_link(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        // Windows: a directory symlink or a junction is a directory entry; `RemoveDirectoryW` deletes the link
        // itself, not the target's content.
        Err(e) if cfg!(windows) => fs::remove_dir(path).map_err(|_| e),
        other => other,
    }
}

/// One entry of the disk menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskSlot {
    pub index: u32,
    /// Core label, else file name, else `Disque <n>`.
    pub label: String,
    pub path: Option<PathBuf>,
}

/// Disk swapping for the running game, borrowed from [`Emulator::disks`]. Sequencing (eject → select →
/// insert) is done here so that the UI only says what it wants. Errors of the core are returned as
/// `RuntimeError::Libretro`.
pub struct Disks<'a> {
    pub(crate) emulator: &'a mut Emulator,
}

impl Disks<'_> {
    fn control(&mut self) -> Result<DiskControl<'_>, RuntimeError> {
        self.emulator
            .core
            .disk_control()
            .ok_or(RuntimeError::NoDiskControl)
    }

    pub fn kind(&mut self) -> DiskControlKind {
        self.control().map_or(DiskControlKind::Basic, |c| c.kind())
    }

    pub fn slots(&mut self) -> Vec<DiskSlot> {
        let known: Vec<PathBuf> = self
            .emulator
            .prepared
            .as_ref()
            .map(|p| p.images.clone())
            .unwrap_or_default();
        let Ok(mut control) = self.control() else {
            return Vec::new();
        };
        // The count comes from the core: never trust it to size a menu.
        let num = control.num_images().min(MAX_MEDIA_IMAGES as u32);
        (0..num)
            .map(|index| {
                // The basic interface has no paths: the prepared list is in the same order.
                let path = control
                    .image_path(index)
                    .or_else(|| known.get(index as usize).cloned());
                let label = control
                    .image_label(index)
                    .or_else(|| {
                        path.as_deref()
                            .and_then(Path::file_stem)
                            .map(|s| s.to_string_lossy().into_owned())
                    })
                    .unwrap_or_else(|| format!("Disque {}", index + 1));
                DiskSlot { index, label, path }
            })
            .collect()
    }

    /// `None` when the drive is empty or ejected.
    pub fn current(&mut self) -> Option<u32> {
        let mut control = self.control().ok()?;
        if control.is_ejected() {
            return None;
        }
        let index = control.image_index();
        (index < control.num_images()).then_some(index)
    }

    /// Ejects, selects `index`, inserts. On a refusal of the core the previous disk is put back when possible.
    pub fn insert(&mut self, index: u32) -> Result<(), RuntimeError> {
        let mut control = self.control()?;
        let num = control.num_images();
        if index >= num {
            return Err(RuntimeError::InvalidMediaSet(format!(
                "no disk {} (the core has {num})",
                index + 1
            )));
        }
        let was_ejected = control.is_ejected();
        let previous = control.image_index();
        if !was_ejected {
            control.set_ejected(true)?;
        }
        if let Err(e) = control.set_image_index(index) {
            if !was_ejected && let Err(restore) = control.set_ejected(false) {
                tracing::warn!("previous disk not reinserted: {restore}");
            }
            return Err(e.into());
        }
        if let Err(e) = control.set_ejected(false) {
            // Back to the previous disk, in the previous tray state.
            if control.set_image_index(previous).is_ok() && !was_ejected {
                let _ = control.set_ejected(false);
            }
            return Err(e.into());
        }
        tracing::info!(index, "disk inserted");
        Ok(())
    }

    /// Next image (wraps); the usual « changer de disquette » action. Returns the inserted index.
    pub fn insert_next(&mut self) -> Result<u32, RuntimeError> {
        let num = self.control()?.num_images().min(MAX_MEDIA_IMAGES as u32);
        if num == 0 {
            return Err(RuntimeError::InvalidMediaSet("no disk image".into()));
        }
        let next = self.current().map_or(0, |current| (current + 1) % num);
        self.insert(next)?;
        Ok(next)
    }

    pub fn eject(&mut self) -> Result<(), RuntimeError> {
        self.control()?.set_ejected(true)?;
        Ok(())
    }

    /// Adds an image during the session and returns its index; it is not inserted. Under `WorkingCopy` (and for
    /// `File` / `NoGame` sessions, whose policy is unknown) the core gets a working copy in
    /// the game's working-copy folder ([`PreparedMedia::work_dir`], else `<saves>/<core_id>/<game>.disks/`): the
    /// copy made the last time the same source was added is reused (with its progress), otherwise a new copy is
    /// made under a free name (never merged with a file of another source); a zip the core cannot read
    /// gives its first accepted entry. The generated playlist is unchanged; a working copy is recorded in the
    /// folder's manifest and re-added at the next sessions (`EmulatorOptions::restore_added_disks`). At most
    /// [`MAX_MEDIA_IMAGES`] images: beyond, the addition is refused (nothing is forgotten to make room).
    pub fn add(&mut self, image: &Path) -> Result<u32, RuntimeError> {
        let num = self.control()?.num_images();
        if num as usize >= MAX_MEDIA_IMAGES {
            return Err(list_full());
        }
        let path = self.emulator.stage_added_image(image)?;
        self.append(&path)
    }

    /// Appends `path` to the core's list (`add_image` + `replace_image`, tray state kept, not inserted) and to the
    /// prepared list; returns its index. Shared by [`Self::add`] and the restoration of added disks.
    pub(crate) fn append(&mut self, path: &Path) -> Result<u32, RuntimeError> {
        let mut control = self.control()?;
        let num = control.num_images();
        if num as usize >= MAX_MEDIA_IMAGES {
            return Err(list_full());
        }
        let was_ejected = control.is_ejected();
        if !was_ejected {
            control.set_ejected(true)?;
        }
        let added = control
            .add_image()
            .map_err(RuntimeError::from)
            .and_then(|()| {
                control.replace_image(num, Some(path)).map_err(|e| {
                    // Drop the empty slot left by `add_image`.
                    let _ = control.replace_image(num, None);
                    RuntimeError::from(e)
                })
            });
        let reinserted = if was_ejected {
            Ok(())
        } else {
            control.set_ejected(false)
        };
        added?;
        reinserted?;
        if let Some(prepared) = self.emulator.prepared.as_mut() {
            prepared.images.push(path.to_path_buf());
        }
        tracing::info!(index = num, "disk {} added", path.display());
        Ok(num)
    }
}

/// The core's list is full: the player must remove an added disk first (the UI says « retirez une disquette
/// ajoutée »).
pub(crate) fn list_full() -> RuntimeError {
    RuntimeError::InvalidMediaSet(format!(
        "at most {MAX_MEDIA_IMAGES} disk images: remove an added disk first"
    ))
}

impl Emulator {
    /// Copy / extraction of an image added from the disk menu, as `prepare_media` would do it.
    fn stage_added_image(&mut self, image: &Path) -> Result<PathBuf, RuntimeError> {
        let policy = self.media_policy.unwrap_or(MediaPolicy::WorkingCopy);
        let work_dir = self
            .prepared
            .as_ref()
            .and_then(|p| p.work_dir.clone())
            .unwrap_or_else(|| work_dir_for(&self.save_root.join(&self.core_id), &self.game_name));
        let exts = self.core.system_info().valid_extensions.clone();
        if policy == MediaPolicy::WorkingCopy {
            let known = self
                .prepared
                .as_ref()
                .map(|p| p.images.as_slice())
                .unwrap_or_default();
            return stage_added_copy(&work_dir, image, &exts, self.limits, known);
        }
        let mut stage = Stage::new(policy, &exts, self.limits, work_dir)?;
        stage.temp_dir = self.prepared.as_ref().and_then(|p| p.temp_dir.clone());
        // Names already handed to the core keep theirs; a new name never overwrites them in the temp directory.
        stage.fresh = true;
        for known in self.prepared.iter().flat_map(|p| &p.images) {
            if let Some(name) = known.file_name() {
                stage.used.insert(name.to_string_lossy().to_lowercase());
            }
        }
        if has_ext(image, "zip") && !accepts_ext(&exts, "zip") {
            stage.add_zip(image, 1)?;
        } else {
            stage.add_source(image)?;
        }
        let path = stage
            .images
            .pop()
            .ok_or_else(|| RuntimeError::NoContentInArchive(image.to_path_buf()))?;
        if let (Some(prepared), Some(temp)) = (self.prepared.as_mut(), stage.temp_dir) {
            prepared.temp_dir.get_or_insert(temp);
        }
        Ok(path)
    }
}

/// Working copy of a disk added from the menu, in `work_dir`. The folder's manifest remembers which source each
/// added copy comes from, so adding the same source again (in a later session) gives back the same copy and its
/// progress; a copy already handed to the core this session (`known`) is never given twice. Anything else gets
/// a new copy under a free name: an added disk never adopts a file it did not record. A folder without a
/// readable manifest (earlier version, damaged) still gets the copy but nothing is recorded there.
fn stage_added_copy(
    work_dir: &Path,
    image: &Path,
    exts: &[String],
    limits: Limits,
    known: &[PathBuf],
) -> Result<PathBuf, RuntimeError> {
    let mut manifest = match workcopy::inspect(work_dir, is_disk_file_name)? {
        DirState::Absent => Some(Vec::new()),
        DirState::Manifest(entries) => Some(entries),
        DirState::Legacy | DirState::Unreadable => None,
        DirState::NotDir => {
            return Err(RuntimeError::InvalidMediaSet(format!(
                "{} is not a folder",
                work_dir.display()
            )));
        }
    };
    if manifest
        .as_ref()
        .is_some_and(|m| m.len() >= workcopy::MAX_MANIFEST_ENTRIES)
    {
        return Err(list_full());
    }
    let mut plan = Plan::new(&[image.to_path_buf()], exts, limits, 1)?;
    if let Some(entries) = manifest.as_mut() {
        let present = workcopy::present(work_dir, entries)?;
        let in_use = |e: &ManifestEntry| known.iter().any(|k| *k == work_dir.join(&e.copy));
        if let Some(j) = workcopy::find_added(entries, &present, &mut plan, in_use)? {
            entries[j].refresh(&plan.sources[0]);
            workcopy::write(work_dir, entries)?;
            let path = work_dir.join(&entries[j].copy);
            tracing::info!("working copy {} reused", path.display());
            return Ok(path);
        }
    }
    let mut stage = Stage::new(
        MediaPolicy::WorkingCopy,
        exts,
        limits,
        work_dir.to_path_buf(),
    )?;
    stage.fresh = true;
    stage.used.insert(MANIFEST_NAME.to_lowercase());
    for name in known.iter().filter_map(|k| k.file_name()) {
        stage.used.insert(name.to_string_lossy().to_lowercase());
    }
    let path = stage.materialize(&mut plan, 0)?;
    if let Some(mut entries) = manifest {
        let name = plan.items[0].entry.as_ref().map(|e| e.name.clone());
        entries.push(ManifestEntry::new(
            Origin::Added,
            copy_name(&path),
            name,
            &mut plan.sources[0],
        )?);
        workcopy::write(work_dir, &entries)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::tests::build_zip;

    fn exts() -> Vec<String> {
        ["st", "msa", "m3u"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    }

    struct Fixture {
        tmp: TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let f = Self {
                tmp: tempfile::tempdir().unwrap(),
            };
            fs::create_dir_all(f.roms()).unwrap();
            fs::create_dir_all(f.saves()).unwrap();
            f
        }
        fn roms(&self) -> PathBuf {
            self.tmp.path().join("roms")
        }
        fn saves(&self) -> PathBuf {
            self.tmp.path().join("saves/hatari")
        }
        fn file(&self, rel: &str, data: &[u8]) -> PathBuf {
            let path = self.roms().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, data).unwrap();
            path
        }
        fn prepare(
            &self,
            set: &MediaSet,
            policy: MediaPolicy,
        ) -> Result<PreparedMedia, RuntimeError> {
            prepare_media(set, policy, &exts(), &self.saves())
        }
        /// Every file under the temp root except `roms/`, relative.
        fn written(&self) -> Vec<String> {
            fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
                for e in fs::read_dir(dir).unwrap() {
                    let p = e.unwrap().path();
                    if p.is_dir() {
                        walk(&p, root, out);
                    } else {
                        out.push(
                            p.strip_prefix(root)
                                .unwrap()
                                .to_string_lossy()
                                .replace('\\', "/"),
                        );
                    }
                }
            }
            let mut out = Vec::new();
            walk(&self.saves(), self.tmp.path(), &mut out);
            out.sort();
            out
        }
    }

    fn set(name: &str, images: Vec<PathBuf>) -> MediaSet {
        MediaSet {
            name: name.into(),
            images,
            playlist: None,
        }
    }

    fn snapshot(paths: &[PathBuf]) -> Vec<Vec<u8>> {
        paths.iter().map(|p| fs::read(p).unwrap()).collect()
    }

    #[test]
    fn added_disk_never_reuses_another_sources_copy() {
        let f = Fixture::new();
        let own = f.file("Game/Disk 1.st", b"own");
        let other = f.file("Other/Disk 1.st", b"other");
        let p = f
            .prepare(&set("Game", vec![own]), MediaPolicy::WorkingCopy)
            .unwrap();
        fs::write(&p.images[0], b"progress").unwrap();
        let emu_stage = |prepared: &PreparedMedia, src: &Path| {
            let work = prepared.work_dir.clone().unwrap();
            let exts = exts();
            let mut st =
                Stage::new(MediaPolicy::WorkingCopy, &exts, Limits::DEFAULT, work).unwrap();
            st.fresh = true;
            for k in &prepared.images {
                st.used
                    .insert(k.file_name().unwrap().to_string_lossy().to_lowercase());
            }
            st.add_source(src).unwrap();
            st.images.pop().unwrap()
        };
        let added = emu_stage(&p, &other);
        assert_ne!(added, p.images[0]);
        assert_eq!(fs::read(&added).unwrap(), b"other");
        assert_eq!(fs::read(&p.images[0]).unwrap(), b"progress");
        // Next session: the name is taken on disk, so a third source cannot adopt it either.
        let third = f.file("Third/Disk 1.st", b"third");
        let again = emu_stage(&p, &third);
        assert_ne!(again, added);
        assert_eq!(fs::read(&again).unwrap(), b"third");
        assert_eq!(fs::read(&added).unwrap(), b"other");
    }

    #[test]
    fn working_copy_single_is_created_once_and_reused() {
        let f = Fixture::new();
        let d1 = f.file("Game.st", b"original");
        let s = set("Game", vec![d1.clone()]);
        let p = f.prepare(&s, MediaPolicy::WorkingCopy).unwrap();
        let work = f.saves().join("Game.disks");
        assert_eq!(p.work_dir.as_deref(), Some(work.as_path()));
        assert_eq!(p.content, work.join("Game.st"));
        assert_eq!(p.images, [work.join("Game.st")]);
        // The core writes into the copy; the next launch keeps that progress.
        fs::write(&p.content, b"progress").unwrap();
        let again = f.prepare(&s, MediaPolicy::WorkingCopy).unwrap();
        assert_eq!(again, p);
        assert_eq!(fs::read(&again.content).unwrap(), b"progress");
        assert_eq!(fs::read(&d1).unwrap(), b"original");
        assert_eq!(
            f.written(),
            [
                "saves/hatari/Game.disks/.emu-rust-disks",
                "saves/hatari/Game.disks/Game.st"
            ]
        );
    }

    #[test]
    fn working_copy_multi_generates_a_relative_playlist() {
        let f = Fixture::new();
        let images = vec![
            f.file("Game (Disk 1 of 2).st", b"one"),
            f.file("Game (Disk 2 of 2).msa", b"two"),
        ];
        let before = snapshot(&images);
        let p = f
            .prepare(&set("Game", images.clone()), MediaPolicy::WorkingCopy)
            .unwrap();
        let work = f.saves().join("Game.disks");
        assert_eq!(p.content, work.join("Game.m3u"));
        assert_eq!(
            fs::read_to_string(&p.content).unwrap(),
            "Game (Disk 1 of 2).st\nGame (Disk 2 of 2).msa\n"
        );
        assert_eq!(p.images[1], work.join("Game (Disk 2 of 2).msa"));
        assert_eq!(snapshot(&images), before);
        let m3u_time = fs::metadata(&p.content).unwrap().modified().unwrap();
        f.prepare(&set("Game", images), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(
            fs::metadata(&p.content).unwrap().modified().unwrap(),
            m3u_time,
            "not rewritten"
        );
    }

    #[test]
    fn same_file_names_from_different_folders_do_not_clash() {
        let f = Fixture::new();
        let a = f.file("a/Disk.st", b"a");
        let b = f.file("b/disk.ST", b"b");
        let p = f
            .prepare(&set("G", vec![a, b]), MediaPolicy::WorkingCopy)
            .unwrap();
        let names: Vec<_> = p
            .images
            .iter()
            .map(|i| i.file_name().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["Disk.st", "02 disk.ST"]);
        assert_eq!(fs::read(&p.images[1]).unwrap(), b"b");
    }

    #[test]
    fn zip_with_several_disks_and_hostile_names() {
        let f = Fixture::new();
        let zip = f.file(
            "Game.zip",
            &build_zip(&[
                (b"../../evil.st", b"1", None),
                (b"/abs/root.st", b"2", None),
                (b"C:\\win\\drive.st", b"3", None),
                (b"readme.txt", b"no", None),
                (b"inner.m3u", b"evil.st\n", None),
                (b"sub/#hash.msa", b"4", None),
                (b"..", b"x", None),
            ]),
        );
        let zip_before = fs::read(&zip).unwrap();
        let p = f
            .prepare(&set("Game", vec![zip.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        let work = f.saves().join("Game.disks");
        let names: Vec<_> = p
            .images
            .iter()
            .map(|i| {
                i.strip_prefix(&work)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["evil.st", "root.st", "drive.st", "_hash.msa"]);
        assert_eq!(
            fs::read_to_string(&p.content).unwrap(),
            "evil.st\nroot.st\ndrive.st\n_hash.msa\n"
        );
        assert_eq!(
            f.written(),
            [
                "saves/hatari/Game.disks/.emu-rust-disks",
                "saves/hatari/Game.disks/Game.m3u",
                "saves/hatari/Game.disks/_hash.msa",
                "saves/hatari/Game.disks/drive.st",
                "saves/hatari/Game.disks/evil.st",
                "saves/hatari/Game.disks/root.st",
            ]
        );
        assert!(!f.tmp.path().join("evil.st").exists());
        assert_eq!(fs::read(&zip).unwrap(), zip_before);
    }

    #[test]
    fn zip_passed_whole_when_the_core_reads_zips() {
        let f = Fixture::new();
        let zip = f.file("Game.zip", &build_zip(&[(b"a.st", b"1", None)]));
        let exts = vec!["st".to_owned(), "zip".to_owned()];
        let p = prepare_media(
            &set("Game", vec![zip.clone()]),
            MediaPolicy::Direct,
            &exts,
            &f.saves(),
        )
        .unwrap();
        assert_eq!(p.content, zip);
        assert!(p.temp_dir().is_none());
    }

    #[test]
    fn zip_entry_lying_about_its_size_is_refused() {
        let f = Fixture::new();
        let zip = f.file("Big.zip", &build_zip(&[(b"big.st", &[7u8; 100], Some(10))]));
        let limits = Limits {
            max_size: 50,
            ..Limits::DEFAULT
        };
        let err = prepare_media_with(
            &set("Big", vec![zip]),
            MediaPolicy::WorkingCopy,
            &exts(),
            &f.saves(),
            limits,
        )
        .unwrap_err();
        // The reader stops at the declared size and the CRC then fails: refused either way.
        assert!(
            matches!(
                err,
                RuntimeError::ContentTooLarge { .. } | RuntimeError::Archive { .. }
            ),
            "{err}"
        );
        let declared_too_big = f.file("Big2.zip", &build_zip(&[(b"big.st", &[7u8; 100], None)]));
        let err = prepare_media_with(
            &set("Big2", vec![declared_too_big]),
            MediaPolicy::WorkingCopy,
            &exts(),
            &f.saves(),
            limits,
        )
        .unwrap_err();
        assert!(matches!(err, RuntimeError::ContentTooLarge { .. }), "{err}");
        let plain = f.file("Plain.st", &[1u8; 100]);
        let err = prepare_media_with(
            &set("P", vec![plain]),
            MediaPolicy::WorkingCopy,
            &exts(),
            &f.saves(),
            limits,
        )
        .unwrap_err();
        assert!(matches!(err, RuntimeError::ContentTooLarge { .. }), "{err}");
        assert_eq!(f.written(), Vec::<String>::new(), "no partial copy left");
    }

    #[test]
    fn too_many_images_or_none() {
        let f = Fixture::new();
        let many: Vec<_> = (0..=MAX_MEDIA_IMAGES)
            .map(|i| f.file(&format!("{i}.st"), b"x"))
            .collect();
        assert!(matches!(
            f.prepare(&set("G", many), MediaPolicy::WorkingCopy),
            Err(RuntimeError::InvalidMediaSet(_))
        ));
        let names: Vec<String> = (0..=MAX_MEDIA_IMAGES).map(|i| format!("{i}.st")).collect();
        let entries: Vec<(&[u8], &[u8], Option<u32>)> = names
            .iter()
            .map(|n| (n.as_bytes(), &b"x"[..], None))
            .collect();
        let zip = f.file("Many.zip", &build_zip(&entries));
        assert!(matches!(
            f.prepare(&set("M", vec![zip]), MediaPolicy::Direct),
            Err(RuntimeError::InvalidMediaSet(_))
        ));
        assert!(matches!(
            f.prepare(&set("E", Vec::new()), MediaPolicy::Direct),
            Err(RuntimeError::InvalidMediaSet(_))
        ));
        let empty = f.file("Empty.zip", &build_zip(&[(b"notes.txt", b"x", None)]));
        assert!(matches!(
            f.prepare(&set("Z", vec![empty]), MediaPolicy::Direct),
            Err(RuntimeError::NoContentInArchive(_))
        ));
    }

    #[test]
    fn direct_passes_files_through_and_keeps_extractions_private() {
        let f = Fixture::new();
        let d1 = f.file("d1.st", b"1");
        let single = f
            .prepare(&set("One", vec![d1.clone()]), MediaPolicy::Direct)
            .unwrap();
        assert_eq!(single.content, d1);
        assert_eq!(single.work_dir, None);
        assert!(single.temp_dir().is_none());

        let zip = f.file("d2.zip", &build_zip(&[(b"d2.st", b"2", None)]));
        let p = f
            .prepare(&set("Two", vec![d1.clone(), zip]), MediaPolicy::Direct)
            .unwrap();
        let temp = p.temp_dir().unwrap().to_path_buf();
        assert_eq!(p.images, [d1.clone(), temp.join("d2.st")]);
        assert_eq!(p.content, temp.join("Two.m3u"));
        assert_eq!(
            fs::read_to_string(&p.content).unwrap(),
            format!("{}\n{}\n", d1.display(), temp.join("d2.st").display())
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&temp).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        assert_eq!(f.written(), Vec::<String>::new(), "nothing in saves");
        let clone = p.clone();
        drop(p);
        assert!(temp.exists(), "kept by the clone");
        drop(clone);
        assert!(!temp.exists());
    }

    #[test]
    fn user_playlist_direct_is_used_as_is() {
        let f = Fixture::new();
        f.file("sub/a.st", b"a");
        let m3u = f.file("Game.m3u", b"#EXTM3U\nsub/a.st|Disk A\n\nsub/b.st\n");
        let s = MediaSet {
            name: "Game".into(),
            images: Vec::new(),
            playlist: Some(m3u.clone()),
        };
        let p = f.prepare(&s, MediaPolicy::Direct).unwrap();
        assert_eq!(p.content, m3u);
        assert_eq!(
            p.images,
            [f.roms().join("sub/a.st"), f.roms().join("sub/b.st")]
        );
        // A lone `.m3u` in `images` is the playlist.
        assert_eq!(
            f.prepare(&set("Game", vec![m3u.clone()]), MediaPolicy::Direct)
                .unwrap(),
            p
        );
    }

    #[test]
    fn user_playlist_working_copy_copies_resolved_entries() {
        let f = Fixture::new();
        let a = f.file("sub/a.st", b"a");
        let abs = f.file("elsewhere/b.st", b"b");
        let m3u = f.file(
            "Game.m3u",
            format!("\u{feff}sub\\a.st\r\n# comment\r\n{}\r\n", abs.display()).as_bytes(),
        );
        let p = f
            .prepare(&set("Game", vec![m3u.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        let work = f.saves().join("Game.disks");
        assert_eq!(p.images, [work.join("a.st"), work.join("b.st")]);
        assert_eq!(fs::read_to_string(&p.content).unwrap(), "a.st\nb.st\n");
        assert_eq!(fs::read(&a).unwrap(), b"a");
        assert_eq!(p.content, work.join("Game.m3u"));
        assert_ne!(p.content, m3u);
    }

    #[test]
    fn hostile_playlists_are_refused() {
        let f = Fixture::new();
        f.file("secret.st", b"s");
        for text in [
            "../secret.st\n",
            "sub/../../secret.st\n",
            "\n# only comments\n",
        ] {
            let m3u = f.file("roms2/Game.m3u", text.as_bytes());
            let err = f
                .prepare(&set("Game", vec![m3u]), MediaPolicy::WorkingCopy)
                .unwrap_err();
            assert!(
                matches!(err, RuntimeError::InvalidMediaSet(_)),
                "{text:?}: {err}"
            );
        }
        let long: String = (0..=MAX_MEDIA_IMAGES)
            .map(|i| format!("{i}.st\n"))
            .collect();
        let m3u = f.file("Long.m3u", long.as_bytes());
        assert!(matches!(
            parse_playlist(&m3u),
            Err(RuntimeError::InvalidMediaSet(_))
        ));
        let big = f.file("Big.m3u", &vec![b'#'; MAX_PLAYLIST_BYTES + 1]);
        assert!(matches!(
            parse_playlist(&big),
            Err(RuntimeError::InvalidMediaSet(_))
        ));
        assert_eq!(f.written(), Vec::<String>::new());
    }

    #[test]
    fn reset_removes_only_the_game_copies() {
        let f = Fixture::new();
        let d = f.file("G.st", b"g");
        let other = f.file("H.st", b"h");
        f.prepare(&set("G", vec![d.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        f.prepare(&set("H", vec![other]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert!(reset_working_copies(&f.saves(), "G").unwrap());
        assert!(!reset_working_copies(&f.saves(), "G").unwrap());
        assert_eq!(
            f.written(),
            [
                "saves/hatari/H.disks/.emu-rust-disks",
                "saves/hatari/H.disks/H.st"
            ]
        );
        assert_eq!(fs::read(&d).unwrap(), b"g");
        // The name is sanitized the same way as at preparation.
        f.prepare(&set("A/B", vec![d]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert!(f.saves().join("A_B.disks").is_dir());
        assert!(reset_working_copies(&f.saves(), "A/B").unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn reset_never_follows_a_symlinked_folder() {
        let f = Fixture::new();
        let target = f.tmp.path().join("precious");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.st"), b"k").unwrap();
        std::os::unix::fs::symlink(&target, f.saves().join("G.disks")).unwrap();
        assert!(reset_working_copies(&f.saves(), "G").unwrap());
        assert!(target.join("keep.st").exists());
    }

    #[test]
    fn legacy_folder_copy_of_another_size_is_not_adopted() {
        let f = Fixture::new();
        let src = f.file("G/Disk 1.st", b"mine");
        let dir = f.saves().join("G.disks");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Disk 1.st"), b"someone else's disk").unwrap();
        let p = f
            .prepare(&set("G", vec![src.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(
            fs::read(dir.join("Disk 1.st")).unwrap(),
            b"someone else's disk"
        );
        assert_ne!(p.images[0], dir.join("Disk 1.st"));
        assert_eq!(fs::read(&p.images[0]).unwrap(), b"mine");
        // Same size: adopted as before.
        let dir2 = f.saves().join("H.disks");
        fs::create_dir_all(&dir2).unwrap();
        fs::write(dir2.join("Disk 1.st"), b"prog").unwrap();
        let q = f
            .prepare(&set("H", vec![src]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(q.images[0], dir2.join("Disk 1.st"));
        assert_eq!(fs::read(&q.images[0]).unwrap(), b"prog");
    }

    #[test]
    fn remembered_added_disks_are_never_forgotten() {
        let f = Fixture::new();
        let src = f.file("G/Disk 1.st", b"mine");
        let p = f
            .prepare(&set("G", vec![src.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        let dir = p.work_dir.unwrap();
        let manifest = dir.join(MANIFEST_NAME);
        let mut text = fs::read_to_string(&manifest).unwrap();
        for i in 0..100 {
            fs::write(dir.join(format!("add{i:03}.st")), b"x").unwrap();
            text.push_str(&format!(
                "added\tadd{i:03}.st\t\t1\t-\t00000001\t/nowhere{i}\n"
            ));
        }
        fs::write(&manifest, text).unwrap();
        f.prepare(&set("G", vec![src]), MediaPolicy::WorkingCopy)
            .unwrap();
        let DirState::Manifest(entries) = workcopy::inspect(&dir, is_disk_file_name).unwrap()
        else {
            panic!("manifest unreadable after rewrite");
        };
        let added: Vec<_> = entries
            .iter()
            .filter(|e| e.origin == Origin::Added)
            .collect();
        assert_eq!(
            added.len(),
            100,
            "nothing forgotten within the manifest's bound"
        );
        assert_eq!(added.first().unwrap().copy, "add000.st");
        assert_eq!(added.last().unwrap().copy, "add099.st");
        // A manifest at its bound refuses one more added disk rather than dropping one.
        let extra = f.file("More/Extra.st", b"more");
        let mut text = fs::read_to_string(&manifest).unwrap();
        for i in 100..workcopy::MAX_MANIFEST_ENTRIES - 1 {
            fs::write(dir.join(format!("add{i:03}.st")), b"x").unwrap();
            text.push_str(&format!(
                "added\tadd{i:03}.st\t\t1\t-\t00000001\t/nowhere{i}\n"
            ));
        }
        fs::write(&manifest, &text).unwrap();
        assert!(matches!(
            stage_added_copy(&dir, &extra, &exts(), Limits::DEFAULT, &[]),
            Err(RuntimeError::InvalidMediaSet(_))
        ));
        assert_eq!(fs::read_to_string(&manifest).unwrap(), text);
    }

    #[test]
    fn quick_match_recognises_the_own_folder_without_hashing() {
        let f = Fixture::new();
        let one = f.file("A/d.st", b"one");
        let two = f.file("B/d.st", b"two");
        let first = f
            .prepare(&set("d", vec![one.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        let second = f
            .prepare(&set("d", vec![two.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        let state = |p: &PreparedMedia| {
            let dir = p.work_dir.clone().unwrap();
            let DirState::Manifest(e) = workcopy::inspect(&dir, is_disk_file_name).unwrap() else {
                panic!()
            };
            let present = workcopy::present(&dir, &e).unwrap();
            (e, present)
        };
        let (e1, p1) = state(&first);
        let (e2, p2) = state(&second);
        let plan = Plan::new(&[two], &exts(), Limits::DEFAULT, MAX_MEDIA_IMAGES).unwrap();
        assert!(plan.quick_match(&e2, &p2));
        assert!(!plan.quick_match(&e1, &p1));
    }

    #[test]
    fn playlist_refuses_control_characters_and_non_utf8() {
        let f = Fixture::new();
        f.file("a.st", b"a");
        for bytes in [
            &b"a\x01.st\n"[..],
            b"a\x1b[2J.st\n",
            b"a\x7f.st\n",
            b"a.st\xff\n",
            b"\xff\xfe#x\na.st\n",
        ] {
            let m3u = f.file("List.m3u", bytes);
            assert!(
                matches!(parse_playlist(&m3u), Err(RuntimeError::InvalidMediaSet(_))),
                "{bytes:?}"
            );
        }
        let ok = f.file("Ok.m3u", b"\xef\xbb\xbf# c\r\na.st|label\r\n");
        assert_eq!(parse_playlist(&ok).unwrap().len(), 1);
    }

    #[test]
    fn homonymous_games_get_their_own_copies() {
        let f = Fixture::new();
        let one = f.file("Jeu1/disk1.st", b"jeu1");
        let two = f.file("Jeu2/disk1.st", b"jeu2");
        let first = f
            .prepare(&set("disk1", vec![one.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(first.notice, None);
        fs::write(&first.content, b"progress 1").unwrap();
        let second = f
            .prepare(&set("disk1", vec![two.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(second.work_dir, Some(f.saves().join("disk1 (2).disks")));
        assert_eq!(second.notice, Some(WorkDirNotice::SameName));
        assert_eq!(fs::read(&second.content).unwrap(), b"jeu2");
        assert_eq!(fs::read(&first.content).unwrap(), b"progress 1");
        fs::write(&second.content, b"progress 2").unwrap();
        // Later launches find their folder again, silently, whatever the order.
        for (src, progress, dir) in [
            (&two, &b"progress 2"[..], "disk1 (2).disks"),
            (&one, b"progress 1", "disk1.disks"),
        ] {
            let p = f
                .prepare(&set("disk1", vec![src.clone()]), MediaPolicy::WorkingCopy)
                .unwrap();
            assert_eq!(p.work_dir, Some(f.saves().join(dir)));
            assert_eq!(p.notice, None);
            assert_eq!(fs::read(&p.content).unwrap(), progress);
        }
        // Resetting the first folder does not hand the second game's copies to the first one.
        assert!(reset_work_dir(&f.saves(), first.work_dir.as_ref().unwrap()).unwrap());
        let again = f
            .prepare(&set("disk1", vec![one]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(again.work_dir, first.work_dir);
        assert_eq!(again.notice, None);
        assert_eq!(fs::read(&again.content).unwrap(), b"jeu1");
        assert_eq!(fs::read(&second.content).unwrap(), b"progress 2");
    }

    #[test]
    fn replaced_source_never_reuses_the_old_progress() {
        let f = Fixture::new();
        let d1 = f.file("G/d1.st", b"v1 disk 1");
        let d2 = f.file("G/d2.st", b"v1 disk 2");
        let s = set("G", vec![d1.clone(), d2.clone()]);
        let p = f.prepare(&s, MediaPolicy::WorkingCopy).unwrap();
        fs::write(&p.images[0], b"progress").unwrap();
        // Same size, other content (a fixed dump). Only the content decides: same size, so the CRC32 is read.
        let mtime =
            fs::metadata(&d2).unwrap().modified().unwrap() + std::time::Duration::from_secs(10);
        fs::write(&d2, b"v2 disk 2").unwrap();
        fs::File::options()
            .write(true)
            .open(&d2)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        let q = f.prepare(&s, MediaPolicy::WorkingCopy).unwrap();
        assert_eq!(q.work_dir, Some(f.saves().join("G (2).disks")));
        assert_eq!(
            q.notice,
            Some(WorkDirNotice::SourceChanged {
                previous: f.saves().join("G.disks")
            })
        );
        assert_eq!(
            snapshot(&q.images),
            [b"v1 disk 1".to_vec(), b"v2 disk 2".to_vec()]
        );
        assert_eq!(fs::read(&p.images[0]).unwrap(), b"progress");
        assert_eq!(fs::read(&p.images[1]).unwrap(), b"v1 disk 2");
        let again = f.prepare(&s, MediaPolicy::WorkingCopy).unwrap();
        assert_eq!(again.work_dir, q.work_dir);
        assert_eq!(again.notice, None);
    }

    #[test]
    fn moved_touched_or_extended_sets_keep_their_copies() {
        let f = Fixture::new();
        let d1 = f.file("G/d1.st", b"disk 1");
        let p = f
            .prepare(&set("G", vec![d1.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        fs::write(&p.content, b"progress").unwrap();
        // Moved elsewhere with the same content, then touched.
        let moved = f.file("Moved/d1.st", b"disk 1");
        fs::remove_file(&d1).unwrap();
        fs::File::options()
            .write(true)
            .open(&moved)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        let q = f
            .prepare(&set("G", vec![moved.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(
            (q.work_dir.clone(), q.notice.clone()),
            (p.work_dir.clone(), None)
        );
        assert_eq!(fs::read(&q.content).unwrap(), b"progress");
        // A disk joins the set: same folder, the new disk gets its copy.
        let d2 = f.file("Moved/d2.st", b"disk 2");
        let r = f
            .prepare(&set("G", vec![moved.clone(), d2]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(r.work_dir, p.work_dir);
        assert_eq!(
            snapshot(&r.images),
            [b"progress".to_vec(), b"disk 2".to_vec()]
        );
        // ... and leaves it: the folder now records it, so the shorter set gets new copies.
        let s = f
            .prepare(&set("G", vec![moved]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(s.work_dir, Some(f.saves().join("G (2).disks")));
        assert!(matches!(
            s.notice,
            Some(WorkDirNotice::SourceChanged { .. })
        ));
        assert_eq!(fs::read(&r.images[0]).unwrap(), b"progress");
    }

    #[test]
    fn folder_without_manifest_is_adopted_once_then_recorded() {
        let f = Fixture::new();
        let d = f.file("Old/Game.st", b"original");
        let work = f.saves().join("Game.disks");
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join("Game.st"), b"progress").unwrap();
        let p = f
            .prepare(&set("Game", vec![d.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(
            (p.work_dir.as_deref(), p.notice.clone()),
            (Some(work.as_path()), None)
        );
        assert_eq!(fs::read(&p.content).unwrap(), b"progress");
        assert!(work.join(MANIFEST_NAME).is_file());
        // Recorded for this source: another game with the same name no longer gets it.
        let other = f.file("Other/Game.st", b"other");
        let q = f
            .prepare(&set("Game", vec![other]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(q.notice, Some(WorkDirNotice::SameName));
        assert_eq!(fs::read(&q.content).unwrap(), b"other");
        assert_eq!(fs::read(&p.content).unwrap(), b"progress");
    }

    #[test]
    fn unreadable_manifest_or_non_folder_is_left_alone() {
        let f = Fixture::new();
        let d = f.file("Game.st", b"g");
        let work = f.saves().join("Game.disks");
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join(MANIFEST_NAME), b"garbage").unwrap();
        fs::write(work.join("Game.st"), b"keep").unwrap();
        fs::write(f.saves().join("Game (2).disks"), b"a file").unwrap();
        let p = f
            .prepare(&set("Game", vec![d]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(p.work_dir, Some(f.saves().join("Game (3).disks")));
        assert_eq!(p.notice, Some(WorkDirNotice::SameName));
        assert_eq!(fs::read(work.join("Game.st")).unwrap(), b"keep");
        assert_eq!(fs::read(work.join(MANIFEST_NAME)).unwrap(), b"garbage");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_a_copy_is_never_adopted() {
        let f = Fixture::new();
        let d = f.file("Game.st", b"g");
        let target = f.tmp.path().join("precious.st");
        fs::write(&target, b"precious").unwrap();
        let work = f.saves().join("Game.disks");
        fs::create_dir_all(&work).unwrap();
        std::os::unix::fs::symlink(&target, work.join("Game.st")).unwrap();
        // Pre-manifest folder: the name is taken by the link, the copy goes next to it.
        let p = f
            .prepare(&set("Game", vec![d.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(p.content, work.join("01 Game.st"));
        assert_eq!(fs::read(&p.content).unwrap(), b"g");
        // Recorded copy replaced by a link: not progress of ours, a new copy is made.
        fs::remove_file(&p.content).unwrap();
        std::os::unix::fs::symlink(&target, &p.content).unwrap();
        let q = f
            .prepare(&set("Game", vec![d]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(q.work_dir.as_deref(), Some(work.as_path()));
        assert_ne!(q.content, p.content);
        assert_eq!(fs::read(&q.content).unwrap(), b"g");
        assert_eq!(fs::read(&target).unwrap(), b"precious");
        // A link in place of the folder is skipped too.
        let other = f.tmp.path().join("elsewhere");
        fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink(&other, f.saves().join("H.disks")).unwrap();
        let h = f.file("H.st", b"h");
        let r = f
            .prepare(&set("H", vec![h]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(r.work_dir, Some(f.saves().join("H (2).disks")));
        assert_eq!(fs::read_dir(&other).unwrap().count(), 0);
    }

    #[test]
    fn disk_added_from_the_menu_is_reused_in_later_sessions() {
        let f = Fixture::new();
        let own = f.file("Game/Disk 1.st", b"own");
        let extra = f.file("Extras/Save.st", b"blank");
        let other = f.file("Other/Save.st", b"other");
        let p = f
            .prepare(&set("Game", vec![own.clone()]), MediaPolicy::WorkingCopy)
            .unwrap();
        let work = p.work_dir.clone().unwrap();
        let add = |src: &Path, known: &[PathBuf]| {
            stage_added_copy(&work, src, &exts(), Limits::DEFAULT, known).unwrap()
        };
        let added = add(&extra, &p.images);
        assert_eq!(added, work.join("Save.st"));
        fs::write(&added, b"saved game").unwrap();
        // Next session: same source, same copy and progress; the set itself is unaffected.
        let p2 = f
            .prepare(&set("Game", vec![own]), MediaPolicy::WorkingCopy)
            .unwrap();
        assert_eq!(p2.work_dir, p.work_dir);
        assert_eq!(add(&extra, &p2.images), added);
        assert_eq!(fs::read(&added).unwrap(), b"saved game");
        // Another source with the same name, or the same one twice in a session: separate copies.
        let o = add(&other, &p2.images);
        assert_ne!(o, added);
        assert_eq!(fs::read(&o).unwrap(), b"other");
        let mut known = p2.images.clone();
        known.push(added.clone());
        let twice = add(&extra, &known);
        assert_ne!(twice, added);
        assert_eq!(fs::read(&twice).unwrap(), b"blank");
    }

    #[test]
    fn reset_work_dir_only_takes_a_disks_folder_of_the_core() {
        let f = Fixture::new();
        let elsewhere = f.tmp.path().join("x.disks");
        fs::create_dir_all(&elsewhere).unwrap();
        for bad in [
            elsewhere.clone(),
            f.saves().join("Game"),
            f.saves().join(".disks"),
        ] {
            assert!(
                matches!(
                    reset_work_dir(&f.saves(), &bad),
                    Err(RuntimeError::InvalidMediaSet(_))
                ),
                "{}",
                bad.display()
            );
        }
        assert!(elsewhere.exists());
        assert!(!reset_work_dir(&f.saves(), &f.saves().join("G (2).disks")).unwrap());
    }

    #[test]
    fn disk_file_names() {
        assert_eq!(disk_file_name("#1.st"), "_1.st");
        assert_eq!(disk_file_name(".."), "disk");
        assert_eq!(disk_file_name("a/b.st"), "a_b.st");
    }
}
