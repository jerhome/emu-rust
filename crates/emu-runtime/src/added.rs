//! Disks added from the disk menu (2.1, `docs/design/v2.1-identite-disques.md` §2.3): restored at each session,
//! removable, and the working-copy folder's unreferenced files (orphans) listed for an explicit clean-up. Never
//! deletes anything without an explicit call; only stale `.tmp-*` files older than an hour go automatically.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::content::io_error;
use crate::emulator::Emulator;
use crate::error::RuntimeError;
use crate::media::{Disks, MediaPolicy, has_ext, is_disk_file_name, list_full};
use crate::saves::read_bounded;
use crate::workcopy::{self, DirState, MANIFEST_NAME, ManifestEntry, Origin, is_regular_file};

/// One disk added from the menu (manifest entry `Origin::Added`) of the running game's working-copy folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedDisk {
    /// Working copy handed to the core.
    pub copy: PathBuf,
    /// Recorded source; `None` for an adopted orphan (« Reprendre »).
    pub source: Option<PathBuf>,
    /// The recorded source still exists (the copy is used either way: progress lives in it).
    pub source_present: bool,
    /// Size of the copy, in bytes.
    pub size: u64,
    /// Index in the core's image list when restored or added this session; `None` if not in the list (or not
    /// identifiable: basic disk interface in a `File` / `NoGame` session, which reports no paths).
    pub index: Option<u32>,
}

/// Why a file of the working-copy folder is listed by [`orphan_copies`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanKind {
    /// A disk image the manifest does not reference (rc.1 copies `02 X.st`, copies forgotten beyond 32).
    Copy,
    /// A generated `.m3u` that is not the folder's current playlist (old `<save_name>.m3u`).
    StalePlaylist,
}

/// One unreferenced file of a working-copy folder (« Nettoyer les copies… »).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanCopy {
    /// Plain file name inside the folder.
    pub name: String,
    pub kind: OrphanKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// Temporary files of `write_atomic` (`.<name>.tmp-<pid>`) older than this are leftovers of a crash.
const STALE_TEMP_AGE: Duration = Duration::from_secs(3600);

/// Working-copy folder of the session: the prepared one, else `<saves>/<core_id>/<save_name>.disks` for
/// `File` / `NoGame` sessions (same rule as `Disks::add`). `None` for a `Direct` set: no working copies.
fn session_work_dir(emu: &Emulator) -> Option<PathBuf> {
    if emu.media_policy == Some(MediaPolicy::Direct) {
        return None;
    }
    if let Some(prepared) = &emu.prepared {
        return prepared.work_dir.clone();
    }
    // `game_name` is already a sanitized single component (`GameIdentity::save_name`).
    Some(
        emu.save_root
            .join(&emu.core_id)
            .join(format!("{}.disks", emu.game_name)),
    )
}

fn manifest_entries(dir: &Path) -> Result<Option<Vec<ManifestEntry>>, RuntimeError> {
    Ok(match workcopy::inspect(dir, is_disk_file_name)? {
        DirState::Manifest(entries) => Some(entries),
        _ => None,
    })
}

fn refused(path: &Path, reason: &str) -> RuntimeError {
    RuntimeError::RemovalRefused {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    }
}

/// Re-adds the added disks of the session's folder to the core, in order of addition, not inserted (design
/// §2.3). Runs once after `retro_load_game`. Nothing without a disk interface; a failure is logged, never fatal.
pub(crate) fn restore_added(emu: &mut Emulator) {
    let Some(dir) = session_work_dir(emu) else {
        return;
    };
    let Some(mut disks) = emu.disks() else {
        return;
    };
    let entries = match manifest_entries(&dir) {
        Ok(Some(entries)) => entries,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("added disks not restored: {e}");
            return;
        }
    };
    let known: Vec<PathBuf> = disks.slots().into_iter().filter_map(|s| s.path).collect();
    for entry in entries.iter().filter(|e| e.origin == Origin::Added) {
        let copy = dir.join(&entry.copy);
        if !is_regular_file(&copy).unwrap_or(false) || known.contains(&copy) {
            continue;
        }
        match disks.append(&copy) {
            Ok(index) => tracing::info!(index, "added disk {} restored", copy.display()),
            Err(e) => tracing::warn!("added disk {} not restored: {e}", copy.display()),
        }
    }
}

impl Disks<'_> {
    /// Index of `copy` in the core's list, comparing paths as given, then canonical (a core may report the path
    /// spelled otherwise).
    fn index_of(&mut self, copy: &Path) -> Option<u32> {
        let slots = self.slots();
        if let Some(s) = slots.iter().find(|s| s.path.as_deref() == Some(copy)) {
            return Some(s.index);
        }
        let wanted = fs::canonicalize(copy).ok()?;
        slots
            .iter()
            .find(|s| {
                s.path
                    .as_deref()
                    .and_then(|p| fs::canonicalize(p).ok())
                    .is_some_and(|p| p == wanted)
            })
            .map(|s| s.index)
    }

    /// Added disks of the session's working-copy folder, in order of addition (copies still present only).
    pub fn added(&mut self) -> Vec<AddedDisk> {
        let Some(dir) = session_work_dir(self.emulator) else {
            return Vec::new();
        };
        let entries = match manifest_entries(&dir) {
            Ok(Some(entries)) => entries,
            Ok(None) => return Vec::new(),
            Err(e) => {
                tracing::warn!("added disks not listed: {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for entry in entries.iter().filter(|e| e.origin == Origin::Added) {
            let copy = dir.join(&entry.copy);
            let Ok(meta) = fs::symlink_metadata(&copy) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let source = entry.source_path();
            let source_present = source.as_deref().is_some_and(Path::is_file);
            let index = self.index_of(&copy);
            out.push(AddedDisk {
                copy,
                source,
                source_present,
                size: meta.len(),
                index,
            });
        }
        out
    }

    /// Removes `copy` from the core's list and from the manifest; with `delete_copy`, also deletes the file.
    /// `DiskInserted` if it is the inserted disk; `RemovalRefused` if `copy` is not an added disk of the folder.
    pub fn forget_added(&mut self, copy: &Path, delete_copy: bool) -> Result<(), RuntimeError> {
        let dir = session_work_dir(self.emulator)
            .ok_or_else(|| refused(copy, "the session has no working-copy folder"))?;
        let name = copy
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|_| copy.parent() == Some(dir.as_path()))
            .ok_or_else(|| refused(copy, "not in the working-copy folder"))?;
        let mut entries =
            manifest_entries(&dir)?.ok_or_else(|| refused(copy, "no readable manifest"))?;
        let pos = entries
            .iter()
            .position(|e| e.origin == Origin::Added && e.copy == name)
            .ok_or_else(|| refused(copy, "not an added disk"))?;
        // Checked before anything changes: a link is never deleted (nor what it points to).
        if delete_copy {
            match fs::symlink_metadata(copy) {
                Ok(meta) if !meta.is_file() => return Err(refused(copy, "not a regular file")),
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_error(copy, e)),
            }
        }
        let index = self.index_of(copy);
        // Not identifiable in the core's list (basic interface without paths in a `File` / `NoGame` session):
        // the inserted disk may be this copy, so its file is not deleted while a disk is in the drive.
        if index.is_none() && delete_copy && self.current().is_some() {
            return Err(RuntimeError::DiskInserted);
        }
        if let Some(index) = index {
            if self.current() == Some(index) {
                return Err(RuntimeError::DiskInserted);
            }
            self.remove_image(index)?;
            if let Some(prepared) = self.emulator.prepared.as_mut()
                && prepared.images.get(index as usize).map(PathBuf::as_path) == Some(copy)
            {
                prepared.images.remove(index as usize);
            }
        }
        entries.remove(pos);
        workcopy::write(&dir, &entries)?;
        if delete_copy {
            match fs::remove_file(copy) {
                Ok(()) => tracing::info!("added disk {} deleted", copy.display()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_error(copy, e)),
            }
        }
        tracing::info!("added disk {} forgotten", copy.display());
        Ok(())
    }

    /// `replace_image(index, None)` with the tray ejected, then the previous disk put back.
    fn remove_image(&mut self, index: u32) -> Result<(), RuntimeError> {
        let mut control = self
            .emulator
            .core
            .disk_control()
            .ok_or(RuntimeError::NoDiskControl)?;
        let was_ejected = control.is_ejected();
        let previous = control.image_index();
        if !was_ejected {
            control.set_ejected(true)?;
        }
        let removed = control.replace_image(index, None);
        // The images after `index` moved down by one.
        if removed.is_ok() && previous > index {
            let _ = control.set_image_index(previous - 1);
        }
        let reinserted = if was_ejected {
            Ok(())
        } else {
            control.set_ejected(false)
        };
        removed?;
        reinserted?;
        Ok(())
    }

    /// « Reprendre »: orphan `name` becomes an added disk (entry `Added` without source) and is added to the
    /// core's list (not inserted); returns its index.
    pub fn adopt_orphan(&mut self, name: &str) -> Result<u32, RuntimeError> {
        let dir = session_work_dir(self.emulator).ok_or_else(|| {
            RuntimeError::InvalidMediaSet("the session has no working-copy folder".into())
        })?;
        let saves_core_dir = self.emulator.save_root.join(&self.emulator.core_id);
        let orphan = scan(&saves_core_dir, &dir, false)?
            .into_iter()
            .find(|o| o.name == name && o.kind == OrphanKind::Copy)
            .ok_or_else(|| {
                RuntimeError::InvalidMediaSet(format!("`{name}` is not an orphan copy"))
            })?;
        // What the manifest reader accepts back (`workcopy::decode`).
        if !is_disk_file_name(name) || has_ext(Path::new(name), "m3u") {
            return Err(RuntimeError::InvalidMediaSet(format!(
                "`{name}` cannot be a disk copy"
            )));
        }
        let mut entries = manifest_entries(&dir)?
            .ok_or_else(|| RuntimeError::InvalidMediaSet("no readable manifest".into()))?;
        if entries.len() >= workcopy::MAX_MANIFEST_ENTRIES {
            return Err(list_full());
        }
        let index = self.append(&dir.join(name))?;
        entries.push(ManifestEntry::adopted(name.to_owned(), orphan.size));
        workcopy::write(&dir, &entries)?;
        tracing::info!(index, "orphan copy {name} adopted");
        Ok(index)
    }
}

/// Guard of `reset_work_dir`: a `*.disks` entry directly inside `saves_core_dir`.
fn check_work_dir(saves_core_dir: &Path, work_dir: &Path) -> Result<(), RuntimeError> {
    let named_disks = work_dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.len() > ".disks".len() && n.ends_with(".disks"));
    if work_dir.parent() != Some(saves_core_dir) || !named_disks {
        return Err(refused(
            work_dir,
            "not a working-copy folder of the core's save folder",
        ));
    }
    Ok(())
}

/// A generated playlist larger than this is not ours (same bound as user playlists).
const MAX_PLAYLIST_BYTES: usize = 64 * 1024;

/// Files of a folder the manifest references (the manifest itself included). Names are compared case-insensitively
/// and, on Unix, by file identity too: a file system that hands names back in another Unicode form (NFD on HFS+ or
/// SMB) must not make a referenced copy look unreferenced, hence removable.
struct Referenced {
    names: HashSet<String>,
    #[cfg(unix)]
    ids: HashSet<(u64, u64)>,
}

impl Referenced {
    fn new(dir: &Path, entries: &[ManifestEntry]) -> Self {
        let mut names: HashSet<String> = entries.iter().map(|e| e.copy.to_lowercase()).collect();
        names.insert(MANIFEST_NAME.to_lowercase());
        #[cfg(unix)]
        let ids = {
            use std::os::unix::fs::MetadataExt;
            entries
                .iter()
                .map(|e| e.copy.as_str())
                .chain([MANIFEST_NAME])
                .filter_map(|copy| fs::symlink_metadata(dir.join(copy)).ok())
                .map(|m| (m.dev(), m.ino()))
                .collect()
        };
        #[cfg(not(unix))]
        let _ = dir;
        Self {
            names,
            #[cfg(unix)]
            ids,
        }
    }

    fn contains(&self, name: &str, meta: &fs::Metadata) -> bool {
        if self.names.contains(&name.to_lowercase()) {
            return true;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            self.ids.contains(&(meta.dev(), meta.ino()))
        }
        #[cfg(not(unix))]
        {
            let _ = meta;
            false
        }
    }
}

fn is_temp(name: &str) -> bool {
    name.starts_with('.') && name.contains(".tmp-")
}

/// Unreferenced files of `work_dir`, which must be a `*.disks` folder directly under `saves_core_dir`
/// (`RemovalRefused` otherwise). The manifest, referenced copies and links are never listed.
///
/// A folder without a readable manifest (earlier versions, damaged) lists nothing: what it references cannot be
/// known. Temporary files of an interrupted write older than an hour are deleted on the way (never listed).
pub fn orphan_copies(
    saves_core_dir: &Path,
    work_dir: &Path,
) -> Result<Vec<OrphanCopy>, RuntimeError> {
    scan(saves_core_dir, work_dir, true)
}

fn scan(
    saves_core_dir: &Path,
    work_dir: &Path,
    clean_temps: bool,
) -> Result<Vec<OrphanCopy>, RuntimeError> {
    check_work_dir(saves_core_dir, work_dir)?;
    match fs::symlink_metadata(work_dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(refused(work_dir, "not a folder")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error(work_dir, e)),
    }
    let Some(entries) = manifest_entries(work_dir)? else {
        return Ok(Vec::new());
    };
    let referenced = Referenced::new(work_dir, &entries);
    let now = SystemTime::now();
    let mut out = Vec::new();
    for item in fs::read_dir(work_dir).map_err(|e| io_error(work_dir, e))? {
        let item = item.map_err(|e| io_error(work_dir, e))?;
        let path = item.path();
        // A name that is not UTF-8 cannot be selected for removal: left alone.
        let Some(name) = item.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let meta = fs::symlink_metadata(&path).map_err(|e| io_error(&path, e))?;
        if !meta.is_file() || referenced.contains(&name, &meta) {
            continue;
        }
        let modified = meta.modified().ok();
        if is_temp(&name) {
            let stale = modified
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|age| age > STALE_TEMP_AGE);
            if clean_temps && stale {
                match fs::remove_file(&path) {
                    Ok(()) => tracing::info!("stale temporary file {} removed", path.display()),
                    Err(e) => tracing::warn!("cannot remove {}: {e}", path.display()),
                }
            }
            continue;
        }
        let kind = if has_ext(Path::new(&name), "m3u") {
            OrphanKind::StalePlaylist
        } else {
            OrphanKind::Copy
        };
        out.push(OrphanCopy {
            name,
            kind,
            size: meta.len(),
            modified,
        });
    }
    if let Some(current) = current_playlist(work_dir, &entries, &out) {
        out.retain(|o| o.name != current);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// The playlist the folder's set uses now: an unreferenced `.m3u` listing exactly the manifest's set copies in
/// order (a set of one image has none). Preferably `<folder stem>.m3u`, else the newest such file: after a
/// homonym got `G (2)`, `G.m3u` and `G (2).m3u` list the same copies and the old one is stale.
fn current_playlist(dir: &Path, entries: &[ManifestEntry], files: &[OrphanCopy]) -> Option<String> {
    let set: Vec<&str> = entries
        .iter()
        .filter(|e| e.origin == Origin::Set)
        .map(|e| e.copy.as_str())
        .collect();
    if set.len() < 2 {
        return None;
    }
    let candidates: Vec<&OrphanCopy> = files
        .iter()
        .filter(|o| o.kind == OrphanKind::StalePlaylist)
        .filter(|o| {
            let Ok(bytes) = read_bounded(&dir.join(&o.name), MAX_PLAYLIST_BYTES) else {
                return false;
            };
            if bytes.len() > MAX_PLAYLIST_BYTES {
                return false;
            }
            let Ok(text) = String::from_utf8(bytes) else {
                return false;
            };
            let lines: Vec<&str> = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect();
            lines == set
        })
        .collect();
    let stem = dir
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".disks"))
        .map(|s| format!("{s}.m3u").to_lowercase());
    candidates
        .iter()
        .find(|o| Some(o.name.to_lowercase()) == stem)
        .or_else(|| candidates.iter().max_by_key(|o| o.modified))
        .map(|o| o.name.clone())
}

/// Deletes the orphans `names` of `work_dir` (checked selection of the clean-up dialog); returns the bytes freed.
/// Refuses (`RemovalRefused`, nothing deleted) any name that is not a plain file name of the folder, a referenced
/// entry, the manifest, a link, or a `work_dir` that is not a `*.disks` directly under `saves_core_dir`.
pub fn remove_orphan_copies(
    saves_core_dir: &Path,
    work_dir: &Path,
    names: &[String],
) -> Result<u64, RuntimeError> {
    check_work_dir(saves_core_dir, work_dir)?;
    let orphans = scan(saves_core_dir, work_dir, false)?;
    let entries = manifest_entries(work_dir)?.unwrap_or_default();
    let referenced = Referenced::new(work_dir, &entries);
    // Every name is checked before the first deletion.
    let mut selected = Vec::with_capacity(names.len());
    for name in names {
        let path = work_dir.join(name);
        let plain = !name.is_empty()
            && !name.contains(['/', '\\'])
            && Path::new(name).file_name().and_then(|n| n.to_str()) == Some(name.as_str());
        if !plain {
            return Err(refused(&path, "not a plain file name"));
        }
        if name.eq_ignore_ascii_case(MANIFEST_NAME) {
            return Err(refused(&path, "the manifest is never deleted"));
        }
        // By name first: a referenced name is refused even when absent (case-insensitive file systems).
        if referenced.names.contains(&name.to_lowercase()) {
            return Err(refused(&path, "referenced by the manifest"));
        }
        let meta = fs::symlink_metadata(&path).map_err(|e| io_error(&path, e))?;
        if meta.file_type().is_symlink() {
            return Err(refused(&path, "a link"));
        }
        if referenced.contains(name, &meta) {
            return Err(refused(&path, "referenced by the manifest"));
        }
        let orphan = orphans
            .iter()
            .find(|o| o.name == *name)
            .ok_or_else(|| refused(&path, "not an unreferenced copy"))?;
        selected.push((path, orphan.size));
    }
    let mut freed = 0;
    for (path, size) in selected {
        fs::remove_file(&path).map_err(|e| io_error(&path, e))?;
        tracing::info!("orphan copy {} deleted", path.display());
        freed += size;
    }
    Ok(freed)
}

#[cfg(test)]
mod tests;
