//! Persistence helpers: atomic writes, save layout, savestate container (ADR 0011).

use std::io;
use std::path::{Path, PathBuf};

/// Savestate slots are `0..STATE_SLOTS`.
pub const STATE_SLOTS: u8 = 10;

/// Magic at offset 0 of an emu-rust savestate file.
pub const STATE_MAGIC: [u8; 8] = *b"EMURSTAT";
/// Container version written by this build; `decode_state` rejects anything else.
pub const STATE_FORMAT_VERSION: u32 = 1;

/// Permissions of a file created by `write_atomic`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FileMode {
    /// Process umask (saves, states).
    #[default]
    Default,
    /// `0o600` on Unix from creation (no window where the file is readable by others); ignored on Windows.
    /// For `config.toml`, which will hold credentials from phase 4 (ADR 0006).
    Private,
}

/// Writes `data` to `path` atomically: parent directories created, temporary file in the same directory
/// (`.<name>.tmp-<pid>`), `sync_all`, `rename` over `path`, then fsync of the directory on Unix.
/// A crash leaves either the old or the new file, never a truncated one. The temporary file is removed on error.
pub fn write_atomic(path: &Path, data: &[u8], mode: FileMode) -> io::Result<()> {
    todo!("{} {} {mode:?}", path.display(), data.len())
}

/// Stable identifier of a core from its library path: file stem, `_libretro` suffix and a leading `lib`
/// (if the rest still ends with `_libretro`) removed, characters outside `[A-Za-z0-9_-]` replaced by `_`.
/// `genesis_plus_gx_libretro.so` → `genesis_plus_gx`. Used for save folders and per-core config.
pub fn core_id_from_path(core: &Path) -> String {
    todo!("{}", core.display())
}

/// Game name used for save files: file stem of the content (the archive for a zip), with `/`, `\`, NUL and
/// control characters replaced by `_`. `Sonic (World).zip` → `Sonic (World)`.
pub fn game_name_from_path(rom: &Path) -> String {
    todo!("{}", rom.display())
}

/// `<saves>/<core_id>/<game>.srm`.
pub fn sram_path(save_dir: &Path, core_id: &str, game: &str) -> PathBuf {
    todo!("{} {core_id} {game}", save_dir.display())
}

/// `<states>/<core_id>/<game>.state<slot>` (`.state0` … `.state9`).
pub fn state_path(state_dir: &Path, core_id: &str, game: &str, slot: u8) -> PathBuf {
    todo!("{} {core_id} {game} {slot}", state_dir.display())
}

/// Metadata stored in front of the core's serialized data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateHeader {
    pub core_id: String,
    /// `SystemInfo::library_version` at save time; a mismatch only warns on load.
    pub core_version: String,
    pub game: String,
    /// Seconds since the Unix epoch.
    pub created_unix: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum StateFileError {
    #[error("not an emu-rust savestate")]
    BadMagic,
    #[error("unsupported savestate format version {0}")]
    UnsupportedVersion(u32),
    #[error("truncated savestate")]
    Truncated,
    #[error("savestate checksum mismatch (corrupt file)")]
    Checksum,
    #[error("invalid savestate header")]
    BadHeader,
    #[error("savestate belongs to core `{found}`, not `{expected}`")]
    WrongCore { expected: String, found: String },
}

/// Little-endian container:
/// `STATE_MAGIC` | `u32` version | `u32` header length | header (UTF-8 `key=value\n` lines: `core_id`,
/// `core_version`, `game`, `created_unix`; unknown keys ignored) | `u64` payload length | payload |
/// `u64` FNV-1a 64 of the payload.
pub fn encode_state(header: &StateHeader, payload: &[u8]) -> Vec<u8> {
    todo!("{header:?} {}", payload.len())
}

/// Parses and checks a container (magic, version, lengths, checksum); returns the header and the payload
/// slice. Never panics on arbitrary input. Does not compare `core_id` (the caller does: `WrongCore`).
pub fn decode_state(bytes: &[u8]) -> Result<(StateHeader, &[u8]), StateFileError> {
    todo!("{}", bytes.len())
}
