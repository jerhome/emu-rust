//! Safe Rust frontend API over a dynamically loaded libretro core (ADR 0001, ADR 0008).
//!
//! Only this crate and `emu-runtime` may contain `unsafe`; everything exported here is safe to call.
#![allow(unsafe_code)]

mod callbacks;
mod core;
mod error;
mod log;
mod options;
mod sys;
mod types;

pub use crate::core::{Core, CoreConfig, FrontendHandler, RunEvents};
pub use crate::error::LibretroError;
pub use crate::types::{
    AvInfo, CoreMessage, CoreOption, CoreOptionCategory, CoreOptionValue, GameGeometry, GameSource,
    InputDevice, InputQuery, JOYPAD_ID_MASK, JoypadButton, LogLevel, MemoryKind, PixelFormat,
    SerializationQuirks, SystemInfo, SystemTiming, VideoFrame,
};
