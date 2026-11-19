//! Window, event loop and egui interface. No `unsafe` here: GL lives in emu-runtime (ADR 0008).

mod app;
mod config;
mod egui_layer;
mod hotkeys;
mod keyboard;
mod ui;

use std::path::PathBuf;

pub use crate::config::{
    AudioSettings, Config, ConfigError, DEFAULT_PROFILE, DataDirs, Hotkeys, InputConfig, MAX_SCALE,
    MIN_SCALE, ScalingChoice, SpeedConfig, SyncChoice, VideoConfig,
};
pub use crate::hotkeys::{HotkeyAction, SpeedState, step_slot};
pub use crate::keyboard::{KeyMap, KeyboardInput, PadStyle};

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
    #[error("event loop error: {0}")]
    EventLoop(String),
}

/// Opens the window and runs the emulator until the window is closed, the quit shortcut is pressed, the core
/// requests shutdown, or `exit_after_frames` is reached. Must be called on the main thread.
pub fn run(options: LaunchOptions) -> Result<(), UiError> {
    app::run(options)
}
