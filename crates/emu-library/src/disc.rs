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
    /// PlayStation: ISO 9660 descriptor with system id `PLAYSTATION` at sector 16 (7.0; recognised by PB1).
    Psx,
    /// 3DO (opera): `01 5A 5A 5A 5A 5A 01` at the start of sector 0 (7.0; recognised by PB1).
    Opera,
}

impl DiscFormat {
    /// Id of the system (`systems.toml`) that owns discs of this format.
    pub fn system_id(self) -> &'static str {
        match self {
            DiscFormat::SegaCd => "megacd",
            DiscFormat::Psx => "psx",
            DiscFormat::Opera => "3do",
        }
    }
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
/// Mega Drive style header inside sector 0: type + serial field (`GM T-93175 -00`), 14 bytes.
const SERIAL_OFFSET: usize = 0x180;
const SERIAL_LEN: usize = 14;
/// 32X cartridge signature at 0x100 (`SEGA 32X`).
const SEGA_32X_MAGIC: &[u8] = b"SEGA 32X";
const CARTRIDGE_MAGIC_OFFSET: usize = 0x100;
/// 3DO (Opera) volume header: record type, five sync bytes, version.
const OPERA_MAGIC: &[u8] = b"\x01\x5A\x5A\x5A\x5A\x5A\x01";
/// User bytes of a data sector (MODE1, MODE2 form 1, ISO).
const USER_DATA: usize = 2048;
/// System identifier of a PlayStation ISO 9660 descriptor (`PLAYSTATION`, space padded).
const PSX_SYSTEM_ID: &[u8] = b"PLAYSTATION";
/// First bytes of a track read by the disc probes: up to the end of the user data of sector 16 in a raw MODE2
/// track (the largest of the three layouts), so the PlayStation descriptor and licence sector come in one read.
const PSX_HEAD_LEN: usize = 16 * 2352 + 24 + USER_DATA;
/// Sector reads of a PlayStation probe past the first `PSX_HEAD_LEN` bytes: root directory, then `SYSTEM.CNF`.
const MAX_PSX_SECTOR_READS: usize = MAX_ROOT_SECTORS as usize + 1;
/// Sectors of the ISO 9660 root directory walked to find `SYSTEM.CNF`.
const MAX_ROOT_SECTORS: u64 = 4;
/// Fixed part of an ISO 9660 directory record (33 bytes + at least one name byte).
const DIR_RECORD_MIN: usize = 34;

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

/// Reads up to `len` bytes from the start of `path`.
fn read_head(path: &Path, len: usize) -> Result<Vec<u8>, LibraryError> {
    let file = open_regular(path)?;
    let mut head = Vec::with_capacity(len);
    file.take(len as u64)
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

/// 3DO volume header (`01 5A 5A 5A 5A 5A 01`) at one of `offsets` of `head`.
fn opera_at(head: &[u8], offsets: &[usize]) -> bool {
    offsets
        .iter()
        .any(|&off| head.get(off..off + OPERA_MAGIC.len()) == Some(OPERA_MAGIC))
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

/// How the sectors of a data track are stored: bytes per sector and offset of the 2048 user bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    sector: usize,
    data: usize,
}

const COOKED: Layout = Layout {
    sector: 2048,
    data: 0,
};
/// MODE2/2352 form 1 (PlayStation): sync, header, 8-byte subheader.
const MODE2_RAW: Layout = Layout {
    sector: 2352,
    data: 24,
};
const MODE1_RAW: Layout = Layout {
    sector: 2352,
    data: RAW_DATA_OFFSET,
};
const ISO_LAYOUTS: &[Layout] = &[COOKED, MODE2_RAW, MODE1_RAW];
const TRACK_LAYOUTS: &[Layout] = &[MODE2_RAW, MODE1_RAW, COOKED];
const ISO_OFFSETS: &[usize] = &[0];
const TRACK_OFFSETS: &[usize] = &[0, RAW_DATA_OFFSET];

/// Data track of a disc: the file to read and where its headers may start.
struct Track {
    path: PathBuf,
    /// Offsets of a sector-0 header (Sega CD, 3DO).
    offsets: &'static [usize],
    /// Layouts tried for the ISO 9660 descriptor (PlayStation).
    layouts: &'static [Layout],
}

/// Data track of a `.iso` (itself), a `.bin` (itself) or a `.cue` (its first file, kept in its folder), opened.
/// `Ok(None)`: no readable track (a cue without `FILE` line or whose track is missing, a `.chd` / `.pbp`, never
/// read, any other extension).
fn open_track(path: &Path) -> Result<Option<(Track, File)>, LibraryError> {
    let Some(ext) = ext_of(path) else {
        return Ok(None);
    };
    let (track, cue) = match ext.as_str() {
        "iso" => (
            Track {
                path: path.to_path_buf(),
                offsets: ISO_OFFSETS,
                layouts: ISO_LAYOUTS,
            },
            false,
        ),
        "bin" | "cue" => {
            let file = if ext == "cue" {
                let Some(track) = first_track(path)? else {
                    return Ok(None);
                };
                track
            } else {
                path.to_path_buf()
            };
            (
                Track {
                    path: file,
                    offsets: TRACK_OFFSETS,
                    layouts: TRACK_LAYOUTS,
                },
                ext == "cue",
            )
        }
        _ => return Ok(None),
    };
    match open_regular(&track.path) {
        Ok(file) => Ok(Some((track, file))),
        // A cue whose first track is missing is not a disc we can recognise.
        Err(LibraryError::Io { source, .. }) if cue && source.kind() == io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// What the probe of a disc found: its format and region, and its serial (Sega CD: the raw header field, type code
/// and revision kept; PlayStation: the canonical `SLES-00032` form).
struct Probe {
    info: DiscInfo,
    serial: Option<String>,
}

/// Probe of a disc (design v7.0 §3.4): Sega CD and 3DO headers in the first 0x21C bytes, then the PlayStation
/// descriptor at sector 16 and the licence sector 4 inside the first `PSX_HEAD_LEN` bytes (one read), then, for
/// the serial (or a region the licence does not give), at most `MAX_PSX_SECTOR_READS` sectors of the ISO 9660
/// root and `SYSTEM.CNF`. Nothing read is trusted: offsets are checked against the file size, no allocation size
/// comes from the disc.
fn probe_disc(path: &Path, want_serial: bool) -> Result<Option<Probe>, LibraryError> {
    let Some((track, file)) = open_track(path)? else {
        return Ok(None);
    };
    let size = file.metadata().map_err(|e| io_error(&track.path, e))?.len();
    let mut file = file;
    let head = {
        let mut head = Vec::with_capacity(PSX_HEAD_LEN);
        (&mut file)
            .take(PSX_HEAD_LEN as u64)
            .read_to_end(&mut head)
            .map_err(|e| io_error(&track.path, e))?;
        head
    };
    if let Some(info) = probe_head(&head, track.offsets) {
        return Ok(Some(Probe {
            info,
            serial: header_serial_field(&head, track.offsets),
        }));
    }
    if opera_at(&head, track.offsets) {
        return Ok(Some(Probe {
            info: DiscInfo {
                format: DiscFormat::Opera,
                region: None,
            },
            serial: None,
        }));
    }
    let Some((layout, pvd)) = psx_descriptor(&head, track.layouts) else {
        return Ok(None);
    };
    let license = license_region(&head, layout);
    let serial = if want_serial || license.is_none() {
        let mut sectors = Sectors {
            src: FileAt { file, size },
            layout,
            reads: 0,
        };
        psx_serial(&mut sectors, pvd).map_err(|e| io_error(&track.path, e))?
    } else {
        None
    };
    Ok(Some(Probe {
        info: DiscInfo {
            format: DiscFormat::Psx,
            region: license.or_else(|| serial.as_deref().and_then(psx_serial_region)),
        },
        serial,
    }))
}

/// Header probe of a `.cue` (its first data track), a `.iso` or a raw `.bin` track: Sega CD, 3DO, PlayStation.
/// `Ok(None)`: not a known disc (or a `.chd` / `.pbp`, never read). Reads at most 64 KiB of the cue, the first
/// `PSX_HEAD_LEN` bytes of the track and, for a PlayStation disc without licence region, a few sectors more.
pub fn disc_info(path: &Path) -> Result<Option<DiscInfo>, LibraryError> {
    Ok(probe_disc(path, false)?.map(|p| p.info))
}

/// The PlayStation ISO 9660 primary volume descriptor at sector 16 under one of `layouts`, inside `head`.
fn psx_descriptor<'h>(head: &'h [u8], layouts: &[Layout]) -> Option<(Layout, &'h [u8])> {
    layouts.iter().find_map(|&layout| {
        let off = 16 * layout.sector + layout.data;
        let pvd = head.get(off..off + USER_DATA)?;
        let system_id = trim_padding(&pvd[8..40]);
        (pvd[0] == 1 && &pvd[1..6] == b"CD001" && system_id == PSX_SYSTEM_ID)
            .then_some((layout, pvd))
    })
}

/// Trailing spaces and NULs dropped (ISO 9660 a-characters are space padded).
fn trim_padding(field: &[u8]) -> &[u8] {
    let end = field
        .iter()
        .rposition(|&b| b != b' ' && b != 0)
        .map_or(0, |i| i + 1);
    &field[..end]
}

/// Region of the licence text of sector 4 (`… Euro pe`, `… Amer  ica`, `… Inc.`), inside `head`.
fn license_region(head: &[u8], layout: Layout) -> Option<DiscRegion> {
    let off = 4 * layout.sector + layout.data;
    let text = head.get(off..head.len().min(off + USER_DATA))?;
    let has = |token: &[u8]| text.windows(token.len()).any(|w| w == token);
    if has(b"Euro") {
        Some(DiscRegion::Europe)
    } else if has(b"Amer") {
        Some(DiscRegion::Usa)
    } else if has(b"Inc.") {
        Some(DiscRegion::Japan)
    } else {
        None
    }
}

/// Region of a canonical PlayStation serial from its prefix (`SLES` / `SCES` Europe, `SLUS` / `SCUS` USA,
/// `SLPS` / `SCPS` / `SLPM` / `SLKA` Japan…); `None` for anything else (`PAPX`, demos).
fn psx_serial_region(serial: &str) -> Option<DiscRegion> {
    match serial.get(2..4)? {
        "ES" | "ED" => Some(DiscRegion::Europe),
        "US" | "UD" => Some(DiscRegion::Usa),
        "PS" | "PM" | "PD" | "KA" => Some(DiscRegion::Japan),
        _ => None,
    }
}

/// Random access to a disc track (a file, or a buffer in tests).
trait ReadAt {
    fn size(&self) -> u64;
    fn read_exact_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
}

struct FileAt {
    file: File,
    size: u64,
}

impl ReadAt for FileAt {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_exact_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        use std::io::{Seek, SeekFrom};
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(buf)
    }
}

/// User data of the sectors of a track under one layout, at most `MAX_PSX_SECTOR_READS` reads.
struct Sectors<R> {
    src: R,
    layout: Layout,
    reads: usize,
}

impl<R: ReadAt> Sectors<R> {
    /// User data of sector `lba`; `Ok(None)` when it lies past the end of the file or the read budget is spent.
    fn read(&mut self, lba: u64) -> io::Result<Option<[u8; USER_DATA]>> {
        if self.reads >= MAX_PSX_SECTOR_READS {
            return Ok(None);
        }
        let Some(offset) = lba
            .checked_mul(self.layout.sector as u64)
            .and_then(|o| o.checked_add(self.layout.data as u64))
        else {
            return Ok(None);
        };
        if offset
            .checked_add(USER_DATA as u64)
            .is_none_or(|end| end > self.src.size())
        {
            return Ok(None);
        }
        self.reads += 1;
        let mut buf = [0u8; USER_DATA];
        self.src.read_exact_at(offset, &mut buf)?;
        Ok(Some(buf))
    }
}

fn le32(bytes: &[u8], at: usize) -> Option<u64> {
    let b = bytes.get(at..at + 4)?;
    Some(u64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])))
}

/// Serial of a PlayStation disc: ISO 9660 root (at most `MAX_ROOT_SECTORS` sectors) ⇒ `SYSTEM.CNF` (at most one
/// sector) ⇒ `BOOT = cdrom:\SLES_000.32;1` ⇒ `SLES-00032`. `Ok(None)` when anything is missing or malformed.
fn psx_serial<R: ReadAt>(sectors: &mut Sectors<R>, pvd: &[u8]) -> io::Result<Option<String>> {
    // Logical block size (both-endian field, little-endian half first): only 2048 is walked.
    if pvd.get(128..130) != Some(&[0x00, 0x08][..]) {
        return Ok(None);
    }
    let (Some(root), Some(root_size)) = (le32(pvd, 156 + 2), le32(pvd, 156 + 10)) else {
        return Ok(None);
    };
    let Some((cnf, cnf_size)) = find_system_cnf(sectors, root, root_size)? else {
        return Ok(None);
    };
    if cnf_size == 0 {
        return Ok(None);
    }
    let Some(sector) = sectors.read(cnf)? else {
        return Ok(None);
    };
    let len = usize::try_from(cnf_size).map_or(USER_DATA, |n| n.min(USER_DATA));
    Ok(serial_of_cnf(&sector[..len]))
}

/// Extent and size of `SYSTEM.CNF` in the root directory (`extent`, `size` read from the descriptor).
fn find_system_cnf<R: ReadAt>(
    sectors: &mut Sectors<R>,
    extent: u64,
    size: u64,
) -> io::Result<Option<(u64, u64)>> {
    let count = size.div_ceil(USER_DATA as u64).min(MAX_ROOT_SECTORS);
    for i in 0..count {
        let Some(sector) = sectors.read(extent.saturating_add(i))? else {
            break;
        };
        let mut pos = 0usize;
        while pos + DIR_RECORD_MIN <= USER_DATA {
            let len = usize::from(sector[pos]);
            // A zero length pads the rest of the sector; a record never spans two sectors.
            if len < DIR_RECORD_MIN || pos + len > USER_DATA {
                break;
            }
            let record = &sector[pos..pos + len];
            let name_len = usize::from(record[32]);
            if let Some(name) = record.get(33..33 + name_len) {
                let name = name.split(|&b| b == b';').next().unwrap_or(name);
                let is_dir = record[25] & 0x02 != 0;
                if !is_dir && name.eq_ignore_ascii_case(b"SYSTEM.CNF") {
                    return Ok(le32(record, 2).zip(le32(record, 10)));
                }
            }
            pos += len;
        }
    }
    Ok(None)
}

/// Serial named by the `BOOT` line of a `SYSTEM.CNF` (`BOOT = cdrom:\SLES_000.32;1`), canonical form.
fn serial_of_cnf(text: &[u8]) -> Option<String> {
    for line in text.split(|&b| b == b'\n' || b == b'\r') {
        let line = String::from_utf8_lossy(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("BOOT") {
            continue;
        }
        let file = value.trim().rsplit(['\\', '/', ':']).next()?;
        let file = file.split(';').next()?.trim();
        return canonical_psx_serial(file, true);
    }
    None
}

/// `SLES_000.32` / `SLES-00032` (any case) ⇒ `SLES-00032`. `strict`: the form `[A-Z]{4}[-_][0-9]{3}\.?[0-9]{2}`
/// only (a disc); otherwise the separator may also be missing (a DAT value, spaces already removed).
fn canonical_psx_serial(text: &str, strict: bool) -> Option<String> {
    let text = text.to_ascii_uppercase();
    let b = text.as_bytes();
    let letters = b.get(..4)?;
    if !letters.iter().all(u8::is_ascii_uppercase) {
        return None;
    }
    let rest = match b.get(4)? {
        b'-' | b'_' => &b[5..],
        _ if !strict => &b[4..],
        _ => return None,
    };
    let digits: Vec<u8> = match rest {
        [a, b2, c, b'.', d, e] => vec![*a, *b2, *c, *d, *e],
        [_, _, _, _, _] => rest.to_vec(),
        _ => return None,
    };
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(format!(
        "{}-{}",
        &text[..4],
        std::str::from_utf8(&digits).ok()?
    ))
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
    /// Track of a PlayStation disc (7.0; never produced before PB1): masked when a `.cue` names it, else a game.
    PsxTrack,
    /// Track of a 3DO disc (7.0; never produced before PB1): same rule as `PsxTrack`.
    OperaTrack,
    /// Too short to hold a header, or unreadable as such.
    Unknown,
}

/// What the first bytes of a `.bin` say ([`probe_bin`]); pure, for the scanner's zip entries too (`head`: the
/// first [`BIN_PROBE_LEN`] bytes, or fewer for a short file).
pub(crate) fn bin_kind(head: &[u8]) -> BinKind {
    if probe_head(head, TRACK_OFFSETS).is_some() {
        return BinKind::SegaCdTrack;
    }
    if opera_at(head, TRACK_OFFSETS) {
        return BinKind::OperaTrack;
    }
    if psx_descriptor(head, TRACK_LAYOUTS).is_some() {
        return BinKind::PsxTrack;
    }
    match head.get(CARTRIDGE_MAGIC_OFFSET..CARTRIDGE_MAGIC_OFFSET + SEGA_32X_MAGIC.len()) {
        Some(magic) if magic == SEGA_32X_MAGIC => BinKind::Sega32x,
        Some(_) => BinKind::MegaDrive,
        None => BinKind::Unknown,
    }
}

/// Bytes [`probe_bin`] reads (and the scanner reads of a zipped `.bin`): up to the PlayStation descriptor of
/// sector 16 in a raw MODE2 track.
pub(crate) const BIN_PROBE_LEN: usize = PSX_HEAD_LEN;

/// Header probe of a `.bin`: reads at most [`BIN_PROBE_LEN`] bytes (one read, < 40 KiB); `SegaCdTrack`,
/// `OperaTrack` and `PsxTrack` as [`disc_info`] sees them, `Sega32x` for `SEGA 32X` at 0x100, else `MegaDrive`
/// (`Unknown` for a file too short to hold the 32X signature). Same untrusted-input rules as the rest of this
/// module (regular file only, bounded read, no panic).
pub fn probe_bin(path: &Path) -> Result<BinKind, LibraryError> {
    Ok(bin_kind(&read_head(path, BIN_PROBE_LEN)?))
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

/// Header facts of a disc read once: format, region and serial (scanner). Sega CD: the raw header field;
/// PlayStation: the canonical serial (`SLES-00032`); 3DO: none.
pub(crate) fn disc_header(path: &Path) -> Result<Option<(DiscInfo, Option<String>)>, LibraryError> {
    Ok(probe_disc(path, true)?.map(|p| (p.info, p.serial)))
}

/// Serial number in the disc header (one probe per [`DiscFormat`]); `Ok(None)` when the disc is of another format,
/// has no header or a blank serial, and always for a `.chd` / `.pbp` (never read). `SegaCd`: MD-style header at
/// 0x100 of sector 0, field at 0x180 (`GM T-93175 -00` ⇒ `T-93175`: type code and revision dropped, trimmed).
/// `Psx`: `BOOT` line of `SYSTEM.CNF` (`cdrom:\SLES_000.32;1` ⇒ `SLES-00032`), accepted only in the form
/// `[A-Z]{4}[-_][0-9]{3}\.?[0-9]{2}`. `Opera`: 3DO discs carry none. Of a `.cue` (its first track, kept in the
/// cue's folder), a `.iso` or a `.bin` track; bounded reads (see [`disc_info`]).
pub fn disc_serial(path: &Path, format: DiscFormat) -> Result<Option<String>, LibraryError> {
    let Some(probe) = probe_disc(path, format == DiscFormat::Psx)? else {
        return Ok(None);
    };
    if probe.info.format != format {
        return Ok(None);
    }
    Ok(match format {
        DiscFormat::SegaCd => probe.serial.as_deref().and_then(clean_serial),
        DiscFormat::Psx => probe.serial,
        DiscFormat::Opera => None,
    })
}

/// Upper case, spaces removed: form compared between a disc and the DAT `serial` field. A PlayStation serial
/// takes its canonical form (`SLES_000.32`, `SLES 00032` ⇒ `SLES-00032`).
pub(crate) fn normalize_serial(serial: &str) -> String {
    let text: String = serial
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    canonical_psx_serial(&text, false).unwrap_or(text)
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

/// Fake PlayStation / 3DO images for the unit tests of this crate (no commercial content).
#[cfg(test)]
pub(crate) mod fixtures {
    /// 2048-byte-sector image of a fake PlayStation disc: licence text at sector 4, `PLAYSTATION` descriptor at 16,
    /// root directory at 18 (`SYSTEM.CNF;1` at 19 when `cnf` is given).
    pub(crate) fn psx_iso(license: Option<&str>, cnf: Option<&[u8]>) -> Vec<u8> {
        let mut img = vec![0u8; 20 * 2048];
        if let Some(text) = license {
            img[4 * 2048..4 * 2048 + text.len()].copy_from_slice(text.as_bytes());
        }
        let pvd = &mut img[16 * 2048..17 * 2048];
        pvd[..7].copy_from_slice(b"\x01CD001\x01");
        pvd[8..40].copy_from_slice(&[b' '; 32]);
        pvd[8..19].copy_from_slice(b"PLAYSTATION");
        pvd[128..132].copy_from_slice(&[0x00, 0x08, 0x08, 0x00]);
        pvd[156] = 34;
        pvd[158..162].copy_from_slice(&18u32.to_le_bytes());
        pvd[166..170].copy_from_slice(&2048u32.to_le_bytes());
        pvd[156 + 25] = 2;
        img[17 * 2048..17 * 2048 + 7].copy_from_slice(b"\xFFCD001\x01");
        let root = 18 * 2048;
        let mut pos = root;
        for (name, extent, size, flags) in [
            (&b"\x00"[..], 18u32, 2048u32, 2u8),
            (&b"\x01"[..], 18, 2048, 2),
        ] {
            pos += dir_record(&mut img[pos..], name, extent, size, flags);
        }
        if let Some(cnf) = cnf {
            dir_record(&mut img[pos..], b"SYSTEM.CNF;1", 19, cnf.len() as u32, 0);
            img[19 * 2048..19 * 2048 + cnf.len()].copy_from_slice(cnf);
        }
        img
    }

    fn dir_record(out: &mut [u8], name: &[u8], extent: u32, size: u32, flags: u8) -> usize {
        let len = (33 + name.len() + 1) & !1;
        out[0] = len as u8;
        out[2..6].copy_from_slice(&extent.to_le_bytes());
        out[10..14].copy_from_slice(&size.to_le_bytes());
        out[25] = flags;
        out[32] = name.len() as u8;
        out[33..33 + name.len()].copy_from_slice(name);
        len
    }

    /// Raw 2352-byte sectors of a 2048-byte image (`mode` 1 or 2 form 1; EDC / ECC left zero).
    pub(crate) fn to_raw(iso: &[u8], mode: u8) -> Vec<u8> {
        let mut out = Vec::new();
        for user in iso.chunks(2048) {
            let mut s = vec![0x00u8];
            s.extend([0xFF; 10]);
            s.extend([0x00, 0x00, 0x02, 0x00, mode]);
            if mode == 2 {
                s.extend([0, 0, 8, 0, 0, 0, 8, 0]);
            }
            s.extend(user);
            s.resize(2352, 0);
            out.extend(s);
        }
        out
    }

    pub(crate) fn opera_sector0() -> Vec<u8> {
        let mut s = vec![0u8; 2048];
        s[..7].copy_from_slice(b"\x01ZZZZZ\x01");
        s[7..17].copy_from_slice(b"EMURUST3DO");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
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

    // --- PlayStation / 3DO (design v7.0 §3.4) ---

    const CNF: &[u8] = b"BOOT = cdrom:\\SLES_000.32;1\r\nTCB = 4\r\nEVENT = 10\r\n";

    fn psx(region: Option<DiscRegion>) -> Option<DiscInfo> {
        Some(DiscInfo {
            format: DiscFormat::Psx,
            region,
        })
    }

    #[test]
    fn psx_iso_mode2_mode1_and_cue() {
        let tmp = tempfile::tempdir().unwrap();
        let iso = psx_iso(
            Some("Licensed by Sony Computer Entertainment Euro pe"),
            Some(CNF),
        );
        let europe = psx(Some(DiscRegion::Europe));
        let p = write(tmp.path(), "g.iso", &iso);
        assert_eq!(disc_info(&p).unwrap(), europe);
        assert_eq!(
            disc_serial(&p, DiscFormat::Psx).unwrap().as_deref(),
            Some("SLES-00032")
        );
        assert_eq!(disc_serial(&p, DiscFormat::SegaCd).unwrap(), None);
        assert_eq!(disc_serial(&p, DiscFormat::Opera).unwrap(), None);
        for mode in [2, 1] {
            let bin = write(tmp.path(), "t.bin", &to_raw(&iso, mode));
            assert_eq!(disc_info(&bin).unwrap(), europe, "mode {mode}");
            assert_eq!(probe_bin(&bin).unwrap(), BinKind::PsxTrack, "mode {mode}");
            let cue = write(
                tmp.path(),
                "t.cue",
                b"FILE \"t.bin\" BINARY\n  TRACK 01 MODE2/2352\nFILE \"a.wav\" WAVE\n",
            );
            assert_eq!(disc_info(&cue).unwrap(), europe, "mode {mode}");
            assert_eq!(
                disc_header(&cue).unwrap(),
                Some((europe.unwrap(), Some("SLES-00032".to_owned())))
            );
            // A raw track named `.iso` is probed too (the three layouts are tried).
            let raw_iso = write(tmp.path(), "raw.iso", &to_raw(&iso, mode));
            assert_eq!(disc_info(&raw_iso).unwrap(), europe, "mode {mode}");
        }
        // 2048-byte sectors in a `.bin`.
        let cooked = write(tmp.path(), "cooked.bin", &iso);
        assert_eq!(probe_bin(&cooked).unwrap(), BinKind::PsxTrack);
    }

    #[test]
    fn psx_licence_texts_and_serial_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        for (text, region) in [
            ("Sony Computer Entertainment Euro pe", DiscRegion::Europe),
            ("Sony Computer Entertainment Amer  ica", DiscRegion::Usa),
            ("Sony Computer Entertainment Inc.", DiscRegion::Japan),
        ] {
            let p = write(tmp.path(), "l.iso", &psx_iso(Some(text), Some(CNF)));
            assert_eq!(disc_info(&p).unwrap(), psx(Some(region)), "{text}");
        }
        // No licence text: the serial prefix decides.
        for (cnf, region) in [
            (&b"BOOT=cdrom:\\SLUS_012.34;1"[..], Some(DiscRegion::Usa)),
            (
                b"BOOT = cdrom0:\\EXE\\SCPS_100.01;1",
                Some(DiscRegion::Japan),
            ),
            (b"BOOT = cdrom:\\SCES_123.45;1", Some(DiscRegion::Europe)),
            (b"BOOT = cdrom:\\PAPX_900.01;1", None),
            (b"BOOT = cdrom:\\PSX.EXE;1", None),
        ] {
            let p = write(tmp.path(), "n.iso", &psx_iso(None, Some(cnf)));
            assert_eq!(disc_info(&p).unwrap(), psx(region));
        }
        // Neither licence nor SYSTEM.CNF (a PS-EXE disc like the 240p suite has a SYSTEM.CNF without serial).
        let bare = write(tmp.path(), "b.iso", &psx_iso(None, None));
        assert_eq!(disc_info(&bare).unwrap(), psx(None));
        assert_eq!(disc_serial(&bare, DiscFormat::Psx).unwrap(), None);
    }

    #[test]
    fn psx_malformed_discs_never_panic() {
        let tmp = tempfile::tempdir().unwrap();
        // Root directory far past the end of the file.
        let mut far = psx_iso(None, Some(CNF));
        far[16 * 2048 + 158..16 * 2048 + 162].copy_from_slice(&u32::MAX.to_le_bytes());
        far[16 * 2048 + 166..16 * 2048 + 170].copy_from_slice(&u32::MAX.to_le_bytes());
        let p = write(tmp.path(), "far.iso", &far);
        assert_eq!(disc_info(&p).unwrap(), psx(None));
        assert_eq!(disc_serial(&p, DiscFormat::Psx).unwrap(), None);
        // SYSTEM.CNF pointing past the end; a block size other than 2048.
        let mut cnf_far = psx_iso(None, Some(CNF));
        let rec = 18 * 2048 + 34 + 34;
        cnf_far[rec + 2..rec + 6].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
        let p = write(tmp.path(), "cnf.iso", &cnf_far);
        assert_eq!(disc_serial(&p, DiscFormat::Psx).unwrap(), None);
        let mut block = psx_iso(None, Some(CNF));
        block[16 * 2048 + 128] = 0x01;
        let p = write(tmp.path(), "block.iso", &block);
        assert_eq!(disc_serial(&p, DiscFormat::Psx).unwrap(), None);
        // Garbage directory records (length below the minimum, name past the record).
        let mut junk = psx_iso(None, Some(CNF));
        junk[18 * 2048] = 7;
        let p = write(tmp.path(), "junk.iso", &junk);
        assert_eq!(disc_serial(&p, DiscFormat::Psx).unwrap(), None);
        let mut long_name = psx_iso(None, Some(CNF));
        long_name[rec + 32] = 0xFF;
        let p = write(tmp.path(), "name.iso", &long_name);
        assert_eq!(disc_serial(&p, DiscFormat::Psx).unwrap(), None);
        // Malformed serials.
        for cnf in [
            &b"BOOT = cdrom:\\SLES_0003.2;1"[..],
            b"BOOT = cdrom:\\SL3S_000.32;1",
            b"BOOT = cdrom:\\SLES000.32;1",
            b"BOOT2 = cdrom0:\\SLES_500.00;1",
            b"BOOT = ",
            b"\xFF\xFE\x00garbage",
        ] {
            let p = write(tmp.path(), "bad.iso", &psx_iso(None, Some(cnf)));
            assert_eq!(disc_serial(&p, DiscFormat::Psx).unwrap(), None, "{cnf:?}");
        }
        // Truncated right after the descriptor; a wrong system id.
        let p = write(
            tmp.path(),
            "cut.iso",
            &psx_iso(None, Some(CNF))[..17 * 2048],
        );
        assert_eq!(disc_info(&p).unwrap(), psx(None));
        let p = write(
            tmp.path(),
            "cut2.iso",
            &psx_iso(None, Some(CNF))[..17 * 2048 - 1],
        );
        assert_eq!(disc_info(&p).unwrap(), None);
        let mut other = psx_iso(None, Some(CNF));
        other[16 * 2048 + 8..16 * 2048 + 19].copy_from_slice(b"PLAYSTATIOX");
        let p = write(tmp.path(), "other.iso", &other);
        assert_eq!(disc_info(&p).unwrap(), None);
    }

    /// In-memory track counting its reads.
    struct SliceAt<'a> {
        data: &'a [u8],
        reads: usize,
    }

    impl ReadAt for SliceAt<'_> {
        fn size(&self) -> u64 {
            self.data.len() as u64
        }

        fn read_exact_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
            self.reads += 1;
            let start = usize::try_from(offset).unwrap();
            buf.copy_from_slice(&self.data[start..start + buf.len()]);
            Ok(())
        }
    }

    #[test]
    fn psx_sector_reads_are_bounded() {
        // A root directory claiming 4 GiB, made of empty sectors: the walk stops at the budget.
        let mut img = psx_iso(None, None);
        img[16 * 2048 + 166..16 * 2048 + 170].copy_from_slice(&u32::MAX.to_le_bytes());
        img.resize(64 * 2048, 0);
        let (_, pvd) = psx_descriptor(&img, ISO_LAYOUTS).unwrap();
        let pvd = pvd.to_vec();
        let mut sectors = Sectors {
            src: SliceAt {
                data: &img,
                reads: 0,
            },
            layout: COOKED,
            reads: 0,
        };
        assert_eq!(psx_serial(&mut sectors, &pvd).unwrap(), None);
        assert_eq!(sectors.src.reads, MAX_ROOT_SECTORS as usize);
        assert!(sectors.reads <= MAX_PSX_SECTOR_READS);
        // A normal disc: one root sector, one SYSTEM.CNF sector.
        let img = psx_iso(None, Some(CNF));
        let (layout, pvd) = psx_descriptor(&img, ISO_LAYOUTS).unwrap();
        let mut sectors = Sectors {
            src: SliceAt {
                data: &img,
                reads: 0,
            },
            layout,
            reads: 0,
        };
        assert_eq!(
            psx_serial(&mut sectors, pvd).unwrap().as_deref(),
            Some("SLES-00032")
        );
        assert_eq!(sectors.src.reads, 2);
    }

    #[test]
    fn zipped_head_is_enough_for_the_psx_descriptor() {
        let raw = to_raw(&psx_iso(None, Some(CNF)), 2);
        assert_eq!(bin_kind(&raw[..BIN_PROBE_LEN]), BinKind::PsxTrack);
        assert_eq!(bin_kind(&raw[..BIN_PROBE_LEN - 1]), BinKind::MegaDrive);
        let mode1 = to_raw(&psx_iso(None, Some(CNF)), 1);
        assert_eq!(bin_kind(&mode1[..BIN_PROBE_LEN]), BinKind::PsxTrack);
    }

    #[test]
    fn opera_discs() {
        let tmp = tempfile::tempdir().unwrap();
        let info = DiscInfo {
            format: DiscFormat::Opera,
            region: None,
        };
        let opera = Some(info);
        let mut iso = opera_sector0();
        iso.resize(32 * 2048, 0);
        let p = write(tmp.path(), "o.iso", &iso);
        assert_eq!(disc_info(&p).unwrap(), opera);
        assert_eq!(disc_serial(&p, DiscFormat::Opera).unwrap(), None);
        assert_eq!(disc_serial(&p, DiscFormat::Psx).unwrap(), None);
        let track = to_raw(&iso, 1);
        let bin = write(tmp.path(), "o.bin", &track);
        assert_eq!(disc_info(&bin).unwrap(), opera);
        assert_eq!(probe_bin(&bin).unwrap(), BinKind::OperaTrack);
        let cue = write(tmp.path(), "o.cue", b"FILE \"o.bin\" BINARY\n");
        assert_eq!(disc_header(&cue).unwrap(), Some((info, None)));
        // A raw track named `.iso`: the header is looked for at 0 only.
        let raw_iso = write(tmp.path(), "r.iso", &track);
        assert_eq!(disc_info(&raw_iso).unwrap(), None);
        // One byte short of the signature; a different version byte.
        let p = write(tmp.path(), "short.iso", &opera_sector0()[..6]);
        assert_eq!(disc_info(&p).unwrap(), None);
        let mut v2 = opera_sector0();
        v2[6] = 2;
        let p = write(tmp.path(), "v2.iso", &v2);
        assert_eq!(disc_info(&p).unwrap(), None);
        // The Sega CD header wins over nothing else: a Sega CD stays a Sega CD.
        assert_eq!(bin_kind(&raw(0x64)), BinKind::SegaCdTrack);
    }

    #[test]
    fn psx_serial_forms() {
        assert_eq!(
            canonical_psx_serial("SLES_000.32", true).as_deref(),
            Some("SLES-00032")
        );
        assert_eq!(
            canonical_psx_serial("sles-00032", true).as_deref(),
            Some("SLES-00032")
        );
        assert_eq!(canonical_psx_serial("SLES00032", true), None);
        assert_eq!(
            canonical_psx_serial("SLES00032", false).as_deref(),
            Some("SLES-00032")
        );
        for bad in [
            "",
            "SLES",
            "SLES_000.3",
            "SLES_00.032",
            "SLES-0003X",
            "SLE5-00032",
            "SLES-000321",
        ] {
            assert_eq!(canonical_psx_serial(bad, false), None, "{bad}");
        }
        // DAT side: same key as the disc; Sega CD serials are untouched.
        assert_eq!(normalize_serial("SLES_000.32"), "SLES-00032");
        assert_eq!(normalize_serial(" SLES 00032 "), "SLES-00032");
        assert_eq!(normalize_serial("t-93175"), "T-93175");
        assert_eq!(serial_keys("SLES-00032"), ["SLES-00032"]);
        assert_eq!(psx_serial_region("SLPM-86001"), Some(DiscRegion::Japan));
        assert_eq!(psx_serial_region("SLKA-25001"), Some(DiscRegion::Japan));
        assert_eq!(psx_serial_region("SCUS-94163"), Some(DiscRegion::Usa));
        assert_eq!(psx_serial_region("X"), None);
    }

    #[test]
    fn pbp_and_chd_are_never_read() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(disc_info(&tmp.path().join("x.pbp")).unwrap(), None);
        assert_eq!(
            disc_serial(&tmp.path().join("x.pbp"), DiscFormat::Psx).unwrap(),
            None
        );
        let p = write(tmp.path(), "real.pbp", &psx_iso(None, Some(CNF)));
        assert_eq!(disc_info(&p).unwrap(), None);
    }
}
