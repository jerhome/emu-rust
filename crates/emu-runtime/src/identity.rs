//! Game identity (2.1, `docs/design/v2.1-identite-disques.md` §2.1): the `save_name` that keys SRAM, states,
//! per-game options and the `.disks` folder, attributed once per core by the registry
//! `<saves>/<core_id>/.emu-rust-games`. Homonyms get `<base> (n)`; nothing existing is ever renamed.
//!
//! Writes (all under `saves_core_dir`): the registry (atomic, under the inter-process lock
//! `.emu-rust-games.lock`), the lock file itself, a `.corrupt-<unix>` copy of an unusable registry, and the
//! inherited `<save_name>.srm` of a homonym (atomic, only when absent). Nothing is ever renamed, moved or deleted.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::content::io_error;
use crate::error::RuntimeError;
use crate::media::{ContentSpec, NO_GAME_NAME, has_ext, parse_playlist};
use crate::saves::{
    FileMode, STATE_SLOTS, game_name_from_path, game_name_from_str, read_bounded, write_atomic,
    write_atomic_with,
};
use crate::workcopy::{self, DirState, Origin, escape, path_from_key, path_key, unescape};

/// Who this game is for saves: resolved before loading (the UI needs it for per-game options), then handed to
/// [`crate::Emulator::load_content_as`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameIdentity {
    /// `<base>` or `<base> (n)`: `.srm`, states, per-game options, `<save_name>.disks`.
    pub save_name: String,
    /// 1 = bare name (`save_name == base_name()`).
    pub index: u32,
    /// `GET_SAVE_DIRECTORY`: `<saves>/<core_id>/` for index 1, `<saves>/<core_id>/<save_name>/` otherwise.
    pub core_save_dir: PathBuf,
    /// Set only by the launch that attributed the name (or found the registry unusable).
    pub notice: Option<IdentityNotice>,
    /// 1.0 / 2.0 name (`game_name_from_path`, `MediaSet::name`, `no-game`).
    base: String,
    /// Fingerprint of the primary source; `None` for `NoGame` and the 2.0 fallback.
    fingerprint: Option<Fingerprint>,
    /// Attributed by this launch to a homonym whose entry 1 is `legacy`.
    inherits_legacy: bool,
    /// Several registry entries share this game's base name (`StateOrigin::Ambiguous`).
    homonyms: bool,
}

/// Size + CRC32 of the whole file (≤ 64 MiB) or of its first and last MiB. Written as `game_fp=<size>:<crc32>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    size: u64,
    crc32: u32,
}

/// One-shot banner for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityNotice {
    /// Homonym separated from the game that keeps the bare name (`first`). `sram_copied`: `<base>.srm` was copied
    /// to `<save_name>.srm`; `legacy_states`: states of the shared 2.0 name exist and stay with `first`.
    Homonym {
        first: PathBuf,
        sram_copied: bool,
        legacy_states: bool,
    },
    /// Registry unreadable or too large: 2.0 behaviour (bare name) for this session, registry left untouched.
    RegistryUnusable,
}

/// Input of [`resolve_game_identity`]. `primary = None` for `NoGame` (fixed identity, outside the registry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityRequest<'a> {
    /// `<saves>/<core_id>/`: registry, `.srm`, `.disks`.
    pub saves_core_dir: &'a Path,
    /// `<states>/<core_id>/`: checked so that `<base> (n)` never captures a foreign state file.
    pub states_core_dir: &'a Path,
    /// Owned: derived from a file stem for `ContentSpec::File`.
    pub base_name: String,
    /// User `.m3u`, else first image, else the file (the archive for a zip); a CD's `.cue` / `.chd` / `.iso`.
    pub primary: Option<&'a Path>,
}

impl GameIdentity {
    /// 2.0 behaviour: bare name, index 1, `core_save_dir = saves_core_dir`, no fingerprint, no notice. Used for
    /// `NoGame`, as the registry fallback, and by [`crate::Emulator::load_content`] when resolution fails.
    pub fn legacy(base_name: &str, saves_core_dir: &Path) -> Self {
        Self {
            save_name: base_name.to_owned(),
            index: 1,
            core_save_dir: saves_core_dir.to_path_buf(),
            notice: None,
            base: base_name.to_owned(),
            fingerprint: None,
            inherits_legacy: false,
            homonyms: false,
        }
    }

    /// The 1.0 / 2.0 game name this identity was resolved from.
    pub fn base_name(&self) -> &str {
        &self.base
    }

    /// `true` if this launch attributed `save_name` to a homonym whose entry 1 is `legacy`: the UI then copies
    /// `<base>.toml` to `<save_name>.toml` (`inherit_game_options`).
    pub fn inherits_legacy(&self) -> bool {
        self.inherits_legacy
    }

    /// Value of the `game_fp=` state header line; `None` when no fingerprint is known.
    pub(crate) fn game_fp(&self) -> Option<String> {
        self.fingerprint
            .map(|fp| format!("{}:{:08x}", fp.size, fp.crc32))
    }

    /// Several games of the registry share the base name: a state without `game_fp` may be another one's.
    pub(crate) fn has_homonyms(&self) -> bool {
        self.homonyms
    }
}

/// Request for `spec` (base name and primary source, §2.1). Filters `Media { set, .. }` only (C1).
pub fn identity_request_for<'a>(
    spec: &'a ContentSpec,
    saves_core_dir: &'a Path,
    states_core_dir: &'a Path,
) -> IdentityRequest<'a> {
    let (base_name, primary) = match spec {
        ContentSpec::File(rom) => (game_name_from_path(rom), Some(rom.as_path())),
        ContentSpec::Media { set, .. } => (
            game_name_from_str(&set.name),
            set.playlist
                .as_deref()
                .or(set.images.first().map(PathBuf::as_path)),
        ),
        ContentSpec::NoGame => (NO_GAME_NAME.to_owned(), None),
    };
    IdentityRequest {
        saves_core_dir,
        states_core_dir,
        base_name,
        primary,
    }
}

/// Registry file in `<saves>/<core_id>/`.
pub(crate) const REGISTRY_NAME: &str = ".emu-rust-games";
const LOCK_NAME: &str = ".emu-rust-games.lock";
const HEADER: &[u8] = b"emu-rust games 1";
/// A larger registry was not written by us: never parsed, never rewritten.
const MAX_REGISTRY_BYTES: usize = 8 * 1024 * 1024;
/// Files up to this size are fingerprinted whole; larger ones by their first and last [`FP_EDGE`] bytes.
const FP_WHOLE_MAX: u64 = 64 * 1024 * 1024;
const FP_EDGE: u64 = 1024 * 1024;
/// `<base> (n)`: most suffixes tried before giving up (the caller then falls back to the 2.0 name).
const MAX_SUFFIX: u32 = 9999;
/// Same bound as the working-copy folders of `media.rs` (`<base> (n).disks`, rule 3).
const MAX_WORK_DIRS: u32 = 64;

/// Resolves (or attributes) the identity of a game in the registry of `req.saves_core_dir`. Idempotent. Writes
/// only the registry, its lock and inherited `.srm` copies (atomic, missing target only). An unusable registry
/// is not an error: 2.0 identity with [`IdentityNotice::RegistryUnusable`].
pub fn resolve_game_identity(req: &IdentityRequest<'_>) -> Result<GameIdentity, RuntimeError> {
    // Already sanitized by `identity_request_for`; done again for hand-built requests (it is idempotent).
    let base = game_name_from_str(&req.base_name);
    let saves = req.saves_core_dir;
    let Some(primary) = req.primary else {
        return Ok(GameIdentity::legacy(&base, saves));
    };
    let mut source = Primary::open(primary)?;
    fs::create_dir_all(saves).map_err(|e| io_error(saves, e))?;
    let _lock = RegistryLock::acquire(saves)?;
    let registry_path = saves.join(REGISTRY_NAME);
    let mut entries = match read_registry(&registry_path)? {
        Registry::Entries(entries) => entries,
        Registry::Unusable(reason) => {
            tracing::warn!(
                "{}: {reason}; game identity falls back to the 2.0 name for this session",
                registry_path.display()
            );
            keep_corrupt_copy(&registry_path);
            let mut identity = GameIdentity::legacy(&base, saves);
            identity.notice = Some(IdentityNotice::RegistryUnusable);
            return Ok(identity);
        }
    };
    let before = entries.clone();
    let mut notice = None;
    let mut inherits_legacy = false;

    let chosen = if let Some(i) = rule_same_path(&entries, &source) {
        // Rule 1: same path, even if the content changed (a replaced ROM keeps its saves, as in 1.0).
        let e = &mut entries[i];
        if e.size != source.size || e.mtime.is_none() || e.mtime != source.mtime {
            let fp = source.fingerprint()?;
            e.size = fp.size;
            e.crc = fp.crc32;
        }
        e.mtime = source.mtime;
        e.path.clone_from(&source.raw_key);
        i
    } else if let Some(i) = rule_moved(&entries, &mut source, &base)? {
        // Rule 2: the recorded file is gone and this one has its content: moved or renamed.
        let e = &mut entries[i];
        tracing::info!(save_name = %e.save_name, "game moved or renamed: saves follow");
        e.mtime = source.mtime;
        e.path.clone_from(&source.raw_key);
        i
    } else {
        let existing = Radicals::list(saves, req.states_core_dir)?;
        let (save_name, index) = match rule_disks_folder(&entries, &mut source, saves, &base)? {
            // Rule 3: a 2.0 working-copy folder recorded copies of this source.
            Some(found) => found,
            // Rule 4: bare name if free in the registry (adopting 1.0 / 2.0 files), else the first free suffix.
            None => attribute(&entries, &existing, saves, &base)?,
        };
        let fp = source.fingerprint()?;
        let legacy = index == 1 && existing.contains_stem(&base);
        entries.push(Entry {
            save_name: save_name.clone(),
            base: base.clone(),
            index,
            legacy,
            size: fp.size,
            mtime: source.mtime,
            crc: fp.crc32,
            path: source.raw_key.clone(),
        });
        if index >= 2 {
            let first = entries.iter().find(|e| same_name(&e.save_name, &base));
            // Without an entry 1, the bare-name files predate the registry: they were shared in 2.0.
            let legacy_base = first.map_or_else(|| existing.contains_stem(&base), |e| e.legacy);
            // Not registered yet: the owner of `<base>.disks` (2.0), else unknown (empty path).
            let first = first
                .map(|e| path_from_key(&e.path))
                .or_else(|| disks_owner(saves, &base))
                .unwrap_or_default();
            inherits_legacy = legacy_base;
            let sram_copied = legacy_base && inherit_sram(saves, &base, &save_name);
            let legacy_states = legacy_base && has_states(req.states_core_dir, &base);
            notice = Some(IdentityNotice::Homonym {
                first,
                sram_copied,
                legacy_states,
            });
            tracing::info!(%save_name, sram_copied, "homonym separated from `{base}`");
        }
        entries.len() - 1
    };

    if entries != before {
        write_registry(&registry_path, &entries)?;
    }
    let e = &entries[chosen];
    let homonyms = entries
        .iter()
        .filter(|o| same_name(&o.base, &e.base))
        .count()
        > 1;
    Ok(GameIdentity {
        save_name: e.save_name.clone(),
        index: e.index,
        core_save_dir: if e.index <= 1 {
            saves.to_path_buf()
        } else {
            saves.join(&e.save_name)
        },
        notice,
        base,
        fingerprint: Some(Fingerprint {
            size: e.size,
            crc32: e.crc,
        }),
        inherits_legacy,
        homonyms,
    })
}

/// One registry line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    save_name: String,
    /// 1.0 / 2.0 name the entry was created for (kept when the file is later renamed: rule 2).
    base: String,
    index: u32,
    /// Files of the bare name existed before this entry: they may have been shared by homonyms in 2.0.
    legacy: bool,
    size: u64,
    mtime: Option<u128>,
    /// Fingerprint CRC32 (whole file or first + last MiB, see [`fingerprint_of`]).
    crc: u32,
    /// `workcopy::path_key` of the canonical primary source.
    path: Vec<u8>,
}

/// Names are compared case-insensitively: on Windows and macOS `G (2).srm` and `g (2).srm` are one file.
fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn suffixed(base: &str, n: u32) -> String {
    if n <= 1 {
        base.to_owned()
    } else {
        format!("{base} ({n})")
    }
}

/// The user's primary source, `stat`ed once; read only when a fingerprint is needed.
struct Primary {
    path: PathBuf,
    /// `path_key` of the canonical path (what the registry stores).
    raw_key: Vec<u8>,
    /// [`registry_key`] of `raw_key` (what is compared).
    key: Vec<u8>,
    size: u64,
    mtime: Option<u128>,
    fingerprint: Option<Fingerprint>,
}

impl Primary {
    fn open(path: &Path) -> Result<Self, RuntimeError> {
        let meta = fs::metadata(path).map_err(|e| io_error(path, e))?;
        if !meta.is_file() {
            return Err(RuntimeError::InvalidMediaSet(format!(
                "{} is not a file",
                path.display()
            )));
        }
        let canonical = canonical(path);
        let raw_key = path_key(&canonical);
        Ok(Self {
            path: path.to_path_buf(),
            key: registry_key(&raw_key),
            raw_key,
            size: meta.len(),
            mtime: mtime_ns(&meta),
            fingerprint: None,
        })
    }

    fn fingerprint(&mut self) -> Result<Fingerprint, RuntimeError> {
        if let Some(fp) = self.fingerprint {
            return Ok(fp);
        }
        let file = File::open(&self.path).map_err(|e| io_error(&self.path, e))?;
        let size = file.metadata().map_err(|e| io_error(&self.path, e))?.len();
        let fp = fingerprint_of(file, size).map_err(|e| io_error(&self.path, e))?;
        // The file may have changed since `open`: what was hashed is what is recorded.
        self.size = fp.size;
        self.fingerprint = Some(fp);
        Ok(fp)
    }
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn mtime_ns(meta: &fs::Metadata) -> Option<u128> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
}

/// Whole file up to [`FP_WHOLE_MAX`], else first and last [`FP_EDGE`] bytes: never more than 2 MiB read for an
/// image CD or a hard-disk file.
fn fingerprint_of(mut input: impl Read + Seek, size: u64) -> io::Result<Fingerprint> {
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut feed = |input: &mut dyn Read, limit: u64| -> io::Result<()> {
        let mut limited = input.take(limit);
        loop {
            match limited.read(&mut buf) {
                Ok(0) => return Ok(()),
                Ok(n) => hasher.update(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    };
    if size <= FP_WHOLE_MAX {
        feed(&mut input, size)?;
    } else {
        feed(&mut input, FP_EDGE)?;
        input.seek(SeekFrom::Start(size - FP_EDGE))?;
        feed(&mut input, FP_EDGE)?;
    }
    Ok(Fingerprint {
        size,
        crc32: hasher.finalize(),
    })
}

/// Path key compared by the registry: the manifest's key, with on Windows the case folded and the `\\?\`
/// prefix of `canonicalize` removed (the manifest format itself is unchanged).
fn registry_key(raw: &[u8]) -> Vec<u8> {
    #[cfg(windows)]
    {
        fold_windows_key(&String::from_utf8_lossy(raw)).into_bytes()
    }
    #[cfg(not(windows))]
    {
        raw.to_vec()
    }
}

#[cfg(any(windows, test))]
fn fold_windows_key(path: &str) -> String {
    let path = match path.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => path.strip_prefix(r"\\?\").unwrap_or(path).to_owned(),
    };
    path.to_lowercase()
}

fn rule_same_path(entries: &[Entry], source: &Primary) -> Option<usize> {
    entries
        .iter()
        .position(|e| registry_key(&e.path) == source.key)
}

/// Rule 2. The fingerprint is computed only if an entry could match (same size, recorded path gone).
fn rule_moved(
    entries: &[Entry],
    source: &mut Primary,
    base: &str,
) -> Result<Option<usize>, RuntimeError> {
    let gone = |e: &Entry| matches!(fs::symlink_metadata(path_from_key(&e.path)), Err(err) if err.kind() == io::ErrorKind::NotFound);
    if !entries.iter().any(|e| e.size == source.size && gone(e)) {
        return Ok(None);
    }
    let fp = source.fingerprint()?;
    let candidates: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.size == fp.size && e.crc == fp.crc32 && gone(e))
        .map(|(i, _)| i)
        .collect();
    // Two moved copies of one file: the one of the same name first.
    Ok(candidates
        .iter()
        .copied()
        .find(|&i| same_name(&entries[i].base, base))
        .or(candidates.first().copied()))
}

/// Rule 3: `<base>.disks` / `<base> (n).disks` whose manifest holds a `Set` copy of this source (path, or
/// size + CRC32 of the whole file) and whose name is not in the registry yet.
fn rule_disks_folder(
    entries: &[Entry],
    source: &mut Primary,
    saves: &Path,
    base: &str,
) -> Result<Option<(String, u32)>, RuntimeError> {
    // A user playlist is not copied: its entries are, so they are what the manifest recorded (path, or size +
    // CRC32 of the whole file for an entry of at most `FP_WHOLE_MAX`: a moved folder).
    let mut files: Vec<SetFile> = if has_ext(&source.path, "m3u") {
        parse_playlist(&source.path)
            .unwrap_or_default()
            .iter()
            .filter_map(|p| SetFile::open(p))
            .collect()
    } else {
        // The manifest records the CRC32 of whole files: comparable to a whole-file fingerprint only.
        let crc = if source.size <= FP_WHOLE_MAX {
            Crc::Pending
        } else {
            Crc::TooLarge
        };
        vec![SetFile {
            path: source.path.clone(),
            key: source.key.clone(),
            size: source.size,
            crc,
        }]
    };
    fn any_name(name: &str) -> bool {
        !name.is_empty() && !name.contains(['/', '\\'])
    }
    for n in 1..=MAX_WORK_DIRS {
        let name = suffixed(base, n);
        if entries.iter().any(|e| same_name(&e.save_name, &name)) {
            continue;
        }
        let dir = saves.join(format!("{name}.disks"));
        let DirState::Manifest(recorded) = workcopy::inspect(&dir, any_name)? else {
            continue;
        };
        let mut found = false;
        'set: for e in recorded.iter().filter(|e| e.origin == Origin::Set) {
            for file in &mut files {
                if registry_key(e.source_key()) == file.key {
                    found = true;
                    break 'set;
                }
                if !e.source_key().is_empty()
                    && e.source_size_crc().0 == file.size
                    && file.crc() == Some(e.source_size_crc().1)
                {
                    found = true;
                    break 'set;
                }
            }
        }
        if found {
            return Ok(Some((name, n)));
        }
    }
    Ok(None)
}

/// One file of the game as copied into a working-copy folder (the primary source, or an entry of its `.m3u`).
struct SetFile {
    path: PathBuf,
    key: Vec<u8>,
    size: u64,
    crc: Crc,
}

/// Whole-file CRC32, computed at most once and only when a recorded copy has the same size.
enum Crc {
    Pending,
    Known(u32),
    /// Over `FP_WHOLE_MAX`: compared by path only here (`choose_work_dir` still finds its folder by content).
    TooLarge,
}

impl SetFile {
    fn open(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok().filter(fs::Metadata::is_file)?;
        let size = meta.len();
        Some(Self {
            path: path.to_path_buf(),
            key: registry_key(&path_key(&canonical(path))),
            size,
            crc: if size <= FP_WHOLE_MAX {
                Crc::Pending
            } else {
                Crc::TooLarge
            },
        })
    }

    /// `None` when too large or unreadable (an unreadable file is simply « no match », logged once).
    fn crc(&mut self) -> Option<u32> {
        if let Crc::Pending = self.crc {
            let read = File::open(&self.path).and_then(|f| fingerprint_of(f, self.size));
            match read {
                Ok(fp) => self.crc = Crc::Known(fp.crc32),
                Err(e) => {
                    tracing::warn!("cannot read {}: {e}", self.path.display());
                    self.crc = Crc::TooLarge;
                }
            }
        }
        match self.crc {
            Crc::Known(crc) => Some(crc),
            _ => None,
        }
    }
}

/// Rule 4.
///
/// Called only when rule 3 found no folder of this source: a `<base>.disks` whose manifest holds present `Set`
/// copies of a source that still exists therefore belongs to another game (the 2.0 owner of the bare name, not launched in 2.1 yet), which must
/// keep `<base>` when it comes (design §2.2). Suffixed candidates already skip any existing `<base> (n).*`, which
/// covers the `<save_name> (n).disks` folders made by `choose_work_dir` for a replaced image.
fn attribute(
    entries: &[Entry],
    existing: &Radicals,
    saves: &Path,
    base: &str,
) -> Result<(String, u32), RuntimeError> {
    let in_registry = |name: &str| entries.iter().any(|e| same_name(&e.save_name, name));
    if !in_registry(base) && disks_owner(saves, base).is_none() {
        return Ok((base.to_owned(), 1));
    }
    (2..=MAX_SUFFIX)
        .map(|n| (suffixed(base, n), n))
        .find(|(name, _)| !in_registry(name) && !existing.contains_stem(name))
        .ok_or_else(|| {
            RuntimeError::InvalidMediaSet(format!("no free save name left for `{base}`"))
        })
}

/// Source of the first present `Set` copy recorded in `<name>.disks` (readable manifest only), if that source
/// still exists: the folder then belongs to the game made from it. A source that is gone (folder moved or renamed
/// before the first 2.1 launch) reserves nothing: the game gets `<name>` as in 2.0 and `choose_work_dir` finds its
/// copies by content.
fn disks_owner(saves: &Path, name: &str) -> Option<PathBuf> {
    fn any_name(name: &str) -> bool {
        !name.is_empty() && !name.contains(['/', '\\'])
    }
    let dir = saves.join(format!("{name}.disks"));
    let DirState::Manifest(recorded) = workcopy::inspect(&dir, any_name).ok()? else {
        return None;
    };
    recorded
        .iter()
        .filter(|e| e.origin == Origin::Set)
        .filter(|e| workcopy::is_regular_file(&dir.join(&e.copy)).unwrap_or(false))
        .filter_map(|e| e.source_path())
        .find(|source| source.is_file())
}

/// Lower-cased names of `<saves>/<core_id>/` and `<states>/<core_id>/`, listed once.
struct Radicals(Vec<String>);

impl Radicals {
    fn list(saves: &Path, states: &Path) -> Result<Self, RuntimeError> {
        let mut names = Vec::new();
        for dir in [saves, states] {
            let read = match fs::read_dir(dir) {
                Ok(read) => read,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io_error(dir, e)),
            };
            for entry in read {
                let entry = entry.map_err(|e| io_error(dir, e))?;
                names.push(entry.file_name().to_string_lossy().to_lowercase());
            }
        }
        Ok(Self(names))
    }

    /// A file or folder named `name` or `name.<ext>` (`G (2).srm`, `G (2).state0`, `G (2).disks`, `G (2)/`).
    fn contains_stem(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        self.0
            .iter()
            .any(|n| *n == name || n.rsplit_once('.').is_some_and(|(stem, _)| stem == name))
    }
}

/// Copies `<base>.srm` to `<save_name>.srm` if the target is absent; `true` if a copy was made.
fn inherit_sram(saves: &Path, base: &str, save_name: &str) -> bool {
    let from = saves.join(format!("{base}.srm"));
    let to = saves.join(format!("{save_name}.srm"));
    match fs::symlink_metadata(&from) {
        Ok(meta) if meta.is_file() => {}
        _ => return false,
    }
    match fs::symlink_metadata(&to) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        _ => return false,
    }
    let copied = File::open(&from).and_then(|mut input| {
        write_atomic_with(&to, FileMode::Default, |out| {
            io::copy(&mut input, out).map(drop)
        })
    });
    match copied {
        Ok(()) => {
            tracing::info!("{} copied to {}", from.display(), to.display());
            true
        }
        Err(e) => {
            tracing::warn!("cannot copy {} to {}: {e}", from.display(), to.display());
            false
        }
    }
}

fn has_states(states: &Path, base: &str) -> bool {
    (0..STATE_SLOTS).any(|slot| states.join(format!("{base}.state{slot}")).is_file())
}

/// Exclusive lock on `.emu-rust-games.lock`, released on drop. A file system without locks (some network
/// mounts) works unprotected rather than not at all, as for `cores/.lock`: two launches at the same instant may
/// then both attribute a name (the last registry write wins; nothing is deleted). The working-copy manifests
/// (`workcopy::write`) take no inter-process lock either: two instances of the same game are not guarded.
struct RegistryLock {
    _file: Option<File>,
}

impl RegistryLock {
    fn acquire(saves: &Path) -> Result<Self, RuntimeError> {
        let path = saves.join(LOCK_NAME);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| io_error(&path, e))?;
        match file.lock() {
            Ok(()) => Ok(Self { _file: Some(file) }),
            Err(e) => {
                tracing::warn!(
                    "{}: lock not available ({e}); concurrent launches are not protected",
                    path.display()
                );
                Ok(Self { _file: None })
            }
        }
    }
}

enum Registry {
    Entries(Vec<Entry>),
    Unusable(&'static str),
}

fn read_registry(path: &Path) -> Result<Registry, RuntimeError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Ok(Registry::Unusable("not a regular file")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Registry::Entries(Vec::new())),
        Err(e) => return Err(io_error(path, e)),
    }
    let bytes = read_bounded(path, MAX_REGISTRY_BYTES).map_err(|e| io_error(path, e))?;
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Ok(Registry::Unusable("larger than 8 MiB"));
    }
    Ok(decode(&bytes).map_or(
        Registry::Unusable("not a valid registry"),
        Registry::Entries,
    ))
}

/// `.emu-rust-games.corrupt-<unix>` next to the registry (never a link, never twice for the same size).
fn keep_corrupt_copy(path: &Path) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if !meta.is_file() {
        return;
    }
    // The original stays in place anyway: an oversized file is not duplicated.
    if meta.len() > MAX_REGISTRY_BYTES as u64 {
        tracing::warn!(
            "{} is larger than 8 MiB: left in place, not copied",
            path.display()
        );
        return;
    }
    let Some(dir) = path.parent() else { return };
    let prefix = format!("{REGISTRY_NAME}.corrupt-");
    let already = fs::read_dir(dir).into_iter().flatten().flatten().any(|e| {
        e.file_name().to_string_lossy().starts_with(&prefix)
            && e.metadata().is_ok_and(|m| m.len() == meta.len())
    });
    if already {
        return;
    }
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let copy = dir.join(format!("{prefix}{unix}"));
    let copied = File::open(path).and_then(|mut input| {
        write_atomic_with(&copy, FileMode::Default, |out| {
            io::copy(&mut input, out).map(drop)
        })
    });
    match copied {
        Ok(()) => tracing::warn!("unusable registry kept as {}", copy.display()),
        Err(e) => tracing::warn!("cannot keep a copy of {}: {e}", path.display()),
    }
}

fn write_registry(path: &Path, entries: &[Entry]) -> Result<(), RuntimeError> {
    write_atomic(path, &encode(entries), FileMode::Default).map_err(|e| io_error(path, e))
}

// Format: a header line, then one line per game, tab-separated:
// save_name, base, index, legacy (0/1), size, mtime in ns (`-` if unknown), fingerprint crc32 (8 hex), source
// path key. Text fields are escaped like the manifest's (`%XX` for `%`, control bytes and DEL).

fn encode(entries: &[Entry]) -> Vec<u8> {
    let mut out = HEADER.to_vec();
    out.push(b'\n');
    for e in entries {
        escape(e.save_name.as_bytes(), &mut out);
        out.push(b'\t');
        escape(e.base.as_bytes(), &mut out);
        let mtime = e.mtime.map_or_else(|| "-".to_owned(), |m| m.to_string());
        out.extend_from_slice(
            format!(
                "\t{}\t{}\t{}\t{mtime}\t{:08x}\t",
                e.index,
                u8::from(e.legacy),
                e.size,
                e.crc
            )
            .as_bytes(),
        );
        escape(&e.path, &mut out);
        out.push(b'\n');
    }
    out
}

fn decode(bytes: &[u8]) -> Option<Vec<Entry>> {
    let body = bytes.strip_suffix(b"\n")?;
    let mut lines = body.split(|&b| b == b'\n');
    if lines.next()? != HEADER {
        return None;
    }
    let mut out: Vec<Entry> = Vec::new();
    for line in lines {
        let fields: Vec<&[u8]> = line.split(|&b| b == b'\t').collect();
        let [save_name, base, index, legacy, size, mtime, crc, path] = fields.as_slice() else {
            return None;
        };
        let text = |f: &[u8]| std::str::from_utf8(f).ok().map(str::to_owned);
        let save_name = String::from_utf8(unescape(save_name)?).ok()?;
        let base = String::from_utf8(unescape(base)?).ok()?;
        let index: u32 = text(index)?.parse().ok()?;
        // Names become file names: only what this module could have produced is accepted.
        if index == 0
            || base.is_empty()
            || game_name_from_str(&base) != base
            || save_name != suffixed(&base, index)
            || out.iter().any(|e| same_name(&e.save_name, &save_name))
        {
            return None;
        }
        let legacy = match *legacy {
            b"0" => false,
            b"1" => true,
            _ => return None,
        };
        let mtime = match *mtime {
            b"-" => None,
            m => Some(text(m)?.parse().ok()?),
        };
        if crc.len() != 8 {
            return None;
        }
        let path = unescape(path)?;
        if path.is_empty() {
            return None;
        }
        out.push(Entry {
            save_name,
            base,
            index,
            legacy,
            size: text(size)?.parse().ok()?,
            mtime,
            crc: u32::from_str_radix(&text(crc)?, 16).ok()?,
            path,
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests;
