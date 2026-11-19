//! Persistence helpers: atomic writes, save layout, savestate container (ADR 0011).

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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

    let file = match create_new(&tmp, mode) {
        // Leftover of a crashed run that had the same pid (common in containers): replace it. `remove_file`
        // removes a symlink itself, never its target, and `create_new` refuses to follow one.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(&tmp)?;
            create_new(&tmp, mode)?
        }
        other => other?,
    };
    let written = write_and_sync(file, data).and_then(|()| fs::rename(&tmp, path));
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

fn write_and_sync(mut file: File, data: &[u8]) -> io::Result<()> {
    file.write_all(data)?;
    file.sync_all()
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
    if id.is_empty() { "core".to_owned() } else { id }
}

/// Game name used for save files: file stem of the content (the archive for a zip), with `/`, `\`, NUL and
/// control characters replaced by `_`. `Sonic (World).zip` → `Sonic (World)`.
pub fn game_name_from_path(rom: &Path) -> String {
    let stem = rom
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let name: String = stem
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Never empty, so `<game>.srm` is not a hidden file. Names are always joined with an extension, so even
    // `..` stays a single path component.
    if name.is_empty() {
        "game".to_owned()
    } else {
        name
    }
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
/// `core_version`, `game`, `created_unix`; unknown keys ignored) | `u64` payload length | payload |
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
    let text = format!(
        "core_id={}\ncore_version={}\ngame={}\ncreated_unix={}\n",
        clean(&header.core_id),
        clean(&header.core_version),
        clean(&header.game),
        header.created_unix
    );
    // At most 4 × MAX_VALUE_CHARS × 4 bytes + keys, well below `MAX_HEADER_LEN`.
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
    let (mut core_id, mut core_version, mut game, mut created) = (None, None, None, None);
    for line in text.split('\n').filter(|l| !l.is_empty()) {
        let (key, value) = line.split_once('=').ok_or(StateFileError::BadHeader)?;
        let slot = match key {
            "core_id" => &mut core_id,
            "core_version" => &mut core_version,
            "game" => &mut game,
            "created_unix" => &mut created,
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
        assert_eq!(name("a\\b\u{1}c.bin"), "a_b_c");
        assert_eq!(name(""), "game");
        assert_eq!(name("/"), "game");
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
