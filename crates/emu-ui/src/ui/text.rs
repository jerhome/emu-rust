//! Every user-facing string of the egui layer, in one place for a future translation.

use emu_runtime::JoypadButton;

use crate::keyboard::PadStyle;
use winit::keyboard::KeyCode;

use crate::hotkeys::HotkeyAction;

pub const MENU_TITLE: &str = "Menu";
pub const RESUME: &str = "Reprendre";
pub const RESET: &str = "Réinitialiser";
pub const SLOT: &str = "Emplacement";
pub const SAVE_STATE: &str = "Sauvegarder l'état";
pub const LOAD_STATE: &str = "Charger l'état";
pub const SLOT_EMPTY: &str = "vide";
pub const KEYS: &str = "Configuration des touches";
pub const CORE_OPTIONS: &str = "Options du core";
pub const QUIT: &str = "Quitter";
pub const BACK: &str = "Retour";
pub const NAV_HINT: &str = "Flèches / Tab : naviguer · Entrée : valider · Échap : retour";

pub const KEYS_PROFILE: &str = "Profil";
pub const PROFILE_CORE: &str = "ce core";
pub const PROFILE_DEFAULT: &str = "défaut (tous les cores)";
pub const KEYS_PLAYER: &str = "Manette";
pub const KEYS_UNBOUND: &str = "—";
pub const KEYS_RESTORE: &str = "Rétablir la disposition intégrée";
pub const KEYS_HINT: &str = "Entrée sur un bouton puis appuyer sur la touche voulue.";
pub const KEYS_CLEARED: &str = "Liaison effacée.";
pub const KEYS_CANCELLED: &str = "Réassignation annulée.";
pub const KEYS_RESTORED: &str = "Disposition intégrée rétablie.";

pub const OPTIONS_NONE: &str = "Ce core n'a pas d'options.";
pub const OPTIONS_GENERAL: &str = "Général";
pub const OPTIONS_DEFAULTS: &str = "Rétablir les valeurs par défaut";
pub const OPTIONS_HINT: &str =
    "Gauche / Droite ou Entrée : changer la valeur (appliquée tout de suite).";

pub const CONFIG_LOCKED: &str = "config.toml est invalide : les changements valent pour cette session \
                                 mais ne seront pas enregistrés (le fichier n'est pas modifié).";
pub const UNLOCK: &str = "Remplacer config.toml par les réglages actuels…";
pub const UNLOCK_CONFIRM: &str = "Le fichier invalide sera écrasé. Confirmer ?";
pub const UNLOCK_DONE: &str = "config.toml remplacé par les réglages actuels.";
pub const YES: &str = "Oui, remplacer";
pub const NO: &str = "Non";
pub const CONFIG_SAVED: &str = "Configuration enregistrée.";
pub const PAUSED: &str = "Pause";
pub const RESUMED: &str = "Reprise";
pub const FAST_FORWARD_ON: &str = "Avance rapide";
pub const RESET_DONE: &str = "Jeu réinitialisé.";

pub const STATS_TITLE: &str = "Statistiques (F3)";

pub fn menu_hint(key: KeyCode) -> String {
    format!("{} : menu", key_label(key))
}

pub fn port_label(port: usize) -> String {
    format!("Joueur {}", port + 1)
}

pub fn slot_value(slot: u8) -> String {
    slot.to_string()
}

pub fn slot_saved_at(date: &str) -> String {
    format!("sauvegardé le {date} (UTC)")
}

pub fn state_saved(slot: u8) -> String {
    format!("État {slot} sauvegardé.")
}

pub fn state_loaded(slot: u8) -> String {
    format!("État {slot} chargé.")
}

pub fn state_save_failed(slot: u8, e: &dyn std::fmt::Display) -> String {
    format!("Sauvegarde de l'état {slot} impossible : {e}")
}

pub fn state_load_failed(slot: u8, e: &dyn std::fmt::Display) -> String {
    format!("Chargement de l'état {slot} impossible : {e}")
}

pub fn slot_selected(slot: u8, info: Option<&str>) -> String {
    match info {
        Some(date) => format!("Emplacement {slot} ({})", slot_saved_at(date)),
        None => format!("Emplacement {slot} ({SLOT_EMPTY})"),
    }
}

pub fn config_save_failed(e: &dyn std::fmt::Display) -> String {
    format!("Configuration non enregistrée : {e}")
}

pub fn option_failed(e: &dyn std::fmt::Display) -> String {
    format!("Option refusée : {e}")
}

pub fn keys_capture(style: PadStyle, port: usize, button: JoypadButton) -> String {
    format!(
        "Appuyez sur une touche pour « {} {} »… (Échap : annuler, Retour arrière : effacer)",
        port_label(port),
        style.button_name(button)
    )
}

pub fn keys_bound(style: PadStyle, key: KeyCode, port: usize, button: JoypadButton) -> String {
    format!(
        "{} → {} {}.",
        key_label(key),
        port_label(port),
        style.button_name(button)
    )
}

pub fn keys_moved(
    style: PadStyle,
    key: KeyCode,
    from: (usize, JoypadButton),
    to: (usize, JoypadButton),
) -> String {
    format!(
        "Conflit : {} était liée à {} {} ; elle passe à {} {}.",
        key_label(key),
        port_label(from.0),
        style.button_name(from.1),
        port_label(to.0),
        style.button_name(to.1)
    )
}

pub fn keys_refused(key: KeyCode, action: HotkeyAction) -> String {
    format!(
        "{} est réservée au raccourci « {} » : choisir une autre touche.",
        key_label(key),
        hotkey_label(action)
    )
}

pub fn stats_fps(fps: f64) -> String {
    format!("Images/s : {fps:.1}")
}

pub fn stats_audio(buffered_ms: Option<u128>, target_ms: Option<u128>) -> String {
    match (buffered_ms, target_ms) {
        (Some(b), Some(t)) => format!("Audio en file : {b} ms (consigne {t} ms)"),
        _ => "Audio : désactivé".to_owned(),
    }
}

pub fn stats_xruns(underruns: u64, dropped: u64) -> String {
    format!("Xruns : {underruns} · échantillons jetés : {dropped}")
}

pub fn stats_frame(frame: u64) -> String {
    format!("Image n° {frame}")
}

pub fn hotkey_label(action: HotkeyAction) -> &'static str {
    use HotkeyAction as A;
    match action {
        A::Menu => "menu",
        A::TogglePause => "pause",
        A::Reset => "réinitialiser",
        A::SaveState => "sauvegarder l'état",
        A::LoadState => "charger l'état",
        A::PrevSlot => "emplacement précédent",
        A::NextSlot => "emplacement suivant",
        A::FastForward => "avance rapide",
        A::ToggleFullscreen => "plein écran",
        A::ToggleStats => "statistiques",
    }
}

/// Readable key name; falls back to the winit name without its `Key` / `Digit` prefix.
pub fn key_label(key: KeyCode) -> String {
    use KeyCode as K;
    let fixed = match key {
        K::ArrowUp => "Flèche haut",
        K::ArrowDown => "Flèche bas",
        K::ArrowLeft => "Flèche gauche",
        K::ArrowRight => "Flèche droite",
        K::Enter => "Entrée",
        K::NumpadEnter => "Entrée (pavé num.)",
        K::Space => "Espace",
        K::Backspace => "Retour arrière",
        K::Escape => "Échap",
        K::Tab => "Tab",
        K::ShiftLeft => "Maj gauche",
        K::ShiftRight => "Maj droite",
        K::ControlLeft => "Ctrl gauche",
        K::ControlRight => "Ctrl droite",
        K::AltLeft => "Alt",
        K::AltRight => "Alt Gr",
        K::Delete => "Suppr",
        K::Insert => "Inser",
        K::PageUp => "Page préc.",
        K::PageDown => "Page suiv.",
        K::Home => "Début",
        K::End => "Fin",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_owned();
    }
    let name = format!("{key:?}");
    if let Some(rest) = name.strip_prefix("Numpad") {
        return format!("{rest} (pavé num.)");
    }
    name.strip_prefix("Key")
        .or_else(|| name.strip_prefix("Digit"))
        .map_or_else(|| name.clone(), str::to_owned)
}

/// `YYYY-MM-DD HH:MM` (UTC) from Unix seconds, without a date crate.
pub fn format_unix_utc(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rem / 3_600,
        (rem % 3_600) / 60
    )
}

const MAX_CORE_TEXT: usize = 512;

/// Text supplied by a core: cut to ~512 bytes (on a char boundary), control characters other than `\n` replaced.
pub fn sanitize_core_text(s: &str) -> String {
    let mut end = s.len().min(MAX_CORE_TEXT);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end]
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_text_is_bounded_and_clean() {
        let long = "é".repeat(400);
        let out = sanitize_core_text(&long);
        assert!(out.len() <= MAX_CORE_TEXT && out.chars().all(|c| c == 'é'));
        assert_eq!(
            sanitize_core_text("a\u{1b}[31mb\nc\0"),
            "a\u{FFFD}[31mb\nc\u{FFFD}"
        );
    }

    #[test]
    fn dates() {
        assert_eq!(format_unix_utc(0), "1970-01-01 00:00");
        assert_eq!(format_unix_utc(951_782_400), "2000-02-29 00:00");
        assert_eq!(format_unix_utc(1_795_100_400), "2026-11-19 15:00");
    }

    #[test]
    fn key_labels() {
        assert_eq!(key_label(KeyCode::KeyZ), "Z");
        assert_eq!(key_label(KeyCode::Digit3), "3");
        assert_eq!(key_label(KeyCode::F2), "F2");
        assert_eq!(key_label(KeyCode::Numpad4), "4 (pavé num.)");
        assert_eq!(key_label(KeyCode::ArrowUp), "Flèche haut");
    }
}
