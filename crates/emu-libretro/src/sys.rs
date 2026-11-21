//! Hand-written subset of the official `libretro.h` (API version 1). Names follow the header.
//! Private: nothing here leaves the crate.
#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int, c_uint, c_void};

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

pub const RETRO_MEMORY_SAVE_RAM: c_uint = 0;
pub const RETRO_MEMORY_RTC: c_uint = 1;
pub const RETRO_MEMORY_SYSTEM_RAM: c_uint = 2;
pub const RETRO_MEMORY_VIDEO_RAM: c_uint = 3;

pub const RETRO_LOG_DEBUG: c_int = 0;
pub const RETRO_LOG_INFO: c_int = 1;
pub const RETRO_LOG_WARN: c_int = 2;
pub const RETRO_LOG_ERROR: c_int = 3;

pub const RETRO_MESSAGE_TARGET_ALL: c_int = 0;
pub const RETRO_MESSAGE_TARGET_OSD: c_int = 1;
pub const RETRO_MESSAGE_TARGET_LOG: c_int = 2;

pub const RETRO_NUM_CORE_OPTION_VALUES_MAX: usize = 128;

pub const RETRO_ENVIRONMENT_GET_OVERSCAN: c_uint = 2;
pub const RETRO_ENVIRONMENT_GET_CAN_DUPE: c_uint = 3;
pub const RETRO_ENVIRONMENT_SET_MESSAGE: c_uint = 6;
pub const RETRO_ENVIRONMENT_SHUTDOWN: c_uint = 7;
pub const RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL: c_uint = 8;
pub const RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY: c_uint = 9;
pub const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
pub const RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS: c_uint = 11;
pub const RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK: c_uint = 12;
pub const RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE: c_uint = 13;
pub const RETRO_ENVIRONMENT_SET_HW_RENDER: c_uint = 14;
pub const RETRO_ENVIRONMENT_GET_VARIABLE: c_uint = 15;
pub const RETRO_ENVIRONMENT_SET_VARIABLES: c_uint = 16;
pub const RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE: c_uint = 17;
pub const RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME: c_uint = 18;
pub const RETRO_ENVIRONMENT_GET_LIBRETRO_PATH: c_uint = 19;
pub const RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES: c_uint = 24;
pub const RETRO_ENVIRONMENT_GET_LOG_INTERFACE: c_uint = 27;
pub const RETRO_ENVIRONMENT_GET_PERF_INTERFACE: c_uint = 28;
pub const RETRO_ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY: c_uint = 30;
pub const RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY: c_uint = 31;
pub const RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO: c_uint = 32;
pub const RETRO_ENVIRONMENT_SET_SUBSYSTEM_INFO: c_uint = 34;
pub const RETRO_ENVIRONMENT_SET_CONTROLLER_INFO: c_uint = 35;
pub const RETRO_ENVIRONMENT_SET_MEMORY_MAPS: c_uint = 36 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_SET_GEOMETRY: c_uint = 37;
pub const RETRO_ENVIRONMENT_GET_LANGUAGE: c_uint = 39;
pub const RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS: c_uint = 42 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_SET_SERIALIZATION_QUIRKS: c_uint = 44;
pub const RETRO_ENVIRONMENT_GET_VFS_INTERFACE: c_uint = 45 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_LED_INTERFACE: c_uint = 46 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE: c_uint = 47 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_SET_AUDIO_BUFFER_STATUS_CALLBACK: c_uint = 62;
pub const RETRO_ENVIRONMENT_SET_MINIMUM_AUDIO_LATENCY: c_uint = 63;
pub const RETRO_ENVIRONMENT_GET_GAME_INFO_EXT: c_uint = 66;
pub const RETRO_ENVIRONMENT_GET_FASTFORWARDING: c_uint = 49 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_INPUT_BITMASKS: c_uint = 51 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION: c_uint = 52;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS: c_uint = 53;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL: c_uint = 54;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY: c_uint = 55;
pub const RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION: c_uint = 57;
pub const RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE: c_uint = 58;
pub const RETRO_ENVIRONMENT_GET_MESSAGE_INTERFACE_VERSION: c_uint = 59;
pub const RETRO_ENVIRONMENT_SET_MESSAGE_EXT: c_uint = 60;
pub const RETRO_ENVIRONMENT_GET_INPUT_MAX_USERS: c_uint = 61;
pub const RETRO_ENVIRONMENT_SET_FASTFORWARDING_OVERRIDE: c_uint = 64;
pub const RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE: c_uint = 65;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2: c_uint = 67;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL: c_uint = 68;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK: c_uint = 69;
pub const RETRO_ENVIRONMENT_SET_VARIABLE: c_uint = 70;

/// `RETRO_HW_FRAME_BUFFER_VALID`: sentinel passed to the video callback by hardware-rendered cores.
pub const RETRO_HW_FRAME_BUFFER_VALID: *const c_void = usize::MAX as *const c_void;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_system_info {
    pub library_name: *const c_char,
    pub library_version: *const c_char,
    pub valid_extensions: *const c_char,
    pub need_fullpath: u8,
    pub block_extract: u8,
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

/// `struct retro_fastforwarding_override`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct retro_fastforwarding_override {
    pub ratio: f32,
    pub fastforward: bool,
    pub notification: bool,
    pub inhibit_toggle: bool,
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

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_message {
    pub msg: *const c_char,
    pub frames: c_uint,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_message_ext {
    pub msg: *const c_char,
    /// Milliseconds.
    pub duration: c_uint,
    pub priority: c_uint,
    /// `enum retro_log_level`.
    pub level: c_int,
    /// `enum retro_message_target`.
    pub target: c_int,
    /// `enum retro_message_type`.
    pub type_: c_int,
    pub progress: i8,
}

/// `retro_log_printf_t` is variadic; Rust only sees it as an opaque function pointer here.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_log_callback {
    pub log: unsafe extern "C" fn(level: c_int, fmt: *const c_char, ...),
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_core_option_value {
    pub value: *const c_char,
    pub label: *const c_char,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_core_option_definition {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub info: *const c_char,
    pub values: [retro_core_option_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
    pub default_value: *const c_char,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_core_options_intl {
    pub us: *const retro_core_option_definition,
    pub local: *const retro_core_option_definition,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_core_option_v2_category {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub info: *const c_char,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_core_option_v2_definition {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub desc_categorized: *const c_char,
    pub info: *const c_char,
    pub info_categorized: *const c_char,
    pub category_key: *const c_char,
    pub values: [retro_core_option_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
    pub default_value: *const c_char,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_core_options_v2 {
    /// Array terminated by a null `key`; may itself be null.
    pub categories: *const retro_core_option_v2_category,
    /// Array terminated by a null `key`.
    pub definitions: *const retro_core_option_v2_definition,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_core_options_v2_intl {
    pub us: *const retro_core_options_v2,
    pub local: *const retro_core_options_v2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_core_option_display {
    pub key: *const c_char,
    pub visible: u8,
}

pub type retro_core_options_update_display_callback_t = unsafe extern "C" fn() -> u8;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_core_options_update_display_callback {
    pub callback: Option<retro_core_options_update_display_callback_t>,
}

pub type retro_keyboard_event_t =
    unsafe extern "C" fn(down: bool, keycode: c_uint, character: u32, key_modifiers: u16);

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_keyboard_callback {
    pub callback: Option<retro_keyboard_event_t>,
}

pub type retro_set_eject_state_t = unsafe extern "C" fn(ejected: bool) -> u8;
pub type retro_get_eject_state_t = unsafe extern "C" fn() -> u8;
pub type retro_get_image_index_t = unsafe extern "C" fn() -> c_uint;
pub type retro_set_image_index_t = unsafe extern "C" fn(index: c_uint) -> u8;
pub type retro_get_num_images_t = unsafe extern "C" fn() -> c_uint;
pub type retro_replace_image_index_t =
    unsafe extern "C" fn(index: c_uint, info: *const retro_game_info) -> u8;
pub type retro_add_image_index_t = unsafe extern "C" fn() -> u8;
pub type retro_set_initial_image_t = unsafe extern "C" fn(index: c_uint, path: *const c_char) -> u8;
pub type retro_get_image_path_t =
    unsafe extern "C" fn(index: c_uint, s: *mut c_char, len: usize) -> u8;
pub type retro_get_image_label_t =
    unsafe extern "C" fn(index: c_uint, s: *mut c_char, len: usize) -> u8;

/// libretro.h declares the pointers non-optional ("all function pointers are required"); `Option` keeps a
/// buggy core's NULL from being undefined behaviour.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_disk_control_callback {
    pub set_eject_state: Option<retro_set_eject_state_t>,
    pub get_eject_state: Option<retro_get_eject_state_t>,
    pub get_image_index: Option<retro_get_image_index_t>,
    pub set_image_index: Option<retro_set_image_index_t>,
    pub get_num_images: Option<retro_get_num_images_t>,
    pub replace_image_index: Option<retro_replace_image_index_t>,
    pub add_image_index: Option<retro_add_image_index_t>,
}

/// Prefix-compatible with `retro_disk_control_callback`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_disk_control_ext_callback {
    pub set_eject_state: Option<retro_set_eject_state_t>,
    pub get_eject_state: Option<retro_get_eject_state_t>,
    pub get_image_index: Option<retro_get_image_index_t>,
    pub set_image_index: Option<retro_set_image_index_t>,
    pub get_num_images: Option<retro_get_num_images_t>,
    pub replace_image_index: Option<retro_replace_image_index_t>,
    pub add_image_index: Option<retro_add_image_index_t>,
    pub set_initial_image: Option<retro_set_initial_image_t>,
    pub get_image_path: Option<retro_get_image_path_t>,
    pub get_image_label: Option<retro_get_image_label_t>,
}

pub const RETRO_VFS_FILE_ACCESS_READ: c_uint = 1 << 0;
pub const RETRO_VFS_FILE_ACCESS_WRITE: c_uint = 1 << 1;
pub const RETRO_VFS_FILE_ACCESS_READ_WRITE: c_uint =
    RETRO_VFS_FILE_ACCESS_READ | RETRO_VFS_FILE_ACCESS_WRITE;
pub const RETRO_VFS_FILE_ACCESS_UPDATE_EXISTING: c_uint = 1 << 2;

pub const RETRO_VFS_SEEK_POSITION_START: c_int = 0;
pub const RETRO_VFS_SEEK_POSITION_CURRENT: c_int = 1;
pub const RETRO_VFS_SEEK_POSITION_END: c_int = 2;

pub const RETRO_VFS_STAT_IS_VALID: c_int = 1 << 0;
pub const RETRO_VFS_STAT_IS_DIRECTORY: c_int = 1 << 1;
// Only reported on Unix (character devices); Windows has no equivalent in `std::fs::Metadata`.
#[cfg_attr(not(unix), allow(dead_code))]
pub const RETRO_VFS_STAT_IS_CHARACTER_SPECIAL: c_int = 1 << 2;

/// Opaque to the core; the frontend defines it (`vfs::VfsFile`).
#[repr(C)]
pub struct retro_vfs_file_handle {
    _private: [u8; 0],
}

/// Opaque to the core; the frontend defines it (`vfs::VfsDir`).
#[repr(C)]
pub struct retro_vfs_dir_handle {
    _private: [u8; 0],
}

pub type retro_vfs_get_path_t =
    unsafe extern "C" fn(stream: *mut retro_vfs_file_handle) -> *const c_char;
pub type retro_vfs_open_t = unsafe extern "C" fn(
    path: *const c_char,
    mode: c_uint,
    hints: c_uint,
) -> *mut retro_vfs_file_handle;
pub type retro_vfs_close_t = unsafe extern "C" fn(stream: *mut retro_vfs_file_handle) -> c_int;
pub type retro_vfs_size_t = unsafe extern "C" fn(stream: *mut retro_vfs_file_handle) -> i64;
pub type retro_vfs_truncate_t =
    unsafe extern "C" fn(stream: *mut retro_vfs_file_handle, length: i64) -> i64;
pub type retro_vfs_tell_t = unsafe extern "C" fn(stream: *mut retro_vfs_file_handle) -> i64;
pub type retro_vfs_seek_t = unsafe extern "C" fn(
    stream: *mut retro_vfs_file_handle,
    offset: i64,
    seek_position: c_int,
) -> i64;
pub type retro_vfs_read_t =
    unsafe extern "C" fn(stream: *mut retro_vfs_file_handle, s: *mut c_void, len: u64) -> i64;
pub type retro_vfs_write_t =
    unsafe extern "C" fn(stream: *mut retro_vfs_file_handle, s: *const c_void, len: u64) -> i64;
pub type retro_vfs_flush_t = unsafe extern "C" fn(stream: *mut retro_vfs_file_handle) -> c_int;
pub type retro_vfs_remove_t = unsafe extern "C" fn(path: *const c_char) -> c_int;
pub type retro_vfs_rename_t =
    unsafe extern "C" fn(old_path: *const c_char, new_path: *const c_char) -> c_int;
pub type retro_vfs_stat_t = unsafe extern "C" fn(path: *const c_char, size: *mut i32) -> c_int;
pub type retro_vfs_mkdir_t = unsafe extern "C" fn(dir: *const c_char) -> c_int;
/// `include_hidden` is a C `bool`, taken as `u8` so a non-0/1 byte is not an invalid Rust `bool`.
pub type retro_vfs_opendir_t =
    unsafe extern "C" fn(dir: *const c_char, include_hidden: u8) -> *mut retro_vfs_dir_handle;
pub type retro_vfs_readdir_t = unsafe extern "C" fn(dirstream: *mut retro_vfs_dir_handle) -> bool;
pub type retro_vfs_dirent_get_name_t =
    unsafe extern "C" fn(dirstream: *mut retro_vfs_dir_handle) -> *const c_char;
pub type retro_vfs_dirent_is_dir_t =
    unsafe extern "C" fn(dirstream: *mut retro_vfs_dir_handle) -> bool;
pub type retro_vfs_closedir_t = unsafe extern "C" fn(dirstream: *mut retro_vfs_dir_handle) -> c_int;

/// `struct retro_vfs_interface`, version 3 layout (v1 up to `rename`, v2 adds `truncate`, v3 the rest).
#[repr(C)]
pub struct retro_vfs_interface {
    pub get_path: retro_vfs_get_path_t,
    pub open: retro_vfs_open_t,
    pub close: retro_vfs_close_t,
    pub size: retro_vfs_size_t,
    pub tell: retro_vfs_tell_t,
    pub seek: retro_vfs_seek_t,
    pub read: retro_vfs_read_t,
    pub write: retro_vfs_write_t,
    pub flush: retro_vfs_flush_t,
    pub remove: retro_vfs_remove_t,
    pub rename: retro_vfs_rename_t,
    pub truncate: retro_vfs_truncate_t,
    pub stat: retro_vfs_stat_t,
    pub mkdir: retro_vfs_mkdir_t,
    pub opendir: retro_vfs_opendir_t,
    pub readdir: retro_vfs_readdir_t,
    pub dirent_get_name: retro_vfs_dirent_get_name_t,
    pub dirent_is_dir: retro_vfs_dirent_is_dir_t,
    pub closedir: retro_vfs_closedir_t,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct retro_vfs_interface_info {
    /// In: version the core needs. Out: version the frontend provides.
    pub required_interface_version: u32,
    /// Out: the frontend's table (the core must not write through it).
    pub iface: *mut retro_vfs_interface,
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
    pub load_game: unsafe extern "C" fn(*const retro_game_info) -> u8,
    pub unload_game: unsafe extern "C" fn(),
    pub serialize_size: unsafe extern "C" fn() -> usize,
    pub serialize: unsafe extern "C" fn(data: *mut c_void, size: usize) -> u8,
    pub unserialize: unsafe extern "C" fn(data: *const c_void, size: usize) -> u8,
    pub get_memory_data: unsafe extern "C" fn(id: c_uint) -> *mut c_void,
    pub get_memory_size: unsafe extern "C" fn(id: c_uint) -> usize,
}
