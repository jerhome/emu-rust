//! Window, event loop and egui interface. No `unsafe` here: GL lives in emu-runtime (ADR 0008).

mod app;
mod config;
mod config_library;
mod cores_ui;
mod egui_layer;
mod hotkeys;
mod keyboard;
mod library;
mod ui;

use std::path::PathBuf;

pub use crate::config::{
    AudioSettings, Config, ConfigError, DEFAULT_PROFILE, DataDirs, Hotkeys, InputConfig, MAX_SCALE,
    MIN_SCALE, ScalingChoice, SpeedConfig, SyncChoice, VideoConfig,
};
pub use crate::config_library::{
    CoresConfig, CoversConfig, CredentialsConfig, IgdbConfig, LibraryConfig, ScreenScraperConfig,
};
pub use crate::cores_ui::open_core_manager;
pub use crate::hotkeys::{HotkeyAction, SpeedState, step_slot};
pub use crate::keyboard::{KeyMap, KeyboardInput, PadStyle};
/// The asset crate, re-exported so `emu-app` reaches the core manager through `emu-ui`.
pub use emu_assets as assets;
/// The library crate, re-exported so `emu-app` reaches it through `emu-ui`.
pub use emu_library as library_db;

/// Screen to open for a deterministic capture (`--screen`); rendered by the egui layer (task U2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Menu,
    Keys,
    CoreOptions,
}

/// What `emu-app` asks the UI to launch (one core + one ROM, no library screen yet).
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchOptions {
    pub core: PathBuf,
    pub rom: PathBuf,
    pub dirs: DataDirs,
    pub config: Config,
    /// Overrides `config.video.scale` for this run only (not persisted).
    pub scale: Option<u32>,
    /// `false` (`--no-audio`): silent `AudioOutput::null()` without probing a device.
    pub audio: bool,
    /// Quit cleanly after this many emulated frames (Xvfb captures, smoke tests).
    pub exit_after_frames: Option<u64>,
    /// On exit, write the last emulated frame as PNG.
    pub dump_last_frame: Option<PathBuf>,
    /// Open this screen after `exit_after_frames` (default 30) frames, then capture and quit.
    pub screen: Option<Screen>,
    /// On exit, write the window contents (back buffer, after drawing) as PNG.
    pub dump_window: Option<PathBuf>,
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
}

/// Screen to capture in library mode (`--screen library|library-settings`, Xvfb).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryScreen {
    Home,
    Settings,
    /// Core manager screen (`--screen cores`, no network).
    Cores,
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
