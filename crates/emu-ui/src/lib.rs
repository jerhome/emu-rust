//! Window, event loop and egui interface. No `unsafe` here: GL lives in emu-runtime (ADR 0008).

mod app;
mod bios_import;
mod computer;
mod config;
mod config_computer;
mod config_gamepad;
mod config_library;
mod content;
mod copyable;
mod cores_ui;
mod dos_launch;
mod egui_layer;
mod file_picker;
mod firmware_hints;
mod game_identity;
mod game_options;
mod gamepad;
mod hotkeys;
mod keyboard;
mod launch_firmware;
mod library;
/// Log capture for the « Journal » panel and `logs/emu-rust.log` (ADR 0023 §3).
pub mod log;
mod pad_mode;
mod platform;
mod shutdown;
mod ui;

use std::path::PathBuf;

pub use crate::bios_import::{
    BiosIdentified, BiosImportItem, BiosImportReport, ROM_KEY_NAME, firmware_catalogue,
    identify_bios, import_bios, import_error, known_import_items, unverified_item,
};
pub use crate::config::{
    AudioSettings, Config, ConfigError, DEFAULT_PROFILE, DataDirs, Hotkeys, InputConfig,
    InterfaceConfig, MAX_SCALE, MIN_SCALE, ScalingChoice, SpeedConfig, SyncChoice, VideoConfig,
};
pub use crate::config_computer::{
    ComputerConfig, KeyboardModeChoice, KeyboardModeSetting, emutos_variant_or_system,
    variant_for_locale,
};
pub use crate::config_gamepad::{GamepadConfig, GamepadDeviceConfig, HotkeyButton};
pub use crate::config_library::{
    CoresConfig, CoversConfig, CredentialsConfig, IgdbConfig, LibraryConfig, ScreenScraperConfig,
};
pub use crate::content::{
    GameContent, LaunchRefusal, content_spec, media_set_name, plan_disc_launch,
};
pub use crate::cores_ui::{open_core_manager, open_free_bios_manager};
pub use crate::dos_launch::{
    clear_dos_cache, dos_cli_content, invalidate_core_caches, prepare_dos_snapshot,
    system_cache_names,
};
pub use crate::game_identity::{ResolvedGame, resolve_game, state_load_needs_confirmation};
#[cfg(feature = "gamepad")]
pub use crate::gamepad::GilrsSource;
pub use crate::gamepad::{
    GamepadError, GamepadInput, GamepadNotice, GamepadSettings, GamepadSource, NavKey, PadAxis,
    PadBackend, PadButton, PadEvent, PadEventKind, PadId, PadMap, ScriptedPadSource,
    retropad_from_name, retropad_name,
};
pub use crate::hotkeys::{HotkeyAction, SpeedState, core_fast_forward_ratio, step_slot};
pub use crate::keyboard::{KeyMap, KeyboardInput, PadStyle, is_computer_profile};
pub use crate::launch_firmware::{FirmwareSetup, prepare_launch_firmware};
pub use crate::platform::{FolderOutcome, open_folder};
pub use crate::shutdown::request_shutdown;
pub use crate::ui::text::sanitize_core_line;
/// The asset crate, re-exported so `emu-app` reaches the core manager through `emu-ui`.
pub use emu_assets as assets;
/// The library crate, re-exported so `emu-app` reaches it through `emu-ui`.
pub use emu_library as library_db;
/// Native error box before any window exists (Windows without a console; elsewhere a no-op). ADR 0023 §3.
pub use emu_runtime::fatal_dialog;

/// Version shown by the footer and `--version` (workspace version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Credit line of the footer (ADR 0023 §1): `emu-rust <VERSION> — <CREDIT>`.
pub const CREDIT: &str = "dev by claude@jerhome";

/// Finishes the ROM tidying of earlier runs (ADR 0023 §4.5): `Library::recover_tidy`, then
/// `emu_runtime::relocate_identities` + `Library::mark_tidy_identities` for every journal of
/// `Library::pending_tidy_identities`. Called when the library opens and before a direct launch (`run`); errors
/// are logged (« Journal » panel), never fatal. Runs on the calling thread, before the window exists, on purpose:
/// the library and a direct launch must see the final paths (rename / hard link on one volume: quick). Not called
/// for a capture (`--dump-window`). With `--save-dir`, the identity step waits for a start without it.
pub fn finish_pending_identities(dirs: &DataDirs) {
    use crate::library::tidy::exec;
    if !exec::recovery_needed(dirs) {
        return;
    }
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let out = exec::run_job(dirs, exec::Job::Recovery, &cancel, &mut |_| {});
    for line in exec::recovery_log(&out) {
        tracing::warn!("{line}");
    }
}

/// Screen to open for a deterministic capture (`--screen`); rendered by the egui layer (task U2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Menu,
    Keys,
    CoreOptions,
    /// Disk menu (computer systems, cores with a disk control interface).
    Disks,
    /// Keyboard and mouse settings of a computer game.
    Computer,
    /// Not a menu screen: the virtual keyboard over the running game (`--screen vkbd` captures).
    VirtualKeyboard,
}

/// What `emu-app` (or the library) asks the UI to launch: one core and its content.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchOptions {
    pub core: PathBuf,
    /// Files of one game (disks in order), or nothing (computer desktop).
    pub content: GameContent,
    /// Computer system (ADR 0015: host key, full keyboard, mouse). Also implied by computer cores (`hatari`…).
    pub computer: bool,
    pub dirs: DataDirs,
    pub config: Config,
    /// Overrides `config.video.scale` for this run only (not persisted).
    pub scale: Option<u32>,
    /// `false` (`--no-audio`): silent `AudioOutput::null()` without probing a device.
    pub audio: bool,
    /// `false` (`--no-gamepad`): no pad backend is opened, whatever `[input.gamepad] enabled` says.
    pub gamepad: bool,
    /// Quit cleanly after this many emulated frames (Xvfb captures, smoke tests).
    pub exit_after_frames: Option<u64>,
    /// On exit, write the last emulated frame as PNG.
    pub dump_last_frame: Option<PathBuf>,
    /// Open this screen after `exit_after_frames` (default 30) frames, then capture and quit.
    pub screen: Option<Screen>,
    /// On exit, write the window contents (back buffer, after drawing) as PNG.
    pub dump_window: Option<PathBuf>,
    /// `--launch` (design v5.0 §4): launch text of this run, over the game's `[media] launch`; refused when the
    /// core takes none.
    pub launch: Option<String>,
    /// `false` (`--no-auto-fast-load`): the core's fast-forward requests are refused for this run, whatever
    /// `[computer] auto_fast_load` says.
    pub auto_fast_load: bool,
    /// `--pad-device` (`digital` | `dualshock`): pad mode of this run, over the game's and the config's; `None`:
    /// the usual rule (design v7.0 §2.4). Read by PU2.
    pub pad_device: Option<String>,
    /// libretro database name of the game's system (library entry); `None` (CLI): found from the content's
    /// extension and header. Chooses the firmware table, the « Réglages » section and the « CD » menu.
    pub game_system: Option<String>,
    /// Region of the game (`Game::region`); `None`: probed from the disc header, then the file name.
    pub region: Option<emu_library::DiscRegion>,
    /// Content already planned by the library (`plan_disc_launch`: several discs of a `disc = true` system);
    /// `None`: built from `content`.
    pub spec: Option<emu_runtime::ContentSpec>,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UiError {
    #[error(transparent)]
    Runtime(#[from] emu_runtime::RuntimeError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Library(#[from] emu_library::LibraryError),
    #[error("event loop error: {0}")]
    EventLoop(String),
    /// Content refused before loading (DOS: `.conf`, folder for a core that does not read folders, bad start
    /// program). The message is in French, ready to show.
    #[error("{0}")]
    Content(String),
}

impl UiError {
    /// Last error the core logged while refusing the game (`LibretroError::core_error`, design v6.0 §2.1): shown
    /// instead of the generic advice (« Unable to open CD BIOS: …/bios_CD_U.bin »). Untrusted text: clean it
    /// before display (`launch_error` does).
    pub fn core_error(&self) -> Option<&str> {
        match self {
            UiError::Runtime(emu_runtime::RuntimeError::Libretro(e)) => e.core_error(),
            _ => None,
        }
    }
}

/// Screen to capture in library mode (`--screen library|library-settings`, Xvfb).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryScreen {
    Home,
    Settings,
    /// Core manager screen (`--screen cores`, no network).
    Cores,
    /// « Journal » panel (`--screen library-log`).
    Log,
    /// « Ranger les ROMs… » preview (`--screen library-tidy`): read-only analysis of `roms/`, nothing moved.
    Tidy,
    /// Report of a tidying with sample content (`--screen library-tidy-report`): nothing is read or moved.
    TidyReport,
    /// Launches the first game of the library (`--screen library-launch`) and captures the game window after a
    /// few frames, or the launch dialog (core to install, BIOS missing…) when the game cannot start.
    Launch,
}

/// Library mode (no `--core/--rom`, e.g. a double-click): the window opens on the game library; a launched game
/// runs in the same window and quitting it returns to the library (ADR 0014 §8).
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryOptions {
    pub dirs: DataDirs,
    pub config: Config,
    /// `false` (`--no-audio`): every launched game is silent.
    pub audio: bool,
    /// `false` (`--no-gamepad`): games are played with the keyboard only.
    pub gamepad: bool,
    /// Capture mode: open this screen, render a few egui frames, write `dump_window`, quit. No scan, no network.
    pub screen: Option<LibraryScreen>,
    pub dump_window: Option<PathBuf>,
}

/// Runs the library until the window is closed. Must be called on the main thread.
pub fn run_library(options: LibraryOptions) -> Result<(), UiError> {
    match options.screen {
        Some(LibraryScreen::Cores) if options.dump_window.is_some() => {
            cores_ui::run_capture(options)
        }
        _ => library::run(options),
    }
}

/// Opens the window and runs the emulator until the window is closed, the quit shortcut is pressed, the core
/// requests shutdown, or `exit_after_frames` is reached. Must be called on the main thread.
pub fn run(options: LaunchOptions) -> Result<(), UiError> {
    app::run(options)
}
