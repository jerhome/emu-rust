//! User-facing strings of the cores screen, in one place for a future translation.

use emu_assets::cores::{CoreError, FirmwareState};

pub const TITLE: &str = "Cores";
pub const BACK: &str = "Retour";
pub const REFRESH: &str = "Vérifier les mises à jour";
pub const REFRESHING: &str = "Vérification du catalogue…";
pub const INSTALL_RECOMMENDED: &str = "Installer les cores recommandés";
pub const NAV_HINT: &str =
    "Flèches / Tab : naviguer · Entrée : valider · Échap : retour (ou fermer le détail)";
pub const NO_MANAGER: &str = "Gestionnaire de cores indisponible";
pub const OFFICIAL_SOURCE: &str =
    "Source : buildbot officiel de libretro (https://buildbot.libretro.com)";
pub const NEVER_CHECKED: &str = "Catalogue jamais vérifié : aucune mise à jour connue.";
pub const NOTHING_TO_SHOW: &str = "Aucun core à afficher.";
pub const NO_CATALOG_HINT: &str = "Catalogue vide : « Vérifier les mises à jour » le télécharge.";

pub const SCOPE_RELEASE: &str = "Systèmes pris en charge";
pub const SCOPE_ALL: &str = "Tous les cores du catalogue";
pub const SEARCH: &str = "Recherche";

pub const INSTALL: &str = "Installer";
pub const UPDATE: &str = "Mettre à jour";
pub const REINSTALL: &str = "Réinstaller";
pub const REMOVE: &str = "Supprimer";
pub const ROLLBACK: &str = "Version précédente";
pub const PIN: &str = "Épingler";
pub const UNPIN: &str = "Désépingler";
pub const DETAILS: &str = "Détails";
pub const CANCEL: &str = "Annuler";
pub const CONFIRM_REMOVE_YES: &str = "Oui, supprimer";
pub const CONFIRM_REMOVE_NO: &str = "Non";
pub const IN_USE: &str = "en cours d'utilisation";
pub const MANUAL: &str = "manuel";
pub const OP_REMOVING: &str = "Suppression";
pub const OP_ROLLING_BACK: &str = "Retour à la version précédente";
pub const OP_PINNING: &str = "Mise à jour de l'épinglage";
pub const ROLLBACK_TAMPERED: &str = "Retour impossible : le fichier du core ne correspond plus à son enregistrement (modifié ou installation interrompue). Réinstaller le core.";
pub const RECOMMENDED: &str = "recommandé";

pub const STATE_UP_TO_DATE: &str = "à jour";
pub const STATE_UPDATE: &str = "mise à jour disponible";
pub const STATE_MANUAL: &str = "manuel";
pub const STATE_TAMPERED: &str = "altéré : réinstaller";
pub const STATE_NOT_INSTALLED: &str = "non installé";
pub const STATE_PINNED: &str = "épinglé";

pub const DETAIL_EXTENSIONS: &str = "Extensions";
pub const DETAIL_FIRMWARE: &str = "BIOS";
pub const DETAIL_NO_FIRMWARE: &str = "Aucun BIOS déclaré par ce core.";
pub const DETAIL_SYSTEM_DIR: &str = "Dossier des BIOS";
pub const DETAIL_FIRMWARE_NOTE: &str =
    "Les BIOS ne sont jamais téléchargés : à copier dans le dossier ci-dessus.";
pub const FIRMWARE_OPTIONAL: &str = "optionnel";
pub const FIRMWARE_REQUIRED: &str = "requis";
pub const DETAIL_CLOSE: &str = "Fermer le détail";

pub fn source_custom(url: &str) -> String {
    format!("Source NON officielle (config.toml) : {url}")
}

pub fn checked_ago(secs: u64) -> String {
    let (n, unit) = match secs {
        0..=59 => return "Catalogue vérifié à l'instant.".to_owned(),
        60..=3599 => (secs / 60, "min"),
        3600..=86_399 => (secs / 3600, "h"),
        _ => (secs / 86_400, "j"),
    };
    format!("Catalogue vérifié il y a {n} {unit}.")
}

pub fn update_all(n: usize) -> String {
    format!("Tout mettre à jour ({n})")
}

pub fn available(date: impl std::fmt::Display) -> String {
    format!("disponible : {date}")
}

pub fn installed(date: impl std::fmt::Display) -> String {
    format!("installé : {date}")
}

pub fn confirm_remove(name: &str) -> String {
    format!("Supprimer {name} ? Les sauvegardes sont conservées.")
}

pub fn firmware_summary(missing: usize, wrong: usize) -> String {
    match (missing, wrong) {
        (0, 0) => String::new(),
        (m, 0) => format!("BIOS : {m} manquant(s)"),
        (0, w) => format!("BIOS : {w} hash incorrect(s)"),
        (m, w) => format!("BIOS : {m} manquant(s), {w} hash incorrect(s)"),
    }
}

pub fn firmware_state(state: &FirmwareState) -> String {
    match state {
        FirmwareState::Ok => "présent".to_owned(),
        FirmwareState::Missing => "manquant".to_owned(),
        FirmwareState::WrongHash { md5 } => {
            let hex: String = md5.iter().map(|b| format!("{b:02x}")).collect();
            format!("hash incorrect (md5 {hex})")
        }
        FirmwareState::Unverified => "présent (non vérifiable)".to_owned(),
        FirmwareState::Free { label } => format!("BIOS libre installé ({label})"),
        FirmwareState::Unreadable(e) => format!("illisible : {e}"),
        FirmwareState::CoreFallback { label } => {
            format!("absent : {label} utilisé à la place (original recommandé)")
        }
        FirmwareState::Builtin { label } => format!("intégré au core ({label})"),
    }
}

/// Row of a core whose table falls back to a built-in ROM: « AROS intégré — Kickstart recommandé » (never shown as
/// an error).
pub fn core_fallback(label: &str, wanted: &str) -> String {
    let short = label.split(" (").next().unwrap_or(label);
    let kind = if wanted.to_ascii_lowercase().starts_with("kick") {
        "Kickstart"
    } else {
        "BIOS d'origine"
    };
    format!("{short} intégré — {kind} recommandé ({wanted})")
}

/// [`core_fallback`] for a region table (`files`: one BIOS per region, PlayStation) and the files of the table
/// already present (`others`, another region's BIOS: the core may use it).
pub fn core_fallback_files(
    label: &str,
    wanted: &str,
    files: &[String],
    others: &[String],
) -> String {
    let mut text = if files.len() > 1 {
        let short = label.split(" (").next().unwrap_or(label);
        let (last, first) = files.split_last().unwrap_or((&files[0], &[]));
        format!(
            "{short} intégré — BIOS d'origine recommandé ({} ou {last} selon la région)",
            first.join(", ")
        )
    } else {
        core_fallback(label, wanted)
    };
    if !others.is_empty() {
        text.push_str(&format!(" ; présent : {}", others.join(", ")));
    }
    text
}

pub fn required_ok(wanted: &str) -> String {
    format!("BIOS présent ({wanted})")
}

pub fn required_problem(wanted: &str, state: &FirmwareState) -> String {
    format!("{wanted} : {}", firmware_state(state))
}

// ---- « Importer un BIOS… » (design v3.0 §2.2) -------------------------------------------------------------

pub const IMPORT_BUTTON: &str = "Importer un BIOS…";
pub const IMPORT_KICKSTART: &str = "Importer un Kickstart…";
pub const IMPORT_ONE: &str = "Importer…";
pub const IMPORT_TITLE: &str = "Importer un BIOS";
pub const IMPORT_NATIVE_OPEN: &str = "Choisissez le fichier dans la fenêtre du système…";
pub const IMPORT_WORKING: &str = "Vérification du fichier…";
pub const IMPORT_INTERRUPTED: &str = "tâche interrompue";
pub const IMPORT_NOTHING: &str = "Rien à importer : le fichier est déjà installé.";
pub const IMPORT_KNOWN: &str = "Fichier reconnu. Il sera copié dans le dossier des BIOS sous :";
pub const IMPORT_DO: &str = "Importer";
pub const IMPORT_REPLACE: &str = "Remplacer les fichiers existants";
pub const IMPORT_REPLACE_NOTE: &str = "Un fichier existant n'est jamais effacé : il est renommé <nom>.user-<date> \
                                       avant le remplacement.";
pub const IMPORT_ROM_KEY: &str = "Clé des ROM chiffrées d'Amiga Forever : elle sera copiée sous le nom rom.key \
                                  (non vérifiée).";
pub const IMPORT_FORCE: &str = "Importer quand même (non vérifié)…";
pub const IMPORT_CHOOSE_NAME: &str = "Nom sous lequel le core attend ce fichier :";
pub const IMPORT_FILTER_HINT: &str = "filtrer (ex. kick, A500)";
pub const IMPORT_UNVERIFIED: &str = "Importer (non vérifié)";
pub const OK: &str = "OK";

pub fn import_wanted(name: &str) -> String {
    format!("Fichier attendu : {name}")
}

pub fn import_failed(e: &str) -> String {
    format!("Import impossible : {e}")
}

pub fn target_new(name: &str, label: &str) -> String {
    if label == name {
        format!("{name} : nouveau")
    } else {
        format!("{name} ({label}) : nouveau")
    }
}

pub fn target_free(name: &str) -> String {
    format!("{name} : remplace le BIOS libre installé par emu-rust")
}

pub fn target_same(name: &str) -> String {
    format!("{name} : déjà installé")
}

pub fn target_other(name: &str) -> String {
    format!(
        "{name} : un autre fichier existe déjà sous ce nom (conservé sauf si vous choisissez « Remplacer »)"
    )
}

pub fn import_unknown(md5: &[u8; 16], size: u64) -> String {
    let hex: String = md5.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "Fichier inconnu (md5 {hex}, {} Kio) : il ne correspond à aucun BIOS connu et n'est pas importé.",
        size / 1024
    )
}

pub fn import_unverified_as(name: &str) -> String {
    format!("Importer sous {name} (non vérifié)")
}

// ---- « Core utilisé » (design v3.0 §3.3) -------------------------------------------------------------------

pub const CORE_CHOICE_TITLE: &str = "Core utilisé par système";
pub const CORE_NOT_INSTALLED: &str = "non installé";

/// What sets apart the cores of one system in « Core utilisé » (design v7.0 §3.5).
pub fn core_choice_hint(core: &str) -> Option<&'static str> {
    match core {
        "pcsx_rearmed" => Some("rapide, défaut"),
        "swanstation" => Some("rendu logiciel"),
        "mednafen_psx" => Some("précis, plus lent"),
        _ => None,
    }
}

pub fn install_chosen(core: &str) -> String {
    format!("Installer {core}")
}

pub fn recommended_on(platforms: &[emu_assets::cores::Platform]) -> Option<String> {
    use emu_assets::cores::Platform;
    let names: Vec<&str> = platforms
        .iter()
        .map(|p| match p {
            Platform::MacosX86_64 => "Mac",
            Platform::WindowsX86_64 => "Windows",
            Platform::LinuxX86_64 => "Linux",
        })
        .collect();
    (!names.is_empty()).then(|| format!("recommandé sur {}", names.join(", ")))
}

pub fn progress(done: u64, total: Option<u64>) -> String {
    let mib = |n: u64| format!("{:.1} Mio", n as f64 / (1024.0 * 1024.0));
    match total {
        Some(t) if t > 0 => format!("{} / {}", mib(done), mib(t)),
        _ => mib(done),
    }
}

/// French, actionable message for a failed operation.
pub fn error(e: &CoreError) -> String {
    match e {
        CoreError::Tampered { .. } => {
            "Le core a été modifié depuis son installation : il ne sera pas chargé. Réinstaller."
                .to_owned()
        }
        CoreError::InUse(_) => {
            "Core en cours d'utilisation : quitter la partie d'abord.".to_owned()
        }
        CoreError::Busy(_) => "Une opération est déjà en cours sur ce core.".to_owned(),
        CoreError::NoCatalog => {
            "Pas encore de catalogue : utiliser « Vérifier les mises à jour ».".to_owned()
        }
        CoreError::UnsupportedPlatform => {
            "Plate-forme non prise en charge par le buildbot : placer le core à la main.".to_owned()
        }
        other => other.to_string(),
    }
}

pub fn op_running(what: &str) -> String {
    format!("{what} en cours…")
}

pub fn queued_waiting(n: usize) -> String {
    format!("{n} installation(s) en attente du catalogue")
}

/// Launch-time messages (library → core).
pub fn no_core_for_system() -> &'static str {
    "Aucun core connu pour ce système."
}

// ---- free BIOS (ADR 0015 §2) -------------------------------------------------------------------------------

pub const BIOS_SECTION: &str = "BIOS et SoundFont libres";
pub const BIOS_VARIANT: &str = "Variante";
pub const BIOS_INSTALL: &str = "Installer";
pub const BIOS_REINSTALL: &str = "Installer cette variante";
pub const BIOS_REMOVE: &str = "Supprimer";
pub const BIOS_REPLACE: &str = "Remplacer le fichier existant…";
pub const BIOS_REPLACE_CONFIRM: &str = "Remplacer votre TOS (fichier non installé par emu-rust, ou modifié depuis) ? \
                                        Votre fichier sera renommé en « .user-<date> », jamais effacé. Confirmer ?";
pub const BIOS_REPLACE_YES: &str = "Oui, remplacer";
pub const BIOS_MISSING: &str = "non installé";
pub fn bios_modified(file: &str) -> String {
    format!("{file} modifié ou remplacé par vous")
}
pub const BIOS_INSTALLED_OK: &str = "BIOS libre installé.";
pub const BIOS_REMOVED: &str = "BIOS libre supprimé.";
pub const BIOS_NOT_REMOVED: &str =
    "Rien à supprimer : le fichier n'a pas été installé par emu-rust ou a été modifié.";
pub const BIOS_INSTALLING: &str = "Téléchargement du BIOS libre…";
pub const BIOS_NOTE: &str = "Téléchargés seulement sur votre demande, vérifiés par empreinte SHA-256. EmuTOS \
                             (SourceForge) : la langue fixe aussi la disposition du clavier de la machine (français = \
                             AZERTY). GeneralUser GS (GitHub, licence GeneralUser GS v2.0, libre) : musique MIDI des \
                             jeux DOS ; les ROM MT-32 se placent avec « Importer un BIOS… ».";
pub const SOUNDFONT_REPLACE_CONFIRM: &str = "Remplacer le SoundFont existant (fichier non installé par emu-rust, ou \
                                             modifié depuis) ? Votre fichier sera renommé en « .user-<date> », \
                                             jamais effacé. Confirmer ?";
pub const SOUNDFONT_INSTALLING: &str = "Téléchargement du SoundFont (32 Mo)…";
pub const SOUNDFONT_INSTALLED_OK: &str = "SoundFont installé : choisissez-le dans « Réglages DOS ▸ MIDI » d'un jeu, \
                                          ou pour tous les jeux DOS ci-dessous.";
pub const JIFFYDOS_USE_FOR_ALL: &str = "Activer JiffyDOS pour tous les jeux C64";
pub const JIFFYDOS_USED_FOR_ALL: &str =
    "JiffyDOS activé pour tous les jeux C64 (un réglage par jeu reste prioritaire).";
pub const SOUNDFONT_USE_FOR_ALL: &str = "Utiliser pour tous les jeux DOS";
pub const SOUNDFONT_USED_FOR_ALL: &str = "MIDI réglé sur GeneralUser GS pour tous les jeux DOS (un réglage par jeu \
                                          reste prioritaire).";

pub fn soundfont_header(name: &str, version: &str, license: &str, file: &str) -> String {
    format!(
        "SoundFont {name} {version} ({license}) — musique MIDI des jeux DOS, installé sous {file}"
    )
}

pub fn bios_installed(label: &str) -> String {
    format!("installé : {label}")
}

pub fn bios_user_file(file: &str) -> String {
    format!("{file} fourni par vous (conservé)")
}

pub fn bios_user_copy(label: &str) -> String {
    format!("{label} (copié hors d'emu-rust, conservé)")
}

pub fn bios_unreadable(e: &str) -> String {
    format!("illisible : {e}")
}

pub fn bios_header(name: &str, version: &str, license: &str, system: &str, file: &str) -> String {
    format!("{name} {version} ({license}) — {system}, installé sous {file}")
}

pub fn bios_error(e: &emu_assets::bios::BiosError) -> String {
    format!("Échec : {e}")
}

pub fn bios_offer(name: &str, license: &str, game: &str) -> String {
    format!(
        "« {game} » a besoin d'un TOS. Installer {name} ({license}, libre, ~2 Mo depuis SourceForge) ? \
         Sinon, copiez votre propre TOS sous le nom tos.img dans le dossier des BIOS."
    )
}

/// Licence notice of the ROMs built into a core (design v3.0 §3.5.7, v5.0 §4): text of `cores.toml`, shown as is.
pub fn builtin_notice(notice: &str) -> String {
    format!("Licence : {notice}")
}
