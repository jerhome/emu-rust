//! Content preparation for `retro_load_game`: plain files pass through, `.zip` archives are opened as untrusted
//! input and one entry is handed to the core in memory or through a private temporary file.

use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use emu_libretro::{GameSource, SystemInfo};
use tempfile::TempDir;
use zip::ZipArchive;

use crate::emulator::MAX_CONTENT_SIZE;
use crate::error::RuntimeError;

/// Archives listing more entries are refused before any entry is inspected.
pub(crate) const MAX_ARCHIVE_ENTRIES: usize = 4096;

/// Upper bound of the up-front allocation for an in-memory entry; the declared size is attacker-controlled,
/// so larger entries grow the buffer as bytes actually arrive.
const MAX_INITIAL_CAPACITY: u64 = 16 * 1024 * 1024;

/// What to load, plus the temporary directory backing it (must outlive the loaded game).
pub(crate) struct PreparedContent {
    pub source: GameSource,
    pub temp_dir: Option<TempDir>,
}

/// Limits applied to an archive; a parameter so tests can exercise them without 512 MiB files.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    /// Extracted entry (declared and actually read).
    pub max_size: u64,
    /// Archive file itself.
    pub max_archive_size: u64,
    pub max_entries: usize,
}

impl Limits {
    pub const DEFAULT: Self = Self {
        max_size: MAX_CONTENT_SIZE,
        // Twice the content limit so a stored (uncompressed) entry right at the limit, plus headers, still fits.
        max_archive_size: 2 * MAX_CONTENT_SIZE,
        max_entries: MAX_ARCHIVE_ENTRIES,
    };
}

/// See the contract on `Emulator::load`.
pub(crate) fn prepare(
    rom: &Path,
    info: &SystemInfo,
    limits: Limits,
) -> Result<PreparedContent, RuntimeError> {
    let passthrough = PreparedContent {
        source: GameSource::Path(rom.to_path_buf()),
        temp_dir: None,
    };
    let is_zip = rom
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    if !is_zip || info.block_extract || info.valid_extensions.iter().any(|e| e == "zip") {
        return Ok(passthrough);
    }
    extract(rom, info, limits)
}

fn archive_error(path: &Path, message: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::Archive {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

fn io_error(path: &Path, source: io::Error) -> RuntimeError {
    RuntimeError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn extract(rom: &Path, info: &SystemInfo, limits: Limits) -> Result<PreparedContent, RuntimeError> {
    let file = File::open(rom).map_err(|e| io_error(rom, e))?;
    let archive_size = file.metadata().map_err(|e| io_error(rom, e))?.len();
    // The file size alone does not bound the central directory that `ZipArchive::new` parses into memory
    // (a small archive can announce millions of entries), so the declared count is checked first.
    if archive_size > limits.max_archive_size {
        return Err(RuntimeError::ContentTooLarge {
            path: rom.to_path_buf(),
            size: archive_size,
        });
    }
    let mut file = file;
    match declared_entries(&mut file, archive_size) {
        Ok(Some(n)) if n > limits.max_entries as u64 => {
            return Err(archive_error(rom, format!("too many entries ({n})")));
        }
        Ok(_) => {}
        Err(e) => return Err(io_error(rom, e)),
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|e| io_error(rom, e))?;
    let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|e| archive_error(rom, e))?;
    if archive.len() > limits.max_entries {
        return Err(archive_error(
            rom,
            format!("too many entries ({})", archive.len()),
        ));
    }

    let mut chosen = None;
    for index in 0..archive.len() {
        // Central-directory metadata only: no seek, no decompressor for entries that are only inspected.
        let entry = archive
            .by_index_data(index)
            .map_err(|e| archive_error(rom, e))?;
        if !entry.is_file() {
            continue;
        }
        let Some(name) = entry_file_name(entry.name_raw()) else {
            continue;
        };
        if accepts(info, &name) {
            chosen = Some((index, name, entry.size()));
            break;
        }
    }
    let Some((index, name, declared)) = chosen else {
        return Err(RuntimeError::NoContentInArchive(rom.to_path_buf()));
    };
    if declared > limits.max_size {
        return Err(RuntimeError::ContentTooLarge {
            path: rom.to_path_buf(),
            size: declared,
        });
    }
    let entry = archive
        .by_index(index)
        .map_err(|e| archive_error(rom, format!("entry `{name}`: {e}")))?;
    // One byte past the limit so an entry lying about its size is detected rather than silently cut.
    let mut reader = entry.take(limits.max_size + 1);
    let too_large = |size: u64| RuntimeError::ContentTooLarge {
        path: rom.to_path_buf(),
        size,
    };
    let read_error = |e: io::Error| archive_error(rom, format!("entry `{name}`: {e}"));

    if info.need_fullpath {
        let mut builder = tempfile::Builder::new();
        builder.prefix("emu-rust-content-");
        // Owner-only from creation: the shared temp dir is world-writable.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let dir = builder
            .tempdir()
            .map_err(|e| io_error(&std::env::temp_dir(), e))?;
        let path = dir.path().join(&name);
        let mut out = create_private(&path).map_err(|e| io_error(&path, e))?;
        let written = copy(&mut reader, &mut out).map_err(read_error)?;
        if written > limits.max_size {
            return Err(too_large(written));
        }
        out.sync_all().map_err(|e| io_error(&path, e))?;
        tracing::debug!(entry = %name, bytes = written, "content extracted to a temporary file");
        Ok(PreparedContent {
            source: GameSource::Path(path),
            temp_dir: Some(dir),
        })
    } else {
        let capacity = declared.min(MAX_INITIAL_CAPACITY) as usize;
        let mut data = Vec::with_capacity(capacity);
        reader.read_to_end(&mut data).map_err(read_error)?;
        if data.len() as u64 > limits.max_size {
            return Err(too_large(data.len() as u64));
        }
        let virtual_path = rom.parent().unwrap_or(Path::new("")).join(&name);
        tracing::debug!(entry = %name, bytes = data.len(), "content extracted in memory");
        Ok(PreparedContent {
            source: GameSource::Memory {
                path: virtual_path,
                data,
            },
            temp_dir: None,
        })
    }
}

/// Total entry count announced by the end-of-central-directory record (ZIP64 record if the 16-bit field is
/// saturated). `None` when no record is found: `ZipArchive::new` then reports the proper error.
fn declared_entries(file: &mut File, size: u64) -> io::Result<Option<u64>> {
    const EOCD_LEN: u64 = 22;
    const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const LOCATOR_LEN: u64 = 20;
    // EOCD + maximum comment length.
    let window = size.min(EOCD_LEN + 65535);
    let start = size - window;
    file.seek(SeekFrom::Start(start))?;
    let mut tail = vec![0u8; window as usize];
    file.read_exact(&mut tail)?;
    let Some(pos) = tail
        .windows(EOCD_SIG.len())
        .rposition(|w| w == EOCD_SIG)
        .filter(|&p| p + EOCD_LEN as usize <= tail.len())
    else {
        return Ok(None);
    };
    let total = u64::from(u16::from_le_bytes([tail[pos + 10], tail[pos + 11]]));
    if total != 0xFFFF {
        return Ok(Some(total));
    }
    // ZIP64: the locator sits right before the EOCD and points at the ZIP64 EOCD record.
    let eocd_at = start + pos as u64;
    if eocd_at < LOCATOR_LEN {
        return Ok(Some(total));
    }
    let mut locator = [0u8; LOCATOR_LEN as usize];
    file.seek(SeekFrom::Start(eocd_at - LOCATOR_LEN))?;
    file.read_exact(&mut locator)?;
    if locator[..4] != [0x50, 0x4b, 0x06, 0x07] {
        return Ok(Some(total));
    }
    let mut offset = [0u8; 8];
    offset.copy_from_slice(&locator[8..16]);
    let offset = u64::from_le_bytes(offset);
    let mut record = [0u8; 56];
    if offset
        .checked_add(record.len() as u64)
        .is_none_or(|end| end > size)
    {
        return Ok(Some(total));
    }
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut record)?;
    if record[..4] != [0x50, 0x4b, 0x06, 0x06] {
        return Ok(Some(total));
    }
    let mut n = [0u8; 8];
    n.copy_from_slice(&record[32..40]);
    Ok(Some(u64::from_le_bytes(n)))
}

fn copy(reader: &mut impl Read, out: &mut File) -> io::Result<u64> {
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(total),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        out.write_all(&buf[..n])?;
        total += n as u64;
    }
}

/// `create_new` (never follows or reuses an existing path), `0600` on Unix.
fn create_private(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Last component of an entry name (`/` or `\` separated), made safe as a single file name on every target.
/// The entry's directories are never used: nothing can land outside the destination directory.
/// `None` for names that are empty, `.` or `..` once reduced.
pub(crate) fn entry_file_name(raw: &[u8]) -> Option<String> {
    let base = raw.rsplit(|&b| b == b'/' || b == b'\\').next()?;
    let base = String::from_utf8_lossy(base);
    let name: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Windows silently strips trailing dots and spaces, which could turn `x.bin.` into another name.
    let name = name.trim_end_matches(['.', ' ']);
    if name.is_empty() || name.len() > 200 {
        return None;
    }
    Some(name.to_owned())
}

fn accepts(info: &SystemInfo, name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    info.valid_extensions
        .iter()
        .any(|e| e.eq_ignore_ascii_case(ext))
}

/// Removes the temporary directory now, logging instead of failing (called from `Drop`).
pub(crate) fn remove_temp_dir(dir: TempDir) {
    let path = dir.path().to_path_buf();
    if let Err(e) = dir.close() {
        tracing::warn!("cannot remove temporary content {}: {e}", path.display());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn info(exts: &[&str], need_fullpath: bool) -> SystemInfo {
        SystemInfo {
            library_name: "t".into(),
            library_version: "1".into(),
            valid_extensions: exts.iter().map(|s| (*s).to_owned()).collect(),
            need_fullpath,
            block_extract: false,
        }
    }

    /// Minimal stored-only zip writer giving full control over names and declared sizes (malicious archives).
    pub(crate) fn build_zip(entries: &[(&[u8], &[u8], Option<u32>)]) -> Vec<u8> {
        fn crc32(data: &[u8]) -> u32 {
            let mut crc = !0u32;
            for &b in data {
                crc ^= u32::from(b);
                for _ in 0..8 {
                    crc = if crc & 1 != 0 {
                        (crc >> 1) ^ 0xEDB8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data, declared) in entries {
            let offset = out.len() as u32;
            let crc = crc32(data);
            let size = declared.unwrap_or(data.len() as u32);
            let mut common = Vec::new();
            common.extend_from_slice(&20u16.to_le_bytes()); // version needed
            common.extend_from_slice(&0u16.to_le_bytes()); // flags
            common.extend_from_slice(&0u16.to_le_bytes()); // stored
            common.extend_from_slice(&0u32.to_le_bytes()); // time + date
            common.extend_from_slice(&crc.to_le_bytes());
            common.extend_from_slice(&size.to_le_bytes()); // compressed
            common.extend_from_slice(&size.to_le_bytes()); // uncompressed
            common.extend_from_slice(&(name.len() as u16).to_le_bytes());
            common.extend_from_slice(&0u16.to_le_bytes()); // extra length

            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&common);
            out.extend_from_slice(name);
            out.extend_from_slice(data);

            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes()); // version made by
            central.extend_from_slice(&common);
            central.extend_from_slice(&0u16.to_le_bytes()); // comment length
            central.extend_from_slice(&0u16.to_le_bytes()); // disk
            central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name);
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn write_zip(dir: &Path, name: &str, entries: &[(&[u8], &[u8], Option<u32>)]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, build_zip(entries)).unwrap();
        path
    }

    #[test]
    fn file_names() {
        let f = |s: &[u8]| entry_file_name(s);
        assert_eq!(f(b"game.bin").as_deref(), Some("game.bin"));
        assert_eq!(f(b"dir/sub/game.md").as_deref(), Some("game.md"));
        assert_eq!(f(b"../../evil.bin").as_deref(), Some("evil.bin"));
        assert_eq!(f(b"/etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(f(b"C:\\Windows\\x.bin").as_deref(), Some("x.bin"));
        assert_eq!(f(b"a:b\x01.bin").as_deref(), Some("a_b_.bin"));
        assert_eq!(f(b"x.bin. ").as_deref(), Some("x.bin"));
        assert_eq!(f(b"dir/"), None);
        assert_eq!(f(b".."), None);
        assert_eq!(f(b"."), None);
        assert_eq!(f(b""), None);
        assert_eq!(f(&[b'a'; 300]), None);
    }

    #[test]
    fn plain_files_and_zip_aware_cores_pass_through() {
        let rom = Path::new("/roms/game.bin");
        let p = prepare(rom, &info(&["bin"], false), Limits::DEFAULT).unwrap();
        assert_eq!(p.source, GameSource::Path(rom.into()));

        // Never opened: the path does not exist.
        let zip = Path::new("/nonexistent/game.ZIP");
        let p = prepare(zip, &info(&["bin", "zip"], false), Limits::DEFAULT).unwrap();
        assert_eq!(p.source, GameSource::Path(zip.into()));
        let mut blocking = info(&["bin"], false);
        blocking.block_extract = true;
        let p = prepare(zip, &blocking, Limits::DEFAULT).unwrap();
        assert_eq!(p.source, GameSource::Path(zip.into()));
    }

    #[test]
    fn first_accepted_entry_in_memory() {
        let tmp = tempfile::tempdir().unwrap();
        let zip = write_zip(
            tmp.path(),
            "g.zip",
            &[
                (b"readme.txt", b"hello", None),
                (b"dir/", b"", None),
                (b"roms/../../Game.MD", b"first", None),
                (b"other.bin", b"second", None),
            ],
        );
        let p = prepare(&zip, &info(&["bin", "md"], false), Limits::DEFAULT).unwrap();
        assert!(p.temp_dir.is_none());
        assert_eq!(
            p.source,
            GameSource::Memory {
                path: tmp.path().join("Game.MD"),
                data: b"first".to_vec()
            }
        );
    }

    #[test]
    fn need_fullpath_uses_private_temp_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let zip = write_zip(
            tmp.path(),
            "g.zip",
            &[(b"../../evil.bin", b"payload", None)],
        );
        let p = prepare(&zip, &info(&["bin"], true), Limits::DEFAULT).unwrap();
        let dir = p.temp_dir.expect("temp dir");
        let GameSource::Path(path) = &p.source else {
            panic!("expected a path: {:?}", p.source);
        };
        assert_eq!(path, &dir.path().join("evil.bin"));
        assert_eq!(fs::read(path).unwrap(), b"payload");
        assert!(!tmp.path().parent().unwrap().join("evil.bin").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(path), 0o600);
            assert_eq!(mode(dir.path()), 0o700);
        }
        let dir_path = dir.path().to_path_buf();
        remove_temp_dir(dir);
        assert!(!dir_path.exists());
    }

    #[test]
    fn refusals() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = info(&["bin"], false);

        let zip = write_zip(tmp.path(), "none.zip", &[(b"a.txt", b"x", None)]);
        assert!(matches!(
            prepare(&zip, &bin, Limits::DEFAULT),
            Err(RuntimeError::NoContentInArchive(p)) if p == zip
        ));
        let zip = write_zip(tmp.path(), "empty.zip", &[]);
        assert!(matches!(
            prepare(&zip, &bin, Limits::DEFAULT),
            Err(RuntimeError::NoContentInArchive(_))
        ));

        let garbage = tmp.path().join("garbage.zip");
        fs::write(&garbage, b"not a zip at all").unwrap();
        assert!(matches!(
            prepare(&garbage, &bin, Limits::DEFAULT),
            Err(RuntimeError::Archive { .. })
        ));

        // Declared size over the limit: refused before reading.
        let zip = write_zip(
            tmp.path(),
            "declared.zip",
            &[(b"big.bin", b"tiny", Some(u32::MAX))],
        );
        assert!(matches!(
            prepare(&zip, &bin, Limits::DEFAULT),
            Err(RuntimeError::ContentTooLarge { size, .. }) if size == u64::from(u32::MAX)
        ));

        let small = Limits {
            max_size: 8,
            max_archive_size: 4096,
            max_entries: 2,
        };
        let zip = write_zip(tmp.path(), "nine.zip", &[(b"a.bin", b"123456789", None)]);
        for fullpath in [false, true] {
            let at_limit = Limits {
                max_size: 9,
                ..small
            };
            assert!(prepare(&zip, &info(&["bin"], fullpath), at_limit).is_ok());
            assert!(matches!(
                prepare(&zip, &info(&["bin"], fullpath), small),
                Err(RuntimeError::ContentTooLarge { .. })
            ));
        }
        let zip = write_zip(
            tmp.path(),
            "many.zip",
            &[
                (b"a.txt", b"", None),
                (b"b.txt", b"", None),
                (b"c.bin", b"", None),
            ],
        );
        assert!(matches!(
            prepare(&zip, &bin, Limits { max_size: 1024, ..small }),
            Err(RuntimeError::Archive { message, .. }) if message.contains("too many")
        ));
        let tiny_archive = Limits {
            max_archive_size: 64,
            ..small
        };
        assert!(matches!(
            prepare(&zip, &bin, tiny_archive),
            Err(RuntimeError::ContentTooLarge { path, .. }) if path == zip
        ));
    }

    #[test]
    fn forged_entry_count_is_refused_before_parsing() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = info(&["bin"], false);
        let mut bytes = build_zip(&[(b"a.bin", b"x", None)]);
        // Patch both 16-bit counts of the EOCD (last 22 bytes) to 60000.
        let eocd = bytes.len() - 22;
        bytes[eocd + 8..eocd + 12].copy_from_slice(&[0x60, 0xEA, 0x60, 0xEA]);
        let path = tmp.path().join("forged.zip");
        fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            prepare(&path, &bin, Limits::DEFAULT),
            Err(RuntimeError::Archive { message, .. }) if message.contains("too many entries (60000)")
        ));

        // ZIP64: saturated 16-bit count, real count in the ZIP64 record.
        let mut z = build_zip(&[(b"a.bin", b"x", None)]);
        let eocd = z.len() - 22;
        let record_at = eocd as u64;
        let mut z64 = vec![0x50, 0x4b, 0x06, 0x06];
        z64.extend_from_slice(&44u64.to_le_bytes());
        z64.extend_from_slice(&[45, 0, 45, 0]);
        z64.extend_from_slice(&[0; 8]); // disk numbers
        z64.extend_from_slice(&1_000_000u64.to_le_bytes());
        z64.extend_from_slice(&1_000_000u64.to_le_bytes());
        z64.extend_from_slice(&[0; 16]); // cd size + offset
        let mut locator = vec![0x50, 0x4b, 0x06, 0x07, 0, 0, 0, 0];
        locator.extend_from_slice(&record_at.to_le_bytes());
        locator.extend_from_slice(&1u32.to_le_bytes());
        let mut tail = z.split_off(eocd);
        tail[8..12].copy_from_slice(&[0xFF; 4]);
        z.extend_from_slice(&z64);
        z.extend_from_slice(&locator);
        z.extend_from_slice(&tail);
        let path = tmp.path().join("forged64.zip");
        fs::write(&path, &z).unwrap();
        assert!(matches!(
            prepare(&path, &bin, Limits::DEFAULT),
            Err(RuntimeError::Archive { message, .. }) if message.contains("too many entries (1000000)")
        ));
    }

    #[test]
    fn lying_declared_size_is_caught() {
        let tmp = tempfile::tempdir().unwrap();
        // Declares 4 bytes while 64 are stored: the reader stops at the declared size, and the CRC (over all
        // 64 bytes) then fails; either way nothing beyond the limit is accepted.
        let data = [7u8; 64];
        let zip = write_zip(tmp.path(), "lie.zip", &[(b"a.bin", &data, Some(4))]);
        let limits = Limits {
            max_size: 16,
            max_archive_size: 4096,
            max_entries: 8,
        };
        match prepare(&zip, &info(&["bin"], false), limits) {
            Err(RuntimeError::Archive { .. } | RuntimeError::ContentTooLarge { .. }) => {}
            Ok(p) => match p.source {
                GameSource::Memory { data, .. } => assert!(data.len() <= 16),
                other => panic!("unexpected {other:?}"),
            },
            Err(e) => panic!("unexpected error {e}"),
        }
    }
}
