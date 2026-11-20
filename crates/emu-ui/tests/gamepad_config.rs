//! Configuration of the pad (ADR 0016, task T6): round trip, backward compatibility with a 2.0 `config.toml`, and
//! the DualShock 4 entries of the SDL mapping database embedded by gilrs (read as text: gilrs is never started).

use std::fs;
use std::path::{Path, PathBuf};

use emu_ui::{Config, GamepadConfig, HotkeyButton};

/// A 1.0/2.0 file: no `[input.gamepad]`, an unknown key, comments, keyboard profile.
const CONFIG_2_0: &str = r#"
future_top_level = "kept"

[video]
vsync = false
scale = 4

[audio]
latency_ms = 120

[input.keyboard.default]
bindings = []

[computer]
mouse_sensitivity = 1.5

[core_options.snes9x]
snes9x_hq2x = "enabled"
"#;

fn table(path: &Path) -> toml::Table {
    toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn a_2_0_config_is_unchanged_by_load_and_save() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("config.toml");
    fs::write(&first, CONFIG_2_0).unwrap();
    let config = Config::load(&first).unwrap();
    assert!(config.input.gamepad.is_default());
    assert!(config.input.gamepad.enabled);

    let saved = dir.path().join("saved.toml");
    config.save(&saved).unwrap();
    let after = table(&saved);
    assert!(
        !after["input"].as_table().unwrap().contains_key("gamepad"),
        "no [input.gamepad] written while default"
    );
    assert_eq!(after["future_top_level"].as_str(), Some("kept"));

    // Saving twice is stable (the second save equals the first, byte for byte), and reloading is lossless.
    let reloaded = Config::load(&saved).unwrap();
    assert_eq!(reloaded, config);
    let again = dir.path().join("again.toml");
    reloaded.save(&again).unwrap();
    assert_eq!(fs::read(&saved).unwrap(), fs::read(&again).unwrap());
    // A 2.0 file that already went through a 2.0 save has the same content as after the 2.1 save.
    assert_eq!(table(&again), after);
}

#[cfg(unix)]
#[test]
fn saved_config_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = Config::default();
    config.input.gamepad.deadzone = 0.3;
    config.save(&path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn full_gamepad_section_round_trips_through_a_file() {
    let guid = "030000004c050000c405000000000000";
    let text = format!(
        r#"
[input.gamepad]
enabled = true
deadzone = 0.15
trigger_threshold = 0.6
hotkey_button = "select"
pause_on_disconnect = true
unknown_future_key = 7

[input.gamepad.bindings.genesis_plus_gx]
south = "b"
west = "none"

[input.gamepad.devices."{guid}"]
port = 1
deadzone = 0.2
"#
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, text).unwrap();
    let config = Config::load(&path).unwrap();
    let pad = &config.input.gamepad;
    assert_eq!(pad.deadzone, 0.15);
    assert_eq!(pad.trigger_threshold, 0.6);
    assert_eq!(pad.hotkey_button, HotkeyButton::Select);
    assert!(pad.pause_on_disconnect);
    assert_eq!(pad.bindings["genesis_plus_gx"]["west"], "none");
    assert_eq!(pad.devices[guid].port, Some(1));
    assert!(pad.extra.contains_key("unknown_future_key"));

    let saved = dir.path().join("saved.toml");
    config.save(&saved).unwrap();
    assert_eq!(Config::load(&saved).unwrap(), config);
    let again = dir.path().join("again.toml");
    Config::load(&saved).unwrap().save(&again).unwrap();
    assert_eq!(fs::read(&saved).unwrap(), fs::read(&again).unwrap());
}

#[test]
fn partial_and_default_sections_load_with_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[input.gamepad]\ndeadzone = 0.4\n").unwrap();
    let pad = Config::load(&path).unwrap().input.gamepad;
    assert_eq!(pad.deadzone, 0.4);
    assert_eq!(
        pad.trigger_threshold,
        GamepadConfig::default().trigger_threshold
    );
    assert_eq!(pad.hotkey_button, HotkeyButton::Mode);
    assert!(!pad.pause_on_disconnect);
    // An empty section equals the default and is not written back.
    fs::write(&path, "[input.gamepad]\n").unwrap();
    let config = Config::load(&path).unwrap();
    assert!(config.input.gamepad.is_default());
}

fn gilrs_database() -> Option<PathBuf> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
    let src = home.join("registry/src");
    for registry in fs::read_dir(src).ok()?.flatten() {
        for krate in fs::read_dir(registry.path()).ok()?.flatten() {
            if krate
                .file_name()
                .to_string_lossy()
                .starts_with("gilrs-0.11")
            {
                let db = krate
                    .path()
                    .join("SDL_GameControllerDB/gamecontrollerdb.txt");
                if db.is_file() {
                    return Some(db);
                }
            }
        }
    }
    None
}

/// The gilrs mapping database cannot be queried without creating `Gilrs` (forbidden here), so the embedded text
/// file of the pinned crate is read directly. Skipped (with a message) when the crate sources are not on disk.
#[test]
fn embedded_sdl_database_knows_the_dualshock_4() {
    let Some(db) = gilrs_database() else {
        eprintln!("gilrs sources not found under CARGO_HOME: skipped");
        return;
    };
    let text = fs::read_to_string(db).unwrap();
    // Linux USB v1 and v2 (the GUID of ADR 0016's example is v1), Windows-style 0x0300 entries.
    for guid in [
        "030000004c050000c405000000000000",
        "030000004c050000cc09000000000000",
    ] {
        let line = text
            .lines()
            .find(|l| l.starts_with(guid))
            .unwrap_or_else(|| panic!("DS4 GUID {guid} missing from the SDL database"));
        for field in [
            "a:",
            "b:",
            "leftx:",
            "lefty:",
            "dpup:",
            "start:",
            "lefttrigger:",
        ] {
            assert!(line.contains(field), "{guid}: no {field} entry");
        }
    }
    // macOS and Bluetooth entries exist as well.
    assert!(text.lines().any(|l| l.starts_with("050000004c050000c405")));
}
