//! Zip reading for downloaded archives (untrusted). Narrower than `emu-runtime`'s content extraction
//! (ADR 0013): archives are read **in memory**, entry names are only compared, never used to build a path,
//! so nothing can be written outside the destination whatever the archive contains.
//! The end-of-central-directory entry count is checked before `ZipArchive::new` (same reasoning as
//! `emu-runtime/src/content.rs::declared_entries`).

use std::io::{Cursor, Read};

use zip::ZipArchive;

use super::CoreId;

/// Initial buffer for an entry; the declared size is never trusted for an allocation.
const MAX_INITIAL_CAPACITY: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractLimits {
    /// Per extracted entry, declared and actually read.
    pub max_entry_size: u64,
    /// Sum of the entries read.
    pub max_total_size: u64,
    pub max_entries: usize,
}

impl ExtractLimits {
    pub const CORE: Self = Self {
        max_entry_size: 256 * 1024 * 1024,
        max_total_size: 256 * 1024 * 1024,
        max_entries: 16,
    };
    pub const INFO_BUNDLE: Self = Self {
        max_entry_size: super::MAX_INFO_SIZE as u64,
        max_total_size: 32 * 1024 * 1024,
        max_entries: 4096,
    };
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ArchiveError {
    #[error("archive invalide : {0}")]
    Invalid(String),
    #[error("archive : trop d'entrées ({0})")]
    TooManyEntries(u64),
    #[error("archive : `{0}` absent")]
    MissingEntry(String),
    #[error("archive : `{0}` présent plusieurs fois")]
    DuplicateEntry(String),
    #[error("archive : `{name}` dépasse la limite ({limit} octets)")]
    TooLarge { name: String, limit: u64 },
    /// CRC of the zip entry itself (the `.index-extended` CRC is checked by the manager).
    #[error("archive : `{name}` corrompu : {message}")]
    Corrupt { name: String, message: String },
}

/// Returns the content of the regular-file entry whose **whole name** is exactly `expected`
/// (`<id>_libretro.<ext>`; no directory part accepted). Other entries are ignored; directories, symlinks
/// (Unix mode `S_IFLNK`) and encrypted entries never match. An archive with any duplicated entry name is refused.
pub fn extract_core(
    zip: &[u8],
    expected: &str,
    limits: ExtractLimits,
) -> Result<Vec<u8>, ArchiveError> {
    let mut archive = open(zip, limits)?;
    let mut found = None;
    let mut name_present = false;
    for index in 0..archive.len() {
        // Central-directory metadata only: no decompressor for entries that are only compared.
        let entry = archive.by_index_data(index).map_err(invalid)?;
        if entry.name_raw() != expected.as_bytes() {
            continue;
        }
        name_present = true;
        if is_plain_file(&entry) {
            found = Some((index, entry.size()));
        }
    }
    // The zip reader keeps a single entry per name and only reports duplicates archive-wide: any duplicate
    // refuses the archive (a core zip has a single entry).
    if archive.has_duplicate_names() {
        return Err(if name_present {
            ArchiveError::DuplicateEntry(expected.to_owned())
        } else {
            ArchiveError::Invalid("noms d'entrée en double".into())
        });
    }
    let Some((index, declared)) = found else {
        return Err(ArchiveError::MissingEntry(expected.to_owned()));
    };
    let limit = limits.max_entry_size.min(limits.max_total_size);
    read_entry(&mut archive, index, expected, declared, limit)
}

/// Every `<id>_libretro.info` entry at the archive root whose id passes [`CoreId::new`], as text
/// (lossy UTF-8). Other entries are ignored; one bad entry (too large, corrupt) is skipped with a warning.
/// For a duplicated name the zip reader keeps the last entry (logged). Going past `max_total_size` fails the
/// whole bundle.
pub fn read_info_bundle(
    zip: &[u8],
    limits: ExtractLimits,
) -> Result<Vec<(CoreId, String)>, ArchiveError> {
    Ok(info_entries(zip, limits)?
        .into_iter()
        .filter_map(|(id, text)| CoreId::new(&id).map(|id| (id, text)))
        .collect())
}

/// [`read_info_bundle`] with ids as strings, already restricted to `[A-Za-z0-9_-]{1,64}`.
fn info_entries(zip: &[u8], limits: ExtractLimits) -> Result<Vec<(String, String)>, ArchiveError> {
    let mut archive = open(zip, limits)?;
    if archive.has_duplicate_names() {
        tracing::warn!("info.zip : noms d'entrée en double, dernière occurrence gardée");
    }
    let mut wanted = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index_data(index).map_err(invalid)?;
        if !is_plain_file(&entry) {
            continue;
        }
        let Some(id) = info_id(entry.name_raw()) else {
            continue;
        };
        wanted.push((index, id.to_owned(), entry.size()));
    }

    let mut budget = limits.max_total_size;
    let mut out = Vec::with_capacity(wanted.len());
    for (index, id, declared) in wanted {
        let name = format!("{id}_libretro.info");
        if declared > limits.max_entry_size {
            tracing::warn!("info.zip : `{name}` ignoré (trop volumineux)");
            continue;
        }
        // The total is enforced on bytes actually read: an entry may not consume more than what is left.
        let limit = limits.max_entry_size.min(budget);
        match read_entry(&mut archive, index, &name, declared, limit) {
            Ok(data) => {
                budget -= data.len() as u64;
                out.push((id, String::from_utf8_lossy(&data).into_owned()));
            }
            Err(ArchiveError::TooLarge { .. }) if limit < limits.max_entry_size => {
                return Err(ArchiveError::TooLarge {
                    name,
                    limit: limits.max_total_size,
                });
            }
            Err(e) => tracing::warn!("info.zip : `{name}` ignoré : {e}"),
        }
    }
    Ok(out)
}

/// `<id>_libretro.info` at the root, `id` in `[A-Za-z0-9_-]{1,64}` (same rule as `CoreId::new`).
fn info_id(name: &[u8]) -> Option<&str> {
    let name = std::str::from_utf8(name).ok()?;
    let id = name.strip_suffix("_libretro.info")?;
    let valid = !id.is_empty()
        && id.len() <= CoreId::MAX_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    valid.then_some(id)
}

fn is_plain_file(entry: &zip::read::ZipFileEntry<'_>) -> bool {
    // `is_file` already excludes directories and S_IFLNK entries; encrypted ones cannot be read anyway.
    entry.is_file() && !entry.encrypted()
}

fn invalid(e: impl std::fmt::Display) -> ArchiveError {
    ArchiveError::Invalid(e.to_string())
}

fn open(zip: &[u8], limits: ExtractLimits) -> Result<ZipArchive<Cursor<&[u8]>>, ArchiveError> {
    // Before `ZipArchive::new`, which sizes its tables from the announced count.
    if let Some(n) = declared_entries(zip)
        && n > limits.max_entries as u64
    {
        return Err(ArchiveError::TooManyEntries(n));
    }
    let archive = ZipArchive::new(Cursor::new(zip)).map_err(invalid)?;
    if archive.len() > limits.max_entries {
        return Err(ArchiveError::TooManyEntries(archive.len() as u64));
    }
    Ok(archive)
}

/// Reads one entry, at most `limit` bytes, whatever its header says; the size actually read must also match
/// the declared size (the zip crate checks the CRC at the end of the stream).
fn read_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    index: usize,
    name: &str,
    declared: u64,
    limit: u64,
) -> Result<Vec<u8>, ArchiveError> {
    let too_large = || ArchiveError::TooLarge {
        name: name.to_owned(),
        limit,
    };
    let corrupt = |message: String| ArchiveError::Corrupt {
        name: name.to_owned(),
        message,
    };
    if declared > limit {
        return Err(too_large());
    }
    let entry = archive
        .by_index(index)
        .map_err(|e| corrupt(e.to_string()))?;
    let capacity = usize::try_from(declared.min(MAX_INITIAL_CAPACITY)).unwrap_or(0);
    let mut data = Vec::with_capacity(capacity);
    // One byte past the limit so an entry lying about its size is detected rather than silently cut.
    entry
        .take(limit.saturating_add(1))
        .read_to_end(&mut data)
        .map_err(|e| corrupt(e.to_string()))?;
    let read = data.len() as u64;
    if read > limit {
        return Err(too_large());
    }
    if read != declared {
        return Err(corrupt(format!(
            "taille lue {read} différente de la taille annoncée {declared}"
        )));
    }
    Ok(data)
}

/// Copy of `emu-runtime/src/content.rs::declared_entries` on a slice (ADR 0013 §4: no shared crate).
/// Total entry count announced by the end-of-central-directory record (ZIP64 record if the 16-bit field is
/// saturated). `None` when no record is found: `ZipArchive::new` then reports the proper error.
fn declared_entries(zip: &[u8]) -> Option<u64> {
    const EOCD_LEN: usize = 22;
    const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const LOCATOR_LEN: usize = 20;
    // EOCD + maximum comment length.
    let start = zip.len().saturating_sub(EOCD_LEN + 65535);
    let tail = &zip[start..];
    let pos = tail
        .windows(EOCD_SIG.len())
        .rposition(|w| w == EOCD_SIG)
        .filter(|&p| p + EOCD_LEN <= tail.len())?;
    let total = u64::from(u16::from_le_bytes([tail[pos + 10], tail[pos + 11]]));
    if total != 0xFFFF {
        return Some(total);
    }
    // ZIP64: the locator sits right before the EOCD and points at the ZIP64 EOCD record.
    let eocd_at = start + pos;
    let Some(locator) = eocd_at.checked_sub(LOCATOR_LEN).map(|at| &zip[at..eocd_at]) else {
        return Some(total);
    };
    if locator[..4] != [0x50, 0x4b, 0x06, 0x07] {
        return Some(total);
    }
    let offset = u64::from_le_bytes(locator[8..16].try_into().ok()?);
    let record = usize::try_from(offset)
        .ok()
        .and_then(|o| zip.get(o..o.checked_add(56)?));
    match record {
        Some(record) if record[..4] == [0x50, 0x4b, 0x06, 0x06] => {
            Some(u64::from_le_bytes(record[32..40].try_into().ok()?))
        }
        _ => Some(total),
    }
}

#[cfg(test)]
mod tests;
