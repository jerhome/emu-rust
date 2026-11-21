//! Disc images (design v2.1 Mega CD express §3.2): `.cue` parsing and Sega CD header probe. Every input here is
//! untrusted: reads are bounded, nothing is followed outside the `.cue` folder, nothing panics.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::LibraryError;

/// Region of a disc: BIOS choice. Generic (Mega CD now; other disc systems later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiscRegion {
    Europe,
    Usa,
    Japan,
}

impl DiscRegion {
    /// Stable key (`"europe"`, `"usa"`, `"japan"`): variant key of the 6.0 firmware tables.
    pub fn key(self) -> &'static str {
        match self {
            DiscRegion::Europe => "europe",
            DiscRegion::Usa => "usa",
            DiscRegion::Japan => "japan",
        }
    }

    /// Inverse of [`DiscRegion::key`] (column `game.region`, schema v6); `None` for anything else.
    pub fn from_key(key: &str) -> Option<Self> {
        [DiscRegion::Europe, DiscRegion::Usa, DiscRegion::Japan]
            .into_iter()
            .find(|r| r.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscFormat {
    SegaCd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscInfo {
    pub format: DiscFormat,
    /// `None`: header found but the file is too short to hold the region byte.
    pub region: Option<DiscRegion>,
}

/// Extensions of disc images handed to cores in place (never hashed, never copied).
pub const DISC_EXTENSIONS: &[&str] = &["cue", "chd", "iso"];

/// Largest `.cue` read: a real one is a few hundred bytes.
const MAX_CUE_SIZE: u64 = 64 * 1024;
/// A CD holds at most 99 tracks.
const MAX_CUE_REFERENCES: usize = 99;
const SEGA_MAGIC: &[u8] = b"SEGADISCSYSTEM";
/// Offset of the user data in a raw 2352-byte sector (12 sync bytes + 4 header bytes).
const RAW_DATA_OFFSET: usize = 0x10;
/// Byte genesis_plus_gx reads to pick the CD BIOS (design C2): 0x64 Europe, 0xA1 Japan, anything else USA.
const REGION_BYTE: usize = 0x20B;
/// Enough for the region byte behind a raw sector header (0x10 + 0x20C), well under the 0x220 bound.
const PROBE_LEN: usize = RAW_DATA_OFFSET + REGION_BYTE + 1;
/// Mega Drive style header inside sector 0: type + serial field (`GM T-93175 -00`), 14 bytes.
const SERIAL_OFFSET: usize = 0x180;
const SERIAL_LEN: usize = 14;
/// 32X cartridge signature at 0x100 (`SEGA 32X`).
const SEGA_32X_MAGIC: &[u8] = b"SEGA 32X";
const CARTRIDGE_MAGIC_OFFSET: usize = 0x100;

fn io_error(path: &Path, source: io::Error) -> LibraryError {
    LibraryError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Opens a regular file. On Unix the open is non-blocking, so a FIFO never hangs the caller; it is then refused.
fn open_regular(path: &Path) -> Result<File, LibraryError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|e| io_error(path, e))?;
    let meta = file.metadata().map_err(|e| io_error(path, e))?;
    if !meta.is_file() {
        return Err(io_error(
            path,
            io::Error::new(io::ErrorKind::InvalidInput, "pas un fichier ordinaire"),
        ));
    }
    Ok(file)
}

fn ext_of(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_ascii_lowercase())
}

/// Text of a `.cue`: UTF-8 (BOM dropped), else Latin-1 (old cue sheets are not always UTF-8).
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// File name of a `FILE` line (`FILE "name" TYPE` or `FILE name TYPE`); `None` for anything else.
fn file_line_name(line: &str) -> Option<&str> {
    let line = line.trim();
    let keyword = line.get(..4)?;
    if !keyword.eq_ignore_ascii_case("FILE") {
        return None;
    }
    let rest = &line[4..];
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let rest = rest.trim();
    if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.find('"')?;
        return Some(&quoted[..end]);
    }
    // Unquoted: everything up to the last word (the type); a lone word is taken as the name.
    match rest.rfind(char::is_whitespace) {
        Some(i) => Some(rest[..i].trim_end()),
        None => Some(rest),
    }
}

/// A plain file name of the cue's folder: no separator, no drive or NTFS stream (`:`), not `.`/`..`, no control
/// character.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
        && !Path::new(name).has_root()
        && !Path::new(name).is_absolute()
}

/// Names of the `FILE` lines of a cue sheet, in order, without duplicates, at most 99; unsafe names skipped.
fn parse_cue(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some(name) = file_line_name(line) else {
            continue;
        };
        if !is_plain_name(name) {
            tracing::debug!("cue FILE entry ignored (not a plain file name)");
            continue;
        }
        if !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
            if names.len() == MAX_CUE_REFERENCES {
                break;
            }
        }
    }
    names
}

/// Files named by the `FILE` lines of a `.cue`, resolved in the cue's folder, in order, without duplicates.
pub fn cue_references(cue: &Path) -> Result<Vec<PathBuf>, LibraryError> {
    let file = open_regular(cue)?;
    let mut bytes = Vec::new();
    file.take(MAX_CUE_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error(cue, e))?;
    if bytes.len() as u64 > MAX_CUE_SIZE {
        return Err(LibraryError::TooLarge {
            path: cue.to_path_buf(),
        });
    }
    let dir = cue.parent().unwrap_or(Path::new(""));
    Ok(parse_cue(&decode(&bytes))
        .into_iter()
        .map(|name| dir.join(name))
        .collect())
}

/// Reads up to `PROBE_LEN` bytes from the start of `path`.
fn read_head(path: &Path) -> Result<Vec<u8>, LibraryError> {
    let file = open_regular(path)?;
    let mut head = Vec::with_capacity(PROBE_LEN);
    file.take(PROBE_LEN as u64)
        .read_to_end(&mut head)
        .map_err(|e| io_error(path, e))?;
    Ok(head)
}

fn region_of_byte(byte: u8) -> DiscRegion {
    match byte {
        0x64 => DiscRegion::Europe,
        0xA1 => DiscRegion::Japan,
        _ => DiscRegion::Usa,
    }
}

/// Sega CD header at one of `offsets` of `head`.
fn probe_head(head: &[u8], offsets: &[usize]) -> Option<DiscInfo> {
    offsets.iter().find_map(|&off| {
        let magic = head.get(off..off + SEGA_MAGIC.len())?;
        (magic == SEGA_MAGIC).then(|| DiscInfo {
            format: DiscFormat::SegaCd,
            region: head.get(off + REGION_BYTE).copied().map(region_of_byte),
        })
    })
}

/// A track of a cue is read only if it stays in the cue's folder: a symlink pointing elsewhere is not followed.
fn track_in_folder(cue: &Path, track: &Path) -> bool {
    match fs::symlink_metadata(track) {
        Ok(m) if m.file_type().is_symlink() => {
            let dir = cue.parent().unwrap_or(Path::new("."));
            let dir = if dir.as_os_str().is_empty() {
                Path::new(".")
            } else {
                dir
            };
            match (fs::canonicalize(dir), fs::canonicalize(track)) {
                (Ok(dir), Ok(target)) => target.parent() == Some(dir.as_path()),
                _ => false,
            }
        }
        _ => true,
    }
}

/// First file of a `.cue` (its data track for a Sega CD), kept only when it stays in the cue's folder; `Ok(None)`
/// for a cue without `FILE` line.
pub(crate) fn first_track(cue: &Path) -> Result<Option<PathBuf>, LibraryError> {
    Ok(cue_references(cue)?
        .into_iter()
        .next()
        .filter(|track| track_in_folder(cue, track)))
}

/// First bytes of a data track, and the offsets where its header may start.
type Head = (Vec<u8>, &'static [usize]);

/// First bytes of the data track of a disc and the offsets where its header may start: a `.iso` (0), a `.bin`
/// track or the first track of a `.cue` (0 or 0x10). `Ok(None)`: no readable track (or a `.chd`, never read).
fn disc_head(path: &Path) -> Result<Option<Head>, LibraryError> {
    const ISO: &[usize] = &[0];
    const TRACK: &[usize] = &[0, RAW_DATA_OFFSET];
    let Some(ext) = ext_of(path) else {
        return Ok(None);
    };
    match ext.as_str() {
        "iso" => Ok(Some((read_head(path)?, ISO))),
        "bin" => Ok(Some((read_head(path)?, TRACK))),
        "cue" => {
            let Some(track) = first_track(path)? else {
                return Ok(None);
            };
            match read_head(&track) {
                Ok(head) => Ok(Some((head, TRACK))),
                // A cue whose first track is missing is not a disc we can recognise.
                Err(LibraryError::Io { source, .. })
                    if source.kind() == io::ErrorKind::NotFound =>
                {
                    Ok(None)
                }
                Err(e) => Err(e),
            }
        }
        _ => Ok(None),
    }
}

/// Header probe of a `.cue` (its first data track), a `.iso` or a raw `.bin` track. `Ok(None)`: not a known disc
/// (or a `.chd`, never read). Reads at most 64 KiB of the cue and 0x220 bytes of the track.
pub fn disc_info(path: &Path) -> Result<Option<DiscInfo>, LibraryError> {
    Ok(disc_head(path)?.and_then(|(head, offsets)| probe_head(&head, offsets)))
}

/// Region tag of a file name (`Dune (Europe)` ⇒ Europe). Fallback when the header cannot be read.
pub fn region_from_name(stem: &str) -> Option<DiscRegion> {
    let mut rest = stem;
    while let Some(open) = rest.find('(') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(')') else {
            break;
        };
        for tag in after[..close].split(',') {
            let region = match tag.trim().to_ascii_lowercase().as_str() {
                "europe" | "france" | "germany" | "spain" | "italy" | "uk" | "australia" => {
                    Some(DiscRegion::Europe)
                }
                "usa" | "brazil" => Some(DiscRegion::Usa),
                "japan" | "korea" | "asia" => Some(DiscRegion::Japan),
                _ => None,
            };
            if region.is_some() {
                return region;
            }
        }
        rest = &after[close + 1..];
    }
    None
}

/// What a `.bin` holds, from its first bytes (design v6.0 §3.4; scan of `.bin` files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinKind {
    /// No other header: the 1.0 rule applies (Mega Drive ROM).
    MegaDrive,
    /// `SEGA 32X` at 0x100: system `32x`.
    Sega32x,
    /// `SEGADISCSYSTEM` at 0 or 0x10 ([`disc_info`]): a CD track, never a ROM (orphan ⇒ ignored, 2.1 rule).
    SegaCdTrack,
    /// Too short to hold a header, or unreadable as such.
    Unknown,
}

/// What the first bytes of a `.bin` say ([`probe_bin`]); pure, for the scanner's zip entries too.
pub(crate) fn bin_kind(head: &[u8]) -> BinKind {
    if probe_head(head, &[0, RAW_DATA_OFFSET]).is_some() {
        return BinKind::SegaCdTrack;
    }
    match head.get(CARTRIDGE_MAGIC_OFFSET..CARTRIDGE_MAGIC_OFFSET + SEGA_32X_MAGIC.len()) {
        Some(magic) if magic == SEGA_32X_MAGIC => BinKind::Sega32x,
        Some(_) => BinKind::MegaDrive,
        None => BinKind::Unknown,
    }
}

/// Bytes [`probe_bin`] reads (and the scanner reads of a zipped `.bin`).
pub(crate) const BIN_PROBE_LEN: usize = PROBE_LEN;

/// Header probe of a `.bin`: reads at most 0x220 bytes; `SegaCdTrack` as [`disc_info`] sees it, `Sega32x`
/// for `SEGA 32X` at 0x100, else `MegaDrive` (`Unknown` for a file too short to hold the 32X signature). Same
/// untrusted-input rules as the rest of this module (regular file only, bounded read, no panic).
pub fn probe_bin(path: &Path) -> Result<BinKind, LibraryError> {
    Ok(bin_kind(&read_head(path)?))
}

/// A header field without its type code (`GM `, `AI `), trimmed.
fn strip_type(text: &str) -> &str {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() > 3 && bytes[..2].iter().all(u8::is_ascii_uppercase) && bytes[2] == b' ' {
        text[3..].trim_start()
    } else {
        text
    }
}

/// A serial without its trailing revision (`T-93175 -00` ⇒ `T-93175`), trimmed.
fn strip_revision(text: &str) -> &str {
    match text.rsplit_once('-') {
        Some((serial, rev))
            if rev.len() == 2
                && rev.bytes().all(|b| b.is_ascii_digit())
                && !serial.trim().is_empty() =>
        {
            serial.trim()
        }
        _ => text.trim(),
    }
}

/// Serial of a header field: type code and trailing revision dropped. `None` when blank.
fn clean_serial(field: &str) -> Option<String> {
    let text = strip_revision(strip_type(field));
    (!text.is_empty()).then(|| text.to_owned())
}

/// Raw header field of a Sega CD (type code and revision kept, trimmed), for matching against the DAT.
pub(crate) fn header_serial_field(head: &[u8], offsets: &[usize]) -> Option<String> {
    let off = offsets.iter().copied().find(|&off| {
        head.get(off..off + SEGA_MAGIC.len())
            .is_some_and(|m| m == SEGA_MAGIC)
    })?;
    let field = head.get(off + SERIAL_OFFSET..off + SERIAL_OFFSET + SERIAL_LEN)?;
    // Space padding is the rule; NUL padding is tolerated.
    let end = field.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    let field = &field[..end];
    if !field.iter().all(|b| (0x20..0x7F).contains(b)) {
        return None;
    }
    let text = std::str::from_utf8(field).ok()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Header facts of a disc read once: region byte and serial field (scanner, < 0x220 bytes of track).
pub(crate) fn disc_header(path: &Path) -> Result<Option<(DiscInfo, Option<String>)>, LibraryError> {
    Ok(disc_head(path)?.and_then(|(head, offsets)| {
        probe_head(&head, offsets).map(|info| (info, header_serial_field(&head, offsets)))
    }))
}

/// Serial number in the disc header (generic: one probe per [`DiscFormat`]). `SegaCd`: MD-style header at 0x100
/// of sector 0, field at 0x180 (`GM T-93175 -00` ⇒ `T-93175`: type code and revision dropped, trimmed); of a
/// `.cue` (its first track, kept in the cue's folder), a `.iso` or a `.bin` track. `Ok(None)`: no header or blank
/// serial, and always for a `.chd` (never read). Bounded reads (64 KiB of cue, 0x220 bytes of track).
pub fn disc_serial(path: &Path, format: DiscFormat) -> Result<Option<String>, LibraryError> {
    match format {
        DiscFormat::SegaCd => Ok(disc_head(path)?
            .and_then(|(head, offsets)| header_serial_field(&head, offsets))
            .and_then(|field| clean_serial(&field))),
    }
}

/// Upper case, spaces removed: form compared between a disc and the DAT `serial` field.
pub(crate) fn normalize_serial(serial: &str) -> String {
    serial
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Comparison keys of a header field (`GM MK-4407-50`): the field, without its type code (`MK-4407-50`), and
/// without its revision too (`MK-4407`), normalized; a DAT listing any of these forms matches.
pub(crate) fn serial_keys(field: &str) -> Vec<String> {
    let untyped = strip_type(field);
    let mut keys = Vec::new();
    for k in [field, untyped, strip_revision(untyped)].map(normalize_serial) {
        if !k.is_empty() && !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sector 0 user data with the Sega CD header and `region` at 0x20B.
    fn sector0(region: u8) -> Vec<u8> {
        let mut s = vec![0u8; 2048];
        s[..16].copy_from_slice(b"SEGADISCSYSTEM  ");
        s[REGION_BYTE] = region;
        s
    }

    fn raw(region: u8) -> Vec<u8> {
        let mut s = vec![0x00u8];
        s.extend([0xFF; 10]);
        s.extend([0x00, 0x00, 0x02, 0x00, 0x01]);
        s.extend(sector0(region));
        s.resize(2352, 0);
        s
    }

    fn write(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, data).unwrap();
        p
    }

    #[test]
    fn region_keys() {
        assert_eq!(DiscRegion::Europe.key(), "europe");
        assert_eq!(DiscRegion::Usa.key(), "usa");
        assert_eq!(DiscRegion::Japan.key(), "japan");
        assert_eq!(DISC_EXTENSIONS, ["cue", "chd", "iso"]);
    }

    #[test]
    fn cue_file_lines() {
        let text = "\u{FEFF}REM x\r\nFILE \"Dune (Europe) (Track 01).bin\" BINARY\r\n  TRACK 01 MODE1/2352\r\n\
                    file Track 02 (a).wav WAVE\r\nFILE \"t3.ogg\" OGG\nFILE t4.mp3 MP3\nFILE \"t3.ogg\" OGG\n\
                    FILE lone.bin\nFILENAME x.bin BINARY\nFILE \"unterminated.bin BINARY\n";
        assert_eq!(
            parse_cue(&decode(text.as_bytes())),
            [
                "Dune (Europe) (Track 01).bin",
                "Track 02 (a).wav",
                "t3.ogg",
                "t4.mp3",
                "lone.bin"
            ]
        );
    }

    #[test]
    fn cue_latin1_and_bom() {
        let latin1 = b"FILE \"Jeu \xE9t\xE9.bin\" BINARY\n";
        assert_eq!(parse_cue(&decode(latin1)), ["Jeu été.bin"]);
        let utf8 = "\u{FEFF}FILE \"Jeu été.bin\" BINARY\n";
        assert_eq!(parse_cue(&decode(utf8.as_bytes())), ["Jeu été.bin"]);
    }

    #[test]
    fn cue_unsafe_names_are_ignored() {
        let text = "FILE \"../up.bin\" BINARY\nFILE \"..\" BINARY\nFILE \"/etc/passwd\" BINARY\n\
                    FILE \"sub/a.bin\" BINARY\nFILE \"sub\\a.bin\" BINARY\nFILE \"C:a.bin\" BINARY\n\
                    FILE \"C:\\a.bin\" BINARY\nFILE \"\" BINARY\nFILE \"a\x01.bin\" BINARY\nFILE \"a.bin:flux\" BINARY\nFILE \"ok..bin\" BINARY\n";
        assert_eq!(parse_cue(text), ["ok..bin"]);
    }

    #[test]
    fn cue_reference_limit() {
        let text: String = (0..150)
            .map(|i| format!("FILE \"t{i}.bin\" BINARY\n"))
            .collect();
        let names = parse_cue(&text);
        assert_eq!(names.len(), 99);
        assert_eq!(names[98], "t98.bin");
    }

    #[test]
    fn cue_references_resolve_in_folder_and_bound_size() {
        let tmp = tempfile::tempdir().unwrap();
        let cue = write(
            tmp.path(),
            "g.cue",
            b"FILE \"g.bin\" BINARY\nFILE \"missing.wav\" WAVE\n",
        );
        assert_eq!(
            cue_references(&cue).unwrap(),
            [tmp.path().join("g.bin"), tmp.path().join("missing.wav")]
        );
        let big = write(tmp.path(), "big.cue", &vec![b' '; 64 * 1024 + 1]);
        assert!(matches!(
            cue_references(&big),
            Err(LibraryError::TooLarge { .. })
        ));
        let exact = write(tmp.path(), "exact.cue", &vec![b'\n'; 64 * 1024]);
        assert!(cue_references(&exact).unwrap().is_empty());
        assert!(matches!(
            cue_references(&tmp.path().join("none.cue")),
            Err(LibraryError::Io { .. })
        ));
        assert!(cue_references(tmp.path()).is_err());
        // Binary garbage: no panic.
        let junk: Vec<u8> = (0..4096u32).map(|i| (i * 7919 % 251) as u8).collect();
        let junk = write(tmp.path(), "junk.cue", &junk);
        let _ = cue_references(&junk).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn fifo_is_never_opened() {
        let tmp = tempfile::tempdir().unwrap();
        let fifo = tmp.path().join("pipe.cue");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .is_ok_and(|s| s.success());
        if !made {
            eprintln!("mkfifo unavailable: skipped");
            return;
        }
        assert!(cue_references(&fifo).is_err());
        let iso = tmp.path().join("pipe.iso");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&iso)
                .status()
                .unwrap()
                .success()
        );
        assert!(disc_info(&iso).is_err());
        // A cue whose first track is a FIFO: refused, not blocked.
        let cue = write(tmp.path(), "c.cue", b"FILE \"pipe.iso\" BINARY\n");
        assert!(disc_info(&cue).is_err());
    }

    #[test]
    fn probe_regions() {
        let tmp = tempfile::tempdir().unwrap();
        for (byte, region) in [
            (0x64, DiscRegion::Europe),
            (0xA1, DiscRegion::Japan),
            (0x7A, DiscRegion::Usa),
            (0x00, DiscRegion::Usa),
            (0xFF, DiscRegion::Usa),
            // ASCII region letters are not what the core reads: anything but 0x64 / 0xA1 is USA.
            (b'E', DiscRegion::Usa),
        ] {
            let expected = Some(DiscInfo {
                format: DiscFormat::SegaCd,
                region: Some(region),
            });
            let iso = write(tmp.path(), "d.iso", &sector0(byte));
            assert_eq!(disc_info(&iso).unwrap(), expected, "iso {byte:#x}");
            let bin2048 = write(tmp.path(), "d.bin", &sector0(byte));
            assert_eq!(disc_info(&bin2048).unwrap(), expected, "bin {byte:#x}");
            let track = write(tmp.path(), "t.bin", &raw(byte));
            assert_eq!(disc_info(&track).unwrap(), expected, "raw {byte:#x}");
            let cue = write(
                tmp.path(),
                "d.cue",
                b"FILE \"t.bin\" BINARY\n  TRACK 01 MODE1/2352\nFILE \"a.wav\" WAVE\n",
            );
            assert_eq!(disc_info(&cue).unwrap(), expected, "cue {byte:#x}");
        }
    }

    #[test]
    fn probe_misses_and_truncation() {
        let tmp = tempfile::tempdir().unwrap();
        // A raw track is not a valid `.iso` (header at 0 only).
        let iso = write(tmp.path(), "raw.iso", &raw(0x64));
        assert_eq!(disc_info(&iso).unwrap(), None);
        let md = write(tmp.path(), "game.bin", &[0x11; 4096]);
        assert_eq!(disc_info(&md).unwrap(), None);
        let short = write(tmp.path(), "short.iso", b"SEGADISCSYSTEM  ");
        assert_eq!(
            disc_info(&short).unwrap(),
            Some(DiscInfo {
                format: DiscFormat::SegaCd,
                region: None
            })
        );
        let tiny = write(tmp.path(), "tiny.bin", b"SEGA");
        assert_eq!(disc_info(&tiny).unwrap(), None);
        // `.chd` and other extensions are never read (the file does not even exist).
        assert_eq!(disc_info(&tmp.path().join("x.chd")).unwrap(), None);
        assert_eq!(disc_info(&tmp.path().join("x.md")).unwrap(), None);
        assert_eq!(disc_info(&tmp.path().join("noext")).unwrap(), None);
        // A cue without FILE line, or whose first track is missing or audio-only.
        let empty = write(tmp.path(), "e.cue", b"REM nothing\n");
        assert_eq!(disc_info(&empty).unwrap(), None);
        let missing = write(tmp.path(), "m.cue", b"FILE \"gone.bin\" BINARY\n");
        assert_eq!(disc_info(&missing).unwrap(), None);
        write(tmp.path(), "a.wav", &[0u8; 1024]);
        let audio = write(tmp.path(), "a.cue", b"FILE \"a.wav\" WAVE\n");
        assert_eq!(disc_info(&audio).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn cue_track_symlink_outside_folder_is_not_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        let disc = tmp.path().join("disc");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(&disc).unwrap();
        write(&outside, "real.iso", &sector0(0x64));
        std::os::unix::fs::symlink(outside.join("real.iso"), disc.join("t.iso")).unwrap();
        let cue = write(&disc, "d.cue", b"FILE \"t.iso\" BINARY\n");
        assert_eq!(disc_info(&cue).unwrap(), None);
        // A link to a file of the same folder is fine.
        write(&disc, "real.iso", &sector0(0x64));
        fs::remove_file(disc.join("t.iso")).unwrap();
        std::os::unix::fs::symlink("real.iso", disc.join("t.iso")).unwrap();
        assert_eq!(
            disc_info(&cue).unwrap().map(|i| i.region),
            Some(Some(DiscRegion::Europe))
        );
    }

    #[test]
    fn names() {
        for (stem, region) in [
            ("Dune (Europe)", Some(DiscRegion::Europe)),
            ("Dune (France)", Some(DiscRegion::Europe)),
            ("X (Germany)", Some(DiscRegion::Europe)),
            ("X (Spain)", Some(DiscRegion::Europe)),
            ("X (Italy)", Some(DiscRegion::Europe)),
            ("X (UK)", Some(DiscRegion::Europe)),
            ("X (Australia)", Some(DiscRegion::Europe)),
            ("X (USA)", Some(DiscRegion::Usa)),
            ("X (Brazil)", Some(DiscRegion::Usa)),
            ("X (Japan)", Some(DiscRegion::Japan)),
            ("X (Korea)", Some(DiscRegion::Japan)),
            ("X (Asia)", Some(DiscRegion::Japan)),
            ("X (Japan, USA)", Some(DiscRegion::Japan)),
            ("X (En,Fr) (Rev 1) (USA)", Some(DiscRegion::Usa)),
            ("X (usa)", Some(DiscRegion::Usa)),
            ("X (World)", None),
            ("Dune", None),
            ("X (Europe", None),
            ("USA", None),
            ("", None),
        ] {
            assert_eq!(region_from_name(stem), region, "{stem}");
        }
    }

    #[test]
    fn region_from_key_is_the_inverse_of_key() {
        for r in [DiscRegion::Europe, DiscRegion::Usa, DiscRegion::Japan] {
            assert_eq!(DiscRegion::from_key(r.key()), Some(r));
        }
        for bad in ["", "USA", "Europe", "world", "usa "] {
            assert_eq!(DiscRegion::from_key(bad), None, "{bad}");
        }
    }

    fn with_serial(mut sector: Vec<u8>, field: &[u8]) -> Vec<u8> {
        sector[0x180..0x180 + field.len()].copy_from_slice(field);
        sector
    }

    #[test]
    fn probe_bin_kinds() {
        let tmp = tempfile::tempdir().unwrap();
        let mut md = vec![0x11u8; 4096];
        md[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
        assert_eq!(
            probe_bin(&write(tmp.path(), "md.bin", &md)).unwrap(),
            BinKind::MegaDrive
        );
        let mut x = vec![0u8; 0x200];
        x[0x100..0x110].copy_from_slice(b"SEGA 32X U      ");
        assert_eq!(
            probe_bin(&write(tmp.path(), "x.bin", &x)).unwrap(),
            BinKind::Sega32x
        );
        // Just long enough for the signature.
        assert_eq!(
            probe_bin(&write(tmp.path(), "x8.bin", &x[..0x108])).unwrap(),
            BinKind::Sega32x
        );
        assert_eq!(
            probe_bin(&write(tmp.path(), "cd2048.bin", &sector0(0x64))).unwrap(),
            BinKind::SegaCdTrack
        );
        assert_eq!(
            probe_bin(&write(tmp.path(), "cd2352.bin", &raw(0xA1))).unwrap(),
            BinKind::SegaCdTrack
        );
        assert_eq!(
            probe_bin(&write(tmp.path(), "tiny.bin", b"SEGA 32X")).unwrap(),
            BinKind::Unknown
        );
        assert_eq!(
            probe_bin(&write(tmp.path(), "empty.bin", b"")).unwrap(),
            BinKind::Unknown
        );
        assert!(probe_bin(&tmp.path().join("none.bin")).is_err());
        assert!(probe_bin(tmp.path()).is_err());
        // A huge file is never read past the probe.
        let big = tmp.path().join("big.bin");
        let f = File::create(&big).unwrap();
        f.set_len(1 << 30).unwrap();
        assert_eq!(probe_bin(&big).unwrap(), BinKind::MegaDrive);
    }

    #[test]
    fn serials() {
        let tmp = tempfile::tempdir().unwrap();
        let s0 = with_serial(sector0(0x7A), b"GM T-93175 -00");
        let iso = write(tmp.path(), "d.iso", &s0);
        assert_eq!(
            disc_serial(&iso, DiscFormat::SegaCd).unwrap().as_deref(),
            Some("T-93175")
        );
        let mut track = raw(0x64);
        track[0x10 + 0x180..0x10 + 0x18E].copy_from_slice(b"GM 00-2501-14 ");
        write(tmp.path(), "t.bin", &track);
        let cue = write(
            tmp.path(),
            "d.cue",
            b"FILE \"t.bin\" BINARY\nFILE \"a.wav\" WAVE\n",
        );
        assert_eq!(
            disc_serial(&cue, DiscFormat::SegaCd).unwrap().as_deref(),
            Some("00-2501")
        );
        assert_eq!(
            disc_header(&cue).unwrap().map(|(i, f)| (i.region, f)),
            Some((Some(DiscRegion::Europe), Some("GM 00-2501-14".to_owned())))
        );
        // Blank or non-text field, no header, `.chd` (never read, absent here), short file.
        let blank = write(tmp.path(), "b.iso", &sector0(0x64));
        assert_eq!(disc_serial(&blank, DiscFormat::SegaCd).unwrap(), None);
        let spaces = write(
            tmp.path(),
            "s.iso",
            &with_serial(sector0(0), b"              "),
        );
        assert_eq!(disc_serial(&spaces, DiscFormat::SegaCd).unwrap(), None);
        let binary = write(
            tmp.path(),
            "n.iso",
            &with_serial(sector0(0), b"GM \xFF\x01"),
        );
        assert_eq!(disc_serial(&binary, DiscFormat::SegaCd).unwrap(), None);
        let none = write(tmp.path(), "z.iso", &[0x20u8; 4096]);
        assert_eq!(disc_serial(&none, DiscFormat::SegaCd).unwrap(), None);
        assert_eq!(
            disc_serial(&tmp.path().join("x.chd"), DiscFormat::SegaCd).unwrap(),
            None
        );
        let short = write(tmp.path(), "short.iso", &sector0(0)[..0x185]);
        assert_eq!(disc_serial(&short, DiscFormat::SegaCd).unwrap(), None);
        let plain = write(tmp.path(), "p.iso", &with_serial(sector0(0), b"MK-4407"));
        assert_eq!(
            disc_serial(&plain, DiscFormat::SegaCd).unwrap().as_deref(),
            Some("MK-4407")
        );
    }

    #[test]
    fn serial_keys_cover_the_dat_forms() {
        assert_eq!(
            serial_keys("GM T-93175 -00"),
            ["GMT-93175-00", "T-93175-00", "T-93175"]
        );
        assert_eq!(
            serial_keys("GM MK-4407-50"),
            ["GMMK-4407-50", "MK-4407-50", "MK-4407"]
        );
        assert_eq!(serial_keys("T-93175"), ["T-93175"]);
        assert!(serial_keys("   ").is_empty());
        assert_eq!(normalize_serial(" t-93175 "), "T-93175");
    }
}
