//! Hand-written subset of the official `libretro.h` (API version 1). Names follow the header.
//! Private: nothing here leaves the crate.
#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_uint, c_void};

pub const RETRO_API_VERSION: c_uint = 1;

pub const RETRO_DEVICE_TYPE_SHIFT: c_uint = 8;
pub const RETRO_DEVICE_MASK: c_uint = (1 << RETRO_DEVICE_TYPE_SHIFT) - 1;
pub const RETRO_DEVICE_NONE: c_uint = 0;
pub const RETRO_DEVICE_JOYPAD: c_uint = 1;
pub const RETRO_DEVICE_MOUSE: c_uint = 2;
pub const RETRO_DEVICE_KEYBOARD: c_uint = 3;
pub const RETRO_DEVICE_LIGHTGUN: c_uint = 4;
pub const RETRO_DEVICE_ANALOG: c_uint = 5;
pub const RETRO_DEVICE_POINTER: c_uint = 6;

pub const RETRO_LANGUAGE_ENGLISH: c_uint = 0;

pub const RETRO_PIXEL_FORMAT_0RGB1555: u32 = 0;
pub const RETRO_PIXEL_FORMAT_XRGB8888: u32 = 1;
pub const RETRO_PIXEL_FORMAT_RGB565: u32 = 2;

pub const RETRO_ENVIRONMENT_EXPERIMENTAL: c_uint = 0x10000;

pub const RETRO_ENVIRONMENT_GET_CAN_DUPE: c_uint = 3;
pub const RETRO_ENVIRONMENT_SHUTDOWN: c_uint = 7;
pub const RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL: c_uint = 8;
pub const RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY: c_uint = 9;
pub const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
pub const RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS: c_uint = 11;
pub const RETRO_ENVIRONMENT_SET_HW_RENDER: c_uint = 14;
pub const RETRO_ENVIRONMENT_GET_VARIABLE: c_uint = 15;
pub const RETRO_ENVIRONMENT_SET_VARIABLES: c_uint = 16;
pub const RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE: c_uint = 17;
pub const RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME: c_uint = 18;
pub const RETRO_ENVIRONMENT_GET_LOG_INTERFACE: c_uint = 27;
pub const RETRO_ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY: c_uint = 30;
pub const RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY: c_uint = 31;
pub const RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO: c_uint = 32;
pub const RETRO_ENVIRONMENT_SET_CONTROLLER_INFO: c_uint = 35;
pub const RETRO_ENVIRONMENT_SET_GEOMETRY: c_uint = 37;
pub const RETRO_ENVIRONMENT_GET_LANGUAGE: c_uint = 39;
pub const RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS: c_uint = 42 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE: c_uint = 47 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_INPUT_BITMASKS: c_uint = 51 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION: c_uint = 52;

/// `RETRO_HW_FRAME_BUFFER_VALID`: sentinel passed to the video callback by hardware-rendered cores.
pub const RETRO_HW_FRAME_BUFFER_VALID: *const c_void = usize::MAX as *const c_void;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_system_info {
    pub library_name: *const c_char,
    pub library_version: *const c_char,
    pub valid_extensions: *const c_char,
    pub need_fullpath: bool,
    pub block_extract: bool,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct retro_game_geometry {
    pub base_width: c_uint,
    pub base_height: c_uint,
    pub max_width: c_uint,
    pub max_height: c_uint,
    pub aspect_ratio: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct retro_system_timing {
    pub fps: f64,
    pub sample_rate: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct retro_system_av_info {
    pub geometry: retro_game_geometry,
    pub timing: retro_system_timing,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_variable {
    pub key: *const c_char,
    pub value: *const c_char,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_game_info {
    pub path: *const c_char,
    pub data: *const c_void,
    pub size: usize,
    pub meta: *const c_char,
}

pub type retro_environment_t = unsafe extern "C" fn(cmd: c_uint, data: *mut c_void) -> bool;
pub type retro_video_refresh_t =
    unsafe extern "C" fn(data: *const c_void, width: c_uint, height: c_uint, pitch: usize);
pub type retro_audio_sample_t = unsafe extern "C" fn(left: i16, right: i16);
pub type retro_audio_sample_batch_t =
    unsafe extern "C" fn(data: *const i16, frames: usize) -> usize;
pub type retro_input_poll_t = unsafe extern "C" fn();
pub type retro_input_state_t =
    unsafe extern "C" fn(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16;

/// Entry points exported by a core, copied out of the library. Valid only while the `Library` is loaded.
#[derive(Clone, Copy)]
pub struct CoreApi {
    pub set_environment: unsafe extern "C" fn(retro_environment_t),
    pub set_video_refresh: unsafe extern "C" fn(retro_video_refresh_t),
    pub set_audio_sample: unsafe extern "C" fn(retro_audio_sample_t),
    pub set_audio_sample_batch: unsafe extern "C" fn(retro_audio_sample_batch_t),
    pub set_input_poll: unsafe extern "C" fn(retro_input_poll_t),
    pub set_input_state: unsafe extern "C" fn(retro_input_state_t),
    pub init: unsafe extern "C" fn(),
    pub deinit: unsafe extern "C" fn(),
    pub api_version: unsafe extern "C" fn() -> c_uint,
    pub get_system_info: unsafe extern "C" fn(*mut retro_system_info),
    pub get_system_av_info: unsafe extern "C" fn(*mut retro_system_av_info),
    pub set_controller_port_device: unsafe extern "C" fn(port: c_uint, device: c_uint),
    pub reset: unsafe extern "C" fn(),
    pub run: unsafe extern "C" fn(),
    pub load_game: unsafe extern "C" fn(*const retro_game_info) -> bool,
    pub unload_game: unsafe extern "C" fn(),
}
