//! Removable media of computer systems (ADR 0015, `docs/design/v2.0-atari-st.md` §3.2): multi-disk sets,
//! working copies, generated `.m3u`, disk swapping during a session.
//!
//! Contract only (version 2.0): bodies are filled by the `runtime-av` agent.

use std::path::{Path, PathBuf};

use emu_libretro::DiskControlKind;

use crate::emulator::Emulator;
use crate::error::RuntimeError;

/// Most images accepted in one set (generated playlist, zip with several disks, disk menu additions).
pub const MAX_MEDIA_IMAGES: usize = 32;

/// What to boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentSpec {
    /// One file, as in 1.0 (zip extraction rules of `content.rs` unchanged).
    File(PathBuf),
    /// Ordered disk set: [`prepare_media`] runs after the core is loaded (it needs the extensions the binary
    /// really accepts, which may differ from its `.info`), then `PreparedMedia::content` is loaded; the other
    /// images are reachable through [`Disks`]. The game name (saves, states) is `MediaSet::name`.
    Media { set: MediaSet, policy: MediaPolicy },
    /// `retro_load_game(NULL)` (GEM desktop); needs `SET_SUPPORT_NO_GAME`.
    NoGame,
}

/// Disk images of one game, in insertion order, as grouped by the library (or one user `.m3u`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSet {
    /// Game name (ADR 0011 naming): working-copy folder and generated playlist are named after it.
    pub name: String,
    /// Plain images or `.zip` archives holding images; at least one, at most [`MAX_MEDIA_IMAGES`].
    pub images: Vec<PathBuf>,
    /// A user `.m3u`: used as is when the policy is `Direct` (its entries are never rewritten).
    pub playlist: Option<PathBuf>,
}

/// Whether the user's files may be handed to the core (from `emu-assets/data/cores.toml`, `media = …`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaPolicy {
    /// The core never writes to content files (or keeps its own copies, e.g. hatariB): pass them through;
    /// zips are extracted to a private temporary directory as in 1.0.
    Direct,
    /// The core writes into disk images in place (hatari): copy them once into
    /// `<saves>/<core_id>/<game>.disks/` and only ever hand the copies to the core.
    WorkingCopy,
}

/// Result of [`prepare_media`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedMedia {
    /// File given to `retro_load_game`: the single image, the user playlist, or a generated `.m3u`.
    pub content: PathBuf,
    /// Images actually seen by the core, in order (working copies when `WorkingCopy`).
    pub images: Vec<PathBuf>,
    /// `Some(<saves>/<core_id>/<game>.disks)` for `WorkingCopy`.
    pub work_dir: Option<PathBuf>,
}

/// Copies / extracts / writes the playlist as the policy requires. Never modifies the files of `set`.
///
/// `saves_core_dir` = `<saves>/<core_id>`. Working copies are created only when absent (an existing copy holds
/// the player's progress and is reused); every write is atomic (`write_atomic`). Zip entries are read with the
/// limits of `content.rs` and only entries whose extension the core accepts are kept; entry names are never
/// used as paths (sanitised with `fsname`). A generated `.m3u` uses names relative to its own folder.
pub fn prepare_media(
    set: &MediaSet,
    policy: MediaPolicy,
    accepted_extensions: &[String],
    saves_core_dir: &Path,
) -> Result<PreparedMedia, RuntimeError> {
    let _ = (set, policy, accepted_extensions, saves_core_dir);
    todo!("runtime-av (2.0)")
}

/// Deletes the working copies of `game` (menu « Réinitialiser les disquettes »); the core must not be running
/// that game. Returns `false` when there was nothing to delete.
pub fn reset_working_copies(saves_core_dir: &Path, game: &str) -> Result<bool, RuntimeError> {
    let _ = (saves_core_dir, game);
    todo!("runtime-av (2.0)")
}

/// One entry of the disk menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskSlot {
    pub index: u32,
    /// Core label, else file name, else `Disque <n>`.
    pub label: String,
    pub path: Option<PathBuf>,
}

/// Disk swapping for the running game, borrowed from [`Emulator::disks`]. Sequencing (eject → select →
/// insert) is done here so that the UI only says what it wants.
pub struct Disks<'a> {
    // Read once the stubs are implemented (runtime-av, version 2.0).
    #[allow(dead_code)]
    pub(crate) emulator: &'a mut Emulator,
}

impl Disks<'_> {
    pub fn kind(&mut self) -> DiskControlKind {
        todo!("runtime-av (2.0)")
    }

    pub fn slots(&mut self) -> Vec<DiskSlot> {
        todo!("runtime-av (2.0)")
    }

    /// `None` when the drive is empty or ejected.
    pub fn current(&mut self) -> Option<u32> {
        todo!("runtime-av (2.0)")
    }

    /// Ejects, selects `index`, inserts.
    pub fn insert(&mut self, index: u32) -> Result<(), RuntimeError> {
        let _ = index;
        todo!("runtime-av (2.0)")
    }

    /// Next image (wraps); the usual « changer de disquette » action.
    pub fn insert_next(&mut self) -> Result<u32, RuntimeError> {
        todo!("runtime-av (2.0)")
    }

    pub fn eject(&mut self) -> Result<(), RuntimeError> {
        todo!("runtime-av (2.0)")
    }

    /// Adds an image during the session (working copy first under `MediaPolicy::WorkingCopy`) and returns its
    /// index; it is not inserted.
    pub fn add(&mut self, image: &Path) -> Result<u32, RuntimeError> {
        let _ = image;
        todo!("runtime-av (2.0)")
    }
}

impl Emulator {
    /// 2.0 entry point; `Emulator::load(core, rom, options)` stays and equals
    /// `load_content(core, ContentSpec::File(rom), options)`.
    pub fn load_content(
        core: &Path,
        content: ContentSpec,
        options: crate::emulator::EmulatorOptions,
    ) -> Result<Self, RuntimeError> {
        let _ = (core, content, options);
        todo!("runtime-av (2.0)")
    }

    /// `None` when the core registered no disk control interface.
    pub fn disks(&mut self) -> Option<Disks<'_>> {
        todo!("runtime-av (2.0)")
    }

    /// Images prepared at load time (`ContentSpec::Media`), for the disk menu and « réinitialiser ».
    pub fn prepared_media(&self) -> Option<&PreparedMedia> {
        todo!("runtime-av (2.0)")
    }

    /// The core registered a keyboard callback or polled `RETRO_DEVICE_KEYBOARD` at least once.
    pub fn uses_keyboard(&self) -> bool {
        todo!("runtime-av (2.0)")
    }

    /// The core polled `RETRO_DEVICE_MOUSE` at least once (drives mouse capture in the UI).
    pub fn uses_mouse(&self) -> bool {
        todo!("runtime-av (2.0)")
    }
}
