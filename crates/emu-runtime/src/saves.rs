//! Persistence helpers: atomic writes, save layout, savestate container (ADR 0011).

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::error::RuntimeError;
use crate::fsname::sanitize_component;

/// Savestate slots are `0..STATE_SLOTS`.
pub const STATE_SLOTS: u8 = 10;

/// Magic at offset 0 of an emu-rust savestate file.
pub const STATE_MAGIC: [u8; 8] = *b"EMURSTAT";
/// Container version written by this build; `decode_state` rejects anything else.
pub const STATE_FORMAT_VERSION: u32 = 1;

/// Permissions of a file created by `write_atomic`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FileMode {
    /// Process umask (saves, states).
    #[default]
    Default,
    /// `0o600` on Unix from creation (no window where the file is readable by others); ignored on Windows.
    /// For `config.toml`, which will hold credentials from phase 4 (ADR 0006).
    Private,
}

/// Writes `data` to `path` atomically: parent directories created, temporary file in the same directory
/// (`.<name>.tmp-<pid>`), `sync_all`, `rename` over `path`, then fsync of the directory on Unix.
/// A crash leaves either the old or the new file, never a truncated one. The temporary file is removed on error.
pub fn write_atomic(path: &Path, data: &[u8], mode: FileMode) -> io::Result<()> {
    write_atomic_with(path, mode, |file| file.write_all(data))
}

/// [`write_atomic`] with the content produced by `fill` (streamed copies of large files). `fill` writes into the
/// temporary file; on error nothing replaces `path`.
pub(crate) fn write_atomic_with(
    path: &Path,
    mode: FileMode,
    fill: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a file path: {}", path.display()),
        )
    })?;
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    fs::create_dir_all(dir)?;

    let mut tmp_name = OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".tmp-{}", std::process::id()));
    let tmp = dir.join(tmp_name);

    let mut file = match create_new(&tmp, mode) {
        // Leftover of a crashed run that had the same pid (common in containers): replace it. `remove_file`
        // removes a symlink itself, never its target, and `create_new` refuses to follow one.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(&tmp)?;
            create_new(&tmp, mode)?
        }
        other => other?,
    };
    let synced = fill(&mut file).and_then(|()| file.sync_all());
    // Closed before the rename: Windows cannot rename an open file.
    drop(file);
    let written = synced.and_then(|()| fs::rename(&tmp, path));
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    sync_dir(dir);
    Ok(())
}

fn create_new(path: &Path, mode: FileMode) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if mode == FileMode::Private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = mode;
    options.open(path)
}

/// Makes the rename durable. Best effort: some filesystems (9p/drvfs under WSL, FUSE) reject fsync on a
/// directory, and the data itself is already synced, so a failure is only logged.
#[cfg(unix)]
fn sync_dir(dir: &Path) {
    if let Err(e) = File::open(dir).and_then(|d| d.sync_all()) {
        tracing::debug!("cannot fsync directory {}: {e}", dir.display());
    }
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) {}

/// Copies the newest `<saves>/<other>/<save_rel>` among the cores of `family` (other than `core_id`) to
/// `<saves>/<core_id>/<save_rel>` when the latter does not exist (design v7.0 §3.2): `create_new`, never
/// overwrites, links never followed, at most 1 MiB, `save_rel` a single plain file name. Returns the id of the
/// source core, `None` when nothing was copied. Called by the UI before `Emulator::load_content`, never during
/// a session.
///
/// Known limit: the checks (`symlink_metadata` then `open`, folder check then publication) are not atomic, so
/// a local process able to swap files in the saves folder could race them. That is not a privilege boundary
/// (same user), and `create_new` / `hard_link` never overwrite an existing file. The target appears complete
/// or not at all.
pub fn adopt_family_save(
    saves_dir: &Path,
    core_id: &str,
    family: &[String],
    save_rel: &Path,
) -> Result<Option<String>, RuntimeError> {
    if !is_plain_name(core_id) || family.iter().any(|c| !is_plain_name(c)) {
        return Err(bad_name(saves_dir, core_id));
    }
    let mut parts = save_rel.components();
    let name = match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(n)), None) if n.to_str().is_some_and(is_plain_name) => n,
        _ => return Err(bad_name(saves_dir, &save_rel.to_string_lossy())),
    };
    let target_dir = saves_dir.join(core_id);
    let target = target_dir.join(name);
    if fs::symlink_metadata(&target).is_ok() {
        return Ok(None);
    }
    // Newest regular file (never a link) among the other cores of the family.
    let mut best: Option<(std::time::SystemTime, &str, PathBuf)> = None;
    for other in family.iter().filter(|c| c.as_str() != core_id) {
        let dir = saves_dir.join(other);
        if !fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let path = dir.join(name);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > ADOPT_MAX_BYTES {
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, ..)| modified > *t) {
            best = Some((modified, other, path));
        }
    }
    let Some((_, source_id, source)) = best else {
        return Ok(None);
    };
    let io_err = |path: &Path, source| RuntimeError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut input = File::open(&source).map_err(|e| io_err(&source, e))?;
    // Checked again on the opened file (narrows the window after `symlink_metadata`; size stays bounded).
    let meta = input.metadata().map_err(|e| io_err(&source, e))?;
    if !meta.is_file() || meta.len() > ADOPT_MAX_BYTES {
        return Ok(None);
    }
    let mut data = Vec::with_capacity(meta.len() as usize);
    (&mut input)
        .take(ADOPT_MAX_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|e| io_err(&source, e))?;
    if data.len() as u64 > ADOPT_MAX_BYTES {
        return Ok(None);
    }
    // A link in place of the target folder would redirect the copy.
    match fs::symlink_metadata(&target_dir) {
        Ok(m) if !m.is_dir() => return Err(bad_name(saves_dir, core_id)),
        Ok(_) => {}
        Err(_) => fs::create_dir_all(&target_dir).map_err(|e| io_err(&target_dir, e))?,
    }
    match publish_new_file(&target_dir, name, &data) {
        Ok(true) => {}
        Ok(false) => return Ok(None),
        Err(e) => return Err(io_err(&target, e)),
    }
    tracing::info!("{} adopted from {}", target.display(), source.display());
    Ok(Some(source_id.to_owned()))
}

/// Writes `data` as `<dir>/<name>` only if it does not exist: temporary file in `dir` (`create_new`, unique
/// name), `sync_all`, then published without overwriting (`hard_link`, temporary removed). A kill while writing
/// leaves at worst a `.tmp` file, never a truncated target. Where hard links are unavailable, `rename` is used
/// after checking the target is still absent (small window between check and rename, accepted: it is not a
/// privilege boundary). `Ok(false)`: the target appeared meanwhile, left untouched.
fn publish_new_file(dir: &Path, name: &std::ffi::OsStr, data: &[u8]) -> io::Result<bool> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let target = dir.join(name);
    let mut attempt = 0;
    let (tmp, mut file) = loop {
        let mut tmp_name = OsString::from(".");
        tmp_name.push(name);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        tmp_name.push(format!(".adopt-{}-{n}.tmp", std::process::id()));
        let tmp = dir.join(tmp_name);
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && attempt < 8 => attempt += 1,
            Err(e) => return Err(e),
        }
    };
    let written = file.write_all(data).and_then(|()| file.sync_all());
    drop(file);
    let published = written.and_then(|()| match fs::hard_link(&tmp, &target) {
        Ok(()) => {
            let _ = fs::remove_file(&tmp);
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(_) if fs::symlink_metadata(&target).is_ok() => Ok(false),
        // No hard links (some network or FAT filesystems).
        Err(_) => fs::rename(&tmp, &target).map(|()| true),
    });
    if !matches!(published, Ok(true)) {
        let _ = fs::remove_file(&tmp);
    }
    if matches!(published, Ok(true)) {
        sync_dir(dir);
    }
    published
}

/// Largest save adopted from another core (a PS1 memory card is 128 KiB).
const ADOPT_MAX_BYTES: u64 = 1 << 20;

fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0', ':'])
}

fn bad_name(dir: &Path, what: &str) -> RuntimeError {
    RuntimeError::Io {
        path: dir.join(what),
        source: io::Error::new(io::ErrorKind::InvalidInput, "not a plain file name"),
    }
}

/// Stable identifier of a core from its library path: file stem, `_libretro` suffix and a leading `lib`
/// (if the rest still ends with `_libretro`) removed, characters outside `[A-Za-z0-9_-]` replaced by `_`.
/// `genesis_plus_gx_libretro.so` → `genesis_plus_gx`. Used for save folders and per-core config.
pub fn core_id_from_path(core: &Path) -> String {
    let stem = core
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let id = match stem.strip_suffix("_libretro") {
        Some(rest) => rest
            .strip_prefix("lib")
            .filter(|r| !r.is_empty())
            .unwrap_or(rest),
        None => &stem,
    };
    let id: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // `CON`, `nul`... are valid ids but not valid file names on Windows.
    let id = sanitize_component(&id, MAX_GAME_NAME_BYTES);
    if id.is_empty() { "core".to_owned() } else { id }
}

/// Game name used for save files: file stem of the content (the archive for a zip), with `/`, `\`, NUL and
/// control characters replaced by `_`, Windows reserved names prefixed (see `fsname`). `Sonic (World).zip` → `Sonic (World)`.
pub fn game_name_from_path(rom: &Path) -> String {
    let stem = rom
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let name = sanitize_component(&stem, MAX_GAME_NAME_BYTES);
    // Never empty, so `<game>.srm` is not a hidden file. Names are always joined with an extension, so even
    // `..` stays a single path component.
    if name.is_empty() {
        "game".to_owned()
    } else {
        name
    }
}

/// Game name from a display name (multi-disk set, `MediaSet::name`): same rules as `game_name_from_path` but the
/// whole string is kept (`Ys.Book.I` stays as is).
pub(crate) fn game_name_from_str(name: &str) -> String {
    let name = sanitize_component(name, MAX_GAME_NAME_BYTES);
    if name.is_empty() {
        "game".to_owned()
    } else {
        name
    }
}

/// Keeps `<game>.state9` and `.<game>.srm.tmp-<pid>` (+ leading dot, pid up to 10 digits) under the 255-byte
/// file name limit of common filesystems.
const MAX_GAME_NAME_BYTES: usize = 200;

/// Reads at most `max + 1` bytes of `path`: a result longer than `max` means the file is bigger than allowed,
/// without ever loading more than one byte past the bound.
pub(crate) fn read_bounded(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut out = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut out)?;
    Ok(out)
}

/// Reads only the container header (`16 + header_len` bytes), for menus: no payload, no checksum check.
pub(crate) fn read_state_header(mut input: impl Read) -> Result<StateHeader, StateFileError> {
    let mut fixed = [0u8; 16];
    let n = read_full(&mut input, &mut fixed).map_err(|_| StateFileError::Truncated)?;
    let magic_len = n.min(STATE_MAGIC.len());
    if fixed[..magic_len] != STATE_MAGIC[..magic_len] {
        return Err(StateFileError::BadMagic);
    }
    if n < fixed.len() {
        return Err(StateFileError::Truncated);
    }
    let version = u32::from_le_bytes([fixed[8], fixed[9], fixed[10], fixed[11]]);
    if version != STATE_FORMAT_VERSION {
        return Err(StateFileError::UnsupportedVersion(version));
    }
    let header_len = u32::from_le_bytes([fixed[12], fixed[13], fixed[14], fixed[15]]) as usize;
    if header_len > MAX_HEADER_LEN {
        return Err(StateFileError::BadHeader);
    }
    let mut header = vec![0u8; header_len];
    if read_full(&mut input, &mut header).map_err(|_| StateFileError::Truncated)? < header_len {
        return Err(StateFileError::Truncated);
    }
    parse_header(&header)
}

fn read_full(input: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match input.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

/// `<saves>/<core_id>/<game>.srm`.
pub fn sram_path(save_dir: &Path, core_id: &str, game: &str) -> PathBuf {
    save_dir.join(core_id).join(format!("{game}.srm"))
}

/// `<states>/<core_id>/<game>.state<slot>` (`.state0` … `.state9`).
pub fn state_path(state_dir: &Path, core_id: &str, game: &str, slot: u8) -> PathBuf {
    state_dir.join(core_id).join(format!("{game}.state{slot}"))
}

/// Metadata stored in front of the core's serialized data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateHeader {
    pub core_id: String,
    /// `SystemInfo::library_version` at save time; a mismatch only warns on load.
    pub core_version: String,
    pub game: String,
    /// Seconds since the Unix epoch.
    pub created_unix: u64,
    /// `game_fp=<size>:<crc32>` (2.1): fingerprint of the game's primary source at save time. `None`: 1.0 / 2.0
    /// state, or no fingerprint known (the line is then not written).
    pub game_fp: Option<String>,
}

/// Where a savestate comes from, relative to the running game (`Emulator::state_origin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateOrigin {
    /// Same `game_fp`.
    Same,
    /// `game_fp` differs: another file of the same name (or the content changed). The UI asks before loading.
    OtherContent,
    /// No `game_fp` (1.0 / 2.0 state) and several registry entries share the base name. The UI asks.
    Ambiguous,
    /// No `game_fp`, no homonym.
    Unknown,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum StateFileError {
    #[error("not an emu-rust savestate")]
    BadMagic,
    #[error("unsupported savestate format version {0}")]
    UnsupportedVersion(u32),
    #[error("truncated savestate")]
    Truncated,
    #[error("savestate checksum mismatch (corrupt file)")]
    Checksum,
    #[error("invalid savestate header")]
    BadHeader,
    #[error("savestate belongs to core `{found}`, not `{expected}`")]
    WrongCore { expected: String, found: String },
}

/// Little-endian container:
/// `STATE_MAGIC` | `u32` version | `u32` header length | header (UTF-8 `key=value\n` lines: `core_id`,
/// `core_version`, `game`, `created_unix`, optional `game_fp`; unknown keys ignored) | `u64` payload length | payload |
/// `u64` FNV-1a 64 of the payload.
pub fn encode_state(header: &StateHeader, payload: &[u8]) -> Vec<u8> {
    // Line breaks in a value would split it into bogus keys, and the length cap keeps the header under
    // `MAX_HEADER_LEN`: core-provided strings are not trusted.
    let clean = |v: &str| -> String {
        v.chars()
            .take(MAX_VALUE_CHARS)
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect()
    };
    let mut text = format!(
        "core_id={}\ncore_version={}\ngame={}\ncreated_unix={}\n",
        clean(&header.core_id),
        clean(&header.core_version),
        clean(&header.game),
        header.created_unix
    );
    // Appended last and only when known: a 2.0 reader ignores the unknown key, a 1.0 / 2.0 state stays byte-identical.
    if let Some(fp) = &header.game_fp {
        text.push_str(&format!("game_fp={}\n", clean(fp)));
    }
    // At most 5 × MAX_VALUE_CHARS × 4 bytes + keys, well below `MAX_HEADER_LEN`.
    let header_len = text.len() as u32;
    let mut out = Vec::with_capacity(8 + 4 + 4 + text.len() + 8 + payload.len() + 8);
    out.extend_from_slice(&STATE_MAGIC);
    out.extend_from_slice(&STATE_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&header_len.to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(payload);
    out.extend_from_slice(&fnv1a64(payload).to_le_bytes());
    out
}

/// Header text larger than this is rejected without being parsed.
const MAX_HEADER_LEN: usize = 64 * 1024;
const MAX_VALUE_CHARS: usize = 1024;

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Bounds-checked little-endian reader over untrusted bytes.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], StateFileError> {
        if n > self.0.len() {
            return Err(StateFileError::Truncated);
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn u32(&mut self) -> Result<u32, StateFileError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, StateFileError> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
}

fn parse_header(bytes: &[u8]) -> Result<StateHeader, StateFileError> {
    let text = std::str::from_utf8(bytes).map_err(|_| StateFileError::BadHeader)?;
    let (mut core_id, mut core_version, mut game, mut created, mut game_fp) =
        (None, None, None, None, None);
    for line in text.split('\n').filter(|l| !l.is_empty()) {
        let (key, value) = line.split_once('=').ok_or(StateFileError::BadHeader)?;
        let slot = match key {
            "core_id" => &mut core_id,
            "core_version" => &mut core_version,
            "game" => &mut game,
            "created_unix" => &mut created,
            "game_fp" => &mut game_fp,
            _ => continue,
        };
        if slot.replace(value).is_some() {
            return Err(StateFileError::BadHeader);
        }
    }
    let (Some(core_id), Some(core_version), Some(game), Some(created)) =
        (core_id, core_version, game, created)
    else {
        return Err(StateFileError::BadHeader);
    };
    Ok(StateHeader {
        core_id: core_id.to_owned(),
        core_version: core_version.to_owned(),
        game: game.to_owned(),
        created_unix: created.parse().map_err(|_| StateFileError::BadHeader)?,
        game_fp: game_fp.map(str::to_owned),
    })
}

/// Parses and checks a container (magic, version, lengths, checksum); returns the header and the payload
/// slice. Never panics on arbitrary input. Does not compare `core_id` (the caller does: `WrongCore`).
///
/// Bytes after the checksum are reported as `Checksum` (the file is not what was written).
pub fn decode_state(bytes: &[u8]) -> Result<(StateHeader, &[u8]), StateFileError> {
    let magic_len = bytes.len().min(STATE_MAGIC.len());
    if bytes[..magic_len] != STATE_MAGIC[..magic_len] {
        return Err(StateFileError::BadMagic);
    }
    let mut cur = Cursor(bytes);
    cur.take(STATE_MAGIC.len())?;
    let version = cur.u32()?;
    if version != STATE_FORMAT_VERSION {
        return Err(StateFileError::UnsupportedVersion(version));
    }
    let header_len = usize::try_from(cur.u32()?).map_err(|_| StateFileError::BadHeader)?;
    if header_len > MAX_HEADER_LEN {
        return Err(StateFileError::BadHeader);
    }
    let header = cur.take(header_len)?;
    let payload_len = usize::try_from(cur.u64()?).map_err(|_| StateFileError::Truncated)?;
    let payload = cur.take(payload_len)?;
    let checksum = cur.u64()?;
    if !cur.0.is_empty() || checksum != fnv1a64(payload) {
        return Err(StateFileError::Checksum);
    }
    // Parsed last: a corrupt file is reported as such rather than as a bad header.
    let header = parse_header(header)?;
    Ok((header, payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tempdir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn family() -> Vec<String> {
        ["pcsx", "swan", "beetle"].map(String::from).to_vec()
    }

    fn put(root: &Path, core: &str, name: &str, data: &[u8]) -> PathBuf {
        let path = root.join(core).join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }

    #[test]
    fn adopt_copies_when_absent_and_never_overwrites() {
        let tmp = tempdir();
        let root = tmp.path();
        let rel = Path::new("Game.srm");
        assert_eq!(
            adopt_family_save(root, "swan", &family(), rel).unwrap(),
            None
        );
        put(root, "pcsx", "Game.srm", b"card");
        assert_eq!(
            adopt_family_save(root, "swan", &family(), rel).unwrap(),
            Some("pcsx".into())
        );
        assert_eq!(fs::read(root.join("swan/Game.srm")).unwrap(), b"card");
        assert!(root.join("pcsx/Game.srm").exists(), "original kept");
        // Target exists: untouched, nothing reported.
        put(root, "beetle", "Game.srm", b"other");
        fs::write(root.join("swan/Game.srm"), b"mine").unwrap();
        assert_eq!(
            adopt_family_save(root, "swan", &family(), rel).unwrap(),
            None
        );
        assert_eq!(fs::read(root.join("swan/Game.srm")).unwrap(), b"mine");
        // A core outside the family is never a source.
        put(root, "stranger", "Y.srm", b"x");
        assert_eq!(
            adopt_family_save(root, "swan", &family(), Path::new("Y.srm")).unwrap(),
            None
        );
    }

    #[test]
    fn adopt_picks_the_newest_source() {
        let tmp = tempdir();
        let root = tmp.path();
        let old = put(root, "pcsx", "G.srm", b"old");
        put(root, "beetle", "G.srm", b"new");
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(past)
            .unwrap();
        assert_eq!(
            adopt_family_save(root, "swan", &family(), Path::new("G.srm")).unwrap(),
            Some("beetle".into())
        );
        assert_eq!(fs::read(root.join("swan/G.srm")).unwrap(), b"new");
    }

    #[test]
    fn adopt_refuses_hostile_input() {
        let tmp = tempdir();
        let root = tmp.path();
        put(root, "pcsx", "G.srm", b"card");
        for bad in ["../G.srm", "a/G.srm", "", ".", "..", "/G.srm", "a\\b.srm"] {
            assert!(
                adopt_family_save(root, "swan", &family(), Path::new(bad)).is_err(),
                "{bad}"
            );
        }
        for bad_core in ["..", "a/b", ""] {
            assert!(adopt_family_save(root, bad_core, &family(), Path::new("G.srm")).is_err());
        }
        let evil = vec!["..".to_string()];
        assert!(adopt_family_save(root, "swan", &evil, Path::new("G.srm")).is_err());
        assert!(!root.join("swan").exists());
        // Too large: skipped.
        put(
            root,
            "pcsx",
            "Big.srm",
            &vec![0u8; (ADOPT_MAX_BYTES + 1) as usize],
        );
        assert_eq!(
            adopt_family_save(root, "swan", &family(), Path::new("Big.srm")).unwrap(),
            None
        );
        assert!(!root.join("swan/Big.srm").exists());
        // Exactly the limit: copied.
        put(
            root,
            "pcsx",
            "Max.srm",
            &vec![1u8; ADOPT_MAX_BYTES as usize],
        );
        assert!(
            adopt_family_save(root, "swan", &family(), Path::new("Max.srm"))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn adopt_leaves_no_temporary_file() {
        let tmp = tempdir();
        let root = tmp.path();
        put(root, "pcsx", "Game.srm", b"card");
        adopt_family_save(root, "swan", &family(), Path::new("Game.srm")).unwrap();
        assert_eq!(entries(&root.join("swan")), ["Game.srm"]);
    }

    #[test]
    fn publish_new_file_never_overwrites_and_cleans_up() {
        let tmp = tempdir();
        let dir = tmp.path();
        let name = std::ffi::OsStr::new("A.srm");
        assert!(publish_new_file(dir, name, b"one").unwrap());
        assert!(!publish_new_file(dir, name, b"two").unwrap());
        assert_eq!(fs::read(dir.join("A.srm")).unwrap(), b"one");
        assert_eq!(entries(dir), ["A.srm"]);
        // Folder missing: error, nothing created.
        assert!(publish_new_file(&dir.join("nope"), name, b"x").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn adopt_never_follows_links() {
        use std::os::unix::fs::symlink;
        let tmp = tempdir();
        let root = tmp.path().join("saves");
        let secret = tmp.path().join("secret.srm");
        fs::write(&secret, b"secret").unwrap();
        // Source is a link.
        fs::create_dir_all(root.join("pcsx")).unwrap();
        symlink(&secret, root.join("pcsx/G.srm")).unwrap();
        assert_eq!(
            adopt_family_save(&root, "swan", &family(), Path::new("G.srm")).unwrap(),
            None
        );
        assert!(!root.join("swan/G.srm").exists());
        // Source folder is a link.
        let real = tmp.path().join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("H.srm"), b"x").unwrap();
        symlink(&real, root.join("beetle")).unwrap();
        assert_eq!(
            adopt_family_save(&root, "swan", &family(), Path::new("H.srm")).unwrap(),
            None
        );
        // Target is a dangling link: not written through.
        put(&root, "pcsx", "T.srm", b"card");
        fs::create_dir_all(root.join("swan")).unwrap();
        let victim = tmp.path().join("victim");
        symlink(&victim, root.join("swan/T.srm")).unwrap();
        assert_eq!(
            adopt_family_save(&root, "swan", &family(), Path::new("T.srm")).unwrap(),
            None
        );
        assert!(!victim.exists());
        // Target folder is a link: refused.
        put(&root, "pcsx", "U.srm", b"card");
        fs::remove_dir_all(root.join("swan")).unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        symlink(&elsewhere, root.join("swan")).unwrap();
        assert!(adopt_family_save(&root, "swan", &family(), Path::new("U.srm")).is_err());
        assert!(!elsewhere.join("U.srm").exists());
    }

    #[test]
    fn write_atomic_creates_and_replaces() {
        let tmp = tempdir();
        let path = tmp.path().join("a/b/game.srm");
        write_atomic(&path, b"first", FileMode::Default).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");
        write_atomic(&path, b"second!", FileMode::Default).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second!");
        assert_eq!(entries(&tmp.path().join("a/b")), ["game.srm"]);
    }

    #[test]
    fn write_atomic_replaces_stale_temp_file() {
        let tmp = tempdir();
        let stale = tmp.path().join(format!(".x.tmp-{}", std::process::id()));
        fs::write(&stale, b"garbage from a crash").unwrap();
        write_atomic(&tmp.path().join("x"), b"ok", FileMode::Default).unwrap();
        assert_eq!(fs::read(tmp.path().join("x")).unwrap(), b"ok");
        assert_eq!(entries(tmp.path()), ["x"]);
    }

    #[test]
    fn write_atomic_failure_leaves_no_residue() {
        let tmp = tempdir();
        // Target is a non-empty directory: rename fails, the temporary file must be cleaned up.
        let target = tmp.path().join("dir");
        fs::create_dir_all(target.join("inner")).unwrap();
        assert!(write_atomic(&target, b"data", FileMode::Default).is_err());
        assert_eq!(entries(tmp.path()), ["dir"]);
        assert!(write_atomic(Path::new("/"), b"data", FileMode::Default).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_private_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir();
        let path = tmp.path().join("config.toml");
        fs::write(&path, b"old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        write_atomic(&path, b"secret = 1", FileMode::Private).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(fs::read(&path).unwrap(), b"secret = 1");
    }

    #[test]
    fn core_ids() {
        let id = |p: &str| core_id_from_path(Path::new(p));
        assert_eq!(id("/cores/genesis_plus_gx_libretro.so"), "genesis_plus_gx");
        assert_eq!(id("C:/cores/snes9x_libretro.dll"), "snes9x");
        assert_eq!(id("cores/mgba_libretro.dylib"), "mgba");
        assert_eq!(id("libmock_libretro.so"), "mock");
        assert_eq!(id("libretro_core.so"), "libretro_core");
        assert_eq!(id("lib_libretro.so"), "lib");
        assert_eq!(id("beetle psx+hw_libretro.so"), "beetle_psx_hw");
        assert_eq!(id("fceumm-libretro.so"), "fceumm-libretro");
        assert_eq!(id("ñ_libretro.so"), "_");
        assert_eq!(id(""), "core");
        assert_eq!(id("_libretro.so"), "core");
    }

    #[test]
    fn game_names() {
        let name = |p: &str| game_name_from_path(Path::new(p));
        assert_eq!(name("/roms/Sonic (World).zip"), "Sonic (World)");
        assert_eq!(name("roms/Ys.Book.I.sfc"), "Ys.Book.I");
        // `\\` is a path separator on Windows only, so the stem differs there.
        #[cfg(not(windows))]
        assert_eq!(name("a\\b\u{1}c.bin"), "a_b_c");
        #[cfg(windows)]
        assert_eq!(name("a\\b\u{1}c.bin"), "b_c");
        assert_eq!(name(""), "game");
        assert_eq!(name("/"), "game");
        assert_eq!(name("/roms/CON.zip"), "_CON");
        assert_eq!(name("nul.tar.gz"), "_nul.tar");
        // `a:` is a drive prefix on Windows.
        #[cfg(not(windows))]
        assert_eq!(name("a:b?.zip"), "a_b_");
        #[cfg(windows)]
        assert_eq!(name("a:b?.zip"), "b_");
        assert_eq!(core_id_from_path(Path::new("aux_libretro.so")), "_aux");
    }

    #[test]
    fn long_game_names_are_bounded() {
        let long = format!("{}.zip", "é".repeat(300));
        let name = game_name_from_path(Path::new(&long));
        assert!(name.len() <= MAX_GAME_NAME_BYTES && name.len() >= MAX_GAME_NAME_BYTES - 1);
        assert!(name.chars().all(|c| c == 'é'));
        let tmp = tempdir();
        let path = sram_path(tmp.path(), "mock", &name);
        write_atomic(&path, b"x", FileMode::Default).unwrap();
        let ascii = game_name_from_path(Path::new(&format!("{}.bin", "a".repeat(500))));
        assert_eq!(ascii.len(), MAX_GAME_NAME_BYTES);
        let path = sram_path(tmp.path(), "mock", &ascii);
        write_atomic(&path, b"x", FileMode::Default).unwrap();
    }

    #[test]
    fn bounded_read_and_header_only() {
        let tmp = tempdir();
        let path = tmp.path().join("f");
        fs::write(&path, [1u8; 100]).unwrap();
        assert_eq!(read_bounded(&path, 100).unwrap().len(), 100);
        assert_eq!(read_bounded(&path, 10).unwrap().len(), 11);

        let bytes = encode_state(&header(), &[9u8; 1000]);
        let header_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        // Only the header bytes are available: the payload is never needed.
        assert_eq!(
            read_state_header(&bytes[..16 + header_len]).unwrap(),
            header()
        );
        assert_eq!(
            read_state_header(&bytes[..16 + header_len - 1]),
            Err(StateFileError::Truncated)
        );
        assert_eq!(
            read_state_header(&b"junk"[..]),
            Err(StateFileError::BadMagic)
        );
        assert_eq!(
            read_state_header(&STATE_MAGIC[..]),
            Err(StateFileError::Truncated)
        );
    }

    #[test]
    fn save_paths() {
        let base = Path::new("/data/saves");
        assert_eq!(
            sram_path(base, "genesis_plus_gx", "Sonic (World)"),
            Path::new("/data/saves/genesis_plus_gx/Sonic (World).srm")
        );
        assert_eq!(
            state_path(Path::new("/data/states"), "mock", "g", 9),
            Path::new("/data/states/mock/g.state9")
        );
        // Even a hostile game name stays one component below the core folder.
        let p = sram_path(base, "mock", "..");
        assert_eq!(p.parent(), Some(Path::new("/data/saves/mock")));
    }

    fn header() -> StateHeader {
        StateHeader {
            core_id: "mock".into(),
            core_version: "1.0 (abc)".into(),
            game: "Sonic = (World)".into(),
            created_unix: 1_763_500_000,
            game_fp: None,
        }
    }

    #[test]
    fn state_round_trip() {
        let payload: Vec<u8> = (0..=255).collect();
        let bytes = encode_state(&header(), &payload);
        assert_eq!(&bytes[..8], &STATE_MAGIC);
        let (h, p) = decode_state(&bytes).unwrap();
        assert_eq!(h, header());
        assert_eq!(p, payload.as_slice());

        let empty = encode_state(&header(), &[]);
        assert_eq!(decode_state(&empty).unwrap().1, &[] as &[u8]);
    }

    #[test]
    fn game_fp_round_trip_and_2_0_states() {
        let mut h = header();
        h.game_fp = Some("737280:deadbeef".into());
        let bytes = encode_state(&h, b"p");
        assert_eq!(decode_state(&bytes).unwrap().0, h);
        let header_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert_eq!(read_state_header(&bytes[..16 + header_len]).unwrap(), h);
        // A 1.0 / 2.0 state (no `game_fp` line) is unchanged byte for byte and reads back as `None`.
        let old = encode_state(&header(), b"p");
        assert!(!old.windows(7).any(|w| w == b"game_fp"));
        assert_eq!(decode_state(&old).unwrap().0.game_fp, None);
    }

    /// Frozen bytes of a 2.1 state: the `game_fp` line is appended after the four 2.0 keys, so a 2.0 reader (which
    /// skips unknown keys) loads it. Changing this layout breaks loading 2.1 states in 2.0.
    #[test]
    fn a_2_1_header_is_frozen_and_readable_by_2_0() {
        let h = StateHeader {
            core_id: "hatari".into(),
            core_version: "1.4".into(),
            game: "G (2)".into(),
            created_unix: 1_763_700_000,
            game_fp: Some("737280:0a1b2c3d".into()),
        };
        let bytes = encode_state(&h, &[0xAA, 0xBB]);
        let text = b"core_id=hatari\ncore_version=1.4\ngame=G (2)\ncreated_unix=1763700000\ngame_fp=737280:0a1b2c3d\n";
        let mut expected = b"EMURSTAT".to_vec();
        expected.extend_from_slice(&[1, 0, 0, 0]);
        expected.extend_from_slice(&[text.len() as u8, 0, 0, 0]);
        expected.extend_from_slice(text);
        expected.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0, 0, 0xAA, 0xBB]);
        expected.extend_from_slice(&fnv1a64(&[0xAA, 0xBB]).to_le_bytes());
        assert_eq!(bytes, expected);
        // What a 2.0 reader sees: an unknown key of the same length, ignored.
        let mut as_2_0 = bytes.clone();
        let at = as_2_0.windows(7).position(|w| w == b"game_fp").unwrap();
        as_2_0[at..at + 7].copy_from_slice(b"unknown");
        let (decoded, payload) = decode_state(&as_2_0).unwrap();
        assert_eq!((decoded.game.as_str(), decoded.game_fp), ("G (2)", None));
        assert_eq!(payload, [0xAA, 0xBB]);
    }

    #[test]
    fn header_values_are_sanitised() {
        let mut h = header();
        h.core_version = "1.0\ncore_id=evil\r".into();
        h.game = "x".repeat(5000);
        let (decoded, _) = decode_state(&encode_state(&h, b"p")).unwrap();
        assert_eq!(decoded.core_id, "mock");
        assert_eq!(decoded.core_version, "1.0 core_id=evil ");
        assert_eq!(decoded.game.len(), MAX_VALUE_CHARS);
    }

    #[test]
    fn every_truncation_is_rejected() {
        let bytes = encode_state(&header(), b"some payload bytes");
        for len in 0..bytes.len() {
            let err = decode_state(&bytes[..len]).unwrap_err();
            assert_eq!(err, StateFileError::Truncated, "prefix {len}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(decode_state(&longer), Err(StateFileError::Checksum));
    }

    #[test]
    fn every_byte_flip_is_rejected_or_harmless() {
        let bytes = encode_state(&header(), b"payload");
        for i in 0..bytes.len() {
            let mut bad = bytes.clone();
            bad[i] ^= 0x41;
            // Header bytes are not checksummed: a flip there may still parse, but never panics and never
            // yields the original payload with a corrupt checksum.
            if let Ok((h, p)) = decode_state(&bad) {
                assert_eq!(p, b"payload");
                assert_ne!(h, header(), "flip at {i}");
            }
        }
    }

    #[test]
    fn specific_rejections() {
        let good = encode_state(&header(), b"abc");
        let mut bad = good.clone();
        bad[0] = b'X';
        assert_eq!(decode_state(&bad), Err(StateFileError::BadMagic));
        assert_eq!(decode_state(b"RASTATE\0"), Err(StateFileError::BadMagic));

        let mut bad = good.clone();
        bad[8..12].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(
            decode_state(&bad),
            Err(StateFileError::UnsupportedVersion(2))
        );

        let mut bad = good.clone();
        let last = bad.len() - 9;
        bad[last] ^= 1; // last payload byte
        assert_eq!(decode_state(&bad), Err(StateFileError::Checksum));

        let mut bad = good.clone();
        bad[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode_state(&bad), Err(StateFileError::BadHeader));

        // Payload length pointing far beyond the end.
        let header_len = u32::from_le_bytes(good[12..16].try_into().unwrap()) as usize;
        let at = 16 + header_len;
        let mut bad = good.clone();
        bad[at..at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(decode_state(&bad), Err(StateFileError::Truncated));
    }

    #[test]
    fn malformed_headers() {
        let build = |text: &str| {
            let mut out = Vec::new();
            out.extend_from_slice(&STATE_MAGIC);
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&(text.len() as u32).to_le_bytes());
            out.extend_from_slice(text.as_bytes());
            out.extend_from_slice(&0u64.to_le_bytes());
            out.extend_from_slice(&fnv1a64(&[]).to_le_bytes());
            out
        };
        let full = "core_id=a\ncore_version=b\ngame=c\ncreated_unix=1\n";
        assert!(decode_state(&build(full)).is_ok());
        let unknown = format!("{full}future_key=zzz\n");
        assert!(decode_state(&build(&unknown)).is_ok());
        for text in [
            "core_id=a\ncore_version=b\ngame=c\n",
            "core_id=a\ncore_version=b\ngame=c\ncreated_unix=-1\n",
            "core_id=a\ncore_id=b\ncore_version=b\ngame=c\ncreated_unix=1\n",
            "core_id=a\nno equals sign\ncore_version=b\ngame=c\ncreated_unix=1\n",
        ] {
            assert_eq!(
                decode_state(&build(text)),
                Err(StateFileError::BadHeader),
                "{text:?}"
            );
        }
        let mut invalid_utf8 = build(full);
        invalid_utf8[16] = 0xff;
        assert_eq!(decode_state(&invalid_utf8), Err(StateFileError::BadHeader));
    }
}
