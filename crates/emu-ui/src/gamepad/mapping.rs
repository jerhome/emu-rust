//! Physical pad buttons and their translation to RetroPad buttons (defaults of ADR 0016).

use emu_runtime::JoypadButton;

use crate::config_gamepad::GamepadConfig;
use crate::keyboard::PadStyle;

/// Physical buttons by position: `South` is Cross on a DualShock 4, `Mode` the PS button,
/// `Select` Share, `Start` Options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PadButton {
    South,
    East,
    West,
    North,
    L1,
    R1,
    L2,
    R2,
    Select,
    Start,
    Mode,
    L3,
    R3,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

impl PadButton {
    pub const ALL: [PadButton; 17] = [
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::L1,
        Self::R1,
        Self::L2,
        Self::R2,
        Self::Select,
        Self::Start,
        Self::Mode,
        Self::L3,
        Self::R3,
        Self::DPadUp,
        Self::DPadDown,
        Self::DPadLeft,
        Self::DPadRight,
    ];

    /// Lowercase positional name used in `config.toml` (`south`, `dpad_up`, `l1`...).
    pub fn config_name(self) -> &'static str {
        match self {
            Self::South => "south",
            Self::East => "east",
            Self::West => "west",
            Self::North => "north",
            Self::L1 => "l1",
            Self::R1 => "r1",
            Self::L2 => "l2",
            Self::R2 => "r2",
            Self::Select => "select",
            Self::Start => "start",
            Self::Mode => "mode",
            Self::L3 => "l3",
            Self::R3 => "r3",
            Self::DPadUp => "dpad_up",
            Self::DPadDown => "dpad_down",
            Self::DPadLeft => "dpad_left",
            Self::DPadRight => "dpad_right",
        }
    }

    pub fn from_config_name(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|b| b.config_name() == name)
    }
}

/// Lowercase RetroPad name used as value in `config.toml` (`b`, `start`, `l2`...).
pub fn retropad_name(button: JoypadButton) -> &'static str {
    use JoypadButton as B;
    match button {
        B::B => "b",
        B::Y => "y",
        B::Select => "select",
        B::Start => "start",
        B::Up => "up",
        B::Down => "down",
        B::Left => "left",
        B::Right => "right",
        B::A => "a",
        B::X => "x",
        B::L => "l",
        B::R => "r",
        B::L2 => "l2",
        B::R2 => "r2",
        B::L3 => "l3",
        B::R3 => "r3",
    }
}

pub fn retropad_from_name(name: &str) -> Option<JoypadButton> {
    let name = name.trim().to_ascii_lowercase();
    JoypadButton::ALL
        .into_iter()
        .find(|b| retropad_name(*b) == name)
}

/// Records the remap of `button` in `[input.gamepad.bindings.<profile_key>]`. `target = None` unbinds (`"none"`).
/// An entry equal to the system default is removed, so the file only keeps real overrides.
pub fn set_binding(
    config: &mut GamepadConfig,
    style: PadStyle,
    profile_key: &str,
    button: PadButton,
    target: Option<JoypadButton>,
) {
    let default = PadMap::default_for(style).target(button);
    let table = config.bindings.entry(profile_key.to_owned()).or_default();
    if target == default {
        table.remove(button.config_name());
    } else {
        let value = target.map_or("none", retropad_name);
        table.insert(button.config_name().to_owned(), value.to_owned());
    }
    if table.is_empty() {
        config.bindings.remove(profile_key);
    }
}

/// `PadButton` -> RetroPad button for one profile. A button without entry goes nowhere (ADR 0016 table).
/// At most one target per physical button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PadMap {
    bindings: Vec<(PadButton, JoypadButton)>,
}

impl PadMap {
    /// Default layout of the system (ADR 0016 table: Mega Drive Cross/Circle/Square = B/C/A -> RetroPad B/A/Y...).
    pub fn default_for(style: PadStyle) -> Self {
        use JoypadButton as J;
        use PadButton as P;
        let dpad = [
            (P::DPadUp, J::Up),
            (P::DPadDown, J::Down),
            (P::DPadLeft, J::Left),
            (P::DPadRight, J::Right),
        ];
        let face_ab = [(P::South, J::B), (P::East, J::A)];
        let extra: &[(PadButton, JoypadButton)] = match style {
            PadStyle::MegaDrive => &[
                (P::West, J::Y),
                (P::North, J::X),
                (P::L1, J::L),
                (P::R1, J::R),
                (P::Select, J::Select),
                (P::Start, J::Start),
            ],
            PadStyle::MasterSystem | PadStyle::GameGear => &[(P::Start, J::Start)],
            PadStyle::Nes | PadStyle::GameBoy => &[(P::Select, J::Select), (P::Start, J::Start)],
            PadStyle::Snes => &[
                (P::West, J::Y),
                (P::North, J::X),
                (P::L1, J::L),
                (P::R1, J::R),
                (P::Select, J::Select),
                (P::Start, J::Start),
            ],
            PadStyle::Generic => &[
                (P::West, J::Y),
                (P::North, J::X),
                (P::L1, J::L),
                (P::R1, J::R),
                (P::L2, J::L2),
                (P::R2, J::R2),
                (P::L3, J::L3),
                (P::R3, J::R3),
                (P::Select, J::Select),
                (P::Start, J::Start),
            ],
        };
        let mut bindings: Vec<_> = dpad.into_iter().chain(face_ab).collect();
        bindings.extend_from_slice(extra);
        Self { bindings }
    }

    /// Default layout of `style` plus the partial overrides of `[input.gamepad.bindings.<profile_key>]`
    /// (physical name -> RetroPad name; `"none"` unbinds; unknown names are ignored).
    pub fn from_config(style: PadStyle, profile_key: &str, config: &GamepadConfig) -> Self {
        let mut map = Self::default_for(style);
        if let Some(table) = config.bindings.get(profile_key) {
            for (physical, target) in table {
                let Some(button) = PadButton::from_config_name(physical) else {
                    continue;
                };
                if target.trim().eq_ignore_ascii_case("none") {
                    map.unbind(button);
                } else if let Some(target) = retropad_from_name(target) {
                    map.bind(button, target);
                }
            }
        }
        map
    }

    /// Empty map (no button reaches the core).
    pub fn new(bindings: Vec<(PadButton, JoypadButton)>) -> Self {
        Self { bindings }
    }

    pub fn bindings(&self) -> &[(PadButton, JoypadButton)] {
        &self.bindings
    }

    /// Replaces or adds one binding (config override or remap screen).
    pub fn bind(&mut self, button: PadButton, target: JoypadButton) {
        match self.bindings.iter_mut().find(|(b, _)| *b == button) {
            Some(entry) => entry.1 = target,
            None => self.bindings.push((button, target)),
        }
    }

    pub fn unbind(&mut self, button: PadButton) -> bool {
        let before = self.bindings.len();
        self.bindings.retain(|(b, _)| *b != button);
        self.bindings.len() != before
    }

    pub fn target(&self, button: PadButton) -> Option<JoypadButton> {
        self.bindings
            .iter()
            .find(|(b, _)| *b == button)
            .map(|(_, t)| *t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(style: PadStyle, b: PadButton) -> Option<JoypadButton> {
        PadMap::default_for(style).target(b)
    }

    #[test]
    fn mega_drive_column() {
        use JoypadButton as J;
        use PadButton as P;
        let s = PadStyle::MegaDrive;
        assert_eq!(t(s, P::South), Some(J::B));
        assert_eq!(t(s, P::East), Some(J::A));
        assert_eq!(t(s, P::West), Some(J::Y));
        assert_eq!(t(s, P::North), Some(J::X));
        assert_eq!(t(s, P::L1), Some(J::L));
        assert_eq!(t(s, P::R1), Some(J::R));
        assert_eq!(t(s, P::Select), Some(J::Select));
        assert_eq!(t(s, P::L2), None);
        assert_eq!(t(s, P::R2), None);
    }

    #[test]
    fn eight_bit_sega_column() {
        use JoypadButton as J;
        use PadButton as P;
        for s in [PadStyle::MasterSystem, PadStyle::GameGear] {
            assert_eq!(t(s, P::South), Some(J::B));
            assert_eq!(t(s, P::East), Some(J::A));
            assert_eq!(t(s, P::Start), Some(J::Start));
            for none in [P::West, P::North, P::L1, P::R1, P::L2, P::Select] {
                assert_eq!(t(s, none), None);
            }
        }
    }

    #[test]
    fn nintendo_8bit_column() {
        use JoypadButton as J;
        use PadButton as P;
        for s in [PadStyle::Nes, PadStyle::GameBoy] {
            assert_eq!(t(s, P::South), Some(J::B));
            assert_eq!(t(s, P::East), Some(J::A));
            assert_eq!(t(s, P::Select), Some(J::Select));
            assert_eq!(t(s, P::Start), Some(J::Start));
            for none in [P::West, P::North, P::L1, P::R1, P::L2, P::R2] {
                assert_eq!(t(s, none), None);
            }
        }
    }

    #[test]
    fn snes_column_has_no_triggers() {
        use JoypadButton as J;
        use PadButton as P;
        let s = PadStyle::Snes;
        assert_eq!(t(s, P::West), Some(J::Y));
        assert_eq!(t(s, P::North), Some(J::X));
        assert_eq!(t(s, P::L1), Some(J::L));
        assert_eq!(t(s, P::R1), Some(J::R));
        assert_eq!(t(s, P::L2), None);
        assert_eq!(t(s, P::R2), None);
    }

    #[test]
    fn generic_exposes_the_whole_retropad() {
        use JoypadButton as J;
        use PadButton as P;
        let s = PadStyle::Generic;
        assert_eq!(t(s, P::L2), Some(J::L2));
        assert_eq!(t(s, P::R3), Some(J::R3));
        assert_eq!(t(s, P::Mode), None);
    }

    #[test]
    fn every_style_maps_the_dpad() {
        for s in [
            PadStyle::MegaDrive,
            PadStyle::MasterSystem,
            PadStyle::GameGear,
            PadStyle::Snes,
            PadStyle::GameBoy,
            PadStyle::Nes,
            PadStyle::Generic,
        ] {
            assert_eq!(t(s, PadButton::DPadUp), Some(JoypadButton::Up));
            assert_eq!(t(s, PadButton::DPadRight), Some(JoypadButton::Right));
        }
    }

    #[test]
    fn bind_replaces_and_unbind_removes() {
        let mut m = PadMap::default_for(PadStyle::Snes);
        m.bind(PadButton::South, JoypadButton::A);
        assert_eq!(m.target(PadButton::South), Some(JoypadButton::A));
        assert!(m.unbind(PadButton::South));
        assert!(!m.unbind(PadButton::South));
        m.bind(PadButton::L2, JoypadButton::L2);
        assert_eq!(m.target(PadButton::L2), Some(JoypadButton::L2));
    }

    #[test]
    fn config_names_round_trip() {
        for b in PadButton::ALL {
            assert_eq!(PadButton::from_config_name(b.config_name()), Some(b));
        }
        for b in JoypadButton::ALL {
            assert_eq!(retropad_from_name(retropad_name(b)), Some(b));
        }
        assert_eq!(PadButton::from_config_name("nope"), None);
    }

    #[test]
    fn config_overrides_are_partial() {
        let mut cfg = GamepadConfig::default();
        cfg.bindings.insert(
            "gambatte".into(),
            [
                ("south".to_owned(), "a".to_owned()),
                ("select".to_owned(), "none".to_owned()),
                ("bogus".to_owned(), "b".to_owned()),
            ]
            .into(),
        );
        let m = PadMap::from_config(PadStyle::GameBoy, "gambatte", &cfg);
        assert_eq!(m.target(PadButton::South), Some(JoypadButton::A));
        assert_eq!(m.target(PadButton::Select), None);
        assert_eq!(m.target(PadButton::East), Some(JoypadButton::A));
        let other = PadMap::from_config(PadStyle::GameBoy, "other", &cfg);
        assert_eq!(other, PadMap::default_for(PadStyle::GameBoy));
    }

    #[test]
    fn set_binding_keeps_only_real_overrides() {
        let mut c = GamepadConfig::default();
        let key = "genesis_plus_gx";
        let style = PadStyle::MegaDrive;
        set_binding(&mut c, style, key, PadButton::South, Some(JoypadButton::X));
        assert_eq!(c.bindings[key]["south"], "x");
        set_binding(&mut c, style, key, PadButton::East, None);
        assert_eq!(c.bindings[key]["east"], "none");
        let map = PadMap::from_config(style, key, &c);
        assert_eq!(map.target(PadButton::South), Some(JoypadButton::X));
        assert_eq!(map.target(PadButton::East), None);
        let default_south = PadMap::default_for(style).target(PadButton::South);
        set_binding(&mut c, style, key, PadButton::South, default_south);
        set_binding(
            &mut c,
            style,
            key,
            PadButton::East,
            PadMap::default_for(style).target(PadButton::East),
        );
        assert!(c.bindings.is_empty());
        assert!(c.is_default());
    }
}
