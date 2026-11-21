//! Pure rules of the 8-bit computers (CPC, C64; design v5.0 §4): initial keyboard mode, tape keys, joystick port.
// Wired by `app.rs` in CU2.
#![allow(dead_code)]

use std::path::Path;

use emu_assets::cores::CoreActions;
use emu_library::InputDefault;
use emu_runtime::{M3uLaunch, RetroKey};

use crate::config_computer::{KeyboardModeChoice, KeyboardModeSetting};

/// Longest launch text accepted (`MediaSet::launch`, design v5.0 §3.2).
const MAX_LAUNCH_CHARS: usize = 64;

/// Mode of a game at launch: the game's remembered one, else `[computer] keyboard_mode` (unless `Auto`), else the
/// system's `default_input`. Without content (BASIC prompt) it is always the keyboard.
pub(crate) fn initial_keyboard_mode(
    game: Option<KeyboardModeChoice>,
    config: KeyboardModeSetting,
    system: InputDefault,
    no_game: bool,
) -> KeyboardModeChoice {
    if no_game {
        return KeyboardModeChoice::Keyboard;
    }
    if let Some(mode) = game {
        return mode;
    }
    match (config, system) {
        (KeyboardModeSetting::Keyboard, _) => KeyboardModeChoice::Keyboard,
        (KeyboardModeSetting::Joystick, _) => KeyboardModeChoice::Joystick,
        (KeyboardModeSetting::Auto, InputDefault::Keyboard) => KeyboardModeChoice::Keyboard,
        (KeyboardModeSetting::Auto, InputDefault::Joystick) => KeyboardModeChoice::Joystick,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TapeAction {
    Play,
    Stop,
    Rewind,
}

/// Key the core listens to for a tape action; `None` when the core declares none or the name is unknown.
pub(crate) fn tape_key(actions: &CoreActions, action: TapeAction) -> Option<RetroKey> {
    let name = match action {
        TapeAction::Play => &actions.tape_play,
        TapeAction::Stop => &actions.tape_stop,
        TapeAction::Rewind => &actions.tape_rewind,
    };
    name.as_deref().and_then(RetroKey::from_name)
}

/// Other joystick port (`vice_joyport`): `"1"` ⇒ `"2"`, anything else ⇒ `"1"`.
pub(crate) fn toggle_joyport(current: &str) -> &'static str {
    if current == "1" { "2" } else { "1" }
}

/// Port forced by the file name, as VICE does: `(j1)` / `_j1` or `(j2)` / `_j2`, case ignored. Both present ⇒
/// `None` (ambiguous, the menu keeps the user's choice).
pub(crate) fn joyport_forced_by_name(path: &Path) -> Option<u8> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let has = |n: u8| name.contains(&format!("(j{n})")) || name.contains(&format!("_j{n}"));
    match (has(1), has(2)) {
        (true, false) => Some(1),
        (false, true) => Some(2),
        _ => None,
    }
}

/// Checks a launch text (CPC command / C64 program) before it reaches the runtime, which applies the same rules
/// (`MediaSet::launch`). Empty is valid (no launch text). The message is shown as is.
pub(crate) fn validate_launch(kind: M3uLaunch, text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    if kind == M3uLaunch::None {
        return Err("Ce core n'accepte pas de commande de démarrage.".to_owned());
    }
    if text.chars().count() > MAX_LAUNCH_CHARS {
        return Err(format!(
            "Texte trop long (au plus {MAX_LAUNCH_CHARS} caractères)."
        ));
    }
    if !text.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
        return Err("Seuls les caractères ASCII imprimables sont acceptés.".to_owned());
    }
    if kind == M3uLaunch::Program && text.contains([':', '|', '#']) {
        return Err("Les caractères « : », « | » et « # » sont interdits ici.".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use KeyboardModeChoice::{Joystick, Keyboard};

    #[test]
    fn initial_mode_precedence() {
        use InputDefault as D;
        use KeyboardModeSetting as S;
        // game > config > system; no content always Keyboard.
        assert_eq!(
            initial_keyboard_mode(Some(Keyboard), S::Joystick, D::Joystick, false),
            Keyboard
        );
        assert_eq!(
            initial_keyboard_mode(Some(Joystick), S::Keyboard, D::Keyboard, false),
            Joystick
        );
        assert_eq!(
            initial_keyboard_mode(None, S::Keyboard, D::Joystick, false),
            Keyboard
        );
        assert_eq!(
            initial_keyboard_mode(None, S::Joystick, D::Keyboard, false),
            Joystick
        );
        assert_eq!(
            initial_keyboard_mode(None, S::Auto, D::Joystick, false),
            Joystick
        );
        assert_eq!(
            initial_keyboard_mode(None, S::Auto, D::Keyboard, false),
            Keyboard
        );
        assert_eq!(
            initial_keyboard_mode(Some(Joystick), S::Joystick, D::Joystick, true),
            Keyboard
        );
    }

    #[test]
    fn tape_keys_come_from_the_core_declaration() {
        let a = CoreActions {
            tape_play: Some("F13".into()),
            tape_stop: Some("nonsense-key".into()),
            tape_rewind: None,
        };
        assert_eq!(tape_key(&a, TapeAction::Play), RetroKey::from_name("F13"));
        assert!(tape_key(&a, TapeAction::Play).is_some());
        assert_eq!(tape_key(&a, TapeAction::Stop), None);
        assert_eq!(tape_key(&a, TapeAction::Rewind), None);
        assert_eq!(tape_key(&CoreActions::default(), TapeAction::Play), None);
    }

    #[test]
    fn joyport_toggle_and_name_rule() {
        assert_eq!(toggle_joyport("1"), "2");
        assert_eq!(toggle_joyport("2"), "1");
        assert_eq!(toggle_joyport(""), "1");
        for (name, want) in [
            ("Game (j1).d64", Some(1)),
            ("Game_J2.prg", Some(2)),
            ("/a/b/Game_j1.d64", Some(1)),
            ("Game.d64", None),
            ("Game (j1)_j2.d64", None),
            ("/dir_j1/Game.d64", None),
        ] {
            assert_eq!(joyport_forced_by_name(Path::new(name)), want, "{name}");
        }
    }

    #[test]
    fn launch_validation() {
        let long = "A".repeat(65);
        assert!(validate_launch(M3uLaunch::None, "").is_ok());
        assert!(validate_launch(M3uLaunch::None, "RUN\"DISC").is_err());
        assert!(validate_launch(M3uLaunch::Command, "RUN\"DISC").is_ok());
        assert!(validate_launch(M3uLaunch::Command, "A:B").is_ok());
        assert!(validate_launch(M3uLaunch::Program, "GAME").is_ok());
        for bad in ["A:B", "A|B", "#X"] {
            assert!(validate_launch(M3uLaunch::Program, bad).is_err(), "{bad}");
        }
        assert!(validate_launch(M3uLaunch::Command, &long).is_err());
        assert!(validate_launch(M3uLaunch::Command, &long[..64]).is_ok());
        for bad in ["a\nb", "é", "a\tb"] {
            assert!(validate_launch(M3uLaunch::Command, bad).is_err(), "{bad:?}");
        }
    }
}
