use super::CoreId;

/// Targets with buildbot cores (ADR 0012). Anything else: core manager unavailable, manual cores only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    LinuxX86_64,
    WindowsX86_64,
    MacosX86_64,
}

impl Platform {
    /// From `cfg(target_os, target_arch)`; `None` on unsupported targets (e.g. aarch64).
    pub fn current() -> Option<Self> {
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Some(Self::LinuxX86_64)
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Some(Self::WindowsX86_64)
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Some(Self::MacosX86_64)
        } else {
            None
        }
    }

    /// Path below `nightly/`: `linux/x86_64`, `windows/x86_64`, `apple/osx/x86_64`.
    pub fn buildbot_dir(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "linux/x86_64",
            Self::WindowsX86_64 => "windows/x86_64",
            Self::MacosX86_64 => "apple/osx/x86_64",
        }
    }

    /// `so`, `dll`, `dylib`.
    pub fn library_extension(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "so",
            Self::WindowsX86_64 => "dll",
            Self::MacosX86_64 => "dylib",
        }
    }

    /// Folder under `<data>/cores/` (ADR 0006): `linux-x86_64`, `windows-x86_64`, `macos-x86_64`.
    /// Must stay equal to `<std::env::consts::OS>-<ARCH>`, which emu-ui's `DataDirs::cores` uses.
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "linux-x86_64",
            Self::WindowsX86_64 => "windows-x86_64",
            Self::MacosX86_64 => "macos-x86_64",
        }
    }

    /// `<id>_libretro.<ext>`.
    pub fn core_file_name(self, id: &CoreId) -> String {
        format!("{id}_libretro.{}", self.library_extension())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_matches_the_data_dir_naming() {
        match Platform::current() {
            Some(p) => assert_eq!(
                p.dir_name(),
                format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
            ),
            None => assert_ne!(std::env::consts::ARCH, "x86_64"),
        }
    }

    #[test]
    fn names() {
        let id = CoreId::new("gambatte").unwrap();
        assert_eq!(
            Platform::LinuxX86_64.core_file_name(&id),
            "gambatte_libretro.so"
        );
        assert_eq!(
            Platform::WindowsX86_64.core_file_name(&id),
            "gambatte_libretro.dll"
        );
        assert_eq!(
            Platform::MacosX86_64.core_file_name(&id),
            "gambatte_libretro.dylib"
        );
        assert_eq!(Platform::MacosX86_64.buildbot_dir(), "apple/osx/x86_64");
        assert_eq!(Platform::WindowsX86_64.dir_name(), "windows-x86_64");
    }
}
