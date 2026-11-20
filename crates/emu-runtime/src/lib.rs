//! Emulation loop, software-frame conversion, audio sinks, input backends, pacing and GL presentation.
#![allow(unsafe_code)]

mod audio;
mod audio_out;
#[cfg(any(feature = "audio-cpal", test))]
mod audio_reopen;
mod audio_ring;
mod content;
mod emulator;
mod error;
mod frame;
mod fsname;
mod gl;
mod input;
mod pacing;
mod resample;
mod saves;
mod timing;

pub use emu_libretro::{
    AvInfo, CoreMessage, CoreOption, CoreOptionCategory, CoreOptionValue, GameGeometry,
    JoypadButton, LogLevel, MemoryKind, SystemInfo, SystemTiming,
};

pub use crate::audio::{AudioSink, CollectSink, NullSink, write_wav};
#[cfg(feature = "audio-cpal")]
pub use crate::audio_out::CpalSink;
pub use crate::audio_out::{AudioOutput, AudioStatus};
pub use crate::audio_ring::{
    AudioConfig, AudioError, AudioStats, OutputSample, RingReader, RingSink, audio_ring,
};
pub use crate::emulator::{Emulator, EmulatorOptions, FrameStatus, MAX_CONTENT_SIZE};
pub use crate::error::RuntimeError;
pub use crate::frame::{FrameRgba, convert_frame};
pub use crate::gl::{GlWindow, Scaling};
pub use crate::input::{InputBackend, InputFrame, MAX_PORTS, NullInput, PadState, ScriptedInput};
pub use crate::pacing::{
    AudioLevel, DEFAULT_FAST_FORWARD_RATIO, MAX_BURST, PaceDecision, Pacer, Speed, SyncMode,
};
pub use crate::resample::{MAX_RATE_ADJUST, StereoResampler, rate_adjust};
pub use crate::saves::{
    FileMode, STATE_FORMAT_VERSION, STATE_MAGIC, STATE_SLOTS, StateFileError, StateHeader,
    core_id_from_path, decode_state, encode_state, game_name_from_path, sram_path, state_path,
    write_atomic,
};
pub use crate::timing::FramePacer;
