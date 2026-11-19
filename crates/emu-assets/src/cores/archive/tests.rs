//! Malicious archives are built byte by byte so that every header field can lie.

use std::io::Write;

use zip::write::SimpleFileOptions;

use super::*;

const NAME: &str = "gambatte_libretro.so";

#[derive(Clone)]
struct Entry<'a> {
    name: &'a [u8],
    data: &'a [u8],
    /// Uncompressed and compressed size written in the headers (default: real size).
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

/// Stored entries; `count` overrides the EOCD entry count.
fn build_zip(entries: &[Entry<'_>], count: Option<u16>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for e in entries {
        let offset = out.len() as u32;
        let crc = e.crc.unwrap_or_else(|| crc32fast::hash(e.data));
        let size = e.declared.unwrap_or(e.data.len() as u32);
        let mut common = Vec::new();
        common.extend_from_slice(&20u16.to_le_bytes()); // version needed
        common.extend_from_slice(&e.flags.to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes()); // stored
        common.extend_from_slice(&0u32.to_le_bytes()); // time + date
        common.extend_from_slice(&crc.to_le_bytes());
        common.extend_from_slice(&size.to_le_bytes());
        common.extend_from_slice(&size.to_le_bytes());
        common.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes()); // extra length

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
        central.extend_from_slice(&0u16.to_le_bytes()); // comment length
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&(e.unix_mode.unwrap_or(0) << 16).to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(e.name);
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    let n = count.unwrap_or(entries.len() as u16);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// Deflated archive written by the zip crate (for compression ratio cases).
fn deflated_zip(name: &str, data: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer.start_file(name, options).unwrap();
    writer.write_all(data).unwrap();
    writer.finish().unwrap().into_inner()
}

/// Rewrites the uncompressed size of the single entry of `zip` in both headers.
fn patch_uncompressed_size(zip: &mut [u8], size: u32) {
    zip[22..26].copy_from_slice(&size.to_le_bytes());
    let cd = zip
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x01, 0x02])
        .unwrap();
    zip[cd + 24..cd + 28].copy_from_slice(&size.to_le_bytes());
}

const SMALL: ExtractLimits = ExtractLimits {
    max_entry_size: 1024,
    max_total_size: 4096,
    max_entries: 16,
};

#[test]
fn extracts_the_exact_root_entry() {
    let zip = build_zip(
        &[
            entry("readme.txt", b"x"),
            entry(NAME, b"core bytes"),
            entry("other_libretro.so", b"no"),
        ],
        None,
    );
    assert_eq!(
        extract_core(&zip, NAME, ExtractLimits::CORE).unwrap(),
        b"core bytes"
    );
}

#[test]
fn deflated_entry_is_extracted() {
    let data = vec![7u8; 100_000];
    let zip = deflated_zip(NAME, &data);
    assert_eq!(extract_core(&zip, NAME, ExtractLimits::CORE).unwrap(), data);
}

#[test]
fn names_are_compared_never_resolved() {
    for name in [
        "../gambatte_libretro.so",
        "/gambatte_libretro.so",
        "C:\\gambatte_libretro.so",
        "dir/gambatte_libretro.so",
        "dir\\gambatte_libretro.so",
        "./gambatte_libretro.so",
        "GAMBATTE_LIBRETRO.SO",
        "gambatte_libretro.so/",
        "gambatte_libretro.so\0",
    ] {
        let zip = build_zip(&[entry(name, b"evil")], None);
        assert!(
            matches!(
                extract_core(&zip, NAME, ExtractLimits::CORE),
                Err(ArchiveError::MissingEntry(_))
            ),
            "{name}"
        );
    }
}

#[test]
fn duplicate_names_are_refused() {
    let zip = build_zip(&[entry(NAME, b"a"), entry(NAME, b"b")], None);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::DuplicateEntry(_))
    ));
    let zip = build_zip(
        &[
            entry("x.txt", b"a"),
            entry("x.txt", b"b"),
            entry(NAME, b"core"),
        ],
        None,
    );
    // Any duplicated name makes the archive suspicious (the reader cannot tell which one it hides).
    assert!(extract_core(&zip, NAME, ExtractLimits::CORE).is_err());
    let zip = build_zip(&[entry("x.txt", b"a"), entry("x.txt", b"b")], None);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::Invalid(_))
    ));
}

#[test]
fn symlink_and_encrypted_entries_never_match() {
    let mut link = entry(NAME, b"/etc/passwd");
    link.unix_mode = Some(0o120_777);
    let zip = build_zip(&[link], None);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::MissingEntry(_))
    ));

    let mut encrypted = entry(NAME, b"secret");
    encrypted.flags = 1;
    let zip = build_zip(&[encrypted], None);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::MissingEntry(_))
    ));

    // The zip reader keeps one entry per name: a symlink shadowing the real file is a duplicate.
    let mut link = entry(NAME, b"/etc/passwd");
    link.unix_mode = Some(0o120_777);
    let mut file = entry(NAME, b"real");
    file.unix_mode = Some(0o100_644);
    let zip = build_zip(&[file, link], None);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::DuplicateEntry(_))
    ));
}

#[test]
fn declared_size_over_the_limit_is_refused() {
    let zip = build_zip(&[entry(NAME, &[0u8; 2000])], None);
    assert!(matches!(
        extract_core(&zip, NAME, SMALL),
        Err(ArchiveError::TooLarge { limit: 1024, .. })
    ));
}

#[test]
fn total_limit_also_bounds_a_single_core() {
    let limits = ExtractLimits {
        max_total_size: 100,
        ..SMALL
    };
    let zip = build_zip(&[entry(NAME, &[0u8; 500])], None);
    assert!(matches!(
        extract_core(&zip, NAME, limits),
        Err(ArchiveError::TooLarge { limit: 100, .. })
    ));
}

#[test]
fn stored_entry_with_lying_size_is_corrupt() {
    let mut lying = entry(NAME, &[1u8; 600]);
    lying.declared = Some(10);
    let zip = build_zip(&[lying], None);
    assert!(extract_core(&zip, NAME, SMALL).is_err());
}

#[test]
fn deflate_bomb_stops_at_the_limit() {
    // 4 MiB of zeros compress about 1000:1.
    let data = vec![0u8; 4 * 1024 * 1024];
    let mut zip = deflated_zip(NAME, &data);
    assert!(
        zip.len() < data.len() / 500,
        "ratio {}",
        data.len() / zip.len()
    );
    assert!(matches!(
        extract_core(&zip, NAME, SMALL),
        Err(ArchiveError::TooLarge { .. })
    ));
    // Same bomb announcing a small size: the real size is still bounded (cut, then CRC or size mismatch).
    patch_uncompressed_size(&mut zip, 100);
    assert!(matches!(
        extract_core(&zip, NAME, SMALL),
        Err(ArchiveError::TooLarge { .. } | ArchiveError::Corrupt { .. })
    ));
}

#[test]
fn deflated_entry_larger_than_declared_is_refused() {
    let data = vec![3u8; 500];
    let mut zip = deflated_zip(NAME, &data);
    patch_uncompressed_size(&mut zip, 100);
    assert!(extract_core(&zip, NAME, SMALL).is_err());
    patch_uncompressed_size(&mut zip, 800);
    assert!(extract_core(&zip, NAME, SMALL).is_err());
}

#[test]
fn wrong_entry_crc_is_corrupt() {
    let mut bad = entry(NAME, b"core bytes");
    bad.crc = Some(0xdead_beef);
    let zip = build_zip(&[bad], None);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::Corrupt { .. })
    ));
}

#[test]
fn announced_entry_count_is_checked_before_parsing() {
    let zip = build_zip(&[entry(NAME, b"x")], Some(5000));
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::TooManyEntries(5000))
    ));
}

/// 70 000 entries announced through a ZIP64 end-of-central-directory record.
#[test]
fn zip64_entry_count_is_checked() {
    let mut zip = build_zip(&[entry(NAME, b"x")], Some(0xFFFF));
    let eocd = zip.len() - 22;
    let mut record = Vec::new();
    record.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
    record.extend_from_slice(&44u64.to_le_bytes());
    record.extend_from_slice(&45u16.to_le_bytes());
    record.extend_from_slice(&45u16.to_le_bytes());
    record.extend_from_slice(&0u32.to_le_bytes());
    record.extend_from_slice(&0u32.to_le_bytes());
    record.extend_from_slice(&70_000u64.to_le_bytes());
    record.extend_from_slice(&70_000u64.to_le_bytes());
    record.extend_from_slice(&0u64.to_le_bytes());
    record.extend_from_slice(&0u64.to_le_bytes());
    let record_at = eocd as u64;
    let mut locator = Vec::new();
    locator.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
    locator.extend_from_slice(&0u32.to_le_bytes());
    locator.extend_from_slice(&record_at.to_le_bytes());
    locator.extend_from_slice(&1u32.to_le_bytes());
    let tail = zip.split_off(eocd);
    zip.extend_from_slice(&record);
    zip.extend_from_slice(&locator);
    zip.extend_from_slice(&tail);
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::TooManyEntries(70_000))
    ));

    // Lying ZIP64 locator (offset past the end): the saturated 16-bit count is used.
    let pos = zip.len() - 22 - 20 + 8;
    zip[pos..pos + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        extract_core(&zip, NAME, ExtractLimits::CORE),
        Err(ArchiveError::TooManyEntries(0xFFFF))
    ));
}

#[test]
fn garbage_and_truncated_archives_are_errors() {
    assert!(matches!(
        extract_core(b"", NAME, ExtractLimits::CORE),
        Err(ArchiveError::Invalid(_))
    ));
    assert!(matches!(
        extract_core(b"<html>not found</html>", NAME, ExtractLimits::CORE),
        Err(ArchiveError::Invalid(_))
    ));
    let zip = build_zip(&[entry(NAME, &[5u8; 300])], None);
    for cut in [1, 10, 22, 100, zip.len() - 1] {
        assert!(extract_core(&zip[..cut], NAME, ExtractLimits::CORE).is_err());
    }
}

/// Every single-byte corruption is an error or a correct result, never a panic.
#[test]
fn byte_flips_never_panic() {
    let zip = build_zip(
        &[entry("readme.txt", b"hello"), entry(NAME, &[9u8; 64])],
        None,
    );
    for i in 0..zip.len() {
        for flip in [0x01u8, 0x80, 0xFF] {
            let mut bad = zip.clone();
            bad[i] ^= flip;
            if let Ok(data) = extract_core(&bad, NAME, SMALL) {
                assert!(data.len() <= 1024);
            }
            let _ = info_entries(&bad, SMALL);
        }
    }
}

#[test]
fn info_bundle_keeps_valid_root_entries() {
    let big = vec![b'x'; 2000];
    let zip = build_zip(
        &[
            entry("gambatte_libretro.info", b"display_name = \"Gambatte\""),
            entry("dir/nested_libretro.info", b"no"),
            entry("../up_libretro.info", b"no"),
            entry("bad.id_libretro.info", b"no"),
            entry("_libretro.info", b"no"),
            entry("readme.txt", b"no"),
            entry("big_libretro.info", &big),
            entry("gambatte_libretro.info", b"second copy"),
            entry("mesen-s_libretro.info", b"\xffok"),
        ],
        None,
    );
    let got = info_entries(&zip, SMALL).unwrap();
    assert_eq!(
        got,
        vec![
            // Duplicated name: the zip reader keeps the last entry.
            ("gambatte".to_owned(), "second copy".to_owned()),
            ("mesen-s".to_owned(), "\u{fffd}ok".to_owned()),
        ]
    );
}

#[test]
fn info_bundle_skips_a_corrupt_entry() {
    let mut bad = entry("bad_libretro.info", b"corrupt");
    bad.crc = Some(1);
    let zip = build_zip(&[bad, entry("good_libretro.info", b"ok")], None);
    assert_eq!(
        info_entries(&zip, SMALL).unwrap(),
        vec![("good".to_owned(), "ok".to_owned())]
    );
}

#[test]
fn info_bundle_total_is_bounded() {
    let chunk = vec![b'a'; 1000];
    let names: Vec<String> = (0..6).map(|i| format!("c{i}_libretro.info")).collect();
    let entries: Vec<Entry<'_>> = names.iter().map(|n| entry(n, &chunk)).collect();
    let zip = build_zip(&entries, None);
    assert!(matches!(
        info_entries(&zip, SMALL),
        Err(ArchiveError::TooLarge { limit: 4096, .. })
    ));
    let zip = build_zip(&entries[..4], None);
    assert_eq!(info_entries(&zip, SMALL).unwrap().len(), 4);
}

#[test]
fn info_bundle_entry_count_is_bounded() {
    let names: Vec<String> = (0..17).map(|i| format!("c{i}_libretro.info")).collect();
    let entries: Vec<Entry<'_>> = names.iter().map(|n| entry(n, b"x")).collect();
    let zip = build_zip(&entries, None);
    assert!(matches!(
        info_entries(&zip, SMALL),
        Err(ArchiveError::TooManyEntries(17))
    ));
}

#[test]
fn info_ids_follow_core_id_rules() {
    assert_eq!(
        info_id(b"genesis_plus_gx_libretro.info"),
        Some("genesis_plus_gx")
    );
    assert_eq!(info_id(b"mesen-s_libretro.info"), Some("mesen-s"));
    let long = format!("{}_libretro.info", "a".repeat(65));
    assert_eq!(info_id(long.as_bytes()), None);
    assert_eq!(info_id(b"a b_libretro.info"), None);
    assert_eq!(info_id(b"x_libretro.info.bak"), None);
    assert_eq!(info_id(b"x/y_libretro.info"), None);
}
