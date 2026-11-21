//! Every user-facing string of the egui layer, in one place for a future translation.

use emu_runtime::{AudioStatus, JoypadButton};

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
pub const BACK_TO_LIBRARY: &str = "Retour à la bibliothèque";
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
pub const OPTIONS_MAIN: &str = "Principales";
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

pub const KEYS_REFUSED_MODIFIER: &str = "Ctrl, Alt et Maj/Super gauche-droite sont réservées aux raccourcis : choisir une autre touche.";

pub fn stats_audio_status(status: AudioStatus, recoveries: u64) -> String {
    let label = match status {
        AudioStatus::Playing => "Lecture",
        AudioStatus::Recovering => "Reprise en cours",
        AudioStatus::Disabled => "Désactivé",
        _ => "Inconnu",
    };
    format!("Sortie audio : {label} · reprises : {recoveries}")
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
        A::ToggleVirtualKeyboard => "clavier virtuel",
        A::ToggleMouseCapture => "capture de la souris",
        A::ToggleKeyboardMode => "clavier / joystick",
        A::NextDisk => "disquette suivante",
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

/// Unicode category Cf (bidi controls, zero-width, BOM...): they can reorder or hide displayed text.
fn is_format_char(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Text supplied by a core: cut to ~512 bytes (on a char boundary), control characters other than `\n` replaced.
pub fn sanitize_core_text(s: &str) -> String {
    let mut end = s.len().min(MAX_CORE_TEXT);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end]
        .chars()
        .map(|c| {
            if (c.is_control() && c != '\n') || is_format_char(c) {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

// ---- computer systems (ADR 0015) ---------------------------------------------------------------------------

pub const VKBD_CLOSE: &str = "Fermer";
pub const VKBD_HINT: &str = "Flèches / Tab : choisir · Entrée : taper · Échap : fermer · Shift, Control, \
                             Alternate restent enfoncées jusqu'à la touche suivante";
pub const VKBD: &str = "Clavier virtuel";
pub const DISKS: &str = "Disquettes…";
pub const DISKS_TITLE: &str = "Disquettes";
pub const DISKS_NONE: &str = "Aucune disquette dans la liste.";
pub const DISKS_EMPTY_DRIVE: &str = "Lecteur vide";
pub const DISKS_CURRENT: &str = "dans le lecteur";
pub const DISKS_EJECT: &str = "Éjecter";
pub const DISKS_ADD: &str = "Ajouter une image du même dossier :";
pub const DISKS_RESET: &str = "Réinitialiser les disquettes…";
pub const DISKS_RESET_CONFIRM: &str = "Les copies de travail (avec la progression enregistrée sur les \
                                       disquettes), disquettes ajoutées comprises, ainsi que les disquettes de \
                                       sauvegarde créées par le core, seront effacées et le jeu redémarré depuis \
                                       vos fichiers d'origine, qui ne sont jamais modifiés. Confirmer ?";
pub const DISKS_RESET_YES: &str = "Oui, réinitialiser";
pub const DISKS_HINT: &str =
    "Entrée sur une disquette : l'insérer. Raccourci : touche hôte + D (suivante).";
pub const DISKS_UNAVAILABLE: &str = "Ce core ne permet pas de changer de disquette.";
pub const COMPUTER: &str = "Clavier et souris…";
pub const COMPUTER_TITLE: &str = "Clavier et souris";
pub const KEYBOARD_MODE: &str = "Mode du clavier (ce jeu)";
pub const MODE_KEYBOARD: &str = "Clavier (toutes les touches vers la machine)";
pub const MODE_JOYSTICK: &str = "Joystick (ZQSD / flèches, J, K)";
pub const AUTO_CAPTURE: &str = "Capturer la souris au clic (ce jeu)";
pub const ON: &str = "oui";
pub const OFF: &str = "non";
pub const SENSITIVITY: &str = "Sensibilité de la souris (tous les jeux)";
pub const OPTIONS_SCOPE: &str = "Portée des changements";
pub const SCOPE_GAME: &str = "ce jeu";
pub const SCOPE_ALL: &str = "tous les jeux";
pub const OPTION_GAME_MARK: &str = "(ce jeu)";
pub const RESTART_GAME: &str = "Redémarrer le jeu (options « Requires restarting »)";
pub const MODE_SWITCHED_KEYBOARD: &str = "Mode clavier : toutes les touches vont à la machine.";
pub const MODE_SWITCHED_JOYSTICK: &str =
    "Mode joystick : ZQSD / flèches et J, K pilotent le joystick.";
pub const MOUSE_RELEASED: &str = "Souris libérée.";
pub const MOUSE_FALLBACK: &str =
    "Souris sans capture (mode de repli) : gardez le pointeur sur l'image.";
pub const STATES_DISKS_WARNING: &str = "Attention : l'état sauvegardé ne contient pas les disquettes ; \
                                        recharger un état après des écritures disque peut être incohérent.";
pub const NO_DISK_CONTROL: &str = "Ce core ne permet pas de changer de disquette.";
pub const GAME_OPTIONS_LOCKED: &str =
    "Options du jeu invalides : fichier conservé, changements non enregistrés.";

pub const GAMEPAD_UNAVAILABLE: &str = "Manettes indisponibles sur ce système : clavier seul.";
pub const GAMEPAD_PAUSED: &str = "Manette déconnectée : jeu en pause.";

/// `port` is 0-based.
pub fn gamepad_connected(name: &str, port: usize) -> String {
    format!(
        "Manette connectée : {} (joueur {})",
        sanitize_core_text(name),
        port + 1
    )
}

pub fn gamepad_disconnected(port: usize) -> String {
    format!("Manette déconnectée (joueur {})", port + 1)
}

pub fn vkbd_title(machine: &str) -> String {
    format!("Clavier {machine}")
}

pub fn host_hint(host: KeyCode) -> String {
    let h = key_label(host);
    format!(
        "{h} : menu · {h} + J : clavier / joystick · {h} + K : clavier virtuel · {h} + M : souris"
    )
}

pub fn mouse_captured(host: KeyCode) -> String {
    format!("Souris capturée — {} pour libérer", key_label(host))
}

pub fn disk_inserted(index: u32, total: usize, label: &str) -> String {
    format!(
        "Disquette {}/{total} : {}",
        index + 1,
        sanitize_core_text(label)
    )
}

pub fn work_dir_notice(n: &emu_runtime::WorkDirNotice) -> String {
    match n {
        emu_runtime::WorkDirNotice::SourceChanged { previous } => format!(
            "Image source modifiée : nouvelles copies de travail ; l'ancienne progression reste dans {}",
            previous.display()
        ),
        emu_runtime::WorkDirNotice::SameName => {
            "Autre jeu du même nom : copies de travail séparées".to_owned()
        }
    }
}

// ---- built-in file browser (file_picker.rs) ----------------------------------------------------------------

pub const BROWSER_UP: &str = "Dossier parent";
pub const BROWSER_GO: &str = "Aller";
pub const BROWSER_CANCEL: &str = "Annuler";
pub const BROWSER_EMPTY: &str = "Dossier vide.";
pub const BROWSER_LOADING: &str = "Chargement…";
pub const BROWSER_ONLY_HIDDEN: &str = "Aucun fichier visible (fichiers cachés masqués).";
pub const BROWSER_LIST_FAILED: &str = "Lecture du dossier interrompue.";

pub fn browser_truncated(max: usize) -> String {
    format!("{max} premiers éléments affichés")
}
pub const BROWSER_PATH_HINT: &str = "chemin d'un dossier ou d'un fichier";

pub fn browser_filter(extensions: &[String]) -> String {
    format!("Fichiers affichés : .{}", extensions.join(", ."))
}

/// OSD shown when a game starts after the pre-launch firmware check (CLI launch, or library launch after its
/// dialog); `None` when nothing is worth saying.
pub fn firmware_launch_notice(d: &crate::launch_firmware::FirmwareDecision) -> Option<String> {
    use crate::launch_firmware::FirmwareDecision as D;
    match d {
        D::Launch => None,
        D::FallbackFirstTime { label, wanted } => Some(format!(
            "{wanted} absent : {label}, compatibilité limitée. Importez votre BIOS depuis l'écran « Cores » \
             (« Importer un BIOS… »)."
        )),
        D::WrongHash { wanted, .. } => Some(format!(
            "{wanted} ne correspond à aucune version connue : le jeu peut ne pas démarrer."
        )),
        D::Missing { wanted } => Some(format!(
            "BIOS absent : {wanted}. Le jeu peut ne pas démarrer."
        )),
        D::Unreadable { wanted, error } => Some(format!("BIOS illisible ({wanted}) : {error}")),
    }
}

/// Error of a launch, with what to do for the cases the player can fix.
pub fn launch_error(e: &crate::UiError) -> String {
    match e {
        crate::UiError::Runtime(emu_runtime::RuntimeError::DiskFull { path }) => format!(
            "Disque plein : la copie de travail ({}) n'a pas pu être écrite. Libérez de la place puis relancez.",
            path.display()
        ),
        crate::UiError::Runtime(emu_runtime::RuntimeError::DiskListFull { .. }) => {
            DISKS_LIST_FULL.to_owned()
        }
        other => other.to_string(),
    }
}

pub fn disk_failed(e: &dyn std::fmt::Display) -> String {
    format!("Changement de disquette impossible : {e}")
}

pub fn disk_added(label: &str) -> String {
    format!("Disquette ajoutée : {}", sanitize_core_text(label))
}

pub fn game_options_failed(e: &dyn std::fmt::Display) -> String {
    format!("Options du jeu non enregistrées : {e}")
}

pub fn sensitivity_value(v: f32) -> String {
    format!("{v:.1}")
}

pub fn host_key_line(host: KeyCode) -> String {
    format!(
        "Touche hôte : {} ([hotkeys] host_key dans config.toml ; Cmd droit = SuperRight)",
        key_label(host)
    )
}

// ---- game identity, added disks (2.1, design v2.1-identite-disques) ---------------------------------------------

pub const IDENTITY_FALLBACK: &str = "Registre des jeux inutilisable : sauvegardes nommées comme en 2.0 pour cette \
                                     session (le registre n'est pas modifié).";

/// Banner of the launch that separated a homonym, or found the registry unusable.
pub fn identity_notice(notice: &emu_runtime::IdentityNotice, base: &str, core_id: &str) -> String {
    match notice {
        emu_runtime::IdentityNotice::Homonym {
            first,
            sram_copied,
            legacy_states,
        } => {
            let base = sanitize_core_text(base);
            // An empty `first`: the game that keeps the bare name is not known (registry entry without path).
            let first = (!first.as_os_str().is_empty())
                .then(|| sanitize_core_text(&first.display().to_string()));
            let mut msg = match &first {
                Some(_) => format!(
                    "Deux jeux portent le même nom « {base} » : leurs sauvegardes sont maintenant séparées."
                ),
                None => format!(
                    "Un autre jeu porte déjà le nom « {base} » : les sauvegardes de celui-ci sont séparées \
                     (nom « {base} » suivi d'un numéro)."
                ),
            };
            if *sram_copied {
                msg.push_str(" La sauvegarde du jeu a été copiée pour celui-ci.");
            }
            if *legacy_states {
                let owner = first.map_or_else(|| "l'autre jeu".to_owned(), |first| first.clone());
                msg.push_str(&format!(
                    " Les états antérieurs ne sont pas copiés : ils restent attachés à {owner} \
                     (states/{core_id}/{base}.state*)."
                ));
            }
            msg
        }
        emu_runtime::IdentityNotice::RegistryUnusable => IDENTITY_FALLBACK.to_owned(),
    }
}

pub fn options_inherited(base: &str) -> String {
    format!(
        "Options du jeu reprises de « {} » (même nom).",
        sanitize_core_text(base)
    )
}

pub const STATE_CONFIRM_TITLE: &str = "Charger cet état ?";
pub const STATE_OTHER_CONTENT_DISKS: &str = "Cet état peut venir d'un autre fichier du même nom : le charger peut \
                                             modifier les disquettes de ce jeu.";
pub const STATE_OTHER_CONTENT: &str = "Cet état peut venir d'un autre fichier du même nom.";
pub const STATE_LOAD_YES: &str = "Oui, charger";
pub const STATE_LOAD_UNCONFIRMED: &str = "Cet état peut venir d'un autre fichier du même nom : chargement \
                                          annulé (menu indisponible pour confirmer).";

pub fn state_confirm(slot: u8, has_disks: bool) -> String {
    let what = if has_disks {
        STATE_OTHER_CONTENT_DISKS
    } else {
        STATE_OTHER_CONTENT
    };
    format!("État {slot} : {what}")
}

pub const DISKS_ADDED: &str = "Disquettes ajoutées (Entrée : retirer…) :";
pub const DISKS_FORGET: &str = "Retirer de la liste (la copie est gardée)";
pub const DISKS_FORGET_DELETE: &str = "Retirer et supprimer la copie…";
pub const DISKS_FORGET_DELETE_YES: &str = "Oui, supprimer la copie";
pub const CANCEL: &str = "Annuler";
pub const DISKS_INSERTED_REFUSED: &str =
    "Cette disquette est dans le lecteur : éjectez-la d'abord pour la retirer.";
pub const DISKS_LIST_FULL: &str = "Liste pleine : retirez d'abord une disquette ajoutée.";
pub const DISKS_CLEAN: &str = "Nettoyer les copies…";
pub const DISKS_CLEAN_TITLE: &str = "Copies non utilisées";
pub const DISKS_CLEAN_INTRO: &str = "Fichiers du dossier des copies que rien n'utilise (anciennes versions, \
                                     disquettes oubliées). Une copie peut contenir une disquette de \
                                     sauvegarde : rien n'est supprimé sans votre choix.";
pub const DISKS_CLEAN_NONE: &str = "Aucune copie inutilisée.";
pub const DISKS_ADOPT: &str = "Reprendre";
pub const DISKS_CLEAN_YES: &str = "Oui, supprimer";
pub const STALE_PLAYLIST: &str = "ancienne liste .m3u";

/// `1,4 Mo`, `720 Ko`, `12 octets`.
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    let tenths = |unit: u64| {
        let t = (u128::from(bytes) * 10 + u128::from(unit) / 2) / u128::from(unit);
        format!("{},{}", t / 10, t % 10)
    };
    if bytes >= MB {
        format!("{} Mo", tenths(MB))
    } else if bytes >= KB {
        format!("{} Ko", bytes.div_ceil(KB))
    } else {
        format!("{bytes} octets")
    }
}

fn format_time(t: Option<std::time::SystemTime>) -> String {
    t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(
            || "date inconnue".to_owned(),
            |d| format!("{} UTC", format_unix_utc(d.as_secs())),
        )
}

/// Row of an added disk: name, size, and whether its source file is gone (the progress is in the copy).
pub fn added_disk_label(name: &str, size: u64, source_missing: bool, inserted: bool) -> String {
    let mut label = format!("{} — {}", sanitize_core_text(name), format_size(size));
    if source_missing {
        label.push_str(" — fichier d'origine absent");
    }
    if inserted {
        label.push_str(&format!(" ({DISKS_CURRENT})"));
    }
    label
}

pub fn forget_title(name: &str) -> String {
    format!("Disquette ajoutée : {}", sanitize_core_text(name))
}

pub fn forget_delete_confirm(
    name: &str,
    size: u64,
    modified: Option<std::time::SystemTime>,
) -> String {
    format!(
        "Supprimer définitivement la copie {} ({}, modifiée le {}) ? La progression enregistrée sur cette \
         disquette sera perdue ; votre fichier d'origine n'est pas touché.",
        sanitize_core_text(name),
        format_size(size),
        format_time(modified)
    )
}

pub fn disk_forgotten(name: &str) -> String {
    format!(
        "Disquette retirée de la liste : {}",
        sanitize_core_text(name)
    )
}

pub fn disk_copy_deleted(name: &str) -> String {
    format!(
        "Disquette retirée, copie supprimée : {}",
        sanitize_core_text(name)
    )
}

pub fn disk_adopted(name: &str) -> String {
    format!("Copie reprise dans la liste : {}", sanitize_core_text(name))
}

pub fn orphan_label(
    name: &str,
    stale_playlist: bool,
    size: u64,
    modified: Option<std::time::SystemTime>,
) -> String {
    let kind = if stale_playlist {
        format!(" ({STALE_PLAYLIST})")
    } else {
        String::new()
    };
    format!(
        "{}{kind} — {} — {}",
        sanitize_core_text(name),
        format_size(size),
        format_time(modified)
    )
}

pub fn clean_delete_button(count: usize) -> String {
    format!("Supprimer la sélection ({count})…")
}

pub fn clean_confirm(count: usize, bytes: u64) -> String {
    format!(
        "Supprimer définitivement {count} fichier(s) ({}) ? Une copie peut contenir une disquette de sauvegarde.",
        format_size(bytes)
    )
}

pub fn copies_removed(count: usize, bytes: u64) -> String {
    format!(
        "{count} fichier(s) supprimé(s) ({} libérés).",
        format_size(bytes)
    )
}

pub fn clean_failed(e: &dyn std::fmt::Display) -> String {
    format!("Nettoyage impossible : {e}")
}

/// Disk operation error, with the actions the player can take for the known cases.
pub fn disk_error(e: &emu_runtime::RuntimeError) -> String {
    match e {
        emu_runtime::RuntimeError::DiskInserted => DISKS_INSERTED_REFUSED.to_owned(),
        emu_runtime::RuntimeError::DiskListFull { .. } => DISKS_LIST_FULL.to_owned(),
        emu_runtime::RuntimeError::DiskFull { path } => format!(
            "Disque plein : {} n'a pas pu être écrit (copie partielle supprimée).",
            path.display()
        ),
        other => disk_failed(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_text_drops_format_characters() {
        let out = sanitize_core_text("a\u{202E}b\u{200B}c\u{FEFF}d\u{2066}e\u{061C}");
        assert!(out.chars().all(|c| !is_format_char(c)));
        assert_eq!(out.chars().filter(|c| *c == '\u{FFFD}').count(), 5);
    }

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
    fn sizes() {
        assert_eq!(format_size(12), "12 octets");
        assert_eq!(format_size(737_280), "720 Ko");
        assert_eq!(format_size(1_474_560), "1,4 Mo");
    }

    #[test]
    fn disk_errors_tell_what_to_do() {
        let full = emu_runtime::RuntimeError::DiskListFull {
            max: emu_runtime::MAX_MEDIA_IMAGES,
        };
        assert_eq!(disk_error(&full), DISKS_LIST_FULL);
        assert_eq!(
            disk_error(&emu_runtime::RuntimeError::DiskInserted),
            DISKS_INSERTED_REFUSED
        );
        let other = emu_runtime::RuntimeError::InvalidMediaSet("other".into());
        assert!(disk_error(&other).starts_with("Changement de disquette impossible"));
    }

    #[test]
    fn identity_banners() {
        use emu_runtime::IdentityNotice;
        let homonym = IdentityNotice::Homonym {
            first: "/jeux/a/Game.st".into(),
            sram_copied: true,
            legacy_states: true,
        };
        let msg = identity_notice(&homonym, "Game", "hatari");
        assert!(
            msg.contains("Deux jeux portent le même nom « Game »"),
            "{msg}"
        );
        assert!(msg.contains("copiée"), "{msg}");
        assert!(msg.contains("ne sont pas copiés") && msg.contains("/jeux/a/Game.st"));
        assert!(msg.contains("states/hatari/Game.state*"), "{msg}");
        let plain = IdentityNotice::Homonym {
            first: "/x".into(),
            sram_copied: false,
            legacy_states: false,
        };
        let msg = identity_notice(&plain, "Game", "hatari");
        assert!(!msg.contains("copiée") && !msg.contains("états"), "{msg}");
        let unknown = IdentityNotice::Homonym {
            first: std::path::PathBuf::new(),
            sram_copied: false,
            legacy_states: true,
        };
        let msg = identity_notice(&unknown, "Game", "hatari");
        assert!(
            msg.starts_with("Un autre jeu porte déjà le nom « Game »"),
            "{msg}"
        );
        assert!(
            msg.contains("attachés à l'autre jeu (states/hatari/Game.state*)"),
            "{msg}"
        );
        assert_eq!(
            identity_notice(&IdentityNotice::RegistryUnusable, "G", "c"),
            IDENTITY_FALLBACK
        );
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
