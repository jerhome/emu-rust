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
    }
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
