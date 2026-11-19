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
