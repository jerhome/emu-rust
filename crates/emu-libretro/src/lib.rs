//! Safe Rust frontend API over a dynamically loaded libretro core (ADR 0001, ADR 0008).
//!
//! Only this crate and `emu-runtime` may contain `unsafe`; everything exported here is safe to call.
#![allow(unsafe_code)]

mod callbacks;
mod controllers;
mod core;
mod disk;
mod error;
mod hw;
mod keyboard;
mod log;
mod options;
mod resident;
mod sys;
mod types;
mod vfs;

pub use crate::core::{Core, CoreConfig, FrontendHandler, RunEvents};
pub use crate::disk::{DiskControl, DiskControlKind};
pub use crate::error::LibretroError;
pub use crate::hw::{
    FrameKind, HwContextKind, HwRenderRequest, InputDescriptor, MAX_INPUT_DESCRIPTORS,
};
pub use crate::keyboard::{KeyEvent, KeyModifiers, RetroKey, mouse_id};
pub use crate::types::{
    AvInfo, ControllerType, CoreMessage, CoreOption, CoreOptionCategory, CoreOptionValue,
    FastForwardOverride, GameGeometry, GameSource, InputDevice, InputQuery, JOYPAD_ID_MASK,
    JoypadButton, LogLevel, MemoryKind, PixelFormat, SerializationQuirks, SystemInfo, SystemTiming,
    VideoFrame,
};
