//! User-facing strings of the library screens, in one place for a future translation.

use std::path::Path;

use emu_library::ScanReport;

pub const SEARCH_HINT: &str = "Rechercher (/ ou Ctrl+F)";
pub const ALL_SYSTEMS: &str = "Tous";
pub const VIEW_ALL: &str = "Tous les jeux";
pub const VIEW_FAVORITES: &str = "Favoris";
pub const VIEW_RECENT: &str = "Récemment joués";
pub const SORT_TITLE: &str = "Tri : titre";
pub const SORT_LAST_PLAYED: &str = "Tri : dernière partie";
pub const SCAN: &str = "Scanner";
pub const SCAN_CANCEL: &str = "Arrêter le scan";
pub const CORES: &str = "Cores";
pub const SETTINGS: &str = "Réglages";
pub const QUIT: &str = "Quitter";
pub const FAVORITE_MARK: &str = "★";
pub const HOME_HINT: &str = "Flèches : choisir un jeu · Entrée : jouer · F : favori · / : rechercher · \
                             Tab : barre du haut · Échap : effacer la recherche";

pub const EMPTY_LIBRARY: &str = "Aucun jeu dans la bibliothèque.";
pub const NO_MATCH: &str = "Aucun jeu ne correspond aux filtres.";
pub const NO_DAT: &str = "Aucune base de jeux (DAT) importée : les jeux portent le nom de leur fichier. \
                          Réglages › Importer un DAT.";
pub const DB_UNAVAILABLE: &str = "Bibliothèque indisponible : les jeux ne peuvent pas être listés.";
pub const COVERS_PENDING: &str =
    "Jaquettes : pas encore disponibles dans cette version (emplacement par défaut).";

pub const LAUNCH_INSTALL_TITLE: &str = "Core manquant";
pub const LAUNCH_INSTALL: &str = "Installer";
pub const LAUNCH_CANCEL: &str = "Annuler";
pub const LAUNCH_ANYWAY: &str = "Lancer quand même";
pub const LAUNCH_FIRMWARE_TITLE: &str = "BIOS manquant";
pub const LAUNCH_REINSTALL: &str = "Réinstaller";
pub const LAUNCH_FAILED_TITLE: &str = "Lancement impossible";
pub const OK: &str = "OK";
pub const NO_MANAGER: &str =
    "Gestionnaire de cores indisponible sur cette plate-forme : lancer avec --core et --rom.";
pub const FIRMWARE_NOTE: &str =
    "Les BIOS ne sont jamais téléchargés : copiez-les dans le dossier des BIOS.";

pub const SETTINGS_TITLE: &str = "Réglages de la bibliothèque";
pub const BACK: &str = "Retour";
pub const SAVE: &str = "Enregistrer";
pub const SAVED: &str = "Réglages enregistrés.";
pub const LOCKED: &str = "config.toml est invalide : les réglages valent pour cette session mais ne sont \
                          pas enregistrés.";
pub const ROOTS: &str = "Dossiers de ROMs";
pub const ROOT_FIXED: &str = "(toujours scanné)";
pub const ROOT_ADD: &str = "Ajouter";
pub const ROOT_REMOVE: &str = "Retirer";
pub const ROOT_HINT: &str = "Chemin d'un dossier à scanner";
pub const ROOT_MISSING: &str = "introuvable";
pub const SCAN_ON_START: &str = "Scanner à l'ouverture de la bibliothèque";
pub const SCAN_NOW: &str = "Scanner maintenant";
pub const DAT_TITLE: &str = "Bases d'identification (DAT No-Intro)";
pub const DAT_NONE: &str = "aucun DAT";
pub const DAT_PATH_HINT: &str = "Chemin d'un fichier .dat (clrmamepro)";
pub const DAT_IMPORT: &str = "Importer le DAT";
pub const DAT_SYSTEM: &str = "Système";
pub const DAT_AUTO: &str = "détection automatique";
pub const DAT_DOWNLOAD: &str = "Télécharger les DAT (bientôt)";
pub const COVERS_TITLE: &str = "Jaquettes";
pub const COVERS_DOWNLOAD: &str = "Télécharger les jaquettes";
pub const SOURCE_UP: &str = "Monter";
pub const SOURCE_DOWN: &str = "Descendre";
pub const CREDENTIALS_TITLE: &str = "Identifiants";
pub const CREDENTIALS_NOTE: &str = "Les mots de passe ne sont jamais affichés ; ils sont enregistrés dans \
                                    config.toml (lisible par vous seul).";
pub const SECRET_SET: &str = "défini";
pub const SECRET_UNSET: &str = "non défini";
pub const SECRET_EDIT: &str = "Modifier";
pub const SECRET_CLEAR: &str = "Effacer";
pub const SECRET_OK: &str = "Valider";
pub const SECRET_CANCEL: &str = "Annuler";
pub const SS_DEV_ID: &str = "ScreenScraper : identifiant développeur";
pub const SS_DEV_PASSWORD: &str = "ScreenScraper : mot de passe développeur";
pub const SS_USER: &str = "ScreenScraper : utilisateur";
pub const SS_PASSWORD: &str = "ScreenScraper : mot de passe";
pub const IGDB_CLIENT_ID: &str = "IGDB : client id";
pub const IGDB_SECRET: &str = "IGDB : client secret";
pub const CORES_LINK: &str = "Gestion des cores…";
pub const SETTINGS_HINT: &str =
    "Tab / flèches : naviguer · Entrée ou Espace : valider · Échap : retour";

pub fn system_count(name: &str, n: usize) -> String {
    format!("{name} ({n})")
}

pub fn source_name(kind: &str) -> &str {
    match kind {
        "libretro" => "libretro-thumbnails",
        "screenscraper" => "ScreenScraper",
        "igdb" => "IGDB",
        other => other,
    }
}

pub fn scan_running(files: u64, hashed: u64) -> String {
    format!("Scan en cours : {files} fichiers vus, {hashed} analysés…")
}

pub fn scan_done(r: &ScanReport) -> String {
    let mut s = format!(
        "Scan terminé : {} ajoutés, {} mis à jour, {} inchangés, {} retirés",
        r.added, r.updated, r.unchanged, r.removed
    );
    if !r.errors.is_empty() {
        s.push_str(&format!(", {} fichiers illisibles", r.errors.len()));
    }
    if !r.missing_roots.is_empty() {
        let roots: Vec<String> = r
            .missing_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        s.push_str(&format!(
            " · dossiers absents (jeux conservés) : {}",
            roots.join(", ")
        ));
    }
    s
}

pub fn scan_failed(e: &str) -> String {
    format!("Scan impossible : {e}")
}

pub fn recovered(quarantined: &Path) -> String {
    format!(
        "La base de la bibliothèque était endommagée : elle a été mise de côté ({}) et recréée. \
         Favoris et historique sont perdus ; un scan reconstruit la liste.",
        quarantined.display()
    )
}

pub fn db_error(e: &str) -> String {
    format!("{DB_UNAVAILABLE} {e}")
}

pub fn dat_imported(system: &str, n: u64) -> String {
    format!("DAT importé pour {system} : {n} entrées.")
}

pub fn dat_failed(e: &str) -> String {
    format!("Import du DAT impossible : {e}")
}

pub fn dat_unknown_system(name: &str) -> String {
    format!("Système du DAT « {name} » non reconnu : choisissez-le avant d'importer.")
}

pub fn dat_info(entries: u64, date: &str) -> String {
    format!("{entries} entrées, importé le {date}")
}

pub fn install_question(core: &str, game: &str) -> String {
    format!(
        "Aucun core installé ne lance « {game} ». Installer le core recommandé « {core} » ? \
         L'écran Cores affiche la progression ; relancez le jeu ensuite."
    )
}

pub fn firmware_question(game: &str, files: &[String]) -> String {
    format!(
        "« {game} » demande des BIOS absents ou différents : {}. Le jeu peut ne pas démarrer.",
        files.join(", ")
    )
}

pub fn firmware_warning(files: &[String]) -> String {
    format!("BIOS facultatifs absents : {}", files.join(", "))
}

pub fn tampered(core: &str) -> String {
    format!(
        "Le core « {core} » a été modifié depuis son installation : il n'est pas chargé. \
         Le réinstaller ?"
    )
}

pub fn game_failed(game: &str, e: &str) -> String {
    format!("« {game} » : {e}")
}

pub fn root_duplicate(path: &str) -> String {
    format!("{path} est déjà dans la liste.")
}

pub fn save_failed(e: &str) -> String {
    format!("Réglages non enregistrés : {e}")
}

pub fn empty_hint(roms: &Path) -> String {
    format!(
        "Copiez vos ROMs dans {} (ou ajoutez un dossier dans les Réglages), puis lancez un scan.",
        roms.display()
    )
}
