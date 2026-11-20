//! Game identity (2.1, `docs/design/v2.1-identite-disques.md` §2.1): the `save_name` that keys SRAM, states,
//! per-game options and the `.disks` folder, attributed once per core by the registry
//! `<saves>/<core_id>/.emu-rust-games`. Homonyms get `<base> (n)`; nothing existing is ever renamed.

use std::path::{Path, PathBuf};

use crate::error::RuntimeError;
use crate::media::ContentSpec;

/// Who this game is for saves: resolved before loading (the UI needs it for per-game options), then handed to
/// [`crate::Emulator::load_content_as`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameIdentity {
    /// `<base>` or `<base> (n)`: `.srm`, states, per-game options, `<save_name>.disks`.
    pub save_name: String,
    /// 1 = bare name (`save_name == base_name()`).
    pub index: u32,
    /// `GET_SAVE_DIRECTORY`: `<saves>/<core_id>/` for index 1, `<saves>/<core_id>/<save_name>/` otherwise.
    pub core_save_dir: PathBuf,
    /// Set only by the launch that attributed the name (or found the registry unusable).
    pub notice: Option<IdentityNotice>,
    /// 1.0 / 2.0 name (`game_name_from_path`, `MediaSet::name`, `no-game`).
    base: String,
    /// Fingerprint of the primary source; `None` for `NoGame` and the 2.0 fallback.
    fingerprint: Option<Fingerprint>,
    /// Attributed by this launch to a homonym whose entry 1 is `legacy`.
    inherits_legacy: bool,
}

/// Size + CRC32 of the whole file (≤ 64 MiB) or of its first and last MiB. Written as `game_fp=<size>:<crc32>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    size: u64,
    crc32: u32,
}

/// One-shot banner for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityNotice {
    /// Homonym separated from the game that keeps the bare name (`first`). `sram_copied`: `<base>.srm` was copied
    /// to `<save_name>.srm`; `legacy_states`: states of the shared 2.0 name exist and stay with `first`.
    Homonym {
        first: PathBuf,
        sram_copied: bool,
        legacy_states: bool,
    },
    /// Registry unreadable or too large: 2.0 behaviour (bare name) for this session, registry left untouched.
    RegistryUnusable,
}

/// Input of [`resolve_game_identity`]. `primary = None` for `NoGame` (fixed identity, outside the registry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityRequest<'a> {
    /// `<saves>/<core_id>/`: registry, `.srm`, `.disks`.
    pub saves_core_dir: &'a Path,
    /// `<states>/<core_id>/`: checked so that `<base> (n)` never captures a foreign state file.
    pub states_core_dir: &'a Path,
    /// Owned: derived from a file stem for `ContentSpec::File`.
    pub base_name: String,
    /// User `.m3u`, else first image, else the file (the archive for a zip); a CD's `.cue` / `.chd` / `.iso`.
    pub primary: Option<&'a Path>,
}

impl GameIdentity {
    /// 2.0 behaviour: bare name, index 1, `core_save_dir = saves_core_dir`, no fingerprint, no notice. Used by
    /// [`crate::Emulator::load_content`] until I1, for `NoGame`, and as the registry fallback.
    pub fn legacy(base_name: &str, saves_core_dir: &Path) -> Self {
        Self {
            save_name: base_name.to_owned(),
            index: 1,
            core_save_dir: saves_core_dir.to_path_buf(),
            notice: None,
            base: base_name.to_owned(),
            fingerprint: None,
            inherits_legacy: false,
        }
    }

    /// The 1.0 / 2.0 game name this identity was resolved from.
    pub fn base_name(&self) -> &str {
        &self.base
    }

    /// `true` if this launch attributed `save_name` to a homonym whose entry 1 is `legacy`: the UI then copies
    /// `<base>.toml` to `<save_name>.toml` (`inherit_game_options`).
    pub fn inherits_legacy(&self) -> bool {
        self.inherits_legacy
    }

    /// Value of the `game_fp=` state header line; `None` when no fingerprint is known.
    pub(crate) fn game_fp(&self) -> Option<String> {
        self.fingerprint
            .map(|fp| format!("{}:{:08x}", fp.size, fp.crc32))
    }
}

/// Request for `spec` (base name and primary source, §2.1). Filters `Media { set, .. }` only (C1).
pub fn identity_request_for<'a>(
    spec: &'a ContentSpec,
    saves_core_dir: &'a Path,
    states_core_dir: &'a Path,
) -> IdentityRequest<'a> {
    let _ = (spec, saves_core_dir, states_core_dir);
    todo!("I1: base = game_name_from_path / MediaSet::name / NO_GAME_NAME; primary per §2.1")
}

/// Resolves (or attributes) the identity of a game in the registry of `req.saves_core_dir`. Idempotent. Writes
/// only the registry, its lock and inherited `.srm` copies (atomic, missing target only). An unusable registry
/// is not an error: 2.0 identity with [`IdentityNotice::RegistryUnusable`].
pub fn resolve_game_identity(req: &IdentityRequest<'_>) -> Result<GameIdentity, RuntimeError> {
    let _ = req;
    todo!("I1: registry, resolution rules 1-4, migration (§2.1, §2.2)")
}
