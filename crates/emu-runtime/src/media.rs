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
use std::ops::Range;
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
    Media { set: MediaSet, rules: MediaRules },
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

/// Largest file copied into a working-copy folder (Amiga `.hdf`); see [`MediaRules::max_copy_bytes`].
pub const MAX_COPY_BYTES: u64 = 2 << 30;

/// How the sources of a [`MediaSet`] reach the core (ADR 0017 §6, `docs/design/v3.0-amiga.md` §3.5.1). Each
/// source is evaluated in this order: (1) extension listed in `media_direct`: handed over in place (absolute
/// path), never copied nor extracted, even a zip; (2) otherwise a `.zip` is inspected whatever extensions the
/// core declares, and extracted when it holds at least one image the core accepts; (3) otherwise `policy`
/// applies (a zip without any accepted image is passed whole when the core declares `zip`, refused if not).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRules {
    /// Default policy of the core (`media` of `emu-assets/data/cores.toml`).
    pub policy: MediaPolicy,
    /// Lower-case extensions without the dot (`media_direct` of `cores.toml`; puae: `ipf`, `lha`).
    pub media_direct: Vec<String>,
    /// A source larger than this is refused (`InvalidMediaSet`) instead of being copied.
    pub max_copy_bytes: u64,
}

impl MediaRules {
    /// No direct extension, [`MAX_COPY_BYTES`].
    pub fn new(policy: MediaPolicy) -> Self {
        Self {
            policy,
            media_direct: Vec::new(),
            max_copy_bytes: MAX_COPY_BYTES,
        }
    }

    /// The source's extension is in `media_direct` (case ignored): it is handed over in place.
    fn is_direct(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| accepts_ext(&self.media_direct, e))
    }
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

/// Copies / extracts / writes the playlist as `rules` require. Never modifies the files of `set`.
///
/// `saves_core_dir` = `<saves>/<core_id>`. Working copies are created only when absent (an existing copy holds
/// the player's progress and is reused); every write is atomic (`write_atomic`).
///
/// Each source (image of the set, or entry of a user `.m3u`) is evaluated in the order of design v3.0 §3.5.1:
/// (1) extension in `media_direct`: referenced in place by its absolute path, never copied nor extracted, even a
/// zip; (2) otherwise a `.zip` is inspected (central directory only) whatever extensions the core declares, and
/// its entries the core accepts (other than `zip`, `7z`, `m3u`) are extracted into the working-copy folder
/// (`WorkingCopy`) or a private temporary directory (`Direct`); (3) otherwise the policy applies: a plain file is
/// copied (`WorkingCopy`, at most `max_copy_bytes`, else `InvalidMediaSet`) or referenced in place (`Direct`); a
/// zip holding no accepted image is handed over whole when the core declares `zip` (copied under
/// `WorkingCopy`, in place under `Direct`) and refused otherwise (`NoContentInArchive`).
///
/// Working-copy folder (`workcopy.rs` for the identity of a source): `<game>.disks`, `<game> (2).disks`, … are
/// examined in order and the first one whose manifest records copies of this set's sources is used; a folder
/// is compatible when each copy recorded there for a set (and still present) has an identical source in this
/// set (new disks of the set are simply added). Otherwise the copies are started in the first free candidate
/// and [`PreparedMedia::notice`] says why: progress is never overwritten nor handed to another game. A folder
/// without manifest (made by an earlier version) is adopted as is by the first set prepared into it, which
/// then records its sources: its origin cannot be known, so this is the earlier behaviour, once. A link or a
/// file in place of a folder, an unreadable manifest, or a link in place of a copy is never used. Zip entries
/// are read with the limits of `content.rs`; entry names are never used as paths (sanitised with `fsname`),
/// links and folders inside an archive are skipped. A `WorkingCopy` set whose sources are all direct needs no
/// folder: like `Direct`, its playlist (if any) goes to a private temporary directory.
///
/// Generated `.m3u`: names relative to its own folder for copies, absolute paths for everything else. When it
/// is made from a user `.m3u`, only white-listed `#` lines are kept, at their place (`#SAVEDISK:`, `#LABEL:`),
/// as is the `|label` of an entry that gives exactly one image; any other `#` line is a comment and is dropped
/// (`#COMMAND:` included: no core of this version needs it). A user `.m3u` under `Direct` is handed over
/// untouched (2.0). Out of disk space: [`RuntimeError::DiskFull`], the partial file is removed.
pub fn prepare_media(
    set: &MediaSet,
    rules: &MediaRules,
    accepted_extensions: &[String],
    saves_core_dir: &Path,
) -> Result<PreparedMedia, RuntimeError> {
    prepare_media_with(
        set,
        rules,
        accepted_extensions,
        saves_core_dir,
        Limits::DEFAULT,
    )
}

pub(crate) fn prepare_media_with(
    set: &MediaSet,
    rules: &MediaRules,
    exts: &[String],
    saves_core_dir: &Path,
    limits: Limits,
) -> Result<PreparedMedia, RuntimeError> {
    prepare_media_inner(set, rules, exts, saves_core_dir, limits).map_err(disk_full)
}

fn prepare_media_inner(
    set: &MediaSet,
    rules: &MediaRules,
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

    if rules.policy == MediaPolicy::Direct
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

    let layout = playlist.as_deref().map(read_playlist).transpose()?;
    let sources = match &layout {
        Some(layout) => layout.entries(),
        None => set.images.clone(),
    };
    let all_direct = sources.iter().all(|s| rules.is_direct(s));
    if rules.policy == MediaPolicy::WorkingCopy && !all_direct {
        return prepare_working_copies(
            &game,
            &sources,
            layout.as_ref(),
            rules,
            exts,
            saves_core_dir,
            limits,
        );
    }
    // `Direct`, or only direct sources: nothing is copied, a playlist goes to a private temporary directory.
    let mut stage = Stage::new(MediaPolicy::Direct, exts, limits, PathBuf::new())?;
    stage.media_direct = &rules.media_direct;
    stage.max_copy = rules.max_copy_bytes;
    let mut ranges = Vec::with_capacity(sources.len());
    for source in &sources {
        let start = stage.images.len();
        stage.add_source(source, MAX_MEDIA_IMAGES)?;
        ranges.push(start..stage.images.len());
    }
    let content = stage.content(&game, layout.as_ref(), &ranges)?;
    Ok(PreparedMedia {
        content,
        images: stage.images,
        work_dir: None,
        notice: None,
        temp_dir: stage.temp_dir,
    })
}

/// An I/O error meaning "no space left" (or quota exceeded) becomes [`RuntimeError::DiskFull`].
/// `write_atomic_with` has already removed the partial file.
fn disk_full(err: RuntimeError) -> RuntimeError {
    match err {
        RuntimeError::Io { path, source }
            if matches!(
                source.kind(),
                io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded
            ) =>
        {
            tracing::warn!("disk full while writing {}", path.display());
            RuntimeError::DiskFull { path }
        }
        other => other,
    }
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
    layout: Option<&UserPlaylist>,
    rules: &MediaRules,
    exts: &[String],
    saves_core_dir: &Path,
    limits: Limits,
) -> Result<PreparedMedia, RuntimeError> {
    // Direct sources are referenced in place: neither planned, nor recorded in the manifest.
    let copied: Vec<PathBuf> = sources
        .iter()
        .filter(|s| !rules.is_direct(s))
        .cloned()
        .collect();
    let mut plan = Plan::new(
        &copied,
        exts,
        limits,
        MAX_MEDIA_IMAGES,
        rules.max_copy_bytes,
    )?;
    let chosen = choose_work_dir(saves_core_dir, game, &mut plan)?;
    let mut stage = Stage::new(MediaPolicy::WorkingCopy, exts, limits, chosen.dir.clone())?;
    stage.media_direct = &rules.media_direct;
    stage.max_copy = rules.max_copy_bytes;
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
    let mut ranges = Vec::with_capacity(sources.len());
    let mut next_item = 0;
    let mut plan_src = 0;
    for source in sources {
        let start = stage.images.len();
        if rules.is_direct(source) {
            stage.push_in_place(source)?;
            ranges.push(start..stage.images.len());
            continue;
        }
        while next_item < plan.items.len() && plan.items[next_item].src == plan_src {
            let i = next_item;
            next_item += 1;
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
        plan_src += 1;
        ranges.push(start..stage.images.len());
    }
    // Disks added from the menu stay available to later sessions.
    // Only the newest ones: the manifest reader refuses more than it expects.
    let kept: Vec<&ManifestEntry> = chosen
        .entries
        .iter()
        .enumerate()
        .filter(|&(j, e)| !consumed[j] && chosen.present[j] && e.origin == Origin::Added)
        .map(|(_, e)| e)
        .collect();
    let skip = kept.len().saturating_sub(MAX_ADDED_REMEMBERED);
    recorded.extend(kept.into_iter().skip(skip).cloned());
    let content = stage.content(game, layout, &ranges)?;
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
fn is_disk_file_name(name: &str) -> bool {
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
    /// `MediaRules::media_direct`: referenced in place.
    media_direct: &'a [String],
    /// `MediaRules::max_copy_bytes`: largest whole file copied.
    max_copy: u64,
    /// Bytes written so far by this stage (copies and extractions); the sum is bounded by `max_copy` too, so a
    /// zip of many large entries cannot fill the disk.
    written: u64,
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
            media_direct: &[],
            max_copy: MAX_COPY_BYTES,
            written: 0,
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

    /// Absolute path of a file handed over where it is (never written by us).
    fn push_in_place(&mut self, source: &Path) -> Result<(), RuntimeError> {
        let meta = fs::metadata(source).map_err(|e| io_error(source, e))?;
        if !meta.is_file() {
            return Err(RuntimeError::InvalidMediaSet(format!(
                "{} is not a file",
                source.display()
            )));
        }
        // Absolute so that a generated playlist in another folder still finds it.
        let path = std::path::absolute(source).map_err(|e| io_error(source, e))?;
        self.push(path)
    }

    /// One source, in the order of design v3.0 §3.5.1 (see [`prepare_media`]); a zip gives at most `max_per_zip`
    /// images (more than [`MAX_MEDIA_IMAGES`] in a set is an error).
    fn add_source(&mut self, source: &Path, max_per_zip: usize) -> Result<(), RuntimeError> {
        if accepts_ext(self.media_direct, ext_of(source)) {
            return self.push_in_place(source);
        }
        if has_ext(source, "zip") {
            let room = MAX_MEDIA_IMAGES
                .saturating_sub(self.images.len())
                .min(max_per_zip);
            match zip_images(source, self.exts, self.limits, room) {
                Ok(Some((mut archive, entries))) => {
                    if entries.len() > room && max_per_zip == MAX_MEDIA_IMAGES {
                        return Err(too_many());
                    }
                    for entry in entries.iter().take(room) {
                        self.extract_entry(&mut archive, source, entry)?;
                    }
                    return Ok(());
                }
                // No image inside: a core reading zips gets the archive whole (step 3).
                Ok(None) if accepts_ext(self.exts, "zip") => {}
                Ok(None) => return Err(RuntimeError::NoContentInArchive(source.to_path_buf())),
                // Invariant (2.0 behaviour kept): a zip refused by the inspection (too many entries, too large,
                // corrupt) is still handed over whole when the core reads zips; the core does its own parsing.
                Err(e) if accepts_ext(self.exts, "zip") => {
                    tracing::warn!(
                        "zip {} not inspected ({e}): handed over whole, the core reads zips",
                        source.display()
                    );
                }
                Err(e) => return Err(e),
            }
        }
        match self.policy {
            MediaPolicy::Direct => self.push_in_place(source),
            MediaPolicy::WorkingCopy => self.copy_plain(source).map(drop),
        }
    }

    /// Working copy of a whole file; an existing regular file at the chosen name is adopted (`!fresh` only).
    fn copy_plain(&mut self, source: &Path) -> Result<PathBuf, RuntimeError> {
        let name = disk_file_name(&source.file_name().unwrap_or_default().to_string_lossy());
        let size = fs::metadata(source).map_err(|e| io_error(source, e))?.len();
        self.reserve(source, size)?;
        let dest = self.dest_path(&name, size)?;
        if !exists(&dest)? {
            copy_file(source, &dest, self.max_copy)?;
            self.written += size;
            tracing::info!("working copy of {} created", source.display());
        }
        self.push(dest.clone())?;
        Ok(dest)
    }

    /// Refuses `size` more bytes when the set would exceed `max_copy` in total (same error as one oversized file).
    fn reserve(&self, source: &Path, size: u64) -> Result<(), RuntimeError> {
        let total = self.written.saturating_add(size);
        if size > self.max_copy || total > self.max_copy {
            return Err(copy_too_large(source, total, self.max_copy));
        }
        Ok(())
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
            self.reserve(zip, entry.declared)?;
            let mut failure = None;
            let mut extracted = 0;
            // An entry lying about its size is cut by the remaining budget, not only by `max_size`.
            let bound = self
                .limits
                .max_size
                .min(self.max_copy.saturating_sub(self.written));
            let written = write_atomic_with(&dest, FileMode::Default, |file| {
                content::copy_entry(archive, zip, entry, bound, file, &dest)
                    .map(|n| extracted = n)
                    .map_err(|e| {
                        failure = Some(e);
                        io::Error::other("zip entry not extracted")
                    })
            });
            if let Some(e) = failure {
                return Err(e);
            }
            written.map_err(|e| io_error(&dest, e))?;
            self.written += extracted;
            tracing::info!(entry = %entry.name, "disk image extracted from {}", zip.display());
        }
        self.push(dest.clone())?;
        Ok(dest)
    }

    /// File given to `retro_load_game`: the only image, or a generated playlist. A user playlist whose kept
    /// `#` lines must reach the core always gives a playlist.
    fn content(
        &mut self,
        game: &str,
        layout: Option<&UserPlaylist>,
        ranges: &[Range<usize>],
    ) -> Result<PathBuf, RuntimeError> {
        match self.images.as_slice() {
            [only] if !layout.is_some_and(UserPlaylist::has_directives) => Ok(only.clone()),
            _ => self.write_playlist(game, layout, ranges),
        }
    }

    /// `<game>.m3u` next to the copies (relative names for the copies, absolute paths otherwise) or in the temp
    /// directory (absolute paths). `layout` (a user playlist) puts its kept `#` lines and labels back in place;
    /// `ranges[k]` are the images given by its `k`-th entry. Rewritten only when its content changes.
    fn write_playlist(
        &mut self,
        game: &str,
        layout: Option<&UserPlaylist>,
        ranges: &[Range<usize>],
    ) -> Result<PathBuf, RuntimeError> {
        let dir = self.dest_dir()?;
        let path = dir.join(format!("{game}.m3u"));
        let mut text = Vec::new();
        let policy = self.policy;
        let line =
            |text: &mut Vec<u8>, image: &Path, label: Option<&str>| -> Result<(), RuntimeError> {
                let entry = match policy {
                    MediaPolicy::WorkingCopy => image.strip_prefix(&dir).unwrap_or(image),
                    MediaPolicy::Direct => image,
                };
                let bytes = path_bytes(entry)?;
                if bytes.iter().any(|&b| b == b'\n' || b == b'\r' || b == b'|')
                    || bytes.first() == Some(&b'#')
                {
                    return Err(RuntimeError::InvalidMediaSet(format!(
                        "{} cannot be listed in a playlist",
                        image.display()
                    )));
                }
                text.extend_from_slice(&bytes);
                if let Some(label) = label {
                    text.push(b'|');
                    text.extend_from_slice(label.as_bytes());
                }
                text.push(b'\n');
                Ok(())
            };
        match layout {
            None => {
                for image in &self.images {
                    line(&mut text, image, None)?;
                }
            }
            Some(layout) => {
                let mut k = 0;
                for item in &layout.lines {
                    match item {
                        M3uLine::Directive(d) => {
                            text.extend_from_slice(d.as_bytes());
                            text.push(b'\n');
                        }
                        M3uLine::Entry { label, .. } => {
                            let range = ranges.get(k).cloned().unwrap_or_default();
                            k += 1;
                            // A label names one disk: a zip giving several loses it.
                            let label = label.as_deref().filter(|_| range.len() == 1);
                            for image in &self.images[range] {
                                line(&mut text, image, label)?;
                            }
                        }
                    }
                }
            }
        }
        let unchanged = read_bounded(&path, text.len()).is_ok_and(|old| old == text);
        if !unchanged {
            write_atomic_with(&path, FileMode::Default, |f| io::Write::write_all(f, &text))
                .map_err(|e| io_error(&path, e))?;
        }
        Ok(path)
    }
}

/// Extension of `path` (empty when none or not UTF-8).
fn ext_of(path: &Path) -> &str {
    path.extension().and_then(|e| e.to_str()).unwrap_or("")
}

fn copy_too_large(source: &Path, size: u64, max: u64) -> RuntimeError {
    RuntimeError::InvalidMediaSet(format!(
        "{} is too large to be copied ({size} bytes, at most {max})",
        source.display()
    ))
}

/// An opened, untrusted zip archive.
pub(crate) type UserZip = ZipArchive<BufReader<fs::File>>;

/// Opens a zip as untrusted input and lists, from its central directory only, up to `max + 1` entries the core
/// accepts (`workcopy::zip_entry_accepted`: never `zip`, `7z` nor `m3u`; links and folders skipped); one more
/// than allowed, so that an oversized set is reported instead of silently losing disks. `None`: no such entry.
pub(crate) fn zip_images(
    zip: &Path,
    exts: &[String],
    limits: Limits,
    max: usize,
) -> Result<Option<(UserZip, Vec<ArchiveEntry>)>, RuntimeError> {
    let mut archive = content::open_archive(zip, limits)?;
    let entries = content::accepted_entries(
        &mut archive,
        zip,
        |name| workcopy::zip_entry_accepted(exts, name),
        max.saturating_add(1),
    )?;
    Ok((!entries.is_empty()).then_some((archive, entries)))
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

/// `#` lines of a user `.m3u` copied into a generated one (design v3.0 §3.5.1, white list): `#SAVEDISK:` (puae:
/// an empty save disk created by the core, named after the playlist) and `#LABEL:`. `#COMMAND:` is not here: it
/// is only meant for cores that type it into the machine (5.0), never for cores that read it as arguments.
const KEPT_DIRECTIVES: [&str; 2] = ["#SAVEDISK:", "#LABEL:"];

/// Longest `#` line or `|label` kept from a user playlist (longer ones are dropped, not cut).
const MAX_KEPT_LINE_BYTES: usize = 256;

/// A user `.m3u`, as kept to produce a working one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserPlaylist {
    pub lines: Vec<M3uLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum M3uLine {
    /// Resolved path of an entry, with its `|label` when it is safe to repeat.
    Entry {
        path: PathBuf,
        label: Option<String>,
    },
    /// A white-listed `#` line, verbatim (trimmed).
    Directive(String),
}

impl UserPlaylist {
    fn entries(&self) -> Vec<PathBuf> {
        self.lines
            .iter()
            .filter_map(|l| match l {
                M3uLine::Entry { path, .. } => Some(path.clone()),
                M3uLine::Directive(_) => None,
            })
            .collect()
    }

    fn has_directives(&self) -> bool {
        self.lines
            .iter()
            .any(|l| matches!(l, M3uLine::Directive(_)))
    }
}

/// Text kept from a user playlist (`#` line or label): no control character, bounded length.
fn keepable(text: &str) -> bool {
    !text.is_empty() && text.len() <= MAX_KEPT_LINE_BYTES && !text.chars().any(char::is_control)
}

/// Entries of a user `.m3u` (see [`read_playlist`]).
pub(crate) fn parse_playlist(playlist: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
    read_playlist(playlist).map(|p| p.entries())
}

/// A user `.m3u` (≤ 64 KiB, ≤ [`MAX_MEDIA_IMAGES`] entries): blank lines skipped, `#` lines kept only when
/// white-listed ([`KEPT_DIRECTIVES`]), a `|label` suffix kept apart, paths absolute or relative to the playlist's
/// folder; `..`, and drive or root prefixes on a relative path, are refused, as are non-UTF-8 files and control
/// characters inside an entry. Backslashes are separators on every platform (playlists made on Windows).
pub(crate) fn read_playlist(playlist: &Path) -> Result<UserPlaylist, RuntimeError> {
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
    let mut lines = Vec::new();
    let mut count = 0;
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            let kept = KEPT_DIRECTIVES.iter().any(|d| line.starts_with(d));
            if kept && keepable(line) {
                lines.push(M3uLine::Directive(line.to_owned()));
            } else if kept {
                tracing::warn!(
                    "{}: `#` line dropped (too long or control characters)",
                    playlist.display()
                );
            }
            continue;
        }
        let (entry, label) = match line.split_once('|') {
            Some((entry, label)) => (entry.trim(), Some(label.trim())),
            None => (line, None),
        };
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
        if count >= MAX_MEDIA_IMAGES {
            return Err(too_many());
        }
        count += 1;
        let label = label.filter(|l| keepable(l)).map(str::to_owned);
        lines.push(M3uLine::Entry { path, label });
    }
    if count == 0 {
        return Err(RuntimeError::InvalidMediaSet(format!(
            "{} lists no disk",
            playlist.display()
        )));
    }
    Ok(UserPlaylist { lines })
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

/// Deletes the save files a core wrote next to its saves for one game (`game_saves` of `cores.toml`, design
/// v3.0 §3.5.3; puae: `{game}.save*.adf` created for `#SAVEDISK:`), and returns the deleted paths. The core must
/// not be running that game.
///
/// `patterns` are **file names**, never paths: `{game}` stands for `content_name` (the content name handed to
/// the core), at most one `*` matches any run of characters other than `.` (so that the files of a game whose
/// name extends this one, `X.saveY` for `X`, are never matched). A pattern holding a path separator, `..`, or more
/// than one `*` is refused (`InvalidMediaSet`), nothing is deleted. Only regular files directly inside
/// `saves_core_dir` are looked at (not recursive, links never followed).
pub fn reset_core_game_files(
    saves_core_dir: &Path,
    content_name: &str,
    patterns: &[String],
) -> Result<Vec<PathBuf>, RuntimeError> {
    let invalid = |why: String| RuntimeError::InvalidMediaSet(why);
    if content_name.is_empty()
        || content_name == "."
        || content_name == ".."
        || content_name.contains(['/', '\\', '\0'])
    {
        return Err(invalid(format!("`{content_name}` is not a content name")));
    }
    let globs = patterns
        .iter()
        .map(|p| {
            SaveGlob::new(p, content_name)
                .ok_or_else(|| invalid(format!("invalid save pattern `{p}`")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if globs.is_empty() {
        return Ok(Vec::new());
    }
    let dir = match fs::read_dir(saves_core_dir) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error(saves_core_dir, e)),
    };
    let mut doomed = Vec::new();
    for entry in dir {
        let entry = entry.map_err(|e| io_error(saves_core_dir, e))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !globs.iter().any(|g| g.matches(&name)) {
            continue;
        }
        let path = entry.path();
        // `symlink_metadata`: a link (or a folder) matching the pattern is not a file the core wrote.
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => doomed.push(path),
            Ok(_) => tracing::warn!(
                "{} matches a save pattern but is not a file: kept",
                path.display()
            ),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(&path, e)),
        }
    }
    doomed.sort();
    let mut removed = Vec::with_capacity(doomed.len());
    for path in doomed {
        match fs::remove_file(&path) {
            Ok(()) => {
                tracing::info!("core save file removed: {}", path.display());
                removed.push(path);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(&path, e)),
        }
    }
    Ok(removed)
}

/// One `game_saves` pattern with `{game}` substituted: a literal file name, or `prefix*suffix`.
#[derive(Debug)]
struct SaveGlob {
    prefix: String,
    /// `None`: no `*`, the whole name is `prefix`.
    suffix: Option<String>,
}

impl SaveGlob {
    /// `None` for anything but a file name: separators, `..`, NUL, more than one `*`, or no `{game}` (a pattern
    /// that does not name the game would reach the files of every game).
    fn new(pattern: &str, game: &str) -> Option<Self> {
        if pattern.is_empty()
            || pattern.contains(['/', '\\', '\0', ':'])
            || pattern == "."
            || pattern == ".."
            || pattern.matches('*').count() > 1
            || !pattern.contains("{game}")
        {
            return None;
        }
        // Split before substituting: a `*` inside the game name is a literal character.
        let (prefix, suffix) = match pattern.split_once('*') {
            Some((p, s)) => (p, Some(s)),
            None => (pattern, None),
        };
        let prefix = prefix.replace("{game}", game);
        let suffix = suffix.map(|s| s.replace("{game}", game));
        let literal = prefix.clone() + suffix.as_deref().unwrap_or("");
        if literal.is_empty() || literal == "." || literal == ".." {
            return None;
        }
        Some(Self { prefix, suffix })
    }

    fn matches(&self, name: &str) -> bool {
        match &self.suffix {
            None => name == self.prefix,
            Some(suffix) => {
                name.len() >= self.prefix.len() + suffix.len()
                    && name.starts_with(&self.prefix)
                    && name.ends_with(suffix.as_str())
                    // The `*` never absorbs a `.`: `X.saveY.save1.adf` (game `X.saveY`) is not a file of
                    // game `X` for `{game}.save*.adf`. Cores number their saves, they do not add dots.
                    && !name[self.prefix.len()..name.len() - suffix.len()].contains('.')
            }
        }
    }
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
    /// gives its first accepted entry. The addition lasts for the session (the generated playlist is unchanged).
    pub fn add(&mut self, image: &Path) -> Result<u32, RuntimeError> {
        let num = self.control()?.num_images();
        if num as usize >= MAX_MEDIA_IMAGES {
            return Err(too_many());
        }
        let path = self.emulator.stage_added_image(image)?;
        let mut control = self.control()?;
        let was_ejected = control.is_ejected();
        if !was_ejected {
            control.set_ejected(true)?;
        }
        let added = control
            .add_image()
            .map_err(RuntimeError::from)
            .and_then(|()| {
                control.replace_image(num, Some(&path)).map_err(|e| {
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
            prepared.images.push(path.clone());
        }
        tracing::info!(index = num, "disk {} added", path.display());
        Ok(num)
    }
}

impl Emulator {
    /// Copy / extraction of an image added from the disk menu, as `prepare_media` would do it.
    fn stage_added_image(&mut self, image: &Path) -> Result<PathBuf, RuntimeError> {
        self.stage_added_image_inner(image).map_err(disk_full)
    }

    fn stage_added_image_inner(&mut self, image: &Path) -> Result<PathBuf, RuntimeError> {
        let rules = self
            .media_rules
            .clone()
            .unwrap_or_else(|| MediaRules::new(MediaPolicy::WorkingCopy));
        let work_dir = self
            .prepared
            .as_ref()
            .and_then(|p| p.work_dir.clone())
            .unwrap_or_else(|| work_dir_for(&self.save_root.join(&self.core_id), &self.game_name));
        let exts = self.core.system_info().valid_extensions.clone();
        if rules.policy == MediaPolicy::WorkingCopy && !rules.is_direct(image) {
            let known = self
                .prepared
                .as_ref()
                .map(|p| p.images.as_slice())
                .unwrap_or_default();
            return stage_added_copy(
                &work_dir,
                image,
                &exts,
                self.limits,
                rules.max_copy_bytes,
                known,
            );
        }
        let mut stage = Stage::new(MediaPolicy::Direct, &exts, self.limits, work_dir)?;
        stage.media_direct = &rules.media_direct;
        stage.max_copy = rules.max_copy_bytes;
        stage.temp_dir = self.prepared.as_ref().and_then(|p| p.temp_dir.clone());
        // Names already handed to the core keep theirs; a new name never overwrites them in the temp directory.
        stage.fresh = true;
        for known in self.prepared.iter().flat_map(|p| &p.images) {
            if let Some(name) = known.file_name() {
                stage.used.insert(name.to_string_lossy().to_lowercase());
            }
        }
        stage.add_source(image, 1)?;
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

/// Most disks added from the menu remembered per folder (the oldest are forgotten; their copies stay on disk).
const MAX_ADDED_REMEMBERED: usize = MAX_MEDIA_IMAGES;

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
    max_copy: u64,
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
    let mut plan = Plan::new(&[image.to_path_buf()], exts, limits, 1, max_copy)?;
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
    stage.max_copy = max_copy;
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
        let added = entries.iter().filter(|e| e.origin == Origin::Added).count();
        let mut forget = added.saturating_sub(MAX_ADDED_REMEMBERED);
        entries.retain(|e| {
            let drop = forget > 0 && e.origin == Origin::Added;
            forget -= usize::from(drop);
            !drop
        });
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
            prepare_media(set, &MediaRules::new(policy), &exts(), &self.saves())
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
            st.add_source(src, 1).unwrap();
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

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn rules(policy: MediaPolicy, direct: &[&str]) -> MediaRules {
        MediaRules {
            media_direct: strs(direct),
            ..MediaRules::new(policy)
        }
    }

    fn sha(paths: &[PathBuf]) -> Vec<u32> {
        paths
            .iter()
            .map(|p| crc32fast::hash(&fs::read(p).unwrap()))
            .collect()
    }

    /// §3.5.1 step 2: a zip holding accepted images is extracted even when the core declares `zip`.
    #[test]
    fn zip_of_disks_is_extracted_even_when_the_core_reads_zips() {
        let f = Fixture::new();
        let zip = f.file("Game.zip", &build_zip(&[(b"a.st", b"1", None)]));
        let exts = strs(&["st", "zip"]);
        let before = sha(std::slice::from_ref(&zip));
        let p = prepare_media(
            &set("Game", vec![zip.clone()]),
            &MediaRules::new(MediaPolicy::Direct),
            &exts,
            &f.saves(),
        )
        .unwrap();
        let temp = p.temp_dir().unwrap().to_path_buf();
        assert_eq!(p.content, temp.join("a.st"));
        let p = prepare_media(
            &set("Game", vec![zip.clone()]),
            &MediaRules::new(MediaPolicy::WorkingCopy),
            &exts,
            &f.saves(),
        )
        .unwrap();
        assert_eq!(p.content, f.saves().join("Game.disks/a.st"));
        assert_eq!(sha(&[zip]), before);
    }

    /// §3.5.1 step 3: a zip without accepted image is handed over whole (core with `zip`) or refused.
    #[test]
    fn zip_without_image_is_passed_whole_or_refused() {
        let f = Fixture::new();
        let zip = f.file(
            "Folder.zip",
            &build_zip(&[(b"data.bin", b"1", None), (b"inner.zip", b"z", None)]),
        );
        let with_zip = strs(&["st", "zip"]);
        let p = prepare_media(
            &set("Folder", vec![zip.clone()]),
            &MediaRules::new(MediaPolicy::Direct),
            &with_zip,
            &f.saves(),
        )
        .unwrap();
        assert_eq!(p.content, zip);
        assert!(p.temp_dir().is_none());
        // Working copy: the archive is copied whole, never extracted.
        let p = prepare_media(
            &set("Folder", vec![zip.clone()]),
            &MediaRules::new(MediaPolicy::WorkingCopy),
            &with_zip,
            &f.saves(),
        )
        .unwrap();
        assert_eq!(p.content, f.saves().join("Folder.disks/Folder.zip"));
        assert_eq!(fs::read(&p.content).unwrap(), fs::read(&zip).unwrap());
        for policy in [MediaPolicy::Direct, MediaPolicy::WorkingCopy] {
            let err = f
                .prepare(&set("Folder2", vec![zip.clone()]), policy)
                .unwrap_err();
            assert!(matches!(err, RuntimeError::NoContentInArchive(_)), "{err}");
        }
        // A broken archive: whole for a core reading zips, refused otherwise.
        let broken = f.file("Broken.zip", b"not a zip");
        let p = prepare_media(
            &set("Broken", vec![broken.clone()]),
            &MediaRules::new(MediaPolicy::Direct),
            &with_zip,
            &f.saves(),
        )
        .unwrap();
        assert_eq!(p.content, broken);
        assert!(
            f.prepare(&set("Broken", vec![broken]), MediaPolicy::Direct)
                .is_err()
        );
    }

    /// §3.5.1 step 1: a direct extension is referenced in place (absolute), never copied nor extracted, even a zip.
    #[test]
    fn direct_extensions_are_referenced_in_place() {
        let f = Fixture::new();
        let adf = f.file("Game/Disk 1.adf", b"adf");
        let ipf = f.file("Game/Disk 2.IPF", b"ipf");
        let zip = f.file("Dos/Game.zip", &build_zip(&[(b"game.adf", b"x", None)]));
        let originals = [adf.clone(), ipf.clone(), zip.clone()];
        let before = sha(&originals);
        let exts = strs(&["adf", "ipf", "zip"]);
        let r = rules(MediaPolicy::WorkingCopy, &["ipf", "zip"]);
        let p = prepare_media(
            &set("Game", vec![adf, ipf.clone(), zip.clone()]),
            &r,
            &exts,
            &f.saves(),
        )
        .unwrap();
        let work = f.saves().join("Game.disks");
        assert_eq!(p.work_dir.as_deref(), Some(work.as_path()));
        assert_eq!(
            p.images,
            [work.join("Disk 1.adf"), ipf.clone(), zip.clone()]
        );
        assert_eq!(
            fs::read_to_string(&p.content).unwrap(),
            format!("Disk 1.adf\n{}\n{}\n", ipf.display(), zip.display())
        );
        assert_eq!(
            f.written(),
            [
                "saves/hatari/Game.disks/.emu-rust-disks",
                "saves/hatari/Game.disks/Disk 1.adf",
                "saves/hatari/Game.disks/Game.m3u",
            ]
        );
        // The manifest only knows the copy: the next launch reuses the folder silently.
        let again = prepare_media(&set("Game", originals.to_vec()), &r, &exts, &f.saves()).unwrap();
        assert_eq!(again, p);
        assert_eq!(sha(&originals), before);

        // Only direct sources: no working-copy folder at all; one image = the file itself.
        let single = prepare_media(&set("Solo", vec![ipf.clone()]), &r, &exts, &f.saves()).unwrap();
        assert_eq!(single.content, ipf);
        assert_eq!(single.work_dir, None);
        assert!(single.temp_dir().is_none());
        let two = prepare_media(
            &set("Two", vec![ipf.clone(), zip.clone()]),
            &r,
            &exts,
            &f.saves(),
        )
        .unwrap();
        let temp = two.temp_dir().unwrap().to_path_buf();
        assert_eq!(two.content, temp.join("Two.m3u"));
        assert!(!f.saves().join("Two.disks").exists());
        assert!(two.images.iter().all(|i| i.is_absolute()));
        // A missing direct file is refused.
        let missing = f.roms().join("Gone.ipf");
        assert!(matches!(
            prepare_media(&set("Gone", vec![missing]), &r, &exts, &f.saves()),
            Err(RuntimeError::Io { .. })
        ));
        assert_eq!(sha(&originals), before);
    }

    /// hatari (no `zip`, no direct extension): exactly the 2.0 result.
    #[test]
    fn hatari_non_regression() {
        let f = Fixture::new();
        let st = f.file("G/d1.st", b"1");
        let zip = f.file(
            "G/d2.zip",
            &build_zip(&[(b"d2.msa", b"2", None), (b"x.txt", b"t", None)]),
        );
        let before = sha(&[st.clone(), zip.clone()]);
        let p = f
            .prepare(
                &set("G", vec![st.clone(), zip.clone()]),
                MediaPolicy::WorkingCopy,
            )
            .unwrap();
        let work = f.saves().join("G.disks");
        assert_eq!(p.images, [work.join("d1.st"), work.join("d2.msa")]);
        assert_eq!(fs::read_to_string(&p.content).unwrap(), "d1.st\nd2.msa\n");
        assert_eq!(sha(&[st, zip]), before);
    }

    #[test]
    fn copy_bound_is_max_copy_bytes() {
        let f = Fixture::new();
        let hdf = f.file("Big.hdf", &[0u8; 64]);
        let exts = strs(&["hdf"]);
        let r = MediaRules {
            max_copy_bytes: 63,
            ..MediaRules::new(MediaPolicy::WorkingCopy)
        };
        let err = prepare_media(&set("Big", vec![hdf.clone()]), &r, &exts, &f.saves()).unwrap_err();
        assert!(matches!(err, RuntimeError::InvalidMediaSet(_)), "{err}");
        assert!(f.written().iter().all(|w| !w.ends_with(".hdf")));
        let r = MediaRules {
            max_copy_bytes: 64,
            ..r
        };
        let p = prepare_media(&set("Big", vec![hdf]), &r, &exts, &f.saves()).unwrap();
        assert_eq!(fs::read(&p.content).unwrap().len(), 64);
        const { assert!(MAX_COPY_BYTES > Limits::DEFAULT.max_size) };
    }

    #[test]
    fn user_playlist_keeps_only_white_listed_lines() {
        let f = Fixture::new();
        f.file("g/a.adf", b"a");
        f.file("g/b.adf", b"b");
        let ipf = f.file("g/c.ipf", b"c");
        let m3u = f.file(
            "g/Game.m3u",
            b"#EXTM3U\n# comment\na.adf|Disk A\n#SAVEDISK:Save\n#COMMAND:RUN\"DISC\n#LABEL:Side B\nb.adf\nc.ipf|Prog\n#EXTINF:x\n",
        );
        let before = sha(std::slice::from_ref(&m3u));
        let exts = strs(&["adf", "ipf", "m3u"]);
        let r = rules(MediaPolicy::WorkingCopy, &["ipf"]);
        let p = prepare_media(&set("Game", vec![m3u.clone()]), &r, &exts, &f.saves()).unwrap();
        let work = f.saves().join("Game.disks");
        assert_eq!(p.content, work.join("Game.m3u"));
        assert_eq!(
            fs::read_to_string(&p.content).unwrap(),
            format!(
                "a.adf|Disk A\n#SAVEDISK:Save\n#LABEL:Side B\nb.adf\n{}|Prog\n",
                ipf.display()
            )
        );
        assert_eq!(p.images, [work.join("a.adf"), work.join("b.adf"), ipf]);
        assert_eq!(sha(&[m3u]), before);

        // One entry plus `#SAVEDISK:`: the playlist is still generated (the core must see the line).
        let one = f.file("h/One.m3u", b"#SAVEDISK:\nx.adf\n");
        f.file("h/x.adf", b"x");
        let p = prepare_media(&set("One", vec![one]), &r, &exts, &f.saves()).unwrap();
        assert_eq!(
            fs::read_to_string(&p.content).unwrap(),
            "#SAVEDISK:\nx.adf\n"
        );
        // One entry, nothing to keep: the copy itself (2.0).
        let plain = f.file("i/Plain.m3u", b"#EXTM3U\ny.adf|Label\n");
        f.file("i/y.adf", b"y");
        let p = prepare_media(&set("Plain", vec![plain]), &r, &exts, &f.saves()).unwrap();
        assert_eq!(p.content, f.saves().join("Plain.disks/y.adf"));

        // Hostile kept lines (too long, control characters) are dropped, never copied.
        let long = format!(
            "#SAVEDISK:{}\nz.adf|{}\nz2.adf|ok\x07\n",
            "x".repeat(300),
            "l".repeat(300)
        );
        f.file("j/z.adf", b"z");
        f.file("j/z2.adf", b"w");
        let hostile = f.file("j/H.m3u", long.as_bytes());
        let p = prepare_media(&set("H", vec![hostile]), &r, &exts, &f.saves()).unwrap();
        assert_eq!(fs::read_to_string(&p.content).unwrap(), "z.adf\nz2.adf\n");
    }

    #[test]
    fn zip_slip_links_and_nested_archives_never_escape() {
        use std::io::Write as _;
        let f = Fixture::new();
        let path = f.roms().join("Evil.zip");
        let mut w = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        w.add_symlink("link.st", "/etc/passwd", opts).unwrap();
        w.start_file("../../../escape.st", opts).unwrap();
        w.write_all(b"e").unwrap();
        w.start_file("nested.zip", opts).unwrap();
        w.write_all(b"z").unwrap();
        w.start_file("d.7z", opts).unwrap();
        w.write_all(b"7").unwrap();
        w.finish().unwrap();
        let before = sha(std::slice::from_ref(&path));
        let exts = strs(&["st", "zip", "7z"]);
        let p = prepare_media(
            &set("Evil", vec![path.clone()]),
            &MediaRules::new(MediaPolicy::WorkingCopy),
            &exts,
            &f.saves(),
        )
        .unwrap();
        let work = f.saves().join("Evil.disks");
        assert_eq!(p.images, [work.join("escape.st")]);
        assert_eq!(fs::read(&p.images[0]).unwrap(), b"e");
        assert!(!f.tmp.path().join("escape.st").exists());
        assert!(!work.join("link.st").exists());
        assert_eq!(sha(&[path]), before);
    }

    #[test]
    fn set_total_is_bounded_by_max_copy_bytes() {
        let f = Fixture::new();
        let rules = MediaRules {
            max_copy_bytes: 150,
            ..MediaRules::new(MediaPolicy::WorkingCopy)
        };
        let zip = f.file(
            "Two.zip",
            &build_zip(&[(b"a.st", &[1u8; 100], None), (b"b.st", &[2u8; 100], None)]),
        );
        let err = prepare_media(&set("Two", vec![zip]), &rules, &exts(), &f.saves()).unwrap_err();
        assert!(matches!(err, RuntimeError::InvalidMediaSet(_)), "{err}");
        let a = f.file("A.st", &[1u8; 100]);
        let b = f.file("B.st", &[2u8; 100]);
        let err = prepare_media(
            &set("Plain", vec![a.clone(), b]),
            &rules,
            &exts(),
            &f.saves(),
        )
        .unwrap_err();
        assert!(matches!(err, RuntimeError::InvalidMediaSet(_)), "{err}");
        // Within the total: accepted.
        prepare_media(&set("One", vec![a]), &rules, &exts(), &f.saves()).unwrap();
    }

    /// Invariant: a zip the inspection refuses is still handed over whole when the core reads zips (2.0).
    #[test]
    fn uninspectable_zip_is_passed_whole_when_the_core_reads_zips() {
        let f = Fixture::new();
        let bad = f.file("Broken.zip", b"PK-not-really-a-zip");
        let with_zip = strs(&["st", "zip"]);
        let p = prepare_media(
            &set("Broken", vec![bad.clone()]),
            &MediaRules::new(MediaPolicy::WorkingCopy),
            &with_zip,
            &f.saves(),
        )
        .unwrap();
        assert_eq!(p.content, f.saves().join("Broken.disks/Broken.zip"));
        assert_eq!(fs::read(&p.content).unwrap(), fs::read(&bad).unwrap());
        // Without `zip`, the same archive is refused.
        assert!(
            prepare_media(
                &set("Broken2", vec![bad]),
                &MediaRules::new(MediaPolicy::WorkingCopy),
                &exts(),
                &f.saves(),
            )
            .is_err()
        );
    }

    #[test]
    fn reset_does_not_reach_the_saves_of_a_longer_game_name() {
        let f = Fixture::new();
        let dir = f.saves();
        for name in ["X.save1.adf", "X.saveY.save1.adf", "X.save.save1.adf"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        let removed = reset_core_game_files(&dir, "X", &strs(&["{game}.save*.adf"])).unwrap();
        assert_eq!(removed, [dir.join("X.save1.adf")]);
        let removed = reset_core_game_files(&dir, "X.saveY", &strs(&["{game}.save*.adf"])).unwrap();
        assert_eq!(removed, [dir.join("X.saveY.save1.adf")]);
    }

    #[test]
    fn reset_core_game_files_follows_the_patterns() {
        let f = Fixture::new();
        let dir = f.saves();
        for name in [
            "Game.save0.adf",
            "Game.save12.adf",
            "Game.srm",
            "Game2.save0.adf",
            "Other.save0.adf",
            "Game.save0.adf.bak",
        ] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        fs::create_dir_all(dir.join("Game.save9.adf")).unwrap();
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/Game.save1.adf"), b"x").unwrap();
        let outside = f.file("Game.save3.adf", b"user");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, dir.join("Game.save5.adf")).unwrap();
        let patterns = strs(&["{game}.save*.adf"]);
        let removed = reset_core_game_files(&dir, "Game", &patterns).unwrap();
        assert_eq!(
            removed,
            [dir.join("Game.save0.adf"), dir.join("Game.save12.adf")]
        );
        for kept in [
            "Game.srm",
            "Game2.save0.adf",
            "Other.save0.adf",
            "Game.save0.adf.bak",
            "sub/Game.save1.adf",
        ] {
            assert!(dir.join(kept).exists(), "{kept}");
        }
        assert!(dir.join("Game.save9.adf").is_dir());
        assert_eq!(fs::read(&outside).unwrap(), b"user");
        assert!(
            reset_core_game_files(&dir, "Game", &patterns)
                .unwrap()
                .is_empty()
        );
        // `{game}` without `*`; a `*` in the game name is literal.
        fs::write(dir.join("St*r.brm"), b"x").unwrap();
        fs::write(dir.join("Star.brm"), b"x").unwrap();
        let removed = reset_core_game_files(&dir, "St*r", &strs(&["{game}.brm"])).unwrap();
        assert_eq!(removed, [dir.join("St*r.brm")]);
        assert!(dir.join("Star.brm").exists());
        // Refused patterns: nothing deleted.
        for bad in [
            "../{game}.adf",
            "sub/{game}*",
            "{game}\\x",
            "*.*{game}",
            "*.adf",
            "",
            "C:{game}",
        ] {
            let err = reset_core_game_files(&dir, "Game", &strs(&["{game}.srm", bad])).unwrap_err();
            assert!(matches!(err, RuntimeError::InvalidMediaSet(_)), "{bad}");
        }
        for bad_name in ["", "..", "a/b", "a\\b"] {
            assert!(
                reset_core_game_files(&dir, bad_name, &patterns).is_err(),
                "{bad_name}"
            );
        }
        assert!(dir.join("Game.srm").exists());
        // Missing folder: nothing to do.
        assert!(
            reset_core_game_files(&dir.join("none"), "Game", &patterns)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn disk_full_is_reported_as_such() {
        let full = RuntimeError::Io {
            path: PathBuf::from("/x/a.adf"),
            source: io::Error::from(io::ErrorKind::StorageFull),
        };
        assert!(
            matches!(disk_full(full), RuntimeError::DiskFull { path } if path == Path::new("/x/a.adf"))
        );
        let other = RuntimeError::Io {
            path: PathBuf::from("/x"),
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        };
        assert!(matches!(disk_full(other), RuntimeError::Io { .. }));
    }

    #[test]
    fn zip_entry_lying_about_its_size_is_refused() {
        let f = Fixture::new();
        let zip = f.file("Big.zip", &build_zip(&[(b"big.st", &[7u8; 100], Some(10))]));
        let limits = Limits {
            max_size: 50,
            ..Limits::DEFAULT
        };
        let wc = MediaRules::new(MediaPolicy::WorkingCopy);
        let err = prepare_media_with(&set("Big", vec![zip]), &wc, &exts(), &f.saves(), limits)
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
            &wc,
            &exts(),
            &f.saves(),
            limits,
        )
        .unwrap_err();
        assert!(matches!(err, RuntimeError::ContentTooLarge { .. }), "{err}");
        // Whole files are bounded by `max_copy_bytes` (2 GiB by default, for `.hdf`), not by the zip limits.
        let plain = f.file("Plain.st", &[1u8; 100]);
        let small = MediaRules {
            max_copy_bytes: 99,
            ..MediaRules::new(MediaPolicy::WorkingCopy)
        };
        let err = prepare_media_with(
            &set("P", vec![plain.clone()]),
            &small,
            &exts(),
            &f.saves(),
            limits,
        )
        .unwrap_err();
        assert!(matches!(err, RuntimeError::InvalidMediaSet(_)), "{err}");
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
    fn remembered_added_disks_are_trimmed_so_the_manifest_stays_readable() {
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
        assert_eq!(added.len(), MAX_ADDED_REMEMBERED);
        assert_eq!(added.last().unwrap().copy, "add099.st");
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
        let plan = Plan::new(
            &[two],
            &exts(),
            Limits::DEFAULT,
            MAX_MEDIA_IMAGES,
            MAX_COPY_BYTES,
        )
        .unwrap();
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
            stage_added_copy(&work, src, &exts(), Limits::DEFAULT, MAX_COPY_BYTES, known).unwrap()
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
