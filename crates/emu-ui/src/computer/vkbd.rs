//! Virtual keyboard of the frontend (ADR 0015 §7): egui buttons over the running game, the keys a 60 % keyboard
//! lacks (F1–F10, Help, Undo, keypad, arrows). Layouts are data (`data/keyboards/*.toml`). Usable with the mouse
//! (pointer not captured) and the keyboard (arrows / Tab, Enter); later the pad drives the same egui focus.
//! The emulation keeps running. Modifiers are sticky: held until the next key, released on close.

use std::collections::BTreeMap;

use egui::{Align2, Button, Context, Id, RichText, Vec2};
use emu_runtime::{InputBackend, InputFrame, KeyEvent, KeyModifiers, RetroKey};
use serde::Deserialize;

use crate::ui::text;

/// A clicked key stays down this many frames: a press and a release in the same frame may be missed by cores
/// that poll the keyboard state.
const HOLD_FRAMES: u8 = 3;

const ATARI_ST: &str = include_str!("../../data/keyboards/atari_st.toml");
const AMIGA: &str = include_str!("../../data/keyboards/amiga.toml");

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayoutKey {
    pub key: RetroKey,
    pub label: String,
    pub legends: BTreeMap<String, String>,
    pub width: f32,
    pub gap: f32,
    pub sticky: bool,
}

impl LayoutKey {
    /// Legend in `language` (`fr`, `de`), else the US label.
    pub fn legend(&self, language: &str) -> &str {
        self.legends.get(language).unwrap_or(&self.label)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KeyboardLayout {
    pub id: String,
    pub name: String,
    pub rows: Vec<Vec<LayoutKey>>,
    /// Physical key (winit `KeyCode` name) → libretro key name, replacing the positional default
    /// (`keymap::KeyRemap::for_layout` validates and applies it).
    pub remap: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLayout {
    id: String,
    name: String,
    row: Vec<RawRow>,
    #[serde(default)]
    remap: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRow {
    keys: Vec<RawKey>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKey {
    key: String,
    label: String,
    #[serde(default)]
    fr: Option<String>,
    #[serde(default)]
    de: Option<String>,
    #[serde(default)]
    width: Option<f32>,
    #[serde(default)]
    gap: Option<f32>,
    #[serde(default)]
    sticky: bool,
}

impl KeyboardLayout {
    /// Built-in layout by id (`atari_st`, `amiga`); `None` for an unknown id.
    pub fn load(id: &str) -> Option<Self> {
        let text = match id {
            "atari_st" => ATARI_ST,
            "amiga" => AMIGA,
            _ => return None,
        };
        match Self::parse(text) {
            Ok(l) => Some(l),
            Err(e) => {
                tracing::error!("disposition {id} invalide : {e}");
                None
            }
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: RawLayout = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut rows = Vec::new();
        for row in raw.row {
            let mut keys = Vec::new();
            for k in row.keys {
                let key = RetroKey::from_name(&k.key)
                    .filter(|r| *r != RetroKey::UNKNOWN)
                    .ok_or_else(|| format!("touche inconnue : {}", k.key))?;
                let width = k.width.unwrap_or(1.0);
                let gap = k.gap.unwrap_or(0.0);
                if !(0.5..=10.0).contains(&width) || !(0.0..=10.0).contains(&gap) {
                    return Err(format!("taille invalide : {}", k.key));
                }
                let mut legends = BTreeMap::new();
                for (lang, legend) in [("fr", k.fr), ("de", k.de)] {
                    if let Some(l) = legend {
                        legends.insert(lang.to_owned(), l);
                    }
                }
                keys.push(LayoutKey {
                    key,
                    label: k.label,
                    legends,
                    width,
                    gap,
                    sticky: k.sticky,
                });
            }
            rows.push(keys);
        }
        Ok(Self {
            id: raw.id,
            name: raw.name,
            rows,
            remap: raw.remap,
        })
    }

    fn row_units(&self) -> f32 {
        self.rows
            .iter()
            .map(|r| r.iter().map(|k| k.width + k.gap).sum::<f32>())
            .fold(1.0, f32::max)
    }
}

/// Legend language for an EmuTOS variant id (`192fr` → `fr`, `192de` / `192sg` → `de`), else `us`.
pub(crate) fn legend_for_variant(variant: Option<&str>) -> &'static str {
    match variant.map(|v| v.trim_start_matches("192")) {
        Some("fr") => "fr",
        Some("de" | "sg") => "de",
        _ => "us",
    }
}

#[derive(Debug)]
pub(crate) struct VirtualKeyboard {
    layout: Option<KeyboardLayout>,
    legend: String,
    open: bool,
    focus_first: bool,
    /// Sticky modifiers currently down.
    sticky: Vec<RetroKey>,
    /// Normal keys down, with the frames left before their release.
    held: Vec<(RetroKey, u8)>,
    events: Vec<KeyEvent>,
}

impl VirtualKeyboard {
    pub fn new(layout_id: &str, legend: &str) -> Self {
        Self {
            layout: KeyboardLayout::load(layout_id),
            legend: legend.to_owned(),
            open: false,
            focus_first: false,
            sticky: Vec::new(),
            held: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_open(&mut self, open: bool) {
        if open == self.open {
            return;
        }
        self.open = open;
        self.focus_first = open;
        if !open {
            self.release_all();
        }
    }

    fn modifiers(&self) -> KeyModifiers {
        let mut bits = 0;
        for k in &self.sticky {
            bits |= match k.name() {
                Some("lshift" | "rshift") => KeyModifiers::SHIFT,
                Some("lctrl" | "rctrl") => KeyModifiers::CTRL,
                Some("lalt" | "ralt") => KeyModifiers::ALT,
                _ => 0,
            };
        }
        KeyModifiers(bits)
    }

    fn push(&mut self, key: RetroKey, down: bool) {
        let modifiers = self.modifiers();
        self.events.push(KeyEvent {
            down,
            key,
            character: 0,
            modifiers,
        });
    }

    /// A key of the layout was activated (click, Enter).
    pub fn activate(&mut self, key: RetroKey, sticky: bool) {
        if sticky {
            if let Some(i) = self.sticky.iter().position(|k| *k == key) {
                self.sticky.remove(i);
                self.push(key, false);
            } else {
                self.sticky.push(key);
                self.push(key, true);
            }
            return;
        }
        if let Some(entry) = self.held.iter_mut().find(|(k, _)| *k == key) {
            entry.1 = HOLD_FRAMES;
            return;
        }
        self.push(key, true);
        self.held.push((key, HOLD_FRAMES));
    }

    /// Everything up (closing, focus lost).
    pub fn release_all(&mut self) {
        for (k, _) in std::mem::take(&mut self.held) {
            self.push(k, false);
        }
        for k in std::mem::take(&mut self.sticky) {
            self.push(k, false);
        }
    }

    /// Draws the keyboard when open; returns `true` when the user closed it.
    pub fn show(&mut self, ctx: &Context) -> bool {
        if !self.open {
            return false;
        }
        let Some(layout) = self.layout.clone() else {
            return false;
        };
        let area = ctx.content_rect();
        let unit = ((area.width() - 40.0) / layout.row_units()).clamp(18.0, 40.0);
        let mut close = false;
        let mut pressed: Vec<(RetroKey, bool)> = Vec::new();
        let legend = self.legend.clone();
        egui::Window::new(text::vkbd_title(&layout.name))
            .id(Id::unique("virtual-keyboard"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -8.0])
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(3.0);
                for (r, row) in layout.rows.iter().enumerate() {
                    ui.horizontal(|ui| {
                        for (i, k) in row.iter().enumerate() {
                            if k.gap > 0.0 {
                                ui.add_space(k.gap * unit);
                            }
                            let down = self.sticky.contains(&k.key)
                                || self.held.iter().any(|(h, _)| *h == k.key);
                            let resp = ui.add(
                                Button::new(RichText::new(k.legend(&legend)).size(unit * 0.38))
                                    .selected(down)
                                    .min_size(Vec2::new(k.width * unit - 3.0, unit * 0.9)),
                            );
                            if r == 0 && i == 0 && std::mem::take(&mut self.focus_first) {
                                resp.request_focus();
                            }
                            if resp.clicked() {
                                pressed.push((k.key, k.sticky));
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button(text::VKBD_CLOSE).clicked() {
                        close = true;
                    }
                    ui.label(RichText::new(text::VKBD_HINT).small().weak());
                });
            });
        for (key, sticky) in pressed {
            self.activate(key, sticky);
        }
        if close {
            self.set_open(false);
        }
        close
    }
}

impl InputBackend for VirtualKeyboard {
    fn poll(&mut self, _frame_index: u64, state: &mut InputFrame) {
        state.key_events.append(&mut self.events);
        for k in &self.sticky {
            state.keyboard.set(*k, true);
        }
        let mut released = Vec::new();
        for (k, left) in &mut self.held {
            state.keyboard.set(*k, true);
            *left = left.saturating_sub(1);
            if *left == 0 {
                released.push(*k);
            }
        }
        if released.is_empty() {
            return;
        }
        self.held.retain(|(_, left)| *left > 0);
        // Events of this frame are already delivered: the releases go out with the next poll.
        for k in released {
            self.push(k, false);
        }
        // One-shot modifiers: released after the key they modified.
        if self.held.is_empty() {
            for k in std::mem::take(&mut self.sticky) {
                self.push(k, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> RetroKey {
        RetroKey::from_name(name).unwrap()
    }

    #[test]
    fn atari_st_layout_parses_with_the_missing_keys() {
        let l = KeyboardLayout::load("atari_st").unwrap();
        let all: Vec<&LayoutKey> = l.rows.iter().flatten().collect();
        for name in [
            "f1", "f10", "help", "undo", "kp5", "kp_enter", "up", "lalt", "escape",
        ] {
            assert!(all.iter().any(|k| k.key == key(name)), "{name}");
        }
        let mut seen = std::collections::BTreeSet::new();
        for k in &all {
            assert!(seen.insert(k.key), "doublon {:?}", k.key.name());
        }
        let q = all.iter().find(|k| k.key == key("q")).unwrap();
        assert_eq!(q.legend("fr"), "A");
        assert_eq!(q.legend("us"), "Q");
        assert!(KeyboardLayout::load("amiga").is_some());
        assert!(KeyboardLayout::load("nope").is_none());
        assert!(
            KeyboardLayout::parse("id='x'\nname='x'\n[[row]]\nkeys=[{key='nope',label='?'}]")
                .is_err()
        );
    }

    #[test]
    fn legends_follow_emutos_variant() {
        assert_eq!(legend_for_variant(Some("192fr")), "fr");
        assert_eq!(legend_for_variant(Some("192sg")), "de");
        assert_eq!(legend_for_variant(Some("1024k")), "us");
        assert_eq!(legend_for_variant(None), "us");
    }

    fn poll(v: &mut VirtualKeyboard) -> InputFrame {
        let mut f = InputFrame::default();
        v.poll(0, &mut f);
        f
    }

    #[test]
    fn click_holds_a_few_frames_then_releases() {
        let mut v = VirtualKeyboard::new("atari_st", "fr");
        v.set_open(true);
        v.activate(key("f1"), false);
        let f = poll(&mut v);
        assert_eq!(f.key_events.len(), 1);
        assert!(f.key_events[0].down && f.keyboard.is_pressed(key("f1")));
        assert!(poll(&mut v).keyboard.is_pressed(key("f1")));
        let f = poll(&mut v);
        assert!(f.key_events.is_empty());
        let f = poll(&mut v);
        assert_eq!(f.key_events.len(), 1);
        assert!(!f.key_events[0].down);
        assert!(!f.keyboard.is_pressed(key("f1")));
    }

    #[test]
    fn sticky_modifier_is_one_shot_and_released_on_close() {
        let mut v = VirtualKeyboard::new("atari_st", "us");
        v.set_open(true);
        v.activate(key("lalt"), true);
        v.activate(key("a"), false);
        let f = poll(&mut v);
        assert_eq!(f.key_events.len(), 2);
        assert!(f.key_events[1].modifiers.contains(KeyModifiers::ALT));
        poll(&mut v);
        poll(&mut v);
        let f = poll(&mut v);
        let ups: Vec<_> = f
            .key_events
            .iter()
            .filter(|e| !e.down)
            .map(|e| e.key)
            .collect();
        assert_eq!(ups, vec![key("a"), key("lalt")]);

        v.activate(key("lshift"), true);
        poll(&mut v);
        v.set_open(false);
        let f = poll(&mut v);
        assert_eq!(f.key_events.len(), 1);
        assert!(!f.key_events[0].down && f.key_events[0].key == key("lshift"));
        assert!(!f.keyboard.is_pressed(key("lshift")));
    }
}
