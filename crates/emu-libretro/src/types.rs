use std::path::PathBuf;
use std::time::Duration;

use crate::sys;

/// Pixel formats a core may select with `RETRO_ENVIRONMENT_SET_PIXEL_FORMAT`.
/// The libretro default (before any call) is `Xrgb1555`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PixelFormat {
    #[default]
    Xrgb1555,
    /// Native-endian `u32` laid out as `0x00RRGGBB`.
    Xrgb8888,
    /// Native-endian `u16`: 5 bits red, 6 green, 5 blue.
    Rgb565,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Xrgb8888 => 4,
            PixelFormat::Xrgb1555 | PixelFormat::Rgb565 => 2,
        }
    }
}

/// A software frame borrowed from the core for the duration of the video callback.
///
/// Invariant: `data.len() == pitch * (height - 1) + width * format.bytes_per_pixel()` (the last row may not be
/// padded to `pitch`), and `pitch >= width * bytes_per_pixel`.
#[derive(Debug, Clone, Copy)]
pub struct VideoFrame<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// Bytes between the starts of two consecutive rows.
    pub pitch: usize,
    pub format: PixelFormat,
}

/// Copy of `retro_system_info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInfo {
    pub library_name: String,
    pub library_version: String,
    /// Lower-case extensions without the dot (`"md"`, `"bin"`, …).
    pub valid_extensions: Vec<String>,
    pub need_fullpath: bool,
    pub block_extract: bool,
}

/// Copy of `retro_game_geometry`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameGeometry {
    pub base_width: u32,
    pub base_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    /// As reported by the core; `<= 0.0` means "use `base_width / base_height`".
    pub aspect_ratio: f32,
}

impl GameGeometry {
    /// Display aspect ratio with the libretro fallback applied.
    ///
    /// Falls back to `base_width / base_height`, then to 4:3 if the base size is degenerate.
    pub fn display_aspect(&self) -> f32 {
        if self.aspect_ratio.is_finite() && self.aspect_ratio > 0.0 {
            self.aspect_ratio
        } else if self.base_width > 0 && self.base_height > 0 {
            self.base_width as f32 / self.base_height as f32
        } else {
            4.0 / 3.0
        }
    }
}

/// Copy of `retro_system_timing`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SystemTiming {
    pub fps: f64,
    pub sample_rate: f64,
}

/// Copy of `retro_system_av_info`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AvInfo {
    pub geometry: GameGeometry,
    pub timing: SystemTiming,
}

/// Base libretro device classes (`RETRO_DEVICE_*`, after masking with `RETRO_DEVICE_MASK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputDevice {
    None,
    Joypad,
    Mouse,
    Keyboard,
    LightGun,
    Analog,
    Pointer,
}

impl InputDevice {
    /// Maps a raw `device` argument (subclasses allowed); `None` for unknown base classes.
    pub fn from_raw(raw: u32) -> Option<Self> {
        match raw & sys::RETRO_DEVICE_MASK {
            sys::RETRO_DEVICE_NONE => Some(InputDevice::None),
            sys::RETRO_DEVICE_JOYPAD => Some(InputDevice::Joypad),
            sys::RETRO_DEVICE_MOUSE => Some(InputDevice::Mouse),
            sys::RETRO_DEVICE_KEYBOARD => Some(InputDevice::Keyboard),
            sys::RETRO_DEVICE_LIGHTGUN => Some(InputDevice::LightGun),
            sys::RETRO_DEVICE_ANALOG => Some(InputDevice::Analog),
            sys::RETRO_DEVICE_POINTER => Some(InputDevice::Pointer),
            _ => None,
        }
    }
}

/// Arguments of `retro_input_state_t`. `id` stays raw: its meaning depends on `device`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputQuery {
    pub port: u32,
    pub device: InputDevice,
    pub index: u32,
    pub id: u32,
}

/// `RETRO_DEVICE_ID_JOYPAD_MASK`: with `InputDevice::Joypad`, the handler returns all buttons as a bitmask
/// (bit n = `JoypadButton` with discriminant n). Advertised via `GET_INPUT_BITMASKS`.
pub const JOYPAD_ID_MASK: u32 = 256;

/// `RETRO_DEVICE_ID_JOYPAD_*`; discriminants are the libretro ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum JoypadButton {
    B = 0,
    Y = 1,
    Select = 2,
    Start = 3,
    Up = 4,
    Down = 5,
    Left = 6,
    Right = 7,
    A = 8,
    X = 9,
    L = 10,
    R = 11,
    L2 = 12,
    R2 = 13,
    L3 = 14,
    R3 = 15,
}

impl JoypadButton {
    pub const ALL: [JoypadButton; 16] = [
        JoypadButton::B,
        JoypadButton::Y,
        JoypadButton::Select,
        JoypadButton::Start,
        JoypadButton::Up,
        JoypadButton::Down,
        JoypadButton::Left,
        JoypadButton::Right,
        JoypadButton::A,
        JoypadButton::X,
        JoypadButton::L,
        JoypadButton::R,
        JoypadButton::L2,
        JoypadButton::R2,
        JoypadButton::L3,
        JoypadButton::R3,
    ];

    pub const fn id(self) -> u32 {
        self as u32
    }

    pub fn from_id(id: u32) -> Option<Self> {
        usize::try_from(id)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
    }

    /// Stable lowercase name used in config files: `"b"`, `"y"`, `"select"`, `"start"`, `"up"`, `"down"`,
    /// `"left"`, `"right"`, `"a"`, `"x"`, `"l"`, `"r"`, `"l2"`, `"r2"`, `"l3"`, `"r3"`. Never rename (persisted).
    pub const fn name(self) -> &'static str {
        match self {
            JoypadButton::B => "b",
            JoypadButton::Y => "y",
            JoypadButton::Select => "select",
            JoypadButton::Start => "start",
            JoypadButton::Up => "up",
            JoypadButton::Down => "down",
            JoypadButton::Left => "left",
            JoypadButton::Right => "right",
            JoypadButton::A => "a",
            JoypadButton::X => "x",
            JoypadButton::L => "l",
            JoypadButton::R => "r",
            JoypadButton::L2 => "l2",
            JoypadButton::R2 => "r2",
            JoypadButton::L3 => "l3",
            JoypadButton::R3 => "r3",
        }
    }

    /// Inverse of `name`; ASCII case-insensitive.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|b| b.name().eq_ignore_ascii_case(name))
    }
}

/// What to hand to `retro_load_game` (phase 2: archives are extracted by emu-runtime).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameSource {
    /// File on disk; read into memory unless the core sets `need_fullpath`.
    Path(PathBuf),
    /// Content already in memory (e.g. extracted from a zip). `path` is the virtual path reported in
    /// `retro_game_info.path` (cores look at its extension); it need not exist. Rejected with
    /// `LibretroError::NeedsFullPath` when the core sets `need_fullpath`.
    Memory { path: PathBuf, data: Vec<u8> },
}

/// `RETRO_MEMORY_*` regions exposed through `retro_get_memory_data` / `retro_get_memory_size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryKind {
    /// `RETRO_MEMORY_SAVE_RAM` (0): battery-backed SRAM, persisted as `.srm`.
    SaveRam,
    /// `RETRO_MEMORY_RTC` (1).
    Rtc,
    /// `RETRO_MEMORY_SYSTEM_RAM` (2).
    SystemRam,
    /// `RETRO_MEMORY_VIDEO_RAM` (3).
    VideoRam,
}

/// `struct retro_fastforwarding_override` received with `SET_FASTFORWARDING_OVERRIDE` (64), answered only when
/// `CoreConfig::fastforward_override` is set (design v5.0 §3.1).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub struct FastForwardOverride {
    /// Speed requested by the core; `0.0` = the frontend's own fast-forward ratio (a negative or non-finite value
    /// sent by the core is stored as `0.0`).
    pub ratio: f32,
    /// `true`: the core wants fast-forward now; `false`: back to normal speed.
    pub fastforward: bool,
    /// The core asks the frontend to show that fast-forward is active.
    pub notification: bool,
    /// While set, the user's fast-forward key is ignored.
    pub inhibit_toggle: bool,
}

/// `RETRO_SERIALIZATION_QUIRK_*` flags declared with `SET_SERIALIZATION_QUIRKS`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct SerializationQuirks(pub u64);

impl SerializationQuirks {
    pub const INCOMPLETE: u64 = 1 << 0;
    pub const MUST_INITIALIZE: u64 = 1 << 1;
    pub const CORE_VARIABLE_SIZE: u64 = 1 << 2;
    pub const FRONT_VARIABLE_SIZE: u64 = 1 << 3;
    pub const SINGLE_SESSION: u64 = 1 << 4;
    pub const ENDIAN_DEPENDENT: u64 = 1 << 5;
    pub const PLATFORM_DEPENDENT: u64 = 1 << 6;

    pub const fn contains(self, flag: u64) -> bool {
        self.0 & flag == flag
    }
}

/// `enum retro_log_level`; also the severity of `CoreMessage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// One device type a core declares for a port through `SET_CONTROLLER_INFO` (env 35): `id` is what
/// `retro_set_controller_port_device` takes (`RETRO_DEVICE_SUBCLASS` ids such as 517 / 261 differ per core).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerType {
    /// Core's own description, at most 64 characters, control characters removed.
    pub description: String,
    pub id: u32,
}

/// User-facing message from `SET_MESSAGE` / `SET_MESSAGE_EXT` (OSD targets only; log targets go to `tracing`).
#[derive(Debug, Clone, PartialEq)]
pub struct CoreMessage {
    pub text: String,
    /// `SET_MESSAGE` frame counts are converted with the current `timing.fps`.
    pub duration: Duration,
    pub level: LogLevel,
}

/// One core option, merged from `SET_VARIABLES` (legacy), `SET_CORE_OPTIONS[_INTL]` (v1) or
/// `SET_CORE_OPTIONS_V2[_INTL]` (v2; English strings are used, no localisation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreOption {
    pub key: String,
    /// v2 `desc_categorized` when the option has a category, else `desc`.
    pub label: String,
    pub info: Option<String>,
    /// Key of a `CoreOptionCategory` (v2 only).
    pub category: Option<String>,
    pub values: Vec<CoreOptionValue>,
    pub default: String,
    pub current: String,
    /// `false` after `SET_CORE_OPTIONS_DISPLAY` hides it; the UI should not list hidden options.
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreOptionValue {
    pub value: String,
    /// Display label; equals `value` when the core gives none.
    pub label: String,
}

/// v2 option category (`retro_core_option_v2_category`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreOptionCategory {
    pub key: String,
    pub label: String,
    pub info: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_device_from_raw_masks_subclasses() {
        assert_eq!(InputDevice::from_raw(0), Some(InputDevice::None));
        assert_eq!(InputDevice::from_raw(1), Some(InputDevice::Joypad));
        assert_eq!(InputDevice::from_raw(6), Some(InputDevice::Pointer));
        // RETRO_DEVICE_SUBCLASS(RETRO_DEVICE_JOYPAD, 2)
        assert_eq!(
            InputDevice::from_raw((3 << 8) | 1),
            Some(InputDevice::Joypad)
        );
        assert_eq!(InputDevice::from_raw(7), None);
        assert_eq!(InputDevice::from_raw(0xff), None);
    }

    #[test]
    fn joypad_from_id_round_trips() {
        for button in JoypadButton::ALL {
            assert_eq!(JoypadButton::from_id(button.id()), Some(button));
        }
        assert_eq!(JoypadButton::from_id(16), None);
        assert_eq!(JoypadButton::from_id(JOYPAD_ID_MASK), None);
        assert_eq!(JoypadButton::from_id(u32::MAX), None);
    }

    #[test]
    fn joypad_names_round_trip() {
        let mut seen = std::collections::HashSet::new();
        for button in JoypadButton::ALL {
            assert!(seen.insert(button.name()), "duplicate {}", button.name());
            assert_eq!(JoypadButton::from_name(button.name()), Some(button));
            let upper = button.name().to_ascii_uppercase();
            assert_eq!(JoypadButton::from_name(&upper), Some(button));
        }
        assert_eq!(JoypadButton::Start.name(), "start");
        assert_eq!(JoypadButton::L2.name(), "l2");
        assert_eq!(JoypadButton::from_name(""), None);
        assert_eq!(JoypadButton::from_name("l4"), None);
        assert_eq!(JoypadButton::from_name(" a"), None);
    }

    #[test]
    fn serialization_quirks_contains() {
        let q = SerializationQuirks(
            SerializationQuirks::INCOMPLETE | SerializationQuirks::MUST_INITIALIZE,
        );
        assert!(q.contains(SerializationQuirks::INCOMPLETE));
        assert!(q.contains(SerializationQuirks::MUST_INITIALIZE));
        assert!(!q.contains(SerializationQuirks::SINGLE_SESSION));
        assert!(q.contains(0));
        assert!(!SerializationQuirks::default().contains(SerializationQuirks::INCOMPLETE));
    }

    #[test]
    fn display_aspect_fallbacks() {
        let mut g = GameGeometry {
            base_width: 320,
            base_height: 224,
            max_width: 320,
            max_height: 480,
            aspect_ratio: 4.0 / 3.0,
        };
        assert_eq!(g.display_aspect(), 4.0 / 3.0);
        g.aspect_ratio = 0.0;
        assert_eq!(g.display_aspect(), 320.0 / 224.0);
        g.aspect_ratio = -1.0;
        assert_eq!(g.display_aspect(), 320.0 / 224.0);
        g.aspect_ratio = f32::NAN;
        assert_eq!(g.display_aspect(), 320.0 / 224.0);
        g.base_height = 0;
        assert_eq!(g.display_aspect(), 4.0 / 3.0);
    }

    #[test]
    fn pixel_format_sizes() {
        assert_eq!(PixelFormat::default(), PixelFormat::Xrgb1555);
        assert_eq!(PixelFormat::Xrgb1555.bytes_per_pixel(), 2);
        assert_eq!(PixelFormat::Rgb565.bytes_per_pixel(), 2);
        assert_eq!(PixelFormat::Xrgb8888.bytes_per_pixel(), 4);
    }
}
