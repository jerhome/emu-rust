use std::io::{self, Read};
use std::path::PathBuf;

use sha1::{Digest, Sha1};

use crate::{HeaderRule, LibraryError};

/// Larger files are listed but not hashed (same bound as `emu_runtime::MAX_CONTENT_SIZE`).
pub const MAX_HASH_SIZE: u64 = 512 * 1024 * 1024;

const BUFFER_SIZE: usize = 64 * 1024;
const INES_MAGIC: &[u8; 4] = b"NES\x1A";
const INES_HEADER: u64 = 16;
const SNES_COPIER_HEADER: u64 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hashes {
    pub size: u64,
    pub crc32: u32,
    pub sha1: [u8; 20],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RomHashes {
    pub full: Hashes,
    /// Present only when `header` applied to this file.
    pub headerless: Option<Hashes>,
}

struct Hasher {
    crc: crc32fast::Hasher,
    sha1: Sha1,
    size: u64,
}

impl Hasher {
    fn new() -> Self {
        Self {
            crc: crc32fast::Hasher::new(),
            sha1: Sha1::new(),
            size: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.crc.update(data);
        self.sha1.update(data);
        self.size += data.len() as u64;
    }

    fn finish(self) -> Hashes {
        Hashes {
            size: self.size,
            crc32: self.crc.finalize(),
            sha1: self.sha1.finalize().into(),
        }
    }
}

/// Error without a path: the caller (scanner) knows which file it was reading and replaces it.
fn size_error(message: &str) -> LibraryError {
    LibraryError::Io {
        path: PathBuf::new(),
        source: io::Error::new(io::ErrorKind::InvalidData, message.to_owned()),
    }
}

/// Streams `reader` once (fixed buffer, no whole-file allocation). `len` is the announced size (needed by
/// `HeaderRule::SnesCopier`); reading more than `len` or more than `max` bytes is an error, never a truncation.
pub fn hash_reader(
    mut reader: impl Read,
    len: u64,
    header: Option<HeaderRule>,
    max: u64,
) -> Result<RomHashes, LibraryError> {
    if len > max {
        return Err(LibraryError::TooLarge {
            path: PathBuf::new(),
        });
    }
    // The headerless hasher always skips `skip` bytes; whether its result is kept is decided at the end
    // (iNES needs the first 4 bytes), so the data is still read only once.
    let skip = match header {
        Some(HeaderRule::INes) => Some(INES_HEADER),
        Some(HeaderRule::SnesCopier) if len % 1024 == SNES_COPIER_HEADER => {
            Some(SNES_COPIER_HEADER)
        }
        _ => None,
    };
    let mut full = Hasher::new();
    let mut stripped = skip.map(|_| Hasher::new());
    let mut magic = [0u8; 4];
    let mut buf = vec![0u8; BUFFER_SIZE];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(LibraryError::Io {
                    path: PathBuf::new(),
                    source: e,
                });
            }
        };
        let offset = full.size;
        let end = offset + n as u64;
        if end > max {
            return Err(LibraryError::TooLarge {
                path: PathBuf::new(),
            });
        }
        if end > len {
            return Err(size_error("taille supérieure à celle annoncée"));
        }
        let chunk = &buf[..n];
        if offset < 4 {
            let from = offset as usize;
            let take = (4 - from).min(n);
            magic[from..from + take].copy_from_slice(&chunk[..take]);
        }
        if let (Some(skip), Some(h)) = (skip, stripped.as_mut())
            && end > skip
        {
            let start = skip.saturating_sub(offset) as usize;
            h.update(&chunk[start..]);
        }
        full.update(chunk);
    }
    if full.size != len {
        return Err(size_error("taille inférieure à celle annoncée"));
    }
    let applies = match header {
        Some(HeaderRule::INes) => &magic == INES_MAGIC && full.size >= INES_HEADER,
        Some(HeaderRule::SnesCopier) => skip.is_some(),
        None => false,
    };
    Ok(RomHashes {
        full: full.finish(),
        headerless: stripped.filter(|_| applies).map(Hasher::finish),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// Delivers at most `n` bytes per `read` to exercise chunk boundaries.
    struct Trickle<'a>(&'a [u8], usize);
    impl Read for Trickle<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = self.0.len().min(self.1).min(out.len());
            out[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }

    fn plain(data: &[u8]) -> Hashes {
        hash_reader(data, data.len() as u64, None, u64::MAX)
            .unwrap()
            .full
    }

    #[test]
    fn reference_values() {
        let h = plain(b"123456789");
        assert_eq!(h.crc32, 0xCBF4_3926);
        assert_eq!(h.size, 9);
        assert_eq!(
            hex(&plain(b"abc").sha1),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        let empty = plain(b"");
        assert_eq!(empty.crc32, 0);
        assert_eq!(hex(&empty.sha1), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn ines_header_is_stripped_only_with_magic() {
        let mut rom = b"NES\x1A".to_vec();
        rom.extend_from_slice(&[0u8; 12]);
        rom.extend_from_slice(b"123456789");
        for step in [1, 3, 5, 17, BUFFER_SIZE] {
            let h = hash_reader(
                Trickle(&rom, step),
                rom.len() as u64,
                Some(HeaderRule::INes),
                u64::MAX,
            )
            .unwrap();
            assert_eq!(h.full, plain(&rom));
            assert_eq!(h.headerless, Some(plain(b"123456789")), "step {step}");
        }
        let raw = [7u8; 40];
        let h = hash_reader(&raw[..], 40, Some(HeaderRule::INes), u64::MAX).unwrap();
        assert_eq!(h.headerless, None);
        // Magic but shorter than the header: nothing to strip.
        let h = hash_reader(&b"NES\x1A"[..], 4, Some(HeaderRule::INes), u64::MAX).unwrap();
        assert_eq!(h.headerless, None);
    }

    #[test]
    fn snes_copier_header_depends_on_size() {
        let mut rom = vec![0xAAu8; 512];
        rom.extend(std::iter::repeat_n(0x55u8, 1024));
        for step in [100, 511, 513, BUFFER_SIZE] {
            let h = hash_reader(
                Trickle(&rom, step),
                rom.len() as u64,
                Some(HeaderRule::SnesCopier),
                u64::MAX,
            )
            .unwrap();
            assert_eq!(h.headerless, Some(plain(&rom[512..])), "step {step}");
        }
        let h = hash_reader(&rom[512..], 1024, Some(HeaderRule::SnesCopier), u64::MAX).unwrap();
        assert_eq!(h.headerless, None);
        let h = hash_reader(&rom[..512], 512, Some(HeaderRule::SnesCopier), u64::MAX).unwrap();
        assert_eq!(h.headerless.unwrap().size, 0);
    }

    #[test]
    fn size_mismatches_are_errors() {
        let data = [1u8; 100];
        assert!(matches!(
            hash_reader(&data[..], 100, None, 99),
            Err(LibraryError::TooLarge { .. })
        ));
        // Announced 10, delivers 100: never silently truncated.
        assert!(hash_reader(&data[..], 10, None, 1000).is_err());
        // Lying about the size to pass `max`.
        assert!(matches!(
            hash_reader(&data[..], 10, None, 50),
            Err(LibraryError::TooLarge { .. }) | Err(LibraryError::Io { .. })
        ));
        assert!(hash_reader(&data[..], 101, None, 1000).is_err());
        assert!(hash_reader(&data[..], 100, None, 100).is_ok());
    }
}
