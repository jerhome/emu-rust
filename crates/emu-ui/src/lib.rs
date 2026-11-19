//! Window, event loop and (from phase 2) egui interface. No `unsafe` here: GL lives in emu-runtime (ADR 0008).

mod app;
mod keyboard;

use std::path::PathBuf;

pub use crate::keyboard::{KeyMap, KeyboardInput};

/// What `emu-app` asks the UI to launch (phase 1: one core + one ROM, no library screen).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptions {
    pub core: PathBuf,
    pub rom: PathBuf,
    pub system_dir: PathBuf,
    pub save_dir: PathBuf,
    /// Initial window size = base geometry × `scale` (≥ 1).
    pub scale: u32,
    /// Quit cleanly after this many emulated frames (Xvfb captures, smoke tests).
    pub exit_after_frames: Option<u64>,
    /// On exit, write the last emulated frame as PNG.
    pub dump_last_frame: Option<PathBuf>,
    /// `false` (`--no-audio`): silent `AudioOutput::null()` without probing a device.
    pub audio: bool,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UiError {
    #[error(transparent)]
    Runtime(#[from] emu_runtime::RuntimeError),
    #[error("event loop error: {0}")]
    EventLoop(String),
}

/// Opens the window and runs the emulator until the window is closed, Escape is pressed, the core requests
/// shutdown, or `exit_after_frames` is reached. Must be called on the main thread.
pub fn run(options: LaunchOptions) -> Result<(), UiError> {
    app::run(options)
}
