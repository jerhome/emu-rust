//! Emulation loop, software-frame conversion, audio sinks, input backends, pacing and GL presentation.
#![allow(unsafe_code)]

mod audio;
mod emulator;
mod error;
mod frame;
mod gl;
mod input;
mod timing;

pub use emu_libretro::{AvInfo, GameGeometry, JoypadButton, SystemInfo, SystemTiming};

pub use crate::audio::{AudioSink, CollectSink, NullSink, write_wav};
pub use crate::emulator::{Emulator, EmulatorOptions, FrameStatus};
pub use crate::error::RuntimeError;
pub use crate::frame::{FrameRgba, convert_frame};
pub use crate::gl::{GlWindow, Scaling};
pub use crate::input::{InputBackend, InputFrame, MAX_PORTS, NullInput, PadState, ScriptedInput};
pub use crate::timing::FramePacer;
