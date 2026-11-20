//! The built-in virtual keyboard layouts parse and their `[remap]` tables name real keys. Checked on the raw files
//! (the loader is crate-private): same schema as `computer/vkbd.rs`.

use std::collections::BTreeMap;

use emu_runtime::RetroKey;
use serde::Deserialize;
use serde::de::value::{Error, StrDeserializer};
use winit::keyboard::KeyCode;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    id: String,
    name: String,
    row: Vec<Row>,
    #[serde(default)]
    remap: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    keys: Vec<Key>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Key {
    key: String,
    label: String,
    fr: Option<String>,
    de: Option<String>,
    width: Option<f32>,
    gap: Option<f32>,
    #[serde(default)]
    sticky: bool,
}

const FILES: &[(&str, &str)] = &[
    ("pc", include_str!("../data/keyboards/pc.toml")),
    ("cpc", include_str!("../data/keyboards/cpc.toml")),
    ("c64", include_str!("../data/keyboards/c64.toml")),
];

fn known(name: &str) -> bool {
    RetroKey::from_name(name).is_some_and(|k| k != RetroKey::UNKNOWN)
}

fn check(id: &str, text: &str) -> Layout {
    let layout: Layout = toml::from_str(text).unwrap_or_else(|e| panic!("{id} : {e}"));
    assert_eq!(layout.id, id);
    assert!(!layout.name.is_empty());
    assert!(layout.row.len() >= 5, "{id} : trop peu de rangées");
    for key in layout.row.iter().flat_map(|r| &r.keys) {
        assert!(known(&key.key), "{id} : touche inconnue {}", key.key);
        let _ = (&key.label, &key.fr, &key.de, key.sticky);
        let width = key.width.unwrap_or(1.0);
        let gap = key.gap.unwrap_or(0.0);
        assert!((0.5..=10.0).contains(&width), "{id} : largeur de {}", key.key);
        assert!((0.0..=10.0).contains(&gap), "{id} : espace de {}", key.key);
    }
    for (from, to) in &layout.remap {
        KeyCode::deserialize(StrDeserializer::<Error>::new(from))
            .unwrap_or_else(|_| panic!("{id} : KeyCode inconnu {from}"));
        assert!(known(to), "{id} : touche libretro inconnue {to}");
    }
    // The loader contract: `[remap]` closes the file so later tables cannot be swallowed by it.
    if !layout.remap.is_empty() {
        assert!(text.rfind("[remap]") > text.rfind("[[row]]"), "{id} : [remap] pas en dernier");
    }
    layout
}

fn has_key(layout: &Layout, key: &str) -> bool {
    layout.row.iter().flat_map(|r| &r.keys).any(|k| k.key == key)
}

#[test]
fn new_layouts_parse() {
    for (id, text) in FILES {
        check(id, text);
    }
}

#[test]
fn existing_layouts_still_parse() {
    check("atari_st", include_str!("../data/keyboards/atari_st.toml"));
    check("amiga", include_str!("../data/keyboards/amiga.toml"));
}

#[test]
fn pc_uses_oem_102() {
    let pc = check("pc", FILES[0].1);
    assert_eq!(pc.remap.get("IntlBackslash").map(String::as_str), Some("oem_102"));
    assert!(has_key(&pc, "oem_102") && has_key(&pc, "kp5") && has_key(&pc, "f12"));
}

#[test]
fn cpc_function_keys_go_to_the_keypad() {
    let cpc = check("cpc", FILES[1].1);
    assert_eq!(cpc.remap.len(), 10);
    assert_eq!(cpc.remap["F1"], "kp1");
    assert_eq!(cpc.remap["F10"], "kp0");
    assert!(has_key(&cpc, "lalt"), "COPY");
}

#[test]
fn c64_special_keys() {
    let c64 = check("c64", FILES[2].1);
    assert_eq!(c64.remap["AltRight"], "pageup");
    assert_eq!(c64.remap["IntlBackslash"], "end");
    for k in ["escape", "pageup", "end", "lctrl"] {
        assert!(has_key(&c64, k), "{k}");
    }
}
