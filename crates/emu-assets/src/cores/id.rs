use std::fmt;

/// Core identifier as defined by ADR 0011 (`genesis_plus_gx_libretro.so` → `genesis_plus_gx`).
/// Only `[a-z0-9_-]`, 1 to 64 bytes: safe as a single path component on every target, which is why every
/// name coming from the network goes through [`CoreId::new`] before touching the file system. Lowercase only, so
/// that two ids never name the same file on a case-insensitive file system (Windows, macOS).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoreId(String);

impl CoreId {
    pub const MAX_LEN: usize = 64;

    /// `None` if `id` is empty, too long or has a character outside `[a-z0-9_-]`.
    pub fn new(id: &str) -> Option<Self> {
        let valid = !id.is_empty()
            && id.len() <= Self::MAX_LEN
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
        valid.then(|| Self(id.to_owned()))
    }

    /// `<id>_libretro.<ext>` → id, for a file name of the given library extension (`so`, `dll`, `dylib`).
    pub fn from_library_file(file_name: &str, extension: &str) -> Option<Self> {
        let stem = file_name
            .strip_suffix(extension)?
            .strip_suffix('.')?
            .strip_suffix("_libretro")?;
        Self::new(stem)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CoreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_only_safe_names() {
        for ok in [
            "gambatte",
            "genesis_plus_gx",
            "mesen-s",
            "a9",
            &"a".repeat(64),
        ] {
            assert_eq!(CoreId::new(ok).unwrap().as_str(), ok);
        }
        for bad in [
            "",
            "..",
            "a/b",
            "a\\b",
            "a.b",
            "a b",
            "c:",
            "é",
            &"a".repeat(65),
            "x\0",
            "A9",
            "Gambatte",
        ] {
            assert!(CoreId::new(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn from_library_file() {
        let id = CoreId::from_library_file("genesis_plus_gx_libretro.so", "so").unwrap();
        assert_eq!(id.as_str(), "genesis_plus_gx");
        assert_eq!(id.to_string(), "genesis_plus_gx");
        for bad in [
            "gambatte_libretro.dll",
            "gambatte_libretroso",
            "gambatte.so",
            "_libretro.so",
            "a.b_libretro.so",
            "Gambatte_libretro.so",
            "gambatte_libretro.so.prev",
            "gambatte_libretro.so.zip",
        ] {
            assert!(CoreId::from_library_file(bad, "so").is_none(), "{bad}");
        }
    }
}
