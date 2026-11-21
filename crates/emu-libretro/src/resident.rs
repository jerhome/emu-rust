//! Cores whose library stays mapped after unloading (ADR 0022).
//!
//! Some libraries cannot be unmapped by the dynamic loader (dosbox_pure: `libstdc++`, which is `NODELETE`,
//! binds to template instantiations it exports). `dlopen` of the same path then returns the old image, whose
//! static state survived `retro_deinit`, and the core crashes. After each unload `Drop for Core` probes the
//! loader; a resident library is recorded here, and the next `Core::load` of that library loads a private copy
//! written under a unique name instead (a real copy: loaders recognise an already loaded image by inode).

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use libloading::Library;

/// Original (canonical) library paths seen resident after an unload in this process. Keyed by the original
/// path even when the resident image was itself a copy, so a copy counts as the same core.
static RESIDENT: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
/// Per-process copy counter (`<stem>-<pid>-<n>.<ext>`).
static COPY_COUNTER: AtomicU64 = AtomicU64::new(0);
/// Copies written by this process. A file carrying our PID that is not listed here belongs to another
/// instance (two containers sharing a volume can both be PID 1) and is only purged once old.
static OWN_COPIES: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// Copies from another process younger than this are left alone by the purge: that process may be between
/// writing its copy and loading it.
const PURGE_MIN_AGE: Duration = Duration::from_secs(60);
/// `create_new` collisions tolerated (stale copy left by an earlier process with the same PID).
const MAX_NAME_ATTEMPTS: u32 = 16;

/// Registry key for `path`: canonical when possible, so two spellings of one file share an entry.
pub(crate) fn origin_key(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn is_resident(origin: &Path) -> bool {
    RESIDENT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|set| set.contains(origin))
}

/// Records `origin` as resident; `true` the first time (callers log once per core).
pub(crate) fn mark_resident(origin: &Path) -> bool {
    RESIDENT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashSet::new)
        .insert(origin.to_path_buf())
}

#[cfg(target_os = "linux")]
const RTLD_NOLOAD: std::ffi::c_int = 0x4;
#[cfg(target_os = "macos")]
const RTLD_NOLOAD: std::ffi::c_int = 0x10;

/// Whether the library loaded from `path` is still mapped. Must only be called after it was closed.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn still_loaded(path: &Path) -> bool {
    use libloading::os::unix::{Library as UnixLibrary, RTLD_LAZY};
    // SAFETY: with RTLD_NOLOAD, dlopen never maps a file nor runs initialisers: it only returns a new
    // reference to an image that is already loaded (and therefore already initialised), or fails. The
    // reference is dropped right away; closing it cannot unload the image while the loader keeps it, and if
    // it was the last reference the image is unloaded exactly as our own close would have done.
    match unsafe { UnixLibrary::open(Some(path), RTLD_LAZY | RTLD_NOLOAD) } {
        Ok(handle) => {
            let _ = handle.close();
            true
        }
        Err(_) => false,
    }
}

#[cfg(windows)]
pub(crate) fn still_loaded(path: &Path) -> bool {
    use libloading::os::windows::Library as WinLibrary;
    // `GetModuleHandleExW` takes a reference without loading anything; it is released at once.
    match WinLibrary::open_already_loaded(path) {
        Ok(handle) => {
            let _ = handle.close();
            true
        }
        Err(_) => false,
    }
}

/// Other platforms: no probe, the library is assumed to unload (ADR 0022: a `cores.toml` override would be the
/// escape hatch).
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) fn still_loaded(_path: &Path) -> bool {
    false
}

/// `<stem>-<pid>-<n>.<ext>` (no extension if the original has none).
fn copy_file_name(original: &Path, pid: u32, n: u64) -> OsString {
    let mut name = original
        .file_stem()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("core"));
    name.push(format!("-{pid}-{n}"));
    if let Some(ext) = original.extension() {
        name.push(".");
        name.push(ext);
    }
    name
}

/// PID embedded in a name built by `copy_file_name`; `None` for anything else.
fn copy_pid(name: &str) -> Option<u32> {
    let base = name.rsplit_once('.').map_or(name, |(base, _)| base);
    let (rest, n) = base.rsplit_once('-')?;
    let (stem, pid) = rest.rsplit_once('-')?;
    if stem.is_empty() || n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    pid.parse().ok()
}

/// Best-effort removal of copies nobody uses any more: ours, or another process's once older than
/// `PURGE_MIN_AGE`. Files still mapped refuse deletion on Windows (ignored); on Unix deleting a mapped copy is
/// harmless. Files not named like a copy are never touched.
fn purge_stale_copies(dir: &Path, pid: u32, own: &mut HashSet<PathBuf>, now: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(owner) = name.to_str().and_then(copy_pid) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age >= PURGE_MIN_AGE);
        let path = entry.path();
        if (owner == pid && own.contains(&path)) || old {
            match fs::remove_file(&path) {
                Ok(()) => {
                    own.remove(&path);
                    tracing::debug!(path = %path.display(), "removed stale core copy");
                }
                Err(err) => tracing::debug!(path = %path.display(), %err, "stale core copy kept"),
            }
        }
    }
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    // A symlink here would redirect `create_dir_all` and `set_permissions` elsewhere.
    match fs::symlink_metadata(dir) {
        Ok(meta) if !meta.file_type().is_dir() => {
            return Err(io::Error::other(format!(
                "{} is not a real directory (symlink or file)",
                dir.display()
            )));
        }
        Ok(_) | Err(_) => {}
    }
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        builder.mode(0o700);
        builder.create(dir)?;
        // `recursive` keeps an existing directory's mode.
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    builder.create(dir)
}

fn create_new_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    // `create_new` = O_EXCL: never overwrites and never follows a symlink planted at `path`.
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o700);
    }
    options.open(path)
}

/// Writes a byte copy of `original` into `dir` under a fresh name; the file is closed on return.
pub(crate) fn write_private_copy(original: &Path, dir: &Path) -> io::Result<PathBuf> {
    create_private_dir(dir)?;
    let pid = std::process::id();
    {
        let mut guard = OWN_COPIES.lock().unwrap_or_else(PoisonError::into_inner);
        purge_stale_copies(
            dir,
            pid,
            guard.get_or_insert_with(HashSet::new),
            SystemTime::now(),
        );
    }
    let mut source = File::open(original)?;
    for _ in 0..MAX_NAME_ATTEMPTS {
        let n = COPY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(copy_file_name(original, pid, n));
        let mut file = match create_new_file(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        };
        let written = io::copy(&mut source, &mut file).and_then(|_| file.sync_all());
        drop(file);
        if let Err(err) = written {
            let _ = fs::remove_file(&path);
            return Err(err);
        }
        OWN_COPIES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(HashSet::new)
            .insert(path.clone());
        return Ok(path);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name for the private core copy",
    ))
}

/// A private copy on disk, deleted (best effort) when dropped unless `keep` is called.
#[derive(Debug)]
pub(crate) struct CopyFile(Option<PathBuf>);

impl CopyFile {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    /// Leaves the file on disk (still mapped on Windows; a later `write_private_copy` purges it).
    pub(crate) fn keep(mut self) {
        self.0 = None;
    }
}

impl Drop for CopyFile {
    fn drop(&mut self) {
        if let Some(path) = self.0.take()
            && let Err(err) = fs::remove_file(&path)
        {
            tracing::debug!(path = %path.display(), %err, "cannot remove private core copy");
        }
    }
}

/// Loads `copy`. Errors are reported as `io::Error` for `LibretroError::CoreCopy`.
pub(crate) fn load_copy(copy: &Path) -> io::Result<Library> {
    // SAFETY: same contract as loading the original library (`Core::load`): the copy holds the same bytes as
    // the user-chosen core, written by us just above in a 0700 directory, and nothing from it has run yet.
    unsafe { Library::new(copy) }.map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "emu-libretro-resident-{tag}-{}-{}",
            std::process::id(),
            COPY_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn registry_records_each_origin_once() {
        let a = PathBuf::from("/nonexistent/emu-libretro-test/a_libretro.so");
        let b = PathBuf::from("/nonexistent/emu-libretro-test/b_libretro.so");
        assert!(!is_resident(&a));
        assert!(mark_resident(&a));
        assert!(!mark_resident(&a), "second insertion is not new");
        assert!(is_resident(&a));
        assert!(!is_resident(&b));
    }

    #[test]
    fn origin_key_canonicalises_existing_paths() {
        let dir = temp_dir("key");
        let file = dir.join("x_libretro.so");
        fs::write(&file, b"x").unwrap();
        let spelled = dir.join(".").join("x_libretro.so");
        assert_eq!(origin_key(&spelled), origin_key(&file));
        let missing = dir.join("missing.so");
        assert_eq!(origin_key(&missing), missing);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copy_names_follow_the_pattern_and_parse_back() {
        let name = copy_file_name(Path::new("/c/dosbox_pure_libretro.so"), 42, 7);
        assert_eq!(name, "dosbox_pure_libretro-42-7.so");
        assert_eq!(copy_pid("dosbox_pure_libretro-42-7.so"), Some(42));
        let dll = copy_file_name(Path::new("C:/c/my-core.dll"), 1, 0);
        assert_eq!(dll, "my-core-1-0.dll");
        assert_eq!(copy_pid("my-core-1-0.dll"), Some(1));
        assert_eq!(copy_file_name(Path::new("core"), 3, 4), "core-3-4");
        assert_eq!(copy_pid("core-3-4"), Some(3));
        for other in [
            "readme.txt",
            "a-b.so",
            "-1-2.so",
            "x-1-.so",
            "x--2.so",
            "x-a-2.so",
        ] {
            assert_eq!(copy_pid(other), None, "{other}");
        }
    }

    #[test]
    fn purge_spares_young_foreign_copies_with_our_pid() {
        let dir = temp_dir("purge");
        let pid = std::process::id();
        let foreign = dir.join(format!("x-{pid}-900.so"));
        let ours = dir.join(format!("x-{pid}-901.so"));
        fs::write(&foreign, b"f").unwrap();
        fs::write(&ours, b"o").unwrap();
        let mut own = HashSet::from([ours.clone()]);
        purge_stale_copies(&dir, pid, &mut own, SystemTime::now());
        assert!(foreign.exists(), "same PID, not ours, young");
        assert!(!ours.exists());
        assert!(own.is_empty());
        let later = SystemTime::now() + PURGE_MIN_AGE + Duration::from_secs(5);
        purge_stale_copies(&dir, pid, &mut own, later);
        assert!(!foreign.exists(), "old enough");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_copy_dir_is_refused() {
        let dir = temp_dir("symlink");
        let target = dir.join("target");
        fs::create_dir(&target).unwrap();
        let link = dir.join("core-copies");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let original = dir.join("o.so");
        fs::write(&original, b"x").unwrap();
        assert!(write_private_copy(&original, &link).is_err());
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn private_copy_is_a_fresh_identical_file() {
        let dir = temp_dir("copy");
        let original = dir.join("orig_libretro.so");
        fs::write(&original, b"\x7fELF fake library bytes").unwrap();
        let copies = dir.join("copies").join("nested");
        let first = write_private_copy(&original, &copies).unwrap();
        assert_eq!(fs::read(&first).unwrap(), fs::read(&original).unwrap());
        let second = write_private_copy(&original, &copies).unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&second).unwrap(), fs::read(&original).unwrap());
        let name = first.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("orig_libretro-"), "{name}");
        assert!(name.ends_with(".so"), "{name}");
        assert_eq!(copy_pid(name), Some(std::process::id()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&copies).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        // Our own earlier copy was purged when the second one was written.
        assert!(!first.exists());
        assert!(second.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copy_of_a_missing_library_fails() {
        let dir = temp_dir("missing");
        let err = write_private_copy(&dir.join("absent.so"), &dir.join("copies")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn purge_spares_young_foreign_copies_and_unrelated_files() {
        let dir = temp_dir("purge");
        let pid = std::process::id();
        let foreign_pid = pid.wrapping_add(1);
        let ours = dir.join(format!("c_libretro-{pid}-1.so"));
        let foreign = dir.join(format!("c_libretro-{foreign_pid}-1.so"));
        let unrelated = dir.join("notes.txt");
        for f in [&ours, &foreign, &unrelated] {
            fs::write(f, b"x").unwrap();
        }
        let now = SystemTime::now();
        let mut own = HashSet::from([ours.clone()]);
        purge_stale_copies(&dir, pid, &mut own, now);
        assert!(!ours.exists());
        assert!(foreign.exists(), "young copy of another process is kept");
        assert!(unrelated.exists());
        purge_stale_copies(
            &dir,
            pid,
            &mut own,
            now + PURGE_MIN_AGE + Duration::from_secs(1),
        );
        assert!(!foreign.exists(), "old copy of another process is purged");
        assert!(unrelated.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copy_file_guard_removes_unless_kept() {
        let dir = temp_dir("guard");
        let removed = dir.join("r");
        let kept = dir.join("k");
        fs::write(&removed, b"x").unwrap();
        fs::write(&kept, b"x").unwrap();
        drop(CopyFile::new(removed.clone()));
        CopyFile::new(kept.clone()).keep();
        assert!(!removed.exists());
        assert!(kept.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn probe_misses_a_library_that_was_never_loaded() {
        let dir = temp_dir("probe");
        let fake = dir.join("never_loaded_libretro.so");
        fs::write(&fake, b"not a library").unwrap();
        assert!(!still_loaded(&fake));
        assert!(!still_loaded(&dir.join("absent.so")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn probe_sees_a_library_that_is_loaded() {
        // libc is mapped in every process; RTLD_NOLOAD finds it by soname.
        assert!(still_loaded(Path::new("libc.so.6")));
    }
}
