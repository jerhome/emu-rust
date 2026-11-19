use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use emu_runtime::{InputBackend, InputFrame, JoypadButton, MAX_PORTS};
use winit::keyboard::KeyCode;

/// Which physical pad a game uses; picks the default layout and the button names shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadStyle {
    MegaDrive,
    MasterSystem,
    GameGear,
    Snes,
    GameBoy,
    Nes,
    Generic,
}

impl PadStyle {
    /// Style implied by the core alone (a multi-system core gets its main system).
    pub fn from_core_id(core_id: &str) -> Self {
        match core_id {
            "genesis_plus_gx" | "picodrive" => Self::MegaDrive,
            "snes9x" | "bsnes" | "mesen-s" => Self::Snes,
            "gambatte" | "sameboy" | "mgba_gb" => Self::GameBoy,
            "nestopia" | "fceumm" | "mesen" | "quicknes" => Self::Nes,
            _ => Self::Generic,
        }
    }

    /// Style implied by a content file extension (case-insensitive, no dot), if it is unambiguous
    /// enough to override the core.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "sms" => Some(Self::MasterSystem),
            "gg" => Some(Self::GameGear),
            "gb" | "gbc" => Some(Self::GameBoy),
            "nes" | "fds" | "unf" | "unif" => Some(Self::Nes),
            "md" | "gen" | "bin" | "smd" => Some(Self::MegaDrive),
            "sfc" | "smc" => Some(Self::Snes),
            _ => None,
        }
    }

    /// Style for `core_id` playing `content`: the content extension (for a `.zip`, the first entry
    /// with a known extension), else the core's own style. `.bin` is ambiguous, so it only counts
    /// when the core itself does not point at another system.
    pub fn detect(core_id: &str, content: &Path) -> Self {
        let core_style = Self::from_core_id(core_id);
        let ext = content
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        let from_ext = if ext.eq_ignore_ascii_case("zip") {
            zip_entry_names(content)
                .iter()
                .find_map(|n| Self::from_extension(n.rsplit_once('.')?.1))
        } else {
            Self::from_extension(&ext)
        };
        match from_ext {
            Some(Self::MegaDrive)
                if ext.eq_ignore_ascii_case("bin")
                    && !matches!(core_style, Self::MegaDrive | Self::Generic) =>
            {
                core_style
            }
            Some(style) => style,
            None => core_style,
        }
    }

    /// Key of the `[input.keyboard.<profile>]` table: the core id, plus a suffix when the system
    /// is not the core's main one (genesis_plus_gx on a Master System game), so each system keeps
    /// its own layout.
    pub fn profile_key(self, core_id: &str) -> String {
        if self == Self::from_core_id(core_id) {
            core_id.to_owned()
        } else {
            format!("{core_id}-{}", self.slug())
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::MegaDrive => "md",
            Self::MasterSystem => "sms",
            Self::GameGear => "gg",
            Self::Snes => "snes",
            Self::GameBoy => "gb",
            Self::Nes => "nes",
            Self::Generic => "generic",
        }
    }

    /// Name of a libretro button on this system's pad.
    pub fn button_name(self, button: JoypadButton) -> &'static str {
        use JoypadButton as B;
        match (self, button) {
            (_, B::Up) => "Haut",
            (_, B::Down) => "Bas",
            (_, B::Left) => "Gauche",
            (_, B::Right) => "Droite",
            (Self::MasterSystem, B::Start) => "Pause",
            (_, B::Start) => "Start",
            (Self::MegaDrive, B::Select) => "Mode",
            (Self::MegaDrive, B::Y) => "A",
            (Self::MegaDrive, B::B) => "B",
            (Self::MegaDrive, B::A) => "C",
            (Self::MegaDrive, B::L) => "X",
            (Self::MegaDrive, B::X) => "Y",
            (Self::MegaDrive, B::R) => "Z",
            // genesis_plus_gx two-button pad: libretro B = button 1, A = button 2.
            (Self::MasterSystem | Self::GameGear, B::B) => "1",
            (Self::MasterSystem | Self::GameGear, B::A) => "2",
            (_, B::Select) => "Select",
            (_, B::A) => "A",
            (_, B::B) => "B",
            (_, B::X) => "X",
            (_, B::Y) => "Y",
            (_, B::L) => "L",
            (_, B::R) => "R",
            (_, B::L2) => "L2",
            (_, B::R2) => "R2",
            (_, B::L3) => "L3",
            (_, B::R3) => "R3",
        }
    }

    /// Buttons listed on the key screen, in display order.
    pub fn buttons(self) -> &'static [JoypadButton] {
        use JoypadButton as B;
        match self {
            // MD A, B, C, X, Y, Z, Mode.
            Self::MegaDrive => &[
                B::Up,
                B::Down,
                B::Left,
                B::Right,
                B::Start,
                B::Select,
                B::Y,
                B::B,
                B::A,
                B::L,
                B::X,
                B::R,
            ],
            Self::MasterSystem | Self::GameGear => {
                &[B::Up, B::Down, B::Left, B::Right, B::Start, B::B, B::A]
            }
            Self::GameBoy | Self::Nes => &[
                B::Up,
                B::Down,
                B::Left,
                B::Right,
                B::Start,
                B::Select,
                B::A,
                B::B,
            ],
            Self::Snes => &[
                B::Up,
                B::Down,
                B::Left,
                B::Right,
                B::Start,
                B::Select,
                B::A,
                B::B,
                B::X,
                B::Y,
                B::L,
                B::R,
            ],
            Self::Generic => &[
                B::Up,
                B::Down,
                B::Left,
                B::Right,
                B::Start,
                B::Select,
                B::A,
                B::B,
                B::X,
                B::Y,
                B::L,
                B::R,
                B::L2,
                B::R2,
                B::L3,
                B::R3,
            ],
        }
    }

    /// One line per binding for the startup log, e.g. `Z=B`.
    pub fn describe(self, map: &KeyMap) -> String {
        map.bindings()
            .iter()
            .map(|(k, p, b)| format!("J{}:{:?}={}", p + 1, k, self.button_name(*b)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Entry names of a zip archive (central directory only; empty on any error or Zip64).
fn zip_entry_names(path: &Path) -> Vec<String> {
    fn read(path: &Path) -> Option<Vec<String>> {
        const MAX_DIR: u64 = 4 << 20;
        let u16_at = |b: &[u8], i: usize| usize::from(u16::from_le_bytes([b[i], b[i + 1]]));
        let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let mut f = File::open(path).ok()?;
        let len = f.metadata().ok()?.len();
        let tail_len = len.min(22 + 65535);
        f.seek(SeekFrom::Start(len - tail_len)).ok()?;
        let mut tail = vec![0; usize::try_from(tail_len).ok()?];
        f.read_exact(&mut tail).ok()?;
        let eocd = tail.windows(4).rposition(|w| w == b"PK\x05\x06")?;
        let eocd = tail.get(eocd..eocd + 22)?;
        let (dir_size, dir_off) = (u64::from(u32_at(eocd, 12)), u64::from(u32_at(eocd, 16)));
        if dir_size > MAX_DIR {
            return None;
        }
        f.seek(SeekFrom::Start(dir_off)).ok()?;
        let mut dir = vec![0; usize::try_from(dir_size).ok()?];
        f.read_exact(&mut dir).ok()?;
        let (mut names, mut i) = (Vec::new(), 0);
        while dir.get(i..i + 46).is_some_and(|h| &h[..4] == b"PK\x01\x02") {
            let (n, extra, comment) = (
                u16_at(&dir, i + 28),
                u16_at(&dir, i + 30),
                u16_at(&dir, i + 32),
            );
            let name = dir.get(i + 46..i + 46 + n)?;
            names.push(String::from_utf8_lossy(name).into_owned());
            i += 46 + n + extra + comment;
        }
        Some(names)
    }
    read(path).unwrap_or_default()
}

/// Physical key → (port, libretro button). Serialized by key/button names (see `config.rs`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyMap {
    bindings: Vec<(KeyCode, usize, JoypadButton)>,
}

impl KeyMap {
    pub fn new(bindings: Vec<(KeyCode, usize, JoypadButton)>) -> Self {
        Self { bindings }
    }

    /// Built-in layout for a pad style.
    pub fn default_for(style: PadStyle) -> Self {
        match style {
            PadStyle::MegaDrive => Self::genesis_default(),
            PadStyle::MasterSystem | PadStyle::GameGear => Self::sega_8bit_default(),
            PadStyle::GameBoy | PadStyle::Nes => Self::nintendo_8bit_default(),
            PadStyle::Snes => Self::snes_default(),
            PadStyle::Generic => Self::generic_default(),
        }
    }

    /// Port 0, genesis_plus_gx layout. The core maps libretro buttons onto the Mega Drive pad
    /// as Y -> A, B -> B, A -> C, Start -> Start (verified headless on the 240p Test Suite:
    /// libretro Y confirms a menu entry, libretro B goes back, libretro A does nothing there).
    /// 6-button pad (not enabled by default in the core): L -> X, X -> Y, R -> Z, Select -> Mode.
    ///
    /// Keys: arrows = D-pad, Enter = Start, Right Shift = Mode, Z/X/C = MD A/B/C (so Z confirms),
    /// A/S/D = MD X/Y/Z.
    pub fn genesis_default() -> Self {
        use JoypadButton as B;
        use KeyCode as K;
        Self::new(vec![
            (K::ArrowUp, 0, B::Up),
            (K::ArrowDown, 0, B::Down),
            (K::ArrowLeft, 0, B::Left),
            (K::ArrowRight, 0, B::Right),
            (K::Enter, 0, B::Start),
            (K::ShiftRight, 0, B::Select),
            (K::KeyZ, 0, B::Y),
            (K::KeyX, 0, B::B),
            (K::KeyC, 0, B::A),
            (K::KeyA, 0, B::L),
            (K::KeyS, 0, B::X),
            (K::KeyD, 0, B::R),
        ])
    }

    /// Port 0, Master System / Game Gear (genesis_plus_gx). Libretro B = button 1 (confirms in
    /// the Game Gear menus, verified headless), A = button 2. To match the SNES, X (confirm) is
    /// button 1 and Z is button 2. Enter = Start (Game Gear) or the console's Pause (Master
    /// System; the Pause mapping is the core's INPUT_START, not verified). No Select key.
    pub fn sega_8bit_default() -> Self {
        use JoypadButton as B;
        use KeyCode as K;
        Self::new(vec![
            (K::ArrowUp, 0, B::Up),
            (K::ArrowDown, 0, B::Down),
            (K::ArrowLeft, 0, B::Left),
            (K::ArrowRight, 0, B::Right),
            (K::Enter, 0, B::Start),
            (K::KeyX, 0, B::B),
            (K::KeyZ, 0, B::A),
        ])
    }

    /// Port 0, Game Boy / NES: libretro A and B are the real A and B, on the SNES keys
    /// (X = A confirms, Z = B). Enter = Start, Right Shift = Select.
    pub fn nintendo_8bit_default() -> Self {
        use JoypadButton as B;
        use KeyCode as K;
        Self::new(vec![
            (K::ArrowUp, 0, B::Up),
            (K::ArrowDown, 0, B::Down),
            (K::ArrowLeft, 0, B::Left),
            (K::ArrowRight, 0, B::Right),
            (K::Enter, 0, B::Start),
            (K::ShiftRight, 0, B::Select),
            (K::KeyX, 0, B::A),
            (K::KeyZ, 0, B::B),
        ])
    }

    /// Port 0, SNES layout (snes9x, bsnes, mesen-s: libretro buttons are the SNES buttons).
    /// The face keys follow the physical pad on the QWERTY home rows: bottom row Z = B, X = A
    /// (A confirms, as on Nintendo games, and sits under the right hand like Z on Mega Drive);
    /// upper row A = Y, S = X; shoulders Q = L, W = R.
    ///
    /// Keys: arrows = D-pad, Enter = Start, Right Shift = Select.
    pub fn snes_default() -> Self {
        use JoypadButton as B;
        use KeyCode as K;
        Self::new(vec![
            (K::ArrowUp, 0, B::Up),
            (K::ArrowDown, 0, B::Down),
            (K::ArrowLeft, 0, B::Left),
            (K::ArrowRight, 0, B::Right),
            (K::Enter, 0, B::Start),
            (K::ShiftRight, 0, B::Select),
            (K::KeyZ, 0, B::B),
            (K::KeyX, 0, B::A),
            (K::KeyA, 0, B::Y),
            (K::KeyS, 0, B::X),
            (K::KeyQ, 0, B::L),
            (K::KeyW, 0, B::R),
        ])
    }

    /// Port 0, plain RetroPad for unknown cores: the SNES layout (RetroPad is SNES-shaped) plus
    /// E/R = L2/R2 and F/G = L3/R3 for PlayStation-like cores.
    pub fn generic_default() -> Self {
        use JoypadButton as B;
        use KeyCode as K;
        let mut map = Self::snes_default();
        map.bind(K::KeyE, 0, B::L2);
        map.bind(K::KeyR, 0, B::R2);
        map.bind(K::KeyF, 0, B::L3);
        map.bind(K::KeyG, 0, B::R3);
        map
    }

    pub fn bindings(&self) -> &[(KeyCode, usize, JoypadButton)] {
        &self.bindings
    }

    /// Binds `key`; one key carries one binding, so any previous binding of that key is replaced.
    pub fn bind(&mut self, key: KeyCode, port: usize, button: JoypadButton) {
        self.unbind(key);
        self.bindings.push((key, port, button));
    }

    /// Removes the binding of `key`; returns whether there was one.
    pub fn unbind(&mut self, key: KeyCode) -> bool {
        let before = self.bindings.len();
        self.bindings.retain(|(k, _, _)| *k != key);
        self.bindings.len() != before
    }

    /// Keys currently bound to (`port`, `button`), in binding order.
    pub fn keys_for(&self, port: usize, button: JoypadButton) -> Vec<KeyCode> {
        self.bindings
            .iter()
            .filter(|(_, p, b)| *p == port && *b == button)
            .map(|(k, _, _)| *k)
            .collect()
    }
}

/// Keyboard `InputBackend` fed with winit key events.
#[derive(Debug, Clone)]
pub struct KeyboardInput {
    map: KeyMap,
    held: HashSet<KeyCode>,
}

impl KeyboardInput {
    pub fn new(map: KeyMap) -> Self {
        Self {
            map,
            held: HashSet::new(),
        }
    }

    /// Records a key transition; returns `true` if the key is bound (event consumed).
    pub fn handle_key(&mut self, key: KeyCode, pressed: bool) -> bool {
        if !self.map.bindings.iter().any(|(k, _, _)| *k == key) {
            return false;
        }
        if pressed {
            self.held.insert(key);
        } else {
            self.held.remove(&key);
        }
        true
    }

    /// Releases every key (window focus lost).
    pub fn release_all(&mut self) {
        self.held.clear();
    }
}

impl InputBackend for KeyboardInput {
    fn poll(&mut self, _frame_index: u64, state: &mut InputFrame) {
        for (key, port, button) in &self.map.bindings {
            if *port < MAX_PORTS && self.held.contains(key) {
                state.pads[*port].set(*button, true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    const ALL_STYLES: [PadStyle; 7] = [
        PadStyle::MegaDrive,
        PadStyle::MasterSystem,
        PadStyle::GameGear,
        PadStyle::Snes,
        PadStyle::GameBoy,
        PadStyle::Nes,
        PadStyle::Generic,
    ];

    /// Minimal zip: central directory + end record only (all `zip_entry_names` reads).
    fn fake_zip(names: &[&str]) -> Vec<u8> {
        let mut dir = Vec::new();
        for n in names {
            let mut h = vec![0u8; 46];
            h[..4].copy_from_slice(b"PK\x01\x02");
            h[28..30].copy_from_slice(&(n.len() as u16).to_le_bytes());
            dir.extend(h);
            dir.extend(n.as_bytes());
        }
        let mut out = dir.clone();
        let mut e = vec![0u8; 22];
        e[..4].copy_from_slice(b"PK\x05\x06");
        e[10..12].copy_from_slice(&(names.len() as u16).to_le_bytes());
        e[12..16].copy_from_slice(&(dir.len() as u32).to_le_bytes());
        out.extend(e);
        out
    }

    #[test]
    fn style_from_content_extension() {
        let d = |core: &str, file: &str| PadStyle::detect(core, Path::new(file));
        assert_eq!(d("genesis_plus_gx", "a.sms"), PadStyle::MasterSystem);
        assert_eq!(d("genesis_plus_gx", "a.GG"), PadStyle::GameGear);
        assert_eq!(d("genesis_plus_gx", "a.md"), PadStyle::MegaDrive);
        assert_eq!(d("genesis_plus_gx", "a.smd"), PadStyle::MegaDrive);
        assert_eq!(d("x", "a.gbc"), PadStyle::GameBoy);
        assert_eq!(d("x", "a.gb"), PadStyle::GameBoy);
        for e in ["nes", "fds", "unf"] {
            let file = format!("a.{e}");
            assert_eq!(d("x", &file), PadStyle::Nes);
        }
        assert_eq!(d("x", "a.sfc"), PadStyle::Snes);
        assert_eq!(d("x", "a.smc"), PadStyle::Snes);
        // Unknown extension: the core decides. `.bin` does not override a SNES core.
        assert_eq!(d("snes9x", "a.rom"), PadStyle::Snes);
        assert_eq!(d("snes9x", "a.bin"), PadStyle::Snes);
        assert_eq!(d("x", "a.bin"), PadStyle::MegaDrive);
        assert_eq!(d("x", "a.rom"), PadStyle::Generic);
    }

    #[test]
    fn style_from_zip_entry() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, entries: &[&str]| {
            let p = dir.path().join(name);
            std::fs::write(&p, fake_zip(entries)).unwrap();
            p
        };
        let sms = write("a.zip", &["readme.txt", "Game.SMS"]);
        assert_eq!(
            PadStyle::detect("genesis_plus_gx", &sms),
            PadStyle::MasterSystem
        );
        let none = write("b.zip", &["readme.txt"]);
        assert_eq!(
            PadStyle::detect("genesis_plus_gx", &none),
            PadStyle::MegaDrive
        );
        let bad = dir.path().join("c.zip");
        std::fs::write(&bad, b"not a zip").unwrap();
        assert_eq!(PadStyle::detect("snes9x", &bad), PadStyle::Snes);
        assert_eq!(
            PadStyle::detect("snes9x", &dir.path().join("missing.zip")),
            PadStyle::Snes
        );
    }

    #[test]
    fn profile_key_separates_systems_of_one_core() {
        assert_eq!(
            PadStyle::MegaDrive.profile_key("genesis_plus_gx"),
            "genesis_plus_gx"
        );
        assert_eq!(
            PadStyle::MasterSystem.profile_key("genesis_plus_gx"),
            "genesis_plus_gx-sms"
        );
        assert_eq!(
            PadStyle::GameGear.profile_key("genesis_plus_gx"),
            "genesis_plus_gx-gg"
        );
        assert_eq!(PadStyle::GameBoy.profile_key("gambatte"), "gambatte");
        assert_eq!(PadStyle::Nes.profile_key("unknown"), "unknown-nes");
    }

    #[test]
    fn eight_bit_button_names_and_layouts() {
        use JoypadButton as B;
        assert_eq!(PadStyle::MasterSystem.button_name(B::B), "1");
        assert_eq!(PadStyle::MasterSystem.button_name(B::A), "2");
        assert_eq!(PadStyle::MasterSystem.button_name(B::Start), "Pause");
        assert_eq!(PadStyle::GameGear.button_name(B::B), "1");
        assert_eq!(PadStyle::GameGear.button_name(B::Start), "Start");
        for s in [PadStyle::GameBoy, PadStyle::Nes] {
            assert_eq!(s.button_name(B::A), "A");
            assert_eq!(s.button_name(B::B), "B");
            assert_eq!(s.button_name(B::Select), "Select");
            // Confirm key is X, as on the SNES.
            assert_eq!(
                KeyMap::default_for(s).keys_for(0, B::A),
                vec![KeyCode::KeyX]
            );
        }
        assert_eq!(
            KeyMap::default_for(PadStyle::GameGear).keys_for(0, B::B),
            vec![KeyCode::KeyX]
        );
    }

    #[test]
    fn pad_style_from_core_id() {
        assert_eq!(
            PadStyle::from_core_id("genesis_plus_gx"),
            PadStyle::MegaDrive
        );
        assert_eq!(PadStyle::from_core_id("picodrive"), PadStyle::MegaDrive);
        for id in ["snes9x", "bsnes", "mesen-s"] {
            assert_eq!(PadStyle::from_core_id(id), PadStyle::Snes);
        }
        assert_eq!(PadStyle::from_core_id("whatever"), PadStyle::Generic);
    }

    #[test]
    fn button_names_per_system() {
        use JoypadButton as B;
        assert_eq!(PadStyle::MegaDrive.button_name(B::Y), "A");
        assert_eq!(PadStyle::MegaDrive.button_name(B::A), "C");
        assert_eq!(PadStyle::MegaDrive.button_name(B::Select), "Mode");
        assert_eq!(PadStyle::Snes.button_name(B::Y), "Y");
        assert_eq!(PadStyle::Snes.button_name(B::Select), "Select");
    }

    #[test]
    fn snes_default_layout() {
        use JoypadButton as B;
        let m = KeyMap::snes_default();
        assert_eq!(m.keys_for(0, B::A), vec![KeyCode::KeyX]);
        assert_eq!(m.keys_for(0, B::B), vec![KeyCode::KeyZ]);
        assert_eq!(m.keys_for(0, B::Y), vec![KeyCode::KeyA]);
        assert_eq!(m.keys_for(0, B::X), vec![KeyCode::KeyS]);
        assert_eq!(m.keys_for(0, B::L), vec![KeyCode::KeyQ]);
        assert_eq!(m.keys_for(0, B::R), vec![KeyCode::KeyW]);
        // Every button shown on the key screen has a key, and no key is bound twice.
        for style in ALL_STYLES {
            let m = KeyMap::default_for(style);
            for b in style.buttons() {
                assert!(!m.keys_for(0, *b).is_empty(), "{style:?} {b:?}");
            }
            let mut keys: Vec<_> = m.bindings().iter().map(|(k, _, _)| *k).collect();
            keys.sort_by_key(|k| format!("{k:?}"));
            keys.dedup();
            assert_eq!(keys.len(), m.bindings().len());
        }
    }

    use super::*;

    #[test]
    fn default_map_has_no_duplicate_keys() {
        let map = KeyMap::genesis_default();
        let keys: HashSet<_> = map.bindings().iter().map(|b| b.0).collect();
        assert_eq!(keys.len(), map.bindings().len());
        assert!(map.bindings().iter().all(|b| b.1 < MAX_PORTS));
    }

    #[test]
    fn genesis_face_buttons_hit_megadrive_abc() {
        let press = |key| {
            let mut kb = KeyboardInput::new(KeyMap::genesis_default());
            kb.handle_key(key, true);
            let mut frame = InputFrame::default();
            kb.poll(0, &mut frame);
            frame.pads[0].bits()
        };
        // MD A, B, C = libretro Y, B, A in genesis_plus_gx.
        assert_eq!(press(KeyCode::KeyZ), 1 << JoypadButton::Y as u32);
        assert_eq!(press(KeyCode::KeyX), 1 << JoypadButton::B as u32);
        assert_eq!(press(KeyCode::KeyC), 1 << JoypadButton::A as u32);
        assert_eq!(press(KeyCode::Enter), 1 << JoypadButton::Start as u32);
    }

    #[test]
    fn bound_key_is_consumed_and_polled() {
        let mut kb = KeyboardInput::new(KeyMap::genesis_default());
        assert!(kb.handle_key(KeyCode::ArrowUp, true));
        assert!(!kb.handle_key(KeyCode::KeyQ, true));
        let mut frame = InputFrame::default();
        kb.poll(0, &mut frame);
        assert!(frame.pads[0].is_pressed(JoypadButton::Up));
        assert!(!frame.pads[0].is_pressed(JoypadButton::Down));
        assert!(!frame.pads[1].is_pressed(JoypadButton::Up));
    }

    #[test]
    fn release_and_release_all() {
        let mut kb = KeyboardInput::new(KeyMap::genesis_default());
        kb.handle_key(KeyCode::KeyZ, true);
        kb.handle_key(KeyCode::KeyX, true);
        kb.handle_key(KeyCode::KeyZ, false);
        let mut frame = InputFrame::default();
        kb.poll(0, &mut frame);
        assert!(!frame.pads[0].is_pressed(JoypadButton::Y));
        assert!(frame.pads[0].is_pressed(JoypadButton::B));
        kb.release_all();
        let mut frame = InputFrame::default();
        kb.poll(1, &mut frame);
        assert_eq!(frame.pads[0].bits(), 0);
    }

    #[test]
    fn bind_replaces_key_and_unbind_removes() {
        let mut map = KeyMap::genesis_default();
        map.bind(KeyCode::KeyZ, 1, JoypadButton::Start);
        assert!(map.keys_for(0, JoypadButton::Y).is_empty());
        assert_eq!(map.keys_for(1, JoypadButton::Start), vec![KeyCode::KeyZ]);
        assert!(map.unbind(KeyCode::KeyZ));
        assert!(!map.unbind(KeyCode::KeyZ));
    }

    #[test]
    fn out_of_range_port_is_ignored() {
        let mut kb = KeyboardInput::new(KeyMap::new(vec![(KeyCode::KeyA, 9, JoypadButton::A)]));
        kb.handle_key(KeyCode::KeyA, true);
        let mut frame = InputFrame::default();
        kb.poll(0, &mut frame);
        assert_eq!(frame.pads[0].bits(), 0);
    }
}
