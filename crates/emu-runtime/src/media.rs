//! Removable media of computer systems (ADR 0015, `docs/design/v2.0-atari-st.md` §3.2): multi-disk sets,
//! working copies, generated `.m3u`, disk swapping during a session.
//!
//! Safety rules (reviewed): the user's files are only ever opened for reading; every write goes to
//! `<saves>/<core_id>/<game>.disks/` (`WorkingCopy`) or to a private temporary directory (`Direct`), through
//! `write_atomic_with`. Zip entries are untrusted: names reduced to a sanitized base name, sizes bounded by the
//! `content.rs` limits.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use emu_libretro::{DiskControl, DiskControlKind};
use tempfile::TempDir;

use crate::content::{self, Limits, io_error};
use crate::emulator::Emulator;
use crate::error::RuntimeError;
use crate::fsname::sanitize_component;
use crate::saves::{FileMode, game_name_from_str, read_bounded, write_atomic_with};

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
    /// `Some(<saves>/<core_id>/<game>.disks)` for `WorkingCopy`.
    pub work_dir: Option<PathBuf>,
    /// `Direct` only: private directory holding extracted zip entries and the generated playlist. Removed when
    /// the last clone is dropped, so keep this value alive as long as the core may read the files.
    pub(crate) temp_dir: Option<Arc<TempDir>>,
}

impl PartialEq for PreparedMedia {
    fn eq(&self, other: &Self) -> bool {
        self.content == other.content
            && self.images == other.images
            && self.work_dir == other.work_dir
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
    saves_core_dir.join(format!("{}.disks", game_name_from_str(game)))
}

/// Copies / extracts / writes the playlist as the policy requires. Never modifies the files of `set`.
///
/// `saves_core_dir` = `<saves>/<core_id>`. Working copies are created only when absent (an existing copy holds
/// the player's progress and is reused); every write is atomic (`write_atomic`). Zip entries are read with the
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
            temp_dir: None,
        });
    }

    let sources = match &playlist {
        Some(playlist) => parse_playlist(playlist)?,
        None => set.images.clone(),
    };
    let mut stage = Stage::new(policy, exts, limits, work_dir_for(saves_core_dir, &game))?;
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
        work_dir: (policy == MediaPolicy::WorkingCopy).then_some(stage.work_dir),
        temp_dir: stage.temp_dir,
    })
}

fn too_many() -> RuntimeError {
    RuntimeError::InvalidMediaSet(format!("more than {MAX_MEDIA_IMAGES} disk images"))
}

fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn accepts_ext(exts: &[String], ext: &str) -> bool {
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
    fn dest_path(&mut self, name: &str) -> Result<PathBuf, RuntimeError> {
        let dir = self.dest_dir()?;
        let mut candidate = name.to_owned();
        let mut n = self.images.len() + 1;
        while !self.used.insert(candidate.to_lowercase()) {
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
            MediaPolicy::WorkingCopy => {
                let name =
                    disk_file_name(&source.file_name().unwrap_or_default().to_string_lossy());
                let dest = self.dest_path(&name)?;
                if !exists(&dest)? {
                    copy_file(source, &dest, self.limits.max_size)?;
                    tracing::info!("working copy of {} created", source.display());
                }
                self.push(dest)
            }
        }
    }

    /// Extracts up to `max` accepted entries (archive order). A playlist inside the archive is ignored.
    fn add_zip(&mut self, zip: &Path, max: usize) -> Result<(), RuntimeError> {
        let mut archive = content::open_archive(zip, self.limits)?;
        let exts = self.exts;
        let room = MAX_MEDIA_IMAGES.saturating_sub(self.images.len()).min(max);
        let entries = content::accepted_entries(
            &mut archive,
            zip,
            |name| match name.rsplit_once('.') {
                Some((_, ext)) => !ext.eq_ignore_ascii_case("m3u") && accepts_ext(exts, ext),
                None => false,
            },
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
            let dest = self.dest_path(&disk_file_name(&entry.name))?;
            if self.policy == MediaPolicy::Direct || !exists(&dest)? {
                let mut failure = None;
                let written = write_atomic_with(&dest, FileMode::Default, |file| {
                    content::copy_entry(&mut archive, zip, entry, self.limits.max_size, file, &dest)
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
            self.push(dest)?;
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
/// on a relative path, are refused. Backslashes are separators on every platform (playlists made on Windows).
pub(crate) fn parse_playlist(playlist: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
    let bytes = read_bounded(playlist, MAX_PLAYLIST_BYTES).map_err(|e| io_error(playlist, e))?;
    if bytes.len() > MAX_PLAYLIST_BYTES {
        return Err(RuntimeError::InvalidMediaSet(format!(
            "{} is larger than {MAX_PLAYLIST_BYTES} bytes",
            playlist.display()
        )));
    }
    let base = playlist.parent().unwrap_or(Path::new(""));
    let text = String::from_utf8_lossy(&bytes);
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

/// Deletes the working copies of `game` (menu « Réinitialiser les disquettes »); the core must not be running
/// that game. Returns `false` when there was nothing to delete.
pub fn reset_working_copies(saves_core_dir: &Path, game: &str) -> Result<bool, RuntimeError> {
    let dir = work_dir_for(saves_core_dir, game);
    let meta = match fs::symlink_metadata(&dir) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(io_error(&dir, e)),
    };
    // A symlink in place of the folder: remove the link, never what it points to.
    let removed = if meta.is_dir() {
        fs::remove_dir_all(&dir)
    } else {
        fs::remove_file(&dir)
    };
    removed.map_err(|e| io_error(&dir, e))?;
    tracing::info!("working copies removed: {}", dir.display());
    Ok(true)
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
        let num = control.num_images();
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
        let num = self.control()?.num_images();
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
    /// `<saves>/<core_id>/<game>.disks/` (an existing copy of the same name is reused); a zip the core cannot read
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
        let policy = self.media_policy.unwrap_or(MediaPolicy::WorkingCopy);
        let work_dir = self
            .prepared
            .as_ref()
            .and_then(|p| p.work_dir.clone())
            .unwrap_or_else(|| work_dir_for(&self.save_root.join(&self.core_id), &self.game_name));
        let exts = self.core.system_info().valid_extensions.clone();
        let mut stage = Stage::new(policy, &exts, self.limits, work_dir)?;
        stage.temp_dir = self.prepared.as_ref().and_then(|p| p.temp_dir.clone());
        // Names already handed to the core keep theirs; a new name never overwrites them in the temp directory.
        if policy == MediaPolicy::Direct {
            for known in self.prepared.iter().flat_map(|p| &p.images) {
                if let Some(name) = known.file_name() {
                    stage.used.insert(name.to_string_lossy().to_lowercase());
                }
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
                        out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
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
        assert_eq!(f.written(), ["saves/hatari/Game.disks/Game.st"]);
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
        assert_eq!(f.written(), ["saves/hatari/H.disks/H.st"]);
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
    fn disk_file_names() {
        assert_eq!(disk_file_name("#1.st"), "_1.st");
        assert_eq!(disk_file_name(".."), "disk");
        assert_eq!(disk_file_name("a/b.st"), "a_b.st");
    }
}
