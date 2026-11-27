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
pub const HELP: &str = "Aide";
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
        "{} = {} {}.",
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
pub(crate) fn is_format_char(c: char) -> bool {
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

/// [`sanitize_core_text`] as one terminal line: newlines, control and format characters (bidi overrides...)
/// become spaces, ends trimmed.
pub fn sanitize_core_line(s: &str) -> String {
    sanitize_core_text(s)
        .chars()
        .map(|c| if c == '\n' || c == '\u{FFFD}' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// A message that may carry text from a core or a child process, shown to the user: one clean line (control and
/// format characters become spaces, see [`sanitize_core_line`]) where the data folder `root` is left out of every
/// path, as the tidy report does (a verbatim `\\?\` prefix and the separator kind do not matter).
pub(crate) fn display_message(message: &str, root: &std::path::Path) -> String {
    strip_root_text(message, root, cfg!(windows))
}

/// A path text with `/` separators and neither the verbatim `\\?\` prefix nor its `UNC` form (`//?/UNC/srv` is
/// `//srv`).
fn plain_path_text(s: &str) -> String {
    s.replace('\\', "/")
        .replace("//?/UNC/", "//")
        .replace("//?/", "")
}

/// [`display_message`] with the case rule given: Windows paths are case-insensitive (drive letter).
fn strip_root_text(message: &str, root: &std::path::Path, ignore_case: bool) -> String {
    let line = plain_path_text(&sanitize_core_line(message));
    let root = plain_path_text(&root.to_string_lossy());
    let root = root.trim_end_matches('/');
    if root.is_empty() {
        return line;
    }
    let prefix = format!("{root}/");
    if ignore_case {
        // ASCII lower case keeps every byte position: the cut is made on the original text.
        let (lower, needle) = (line.to_ascii_lowercase(), prefix.to_ascii_lowercase());
        let mut out = String::new();
        let mut at = 0;
        while let Some(i) = lower[at..].find(&needle) {
            out.push_str(&line[at..at + i]);
            at += i + needle.len();
        }
        out.push_str(&line[at..]);
        return out;
    }
    line.replace(&prefix, "")
}

// ---- PC-DOS (design v4.0) ---------------------------------------------------------------------------------

pub const DOS_FIRST_LAUNCH: &str = "Menu DOS : Haut / Bas puis Entrée pour lancer un programme · Droite : le lancer automatiquement la prochaine fois";
pub const DOS_CHANGE_PROGRAM: &str = "Changer de programme de démarrage";
pub const DOS_CHANGE_PROGRAM_DONE: &str =
    "Menu DOS : choisissez le programme (Droite : le lancer automatiquement la prochaine fois)";
pub const DOS_RESET: &str = "Réinitialiser les modifications du jeu…";
pub const DOS_RESET_TITLE: &str = "Réinitialiser les modifications du jeu";
pub const DOS_RESET_CONFIRM: &str = "Les sauvegardes de ce jeu, ses réglages (SETUP), le programme de démarrage \
                                     choisi et les correspondances de la manette seront effacés. Vos fichiers \
                                     d'origine ne sont pas touchés. Le jeu redémarre.";
pub const DOS_RESET_YES: &str = "Oui, tout réinitialiser";
pub const DOS_SOUNDFONT_HINT: &str = "Musique MIDI : installez le SoundFont libre GeneralUser GS (32 Mo) depuis \
                                      l'écran Cores de la bibliothèque, ou importez des ROM MT-32, puis redémarrez \
                                      le jeu.";

/// Title of the options `featured_options` puts first (« Réglages DOS »).
pub fn featured_title(system: &str) -> String {
    let short = match system {
        "Amstrad - CPC" => "CPC",
        "Commodore - 64" => "C64",
        "Sega - Mega-CD - Sega CD" => "Mega CD",
        "Nintendo - Game Boy Advance" => "GBA",
        "Sega - 32X" => "32X",
        other => other,
    };
    format!("Réglages {short}")
}

pub const DOS_CONF_REFUSED: &str =
    "Les fichiers .conf ne se lancent pas : choisissez le dossier ou le zip du jeu.";

pub fn dos_folder_refused(core: &str) -> String {
    format!("Le core {core} ne lance pas les dossiers de jeu : choisissez un zip ou une image.")
}

pub fn dos_start_invalid(name: &str) -> String {
    format!(
        "« {} » n'est pas un programme de la racine du jeu (nom court en .exe, .com ou .bat).",
        sanitize_core_text(name)
    )
}

pub fn dos_snapshot_failed(e: &emu_runtime::RuntimeError) -> String {
    use emu_runtime::RuntimeError as E;
    match e {
        E::SnapshotTooLarge { limit } => format!(
            "Dossier de jeu trop gros pour être préparé (limite : {}).",
            format_size(*limit)
        ),
        E::SnapshotCancelled => "Préparation du jeu annulée.".to_owned(),
        E::UnrepresentableName(p) => format!(
            "Nom de fichier impossible à préparer : {}",
            sanitize_core_text(&p.display().to_string())
        ),
        E::InvalidStart(s) => dos_start_invalid(s),
        other => format!("Préparation du jeu impossible : {other}"),
    }
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

// ---- 8-bit computers (CPC, C64; design v5.0 §2.3-§2.5, §4) -------------------------------------------------------

pub const TAPE: &str = "Cassette :";
pub const TAPE_PLAY: &str = "Lecture";
pub const TAPE_STOP: &str = "Arrêt";
pub const TAPE_REWIND: &str = "Rembobiner";
pub const TAPE_PLAYING: &str = "Cassette : lecture";
pub const TAPE_STOPPED: &str = "Cassette : arrêt";
pub const TAPE_REWOUND: &str = "Cassette : rembobinage";
pub const FAST_LOAD_ON: &str = "Chargement accéléré";
pub const FAST_LOAD_OFF: &str = "Chargement terminé : vitesse normale";
pub const FAST_FORWARD_INHIBITED: &str =
    "Avance rapide indisponible pendant le chargement accéléré.";

pub fn joyport_row(port: u8, forced: bool) -> String {
    if forced {
        format!("Port joystick : {port} (fixé par le nom du fichier)")
    } else {
        format!("Port joystick : {port}")
    }
}

pub fn joyport_switched(port: &str) -> String {
    format!("Joystick sur le port {port}")
}

/// Menu row of the pad mode (design v7.0 §2.4).
pub fn pad_mode_row(dualshock: bool) -> String {
    format!("Manette : {}", pad_mode_name(dualshock))
}

fn pad_mode_name(dualshock: bool) -> &'static str {
    if dualshock {
        "DualShock (analogique)"
    } else {
        "Numérique"
    }
}

pub fn pad_mode_switched(dualshock: bool) -> String {
    format!(
        "Manette {} (retenu pour ce jeu)",
        if dualshock {
            "DualShock : sticks analogiques"
        } else {
            "numérique : le stick gauche fait la croix"
        }
    )
}

pub const PAD_MODE_REFUSED: &str =
    "Le core refuse la manette DualShock : manette numérique conservée.";

/// The memory card of this game was copied from another core of the same family (design v7.0 §2.3).
pub fn memory_card_adopted(source: &str) -> String {
    format!("Carte mémoire reprise de {}", sanitize_core_text(source))
}

pub const LAUNCH_COMMAND: &str = "Commande de démarrage…";
pub const LAUNCH_PROGRAM: &str = "Programme à lancer…";
pub const LAUNCH_COMMAND_HINT: &str = "Commande tapée au démarrage, par exemple RUN\"DISC (vide : démarrage \
                                       automatique du core).";
pub const LAUNCH_PROGRAM_HINT: &str = "Nom du programme de la disquette à lancer, par exemple INTRO (vide : \
                                       premier programme).";
pub const LAUNCH_SAVE: &str = "Enregistrer";
pub const LAUNCH_SAVED: &str = "Enregistré pour ce jeu : pris en compte au prochain lancement.";
pub const LAUNCH_CLEARED: &str = "Commande de démarrage retirée pour ce jeu (prochain lancement).";

pub fn launch_row(label: &str, current: Option<&str>) -> String {
    match current {
        Some(t) if !t.is_empty() => format!("{label}  ({t})"),
        _ => label.to_owned(),
    }
}

pub fn launch_ignored(error: &str) -> String {
    format!("Commande de démarrage de ce jeu ignorée : {error}")
}

/// OSD of a launch in joystick mode (design v5.0 §4): how to type on the machine's keyboard.
pub fn joystick_launch_hint(host: KeyCode) -> String {
    format!(
        "Joystick : ZQSD + J / K · {} + J : clavier",
        key_label(host)
    )
}
pub const MOUSE_FALLBACK: &str =
    "Souris sans capture (mode de repli) : gardez le pointeur sur l'image.";
pub const STATES_DISKS_WARNING: &str = "Attention : l'état sauvegardé ne contient pas les disquettes ; \
                                        recharger un état après des écritures disque peut être incohérent.";
pub const NO_DISK_CONTROL: &str = "Ce core ne permet pas de changer de disquette.";
pub const GAME_OPTIONS_LOCKED: &str =
    "Options du jeu invalides : fichier conservé, changements non enregistrés.";

pub const GAMEPAD_UNAVAILABLE: &str = "Manettes indisponibles sur ce système : clavier seul.";
pub const GAMEPAD_PAUSED: &str = "Manette déconnectée : jeu en pause.";

pub fn gamepad_unrecognized(name: &str) -> String {
    format!(
        "Manette non reconnue : {}. À configurer après la partie : Réglages > Manette > Configurer.",
        sanitize_core_text(name)
    )
}

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

/// Disk menu entry: « CD… » for a system of CDs (design v6.0 §3.5).
pub fn disks_menu(disc: bool) -> &'static str {
    if disc { CDS } else { DISKS }
}

pub fn disks_title(disc: bool) -> &'static str {
    if disc { CDS_TITLE } else { DISKS_TITLE }
}

pub fn disks_none(disc: bool) -> &'static str {
    if disc { CDS_NONE } else { DISKS_NONE }
}

pub fn disks_hint(disc: bool) -> &'static str {
    if disc { CDS_HINT } else { DISKS_HINT }
}

pub const CDS: &str = "CD…";
pub const CDS_TITLE: &str = "CD";
pub const CDS_NONE: &str = "Aucun CD dans la liste.";
pub const CDS_HINT: &str = "Entrée sur un CD : l'insérer (« Éjecter » d'abord si le jeu demande d'ouvrir le \
                            lecteur).";

/// [`disk_inserted`], « CD 2/3 : … » for a system of CDs.
pub fn media_inserted(disc: bool, index: u32, total: usize, label: &str) -> String {
    if disc {
        format!("CD {}/{total} : {}", index + 1, sanitize_core_text(label))
    } else {
        disk_inserted(index, total, label)
    }
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
/// The wanted file is an Amiga Kickstart (texts about AROS), not a console BIOS.
pub fn is_kickstart(wanted: &str) -> bool {
    wanted.to_ascii_lowercase().starts_with("kick")
}

pub fn firmware_launch_notice(d: &crate::launch_firmware::FirmwareDecision) -> Option<String> {
    use crate::launch_firmware::FirmwareDecision as D;
    match d {
        D::Launch => None,
        D::FallbackFirstTime {
            label,
            wanted,
            others,
        } => Some(if is_kickstart(wanted) {
            format!(
                "{wanted} absent : {label}, compatibilité limitée. Importez votre BIOS depuis l'écran « Cores » \
                 (« Importer un BIOS… »)."
            )
        } else {
            format!(
                "{} Importez votre BIOS depuis l'écran « Cores » (« Importer un BIOS… »).",
                core_fallback_warning(label, wanted, others)
            )
        }),
        D::WrongHash { wanted, md5 } => {
            Some(match crate::ps1_bios::wrong_region_text(wanted, md5) {
                Some(region) => format!("{wanted} : {region}. Le jeu peut ne pas démarrer."),
                None => format!(
                    "{wanted} ne correspond à aucune version connue : le jeu peut ne pas démarrer."
                ),
            })
        }
        D::Missing { wanted } => Some(format!(
            "BIOS absent : {wanted}. Le jeu peut ne pas démarrer."
        )),
        D::Unreadable { wanted, error } => Some(format!("BIOS illisible ({wanted}) : {error}")),
    }
}

/// Warning of `CoreFallback` for a core that replaces the missing BIOS by its own (design v7.0 §3.5; shown once per
/// game): the wanted file, and the other files of the table already present (`FirmwareCheck::others_present`,
/// e.g. a BIOS of another region). `label` is the core's `builtin_firmware` label.
pub fn core_fallback_warning(label: &str, wanted: &str, others_present: &[String]) -> String {
    let mut text = String::from(
        "BIOS absent : le core utilise son remplaçant intégré, certains jeux peuvent ne pas fonctionner",
    );
    let label = label.trim();
    if !label.is_empty() {
        text.push_str(&format!(" ({label})"));
    }
    text.push('.');
    if !wanted.is_empty() {
        text.push_str(&format!(" Fichier attendu : {wanted}."));
    }
    if !others_present.is_empty() {
        text.push_str(&format!(
            " Présent mais d'une autre région : {}.",
            others_present.join(", ")
        ));
    }
    text
}

/// `retro_load_game` refused, with the core's own explanation (design v6.0 §2.1).
pub fn core_rejected(core_error: &str) -> String {
    format!(
        "le core a refusé le jeu : {}",
        sanitize_core_text(core_error)
    )
}

/// Error of a launch, with what to do for the cases the player can fix.
/// A core that stays in memory (dosbox_pure) is reloaded from a private copy (ADR 0022); the copy failed.
pub fn core_copy_failed(path: &std::path::Path, e: &dyn std::fmt::Display) -> String {
    format!(
        "Ce core ne peut pas être rechargé dans cette session (copie de {} impossible : {e}). Quittez puis \
         relancez emu-rust.",
        path.display()
    )
}

pub fn launch_error(e: &crate::UiError) -> String {
    if let Some(core_error) = e.core_error() {
        return core_rejected(core_error);
    }
    match e {
        crate::UiError::Runtime(emu_runtime::RuntimeError::DiskFull { path }) => format!(
            "Disque plein : la copie de travail ({}) n'a pas pu être écrite. Libérez de la place puis relancez.",
            path.display()
        ),
        crate::UiError::Runtime(emu_runtime::RuntimeError::DiskListFull { .. }) => {
            DISKS_LIST_FULL.to_owned()
        }
        crate::UiError::Runtime(emu_runtime::RuntimeError::Libretro(
            emu_runtime::LibretroError::CoreCopy { path, source },
        )) => core_copy_failed(path, source),
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

// ---- Footer, « Journal » and copyable messages (ADR 0023 §1, §3) ----------------------------------------------

pub const LOG_BUTTON: &str = "Journal";
pub const LOG_TITLE: &str = "Journal";
pub const LOG_FILTER_ERRORS: &str = "Erreurs";
pub const LOG_FILTER_WARNINGS: &str = "Avertissements";
pub const LOG_FILTER_ALL: &str = "Tout";
pub const LOG_OPEN_FOLDER: &str = "Ouvrir le dossier des journaux";
pub const LOG_EMPTY: &str = "Aucun message pour ce filtre.";
pub const LOG_FILE_IN_USE: &str =
    "fichier journal utilisé par une autre instance : cette session n'est pas enregistrée";
pub const LOG_HINT: &str = "PgUp / PgDn : défiler · Échap : retour";
pub const RECENT_MESSAGES: &str = "Derniers messages";
pub const COPY: &str = "Copier";
/// « Ranger les ROMs… » report: the whole report as plain text.
pub const COPY_REPORT: &str = "Copier le rapport";
pub const COPIED: &str = "Copié";
pub const LOG_COPY_ALL: &str = "Copier le journal";

/// Footer line: `emu-rust <version> — dev by claude@jerhome`.
pub fn footer_credit(version: &str, credit: &str) -> String {
    format!("emu-rust {version} — {credit}")
}

/// Label of the footer button: the unread errors and warnings, when there are some.
pub(crate) fn log_button_label(errors: u32, warnings: u32) -> String {
    match (errors, warnings) {
        (0, 0) => LOG_BUTTON.to_owned(),
        (e, 0) => format!("{LOG_BUTTON} ({e} erreur{})", plural(e)),
        (0, w) => format!("{LOG_BUTTON} ({w} avertissement{})", plural(w)),
        (e, w) => format!(
            "{LOG_BUTTON} ({e} erreur{}, {w} avertissement{})",
            plural(e),
            plural(w)
        ),
    }
}

fn plural(n: u32) -> &'static str {
    if n > 1 { "s" } else { "" }
}

/// Where the log folder is when it cannot be opened (container, no file manager).
pub fn log_folder_path(path: &str) -> String {
    format!("Dossier des journaux : {path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_displayed_message_is_one_clean_line_without_the_data_folder() {
        let root = std::path::Path::new("/data/emu");
        let shown = display_message("cannot open /data/emu/cores/x.so:\nbad\u{202E}elf", root);
        assert_eq!(shown, "cannot open cores/x.so: bad elf");
    }

    #[test]
    fn the_data_folder_is_left_out_whatever_the_separator_or_the_verbatim_prefix() {
        let root = std::path::Path::new(r"\\?\C:\Users\me\emu");
        let shown = display_message(r"load C:\Users\me\emu\cores\x.dll failed", root);
        assert_eq!(shown, "load cores/x.dll failed");
    }

    #[test]
    fn a_case_insensitive_cut_never_panics_on_characters_that_change_length() {
        // `to_lowercase` turns U+0130 into two characters and U+212A (Kelvin) into one shorter one.
        let root = std::path::Path::new(r"C:\emu");
        let shown = strip_root_text("\u{130}\u{130}\u{212A} c:/EMU/cores/x", root, true);
        assert_eq!(shown, "\u{130}\u{130}\u{212A} cores/x");
    }

    #[test]
    fn a_unc_root_is_left_out_verbatim_or_not() {
        for (root, line) in [
            (
                r"\\?\UNC\srv\share\emu",
                r"\\?\UNC\srv\share\emu\cores\x.dll",
            ),
            (r"\\srv\share\emu", r"\\srv\share\emu\cores\x.dll"),
            (r"\\?\UNC\srv\share\emu", r"\\srv\share\emu\cores\x.dll"),
            (r"\\srv\share\emu", r"\\?\UNC\srv\share\emu\cores\x.dll"),
        ] {
            let root = std::path::Path::new(root);
            for ignore_case in [false, true] {
                let shown = strip_root_text(&format!("load {line} failed"), root, ignore_case);
                assert_eq!(shown, "load cores/x.dll failed", "{root:?} {line}");
            }
        }
    }

    #[test]
    fn another_folder_is_kept_as_it_is() {
        let root = std::path::Path::new("/data/emu");
        assert_eq!(display_message("see /tmp/x", root), "see /tmp/x");
    }

    /// The default egui fonts have no arrows nor geometric symbols: they would show as empty squares. Every text
    /// module, the manual and the wizard are checked (comments skipped).
    #[test]
    fn texts_avoid_glyphs_missing_from_the_font() {
        let sources = [
            ("ui/text.rs", include_str!("text.rs")),
            ("library/text.rs", include_str!("../library/text.rs")),
            (
                "library/wizard/text.rs",
                include_str!("../library/wizard/text.rs"),
            ),
            ("cores_ui/text.rs", include_str!("../cores_ui/text.rs")),
            (
                "library/tidy/text.rs",
                include_str!("../library/tidy/text.rs"),
            ),
            ("bios_import.rs", include_str!("../bios_import.rs")),
            ("ps1_bios.rs", include_str!("../ps1_bios.rs")),
            ("docs/manuel.md", include_str!("../../../../docs/manuel.md")),
        ];
        for (name, src) in sources {
            for (n, line) in src.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                if let Some(c) = line.chars().find(|c| {
                    matches!(*c, '\u{2190}'..='\u{21FF}' | '\u{25A0}'..='\u{25FF}' | '\u{2700}'..='\u{27BF}')
                }) {
                    panic!(
                        "{name}:{}: glyph {c:?} is missing from the default font",
                        n + 1
                    );
                }
            }
        }
    }

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

    #[test]
    fn core_fallback_warning_text() {
        let t = core_fallback_warning("HLE", "scph5501.bin", &[]);
        assert!(t.starts_with("BIOS absent : le core utilise son remplaçant intégré"));
        assert!(t.contains("scph5501.bin") && t.contains("(HLE)") && !t.contains("région"));
        let t = core_fallback_warning("", "scph5501.bin", &["scph5502.bin".into()]);
        assert!(t.contains("scph5502.bin") && !t.contains("()"));
    }

    /// 7.0: the OSD of a console BIOS replaced by the core says so, with the import hint; the Kickstart keeps its
    /// 3.0 text.
    #[test]
    fn fallback_notice_console_and_kickstart() {
        use crate::launch_firmware::FirmwareDecision as D;
        let ps = firmware_launch_notice(&D::FallbackFirstTime {
            label: "BIOS HLE (intégré au core)".into(),
            wanted: "scph5502.bin".into(),
            others: vec!["scph5501.bin".into()],
        })
        .unwrap();
        assert!(ps.starts_with("BIOS absent : le core utilise son remplaçant intégré"));
        assert!(ps.contains("scph5502.bin") && ps.contains("scph5501.bin"));
        assert!(ps.contains("Importer un BIOS"));
        let kick = firmware_launch_notice(&D::FallbackFirstTime {
            label: "AROS".into(),
            wanted: "kick34005.A500".into(),
            others: Vec::new(),
        })
        .unwrap();
        assert!(kick.starts_with("kick34005.A500 absent : AROS"));
        assert!(
            pad_mode_row(true).contains("DualShock") && pad_mode_row(false).contains("Numérique")
        );
        assert_eq!(
            memory_card_adopted("swanstation"),
            "Carte mémoire reprise de swanstation"
        );
    }
}
