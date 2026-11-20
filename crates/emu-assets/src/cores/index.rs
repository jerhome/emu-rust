use std::collections::BTreeMap;
use std::fmt;

use super::{CoreId, Platform};

/// Lines kept from an index; the rest is ignored with a warning (the real index has ≈ 300 lines).
pub const MAX_INDEX_ENTRIES: usize = 4096;

/// Build date of a nightly, from `.index-extended` (`YYYY-MM-DD`). Ordered chronologically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl BuildDate {
    /// Strict `YYYY-MM-DD`, month 1–12, day 1–31.
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        let shape = b.len() == 10
            && b[4] == b'-'
            && b[7] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
        if !shape {
            return None;
        }
        let year = s[0..4].parse().ok()?;
        let month = s[5..7].parse().ok()?;
        let day = s[8..10].parse().ok()?;
        ((1..=12).contains(&month) && (1..=31).contains(&day)).then_some(Self { year, month, day })
    }
}

impl fmt::Display for BuildDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// One core available on the buildbot for a platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCore {
    pub id: CoreId,
    /// `<id>_libretro.<ext>.zip`, exactly as listed.
    pub file_name: String,
    pub date: BuildDate,
    /// CRC32 of the **core file inside the zip** (not of the zip): checked after extraction.
    pub crc32: u32,
}

/// Parses `.index-extended` (`<YYYY-MM-DD> <crc32 hex, 8 digits> <id>_libretro.<ext>.zip` per line).
/// Lines that do not match exactly — other extension than the platform's, invalid id, bad date or CRC —
/// are skipped (`debug!`); never fails. Duplicated ids: the last line wins. Sorted by id.
pub fn parse_index_extended(text: &str, platform: Platform) -> Vec<RemoteCore> {
    let mut by_id: BTreeMap<CoreId, RemoteCore> = BTreeMap::new();
    let mut skipped = 0usize;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(line, platform) {
            Some(core) if by_id.len() < MAX_INDEX_ENTRIES || by_id.contains_key(&core.id) => {
                by_id.insert(core.id.clone(), core);
            }
            Some(_) => skipped += 1,
            None => tracing::debug!("index : ligne ignorée"),
        }
    }
    if skipped > 0 {
        tracing::warn!("index : {skipped} entrées ignorées au-delà de {MAX_INDEX_ENTRIES}");
    }
    by_id.into_values().collect()
}

/// Exactly three space-separated fields; nothing from the line is logged (untrusted, possibly huge).
fn parse_line(line: &str, platform: Platform) -> Option<RemoteCore> {
    let mut fields = line.split(' ');
    let (date, crc, file) = (fields.next()?, fields.next()?, fields.next()?);
    if fields.next().is_some() {
        return None;
    }
    let date = BuildDate::parse(date)?;
    if crc.len() != 8 || !crc.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let crc32 = u32::from_str_radix(crc, 16).ok()?;
    let library = file.strip_suffix(".zip")?;
    let id = CoreId::from_library_file(library, platform.library_extension())?;
    Some(RemoteCore {
        id,
        file_name: file.to_owned(),
        date,
        crc32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Line format of the real linux/x86_64 index; the gambatte line is the one quoted in ADR 0013, the others
    // are synthetic (upper-case hex included).
    const SAMPLE: &str = "2026-11-16 e09352e9 gambatte_libretro.so.zip\n\
        2026-11-18 0a1b2c3d genesis_plus_gx_libretro.so.zip\n\
        2026-11-17 DEADBEEF mesen-s_libretro.so.zip\n";

    fn ids(cores: &[RemoteCore]) -> Vec<&str> {
        cores.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn parses_real_lines() {
        let cores = parse_index_extended(SAMPLE, Platform::LinuxX86_64);
        assert_eq!(ids(&cores), ["gambatte", "genesis_plus_gx", "mesen-s"]);
        let g = &cores[0];
        assert_eq!(g.file_name, "gambatte_libretro.so.zip");
        assert_eq!(g.crc32, 0xe093_52e9);
        assert_eq!(g.date.to_string(), "2026-11-16");
        assert_eq!(cores[2].crc32, 0xdead_beef);
    }

    #[test]
    fn other_platform_extension_is_skipped() {
        assert!(parse_index_extended(SAMPLE, Platform::WindowsX86_64).is_empty());
        let win = "2026-11-16 e09352e9 gambatte_libretro.dll.zip\r\n";
        assert_eq!(
            ids(&parse_index_extended(win, Platform::WindowsX86_64)),
            ["gambatte"]
        );
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let text = "\
2026-11-16 e09352e9 ../x_libretro.so.zip
2026-11-16 e09352e9 a/b_libretro.so.zip
2026-11-16 e09352e9 a\\b_libretro.so.zip
2026-11-16 e09352e9 .._libretro.so.zip
2026-11-16 e09352eZ bad_crc_libretro.so.zip
2026-11-16 e09352e bad_crc_libretro.so.zip
2026-11-16 +09352e9 bad_crc_libretro.so.zip
2026-13-16 e09352e9 bad_date_libretro.so.zip
2026-11-00 e09352e9 bad_date_libretro.so.zip
26-11-16 e09352e9 bad_date_libretro.so.zip
2026-11-16  e09352e9 two_spaces_libretro.so.zip
2026-11-16 e09352e9 extra_libretro.so.zip trailing
2026-11-16 e09352e9 nozip_libretro.so
2026-11-16 e09352e9 noprefix.so.zip
garbage
\u{0}\u{ff}

2026-11-16 e09352e9 good_libretro.so.zip
";
        assert_eq!(
            ids(&parse_index_extended(text, Platform::LinuxX86_64)),
            ["good"]
        );
    }

    #[test]
    fn duplicates_last_wins() {
        let text = "2026-11-16 00000001 a_libretro.so.zip\n2026-11-17 00000002 a_libretro.so.zip\n";
        let cores = parse_index_extended(text, Platform::LinuxX86_64);
        assert_eq!(cores.len(), 1);
        assert_eq!(cores[0].crc32, 2);
    }

    #[test]
    fn entry_count_is_bounded() {
        let mut text = String::new();
        for i in 0..MAX_INDEX_ENTRIES + 10 {
            text.push_str(&format!("2026-11-16 00000000 c{i}_libretro.so.zip\n"));
        }
        assert_eq!(
            parse_index_extended(&text, Platform::LinuxX86_64).len(),
            MAX_INDEX_ENTRIES
        );
    }

    #[test]
    fn build_date() {
        assert!(BuildDate::parse("2026-11-16").unwrap() < BuildDate::parse("2026-12-01").unwrap());
        for bad in [
            "2026-1-16",
            "2026/11/16",
            "２０２６-11-16",
            "2026-11-16 ",
            "",
            "2026-00-10",
        ] {
            assert!(BuildDate::parse(bad).is_none(), "{bad}");
        }
    }
}
