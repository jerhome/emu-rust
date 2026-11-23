//! Known PlayStation (PS1) BIOS revisions: file name, region and fingerprints (ADR 0021, ADR 0005).
//!
//! Fingerprints only, copied from libretro-database `dat/System.dat` (section « Sony - PlayStation », commit
//! 2a8de0276fd7d3095f2501c3de3799a14c37e910, CC-BY-SA-4.0; see `data/firmware.toml`). Nothing is downloaded: the
//! user provides the files. Revisions absent from that DAT (SCPH-7000, 9000, 9001...) have no hash here: they
//! are recognised by name only ([`ps1_region_for_name`]), never by content. `psxonpsp660.bin` (PSP-derived) and
//! `ps1_rom.bin` (PS3-derived) are valid for every region and so not listed.

/// Region of a console revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ps1Region {
    Japan,
    Usa,
    Europe,
}

/// One known BIOS dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ps1Bios {
    /// Name used by libretro-database (and by the cores that look the file up).
    pub file: &'static str,
    pub region: Ps1Region,
    pub crc32: u32,
    pub md5: &'static str,
    pub sha1: &'static str,
}

/// Every revision with a confirmed fingerprint (several names can share one dump: SCPH-5502 = 5552, 7502 = 9002).
pub const PS1_BIOS: &[Ps1Bios] = &[
    Ps1Bios {
        file: "scph1000.bin",
        region: Ps1Region::Japan,
        crc32: 0x3B601FC8,
        md5: "239665b1a3dade1b5a52c06338011044",
        sha1: "343883a7b555646da8cee54aadd2795b6e7dd070",
    },
    Ps1Bios {
        file: "scph1001.bin",
        region: Ps1Region::Usa,
        crc32: 0x37157331,
        md5: "924e392ed05558ffdb115408c263dccf",
        sha1: "10155d8d6e6e832d6ea66db9bc098321fb5e8ebf",
    },
    Ps1Bios {
        file: "scph1002.bin",
        region: Ps1Region::Europe,
        crc32: 0x9BB87C4B,
        md5: "54847e693405ffeb0359c6287434cbef",
        sha1: "20b98f3d80f11cbf5a7bfd0779b0e63760ecc62c",
    },
    Ps1Bios {
        file: "scph100.bin",
        region: Ps1Region::Japan,
        crc32: 0xF2AF798B,
        md5: "8abc1b549a4a80954addc48ef02c4521",
        sha1: "339a48f4fcf63e10b5b867b8c93cfd40945faf6c",
    },
    Ps1Bios {
        file: "scph101.bin",
        region: Ps1Region::Usa,
        crc32: 0x171BDCEC,
        md5: "6e3735ff4c7dc899ee98981385f6f3d0",
        sha1: "dcffe16bd90a723499ad46c641424981338d8378",
    },
    Ps1Bios {
        file: "scph102A.bin",
        region: Ps1Region::Europe,
        crc32: 0x0BAD7EA9,
        md5: "b10f5e0e3d9eb60e5159690680b1e774",
        sha1: "beb0ac693c0dc26daf5665b3314db81480fa5c7c",
    },
    Ps1Bios {
        file: "scph102B.bin",
        region: Ps1Region::Europe,
        crc32: 0x76B880E5,
        md5: "de93caec13d1a141a40a79f5c86168d6",
        sha1: "dbc7339e5d85827c095764fc077b41f78fd2ecae",
    },
    Ps1Bios {
        file: "scph102C.bin",
        region: Ps1Region::Europe,
        crc32: 0x76B880E5,
        md5: "de93caec13d1a141a40a79f5c86168d6",
        sha1: "dbc7339e5d85827c095764fc077b41f78fd2ecae",
    },
    Ps1Bios {
        file: "scph3000.bin",
        region: Ps1Region::Japan,
        crc32: 0x3539DEF6,
        md5: "849515939161e62f6b866f6853006780",
        sha1: "b06f4a861f74270be819aa2a07db8d0563a7cc4e",
    },
    Ps1Bios {
        file: "scph3500.bin",
        region: Ps1Region::Japan,
        crc32: 0xBC190209,
        md5: "cba733ceeff5aef5c32254f1d617fa62",
        sha1: "e38466a4ba8005fba7e9e3c7b9efeba7205bee3f",
    },
    Ps1Bios {
        file: "scph5000.bin",
        region: Ps1Region::Japan,
        crc32: 0x8C93A399,
        md5: "eb201d2d98251a598af467d4347bb62f",
        sha1: "e340db2696274dda5fdc25e434a914db71e8b02b",
    },
    Ps1Bios {
        file: "scph5500.bin",
        region: Ps1Region::Japan,
        crc32: 0xFF3EEB8C,
        md5: "8dd7d5296a650fac7319bce665a6a53c",
        sha1: "b05def971d8ec59f346f2d9ac21fb742e3eb6917",
    },
    Ps1Bios {
        file: "scph5501.bin",
        region: Ps1Region::Usa,
        crc32: 0x8D8CB7E4,
        md5: "490f666e1afb15b7362b406ed1cea246",
        sha1: "0555c6fae8906f3f09baf5988f00e55f88e9f30b",
    },
    Ps1Bios {
        file: "scph5502.bin",
        region: Ps1Region::Europe,
        crc32: 0xD786F0B9,
        md5: "32736f17079d0b2b7024407c39bd3050",
        sha1: "f6bc2d1f5eb6593de7d089c425ac681d6fffd3f0",
    },
    Ps1Bios {
        file: "scph5552.bin",
        region: Ps1Region::Europe,
        crc32: 0xD786F0B9,
        md5: "32736f17079d0b2b7024407c39bd3050",
        sha1: "f6bc2d1f5eb6593de7d089c425ac681d6fffd3f0",
    },
    Ps1Bios {
        file: "scph7001.bin",
        region: Ps1Region::Usa,
        crc32: 0x502224B6,
        md5: "1e68c231d0896b7eadcad1d7d8e76129",
        sha1: "14df4f6c1e367ce097c11deae21566b4fe5647a9",
    },
    Ps1Bios {
        file: "scph7002.bin",
        region: Ps1Region::Europe,
        crc32: 0x318178BF,
        md5: "b9d9a0286c33dc6b7237bb13cd46fdee",
        sha1: "8d5de56a79954f29e9006929ba3fed9b6a418c1d",
    },
    Ps1Bios {
        file: "scph7003.bin",
        region: Ps1Region::Usa,
        crc32: 0x8D8CB7E4,
        md5: "490f666e1afb15b7362b406ed1cea246",
        sha1: "0555c6fae8906f3f09baf5988f00e55f88e9f30b",
    },
    Ps1Bios {
        file: "scph7502.bin",
        region: Ps1Region::Europe,
        crc32: 0x318178BF,
        md5: "b9d9a0286c33dc6b7237bb13cd46fdee",
        sha1: "8d5de56a79954f29e9006929ba3fed9b6a418c1d",
    },
    Ps1Bios {
        file: "scph9002(7502).bin",
        region: Ps1Region::Europe,
        crc32: 0x318178BF,
        md5: "b9d9a0286c33dc6b7237bb13cd46fdee",
        sha1: "8d5de56a79954f29e9006929ba3fed9b6a418c1d",
    },
];

impl Ps1Region {
    /// File name every core looks the region's BIOS up under (`scph5500/5501/5502.bin`), the target of an alias
    /// (ADR 0021, amendment 2026-11-23).
    pub fn canonical_file(self) -> &'static str {
        match self {
            Self::Japan => "scph5500.bin",
            Self::Usa => "scph5501.bin",
            Self::Europe => "scph5502.bin",
        }
    }

    /// Inverse of [`Self::canonical_file`] (exact, case-sensitive: the names cores search for).
    pub fn from_canonical_file(name: &str) -> Option<Self> {
        [Self::Japan, Self::Usa, Self::Europe]
            .into_iter()
            .find(|r| r.canonical_file() == name)
    }
}

impl Ps1Bios {
    /// Console model for display (`scph7001.bin` -> `SCPH-7001`, `scph102A.bin` -> `SCPH-102A`).
    pub fn model(&self) -> String {
        let stem = self.file.strip_suffix(".bin").unwrap_or(self.file);
        match stem.strip_prefix("scph") {
            Some(rest) => format!("SCPH-{rest}"),
            None => stem.to_owned(),
        }
    }
}

/// `Some(region of the dump)` when `md5` is a known PS1 BIOS of a region other than the one `wanted` (a
/// canonical `scph550x.bin`) is for: the « wrong region » case of `check_required`, whose state stays
/// `FirmwareState::WrongHash`. `None` for an unknown hash, a same-region dump or a non-canonical `wanted`.
pub fn ps1_wrong_region(wanted: &str, md5: &[u8; 16]) -> Option<Ps1Region> {
    let expected = Ps1Region::from_canonical_file(wanted)?;
    let found = lookup(PS1_BIOS, md5)?.region;
    (found != expected).then_some(found)
}

/// Entry of `table` whose md5 is `md5` (the first one for a shared dump). `table` is a parameter so tests can
/// inject fingerprints of fake files.
pub(crate) fn lookup<'a>(table: &'a [Ps1Bios], md5: &[u8; 16]) -> Option<&'a Ps1Bios> {
    let hex: String = md5.iter().map(|b| format!("{b:02x}")).collect();
    table.iter().find(|b| b.md5.eq_ignore_ascii_case(&hex))
}

/// Entry for a md5 (hex, any case). Dumps shared by several names return the first one in [`PS1_BIOS`].
pub fn ps1_bios_for_md5(md5: &str) -> Option<&'static Ps1Bios> {
    let md5 = md5.to_ascii_lowercase();
    PS1_BIOS.iter().find(|b| b.md5 == md5)
}

/// Region of a console model number (`SCPH-7001`, `scph7001.bin`, `scph102B.bin`...), from the name alone.
/// Covers revisions with no known hash (7000, 9000, 9001). Never trust it over [`ps1_bios_for_md5`].
pub fn ps1_region_for_name(name: &str) -> Option<Ps1Region> {
    use Ps1Region::{Europe, Japan, Usa};
    let lower = name.to_ascii_lowercase();
    let rest = lower
        .split_once("scph")?
        .1
        .trim_start_matches(['-', '_', ' ']);
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some(match digits.as_str() {
        "1000" | "3000" | "3500" | "5000" | "5500" | "7000" | "9000" | "100" => Japan,
        "1001" | "5501" | "7001" | "7003" | "9001" | "101" => Usa,
        "1002" | "5502" | "5552" | "7002" | "7502" | "9002" | "102" => Europe,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cores::KnownFirmware;

    fn hex(s: &str) -> [u8; 16] {
        let mut out = [0u8; 16];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
        }
        out
    }

    #[test]
    fn table_matches_firmware_toml() {
        let known = KnownFirmware::builtin();
        for b in PS1_BIOS {
            assert_eq!(known.md5_for(b.file), [hex(b.md5)], "{}", b.file);
            assert_eq!(known.size_for(b.file), Some(524_288), "{}", b.file);
            assert_eq!(b.sha1.len(), 40);
        }
        // Every PS1 `scph*` of firmware.toml is described here.
        for path in known.paths().filter(|p| p.starts_with("scph")) {
            assert!(PS1_BIOS.iter().any(|b| b.file == path), "{path}");
        }
    }

    #[test]
    fn regions_by_hash_and_by_name_agree() {
        for b in PS1_BIOS {
            assert_eq!(ps1_region_for_name(b.file), Some(b.region), "{}", b.file);
        }
        assert_eq!(
            ps1_bios_for_md5("1E68C231D0896B7EADCAD1D7D8E76129").map(|b| (b.file, b.region)),
            Some(("scph7001.bin", Ps1Region::Usa))
        );
        assert_eq!(ps1_bios_for_md5("00"), None);
    }

    #[test]
    fn names_without_hash_and_unknown_names() {
        assert_eq!(ps1_region_for_name("SCPH-7000.bin"), Some(Ps1Region::Japan));
        assert_eq!(ps1_region_for_name("scph9000.bin"), Some(Ps1Region::Japan));
        assert_eq!(ps1_region_for_name("SCPH_9001"), Some(Ps1Region::Usa));
        assert_eq!(ps1_region_for_name("scph102B.bin"), Some(Ps1Region::Europe));
        assert_eq!(ps1_region_for_name("scph9999.bin"), None);
        assert_eq!(ps1_region_for_name("bios.bin"), None);
        assert_eq!(ps1_region_for_name("scph.bin"), None);
    }

    #[test]
    fn the_cores_region_files_are_listed() {
        for (file, region) in [
            ("scph5500.bin", Ps1Region::Japan),
            ("scph5501.bin", Ps1Region::Usa),
            ("scph5502.bin", Ps1Region::Europe),
        ] {
            assert!(
                PS1_BIOS
                    .iter()
                    .any(|b| b.file == file && b.region == region)
            );
        }
    }
}
