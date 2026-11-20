//! Hostile zips against the public extraction API (ADR 0013 §4). Archives are built byte by byte so that every
//! header field can lie. Contract: a clean error, never a panic, never an unbounded allocation.

use std::io::{Cursor, Write};

use emu_assets::cores::{ArchiveError, ExtractLimits, extract_core, read_info_bundle};
use zip::write::SimpleFileOptions;

const NAME: &str = "gambatte_libretro.so";

#[derive(Clone)]
struct Entry<'a> {
    name: &'a [u8],
    data: &'a [u8],
    declared: Option<u32>,
    crc: Option<u32>,
    flags: u16,
    unix_mode: Option<u32>,
}

fn entry<'a>(name: &'a str, data: &'a [u8]) -> Entry<'a> {
    Entry {
        name: name.as_bytes(),
        data,
        declared: None,
        crc: None,
        flags: 0,
        unix_mode: None,
    }
}

/// Stored entries; `count` overrides the end-of-central-directory entry count.
fn build_zip(entries: &[Entry<'_>], count: Option<u16>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for e in entries {
        let offset = out.len() as u32;
        let crc = e.crc.unwrap_or_else(|| crc32fast::hash(e.data));
        let size = e.declared.unwrap_or(e.data.len() as u32);
        let mut common = Vec::new();
        common.extend_from_slice(&20u16.to_le_bytes());
        common.extend_from_slice(&e.flags.to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes());
        common.extend_from_slice(&0u32.to_le_bytes());
        common.extend_from_slice(&crc.to_le_bytes());
        common.extend_from_slice(&size.to_le_bytes());
        common.extend_from_slice(&size.to_le_bytes());
        common.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes());

        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&common);
        out.extend_from_slice(e.name);
        out.extend_from_slice(e.data);

        let made_by: u16 = if e.unix_mode.is_some() {
            3 << 8 | 20
        } else {
            20
        };
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&made_by.to_le_bytes());
        central.extend_from_slice(&common);
        central.extend_from_slice(&[0u8; 6]); // comment length, disk, internal attributes
        central.extend_from_slice(&(e.unix_mode.unwrap_or(0) << 16).to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(e.name);
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    let n = count.unwrap_or(entries.len() as u16);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn deflated_zip(name: &str, data: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer.start_file(name, options).unwrap();
    writer.write_all(data).unwrap();
    writer.finish().unwrap().into_inner()
}

const SMALL: ExtractLimits = ExtractLimits {
    max_entry_size: 1024,
    max_total_size: 4096,
    max_entries: 16,
};

#[test]
fn well_formed_core_zip_is_extracted() {
    let zip = deflated_zip(NAME, b"\x7fELF-not-really");
    let data = extract_core(&zip, NAME, ExtractLimits::CORE).unwrap();
    assert_eq!(data, b"\x7fELF-not-really");
}

#[test]
fn duplicate_entries_are_refused() {
    let zip = build_zip(&[entry(NAME, b"first"), entry(NAME, b"second")], None);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::DuplicateEntry(_))
    ));
    // Duplicate of an unrelated name: the whole archive is refused too.
    let zip = build_zip(
        &[
            entry(NAME, b"ok"),
            entry("a.txt", b"1"),
            entry("a.txt", b"2"),
        ],
        None,
    );
    assert!(extract_core(&zip, NAME, ExtractLimits::CORE).is_err());
}

#[test]
fn lying_sizes_are_errors() {
    // Declares 4 bytes, holds 100: reading yields a mismatch or a CRC error, never a silent truncation.
    let data = [7u8; 100];
    let mut e = entry(NAME, &data);
    e.declared = Some(4);
    let zip = build_zip(&[e], None);
    assert!(extract_core(&zip, NAME, SMALL).is_err());

    // Declares more than the limit while being tiny.
    let mut e = entry(NAME, b"tiny");
    e.declared = Some(u32::MAX);
    let zip = build_zip(&[e], None);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::TooLarge { .. })
    ));

    // Declares more than it holds (within limits).
    let mut e = entry(NAME, b"tiny");
    e.declared = Some(900);
    let zip = build_zip(&[e], None);
    assert!(extract_core(&zip, NAME, SMALL).is_err());

    // Right size, wrong CRC.
    let mut e = entry(NAME, b"payload");
    e.crc = Some(0xdead_beef);
    let zip = build_zip(&[e], None);
    assert!(matches!(
        extract_core(&zip, NAME, SMALL),
        Err(ArchiveError::Corrupt { .. })
    ));
}

#[test]
fn zip_bomb_is_stopped() {
    // 2 MiB of zeros deflates to a few KiB: honest header, but over a 1 KiB limit.
    let zeros = vec![0u8; 2 * 1024 * 1024];
    let zip = deflated_zip(NAME, &zeros);
    assert!(zip.len() < 8 * 1024);
    assert!(matches!(
        extract_core(&zip, NAME, SMALL),
        Err(ArchiveError::TooLarge { .. })
    ));

    // Same bomb with the sizes rewritten to look harmless: reading is bounded by the limit anyway.
    let mut lying = zip.clone();
    let cd = lying
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x01, 0x02])
        .unwrap();
    lying[22..26].copy_from_slice(&512u32.to_le_bytes());
    lying[cd + 24..cd + 28].copy_from_slice(&512u32.to_le_bytes());
    assert!(extract_core(&lying, NAME, SMALL).is_err());

    // Declared above the real production limit (300 MiB of zeros): refused before any decompression.
    let big = deflated_zip(NAME, &vec![0u8; 300 * 1024 * 1024]);
    assert!(matches!(
        extract_core(&big, NAME, ExtractLimits::CORE),
        Err(ArchiveError::TooLarge { .. })
    ));
}

#[test]
fn encrypted_entry_never_matches() {
    let mut e = entry(NAME, b"secret");
    e.flags = 1;
    let zip = build_zip(&[e], None);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::MissingEntry(_))
    ));
}

#[test]
fn symlink_entry_never_matches() {
    let mut link = entry(NAME, b"/etc/passwd");
    link.unix_mode = Some(0o120_777);
    let zip = build_zip(&[link], None);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::MissingEntry(_))
    ));
}

#[test]
fn path_like_names_never_match() {
    for name in [
        "../gambatte_libretro.so",
        "sub/gambatte_libretro.so",
        "/gambatte_libretro.so",
    ] {
        let zip = build_zip(&[entry(name, b"x")], None);
        assert!(
            matches!(
                extract_core(&zip, NAME, ExtractLimits::CORE),
                Err(ArchiveError::MissingEntry(_))
            ),
            "{name}"
        );
    }
}

/// ZIP64 record announcing 1 000 000 entries behind a saturated 16-bit count.
#[test]
fn zip64_million_entries_is_refused_without_allocating() {
    let mut zip = build_zip(&[entry(NAME, b"x")], Some(0xFFFF));
    let eocd = zip.len() - 22;
    let mut record = Vec::new();
    record.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
    record.extend_from_slice(&44u64.to_le_bytes());
    record.extend_from_slice(&45u16.to_le_bytes());
    record.extend_from_slice(&45u16.to_le_bytes());
    record.extend_from_slice(&[0u8; 8]);
    record.extend_from_slice(&1_000_000u64.to_le_bytes());
    record.extend_from_slice(&1_000_000u64.to_le_bytes());
    record.extend_from_slice(&[0u8; 16]);
    let mut locator = Vec::new();
    locator.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
    locator.extend_from_slice(&0u32.to_le_bytes());
    locator.extend_from_slice(&(eocd as u64).to_le_bytes());
    locator.extend_from_slice(&1u32.to_le_bytes());
    let tail = zip.split_off(eocd);
    zip.extend_from_slice(&record);
    zip.extend_from_slice(&locator);
    zip.extend_from_slice(&tail);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::TooManyEntries(1_000_000))
    ));
    assert!(matches!(
        read_info_bundle(&zip, ExtractLimits::INFO_BUNDLE),
        Err(ArchiveError::TooManyEntries(1_000_000))
    ));
}

#[test]
fn reversed_bytes_are_errors() {
    let valid = deflated_zip(NAME, &[1u8; 5000]);
    let mut reversed = valid.clone();
    reversed.reverse();
    assert!(extract_core(&reversed, NAME, ExtractLimits::CORE).is_err());
    assert!(read_info_bundle(&reversed, ExtractLimits::INFO_BUNDLE).is_err());

    // Every truncation and a flip of each byte of the headers: errors or data, never a panic.
    for cut in 0..valid.len() {
        let _ = extract_core(&valid[..cut], NAME, ExtractLimits::CORE);
    }
    for i in (0..valid.len()).step_by(7) {
        let mut flipped = valid.clone();
        flipped[i] ^= 0xFF;
        let _ = extract_core(&flipped, NAME, ExtractLimits::CORE);
        let _ = read_info_bundle(&flipped, ExtractLimits::INFO_BUNDLE);
    }
    assert!(extract_core(&[], NAME, ExtractLimits::CORE).is_err());
    assert!(extract_core(b"PK\x05\x06", NAME, ExtractLimits::CORE).is_err());
}

#[test]
fn hostile_info_bundle_does_not_poison_good_entries() {
    let mut huge = entry("huge_libretro.info", b"x");
    huge.declared = Some(u32::MAX);
    let mut bad_crc = entry("badcrc_libretro.info", b"display_name = \"x\"");
    bad_crc.crc = Some(1);
    let zip = build_zip(
        &[
            entry("good_libretro.info", b"display_name = \"Good\""),
            huge,
            bad_crc,
            entry("../evil_libretro.info", b"x"),
            entry("dir/nested_libretro.info", b"x"),
        ],
        None,
    );
    let bundle = read_info_bundle(&zip, ExtractLimits::INFO_BUNDLE).unwrap();
    let ids: Vec<_> = bundle.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["good"]);
}
