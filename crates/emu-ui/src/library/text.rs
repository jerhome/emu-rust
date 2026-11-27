//! User-facing strings of the library screens, in one place for a future translation.

use std::path::Path;

use emu_library::ScanReport;

/// Untrusted text (game titles and names from DAT files, `.info` contents): bounded, no control characters.
fn clean(s: &str) -> String {
    crate::ui::text::sanitize_core_text(s)
}

fn clean_list(items: &[String]) -> String {
    items
        .iter()
        .map(|s| clean(s))
        .collect::<Vec<_>>()
        .join(", ")
}

pub const SEARCH_HINT: &str = "Rechercher (/ ou Ctrl+F)";
pub const ALL_SYSTEMS: &str = "Tous";
pub const VIEW_ALL: &str = "Tous les jeux";
pub const VIEW_FAVORITES: &str = "Favoris";
pub const VIEW_RECENT: &str = "Récemment joués";
pub const SORT_TITLE: &str = "Tri : titre";
pub const SORT_LAST_PLAYED: &str = "Tri : dernière partie";

pub const FIRMWARE_IMPORT: &str = "Importer…";
pub const FIRMWARE_STATE_MISSING: &str = "absent";
pub const FIRMWARE_STATE_WRONG: &str = "présent, md5 inattendu (autre version)";
pub const FIRMWARE_STATE_UNVERIFIED: &str = "présent, non vérifié (md5 inconnu)";
pub const FIRMWARE_STATE_UNREADABLE: &str = "illisible";
pub const FIRMWARE_STATE_OK: &str = "présent";

/// One file of the banner: « gba_bios.bin : absent. Écran de démarrage… ». `benefit` comes from `cores.toml`.
pub fn firmware_advice_line(file: &str, state: &str, benefit: &str) -> String {
    format!("{} : {state}. {}", clean(file), clean(benefit))
}

/// Banner of the selected game (design v7.1 point 5): `names` are the missing files (alternatives when several),
/// `required`: the core cannot run the game without one of them.
pub fn firmware_advice(names: &[String], required: bool) -> String {
    let verb = if required {
        "exige"
    } else {
        "fonctionne mieux avec"
    };
    let files = if names.len() > 1 {
        format!("l'un de ces fichiers : {}", clean_list(names))
    } else {
        clean_list(names)
    };
    format!(
        "Ce core {verb} un fichier d'origine que vous devez fournir vous-même ({files}). \
         emu-rust ne le télécharge pas : copiez-le depuis votre propre matériel."
    )
}

pub const SCAN: &str = "Scanner";
pub const SCAN_CANCEL: &str = "Arrêter le scan";
pub const CORES: &str = "Cores";
pub const SETTINGS: &str = "Réglages";
pub const QUIT: &str = "Quitter";
pub const FAVORITE_MARK: &str = "★";
pub const BADGE_32X: &str = "32X";
pub const NOTE_32X: &str = "CD Mega CD 32X : lancé avec le core picodrive (BIOS Mega CD requis, BIOS 32X intégrés au core).";
pub const HOME_HINT: &str = "Flèches : choisir un jeu · Entrée : jouer · F : favori · / : rechercher · \
                             Tab : barre du haut · Échap : effacer la recherche";

pub const EMPTY_LIBRARY: &str = "Aucun jeu dans la bibliothèque.";
pub const EMPTY_ADD_FOLDER: &str = "Ouvrir les réglages des dossiers";
pub const EMPTY_SCAN: &str = "Scanner maintenant";
pub const EMPTY_NO_FILES: &str = "Aucun jeu trouvé dans les dossiers scannés.";
pub const EMPTY_NO_FILES_HINT: &str = "Formats reconnus : .zip, .7z, .sfc, .smc, .nes, .gb, .gbc, .gba, .md, \
                                       .sms, .gg, .n64, .iso, .cue, .chd, .adf, .d64… Vérifiez le dossier, \
                                       puis rescannez.";
pub const EMPTY_RESCAN: &str = "Rescanner";
pub const NO_MATCH: &str = "Aucun jeu ne correspond aux filtres.";
pub const NO_MATCH_HINT: &str = "Essayez d'autres mots ou retirez le filtre.";
pub const NO_MATCH_CLEAR: &str = "Effacer le filtre";
pub const EMPTY_FAVORITES: &str = "Aucun favori pour l'instant.";
pub const EMPTY_FAVORITES_HINT: &str =
    "Sélectionnez un jeu dans « Tous » et appuyez sur F pour l'ajouter.";
pub const EMPTY_RECENT: &str = "Aucun jeu lancé récemment.";
pub const EMPTY_RECENT_HINT: &str = "Lancez un jeu : il apparaîtra ici.";
pub const EMPTY_CORES_HINT: &str =
    "Il faut aussi un core par système : installez-les depuis l'écran Cores.";
pub const EMPTY_CORES_BUTTON: &str = "Ouvrir l'écran Cores";
pub const LAUNCH_FIX: &str =
    "Vérifiez que le fichier existe et que le core est installé (écran Cores), puis réessayez.";
pub const NO_DAT: &str = "Aucune base de jeux (DAT) importée : les jeux portent le nom de leur fichier. \
                          Réglages › Mettre à jour la base (DAT).";
pub const DB_UNAVAILABLE: &str = "Bibliothèque indisponible : les jeux ne peuvent pas être listés. \
                                  Relancez l'application ; si cela persiste, consultez le Journal (Réglages).";
pub const COVERS_DISABLED: &str =
    "Jaquettes : téléchargement désactivé (seules les jaquettes déjà en cache s'affichent).";
pub const COVERS_ENABLE: &str = "Activer le téléchargement des jaquettes";
pub const COVERS_OFFLINE: &str =
    "Jaquettes : hors ligne (cache seulement). Réglages › Rechercher à nouveau pour réessayer.";
pub const COVERS_NO_SOURCE: &str =
    "Jaquettes : aucune source utilisable (sources désactivées ou identifiants refusés).";
pub const COVERS_UNAVAILABLE: &str = "Jaquettes : indisponibles (file de téléchargement arrêtée).";
pub const COVERS_RETRY: &str = "Rechercher à nouveau les jaquettes absentes";
pub const COVERS_RETRY_DONE: &str = "Les jaquettes absentes vont être recherchées à nouveau.";
pub const COVER_RESET: &str = "Réinitialiser la jaquette";
pub const COVER_RESET_DONE: &str = "Jaquette réinitialisée : nouvelle recherche en cours.";
pub const COVERS_RESET_ALL: &str = "Réinitialiser toutes les jaquettes";
pub const COVERS_RESET_ALL_ASK: &str =
    "Oublier toutes les jaquettes (fichiers en cache compris) et les rechercher à nouveau ?";
pub const COVERS_RESET_ALL_YES: &str = "Oui, tout réinitialiser";
pub const COVERS_RESET_ALL_NO: &str = "Annuler";
pub const COVERS_RESET_ALL_DONE: &str =
    "Toutes les jaquettes ont été réinitialisées : nouvelle recherche en cours.";
pub const DAT_OFFER_TITLE: &str = "Base de jeux";
pub const DAT_OFFER: &str = "Aucune base d'identification (DAT No-Intro) n'est installée : les jeux portent \
                             le nom de leur fichier et les jaquettes sont plus rarement trouvées. Télécharger les \
                             DAT depuis libretro-database (quelques Mio, une fois) ?";
pub const DAT_OFFER_YES: &str = "Télécharger";
pub const DAT_OFFER_LATER: &str = "Plus tard";

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
pub const DAT_DOWNLOAD: &str = "Mettre à jour la base (DAT)";
pub const DAT_DOWNLOAD_NOTE: &str =
    "Télécharge les DAT No-Intro de libretro-database (seulement s'ils ont changé).";
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
    if r.unclassified > 0 {
        s.push_str(&format!(", {} fichiers non classés", r.unclassified));
    }
    if r.ignored_mixed_archives > 0 {
        s.push_str(&format!(
            ", {} archives ignorées (images de deux ordinateurs différents)",
            r.ignored_mixed_archives
        ));
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
    if r.cautious {
        s.push_str(" · ");
        s.push_str(SCAN_CAUTIOUS);
    }
    if r.marker_failed {
        s.push_str(" · ");
        s.push_str(SCAN_MARKER_FAILED);
    }
    s
}

/// Shown when the "scan in progress" marker could not be written or removed.
pub const SCAN_MARKER_FAILED: &str = "Le fichier de suivi du scan n'a pas pu être écrit ou retiré (<base>.scanning, à côté de la bibliothèque) : vérifiez les droits du dossier, sinon les scans suivants resteront prudents.";

/// Shown after a scan that followed an abrupt stop: nothing was purged.
pub const SCAN_CAUTIOUS: &str = "Le scan précédent avait été interrompu : les jeux dont le fichier est absent sont conservés jusqu'au prochain scan.";

pub fn scan_failed(e: &str) -> String {
    format!(
        "Scan impossible : {e}. Vérifiez que le dossier existe et est lisible (Réglages › Dossiers de ROMs), puis rescannez."
    )
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
    let system = clean(system);
    format!("DAT importé pour {system} : {n} entrées.")
}

pub fn dat_failed(e: &str) -> String {
    let e = clean(e);
    format!(
        "Import du DAT impossible : {e}. Vérifiez le fichier ou votre connexion, puis réessayez (Réglages › Bases d'identification)."
    )
}

pub fn dat_unknown_system(name: &str) -> String {
    let name = clean(name);
    format!("Système du DAT « {name} » non reconnu : choisissez-le avant d'importer.")
}

pub fn dat_info(entries: u64, date: &str) -> String {
    format!("{entries} entrées, importé le {date}")
}

pub fn install_question(core: &str, game: &str) -> String {
    let (core, game) = (clean(core), clean(game));
    format!(
        "Aucun core installé ne lance « {game} ». Installer le core recommandé « {core} » ? \
         L'écran Cores affiche la progression ; relancez le jeu ensuite."
    )
}

pub fn install_question_32x(core: &str, game: &str) -> String {
    let (core, game) = (clean(core), clean(game));
    format!(
        "« {game} » est un CD Mega CD 32X : seul le core « {core} » émule la 32X \
         (genesis_plus_gx ne lance que le Mega CD). Installer « {core} » ? \
         L'écran Cores affiche la progression ; relancez le jeu ensuite."
    )
}

pub fn firmware_question(game: &str, files: &[String]) -> String {
    format!(
        "« {} » demande des BIOS absents ou différents : {}. Le jeu peut ne pas démarrer.",
        clean(game),
        clean_list(files)
    )
}

pub fn firmware_warning(files: &[String]) -> String {
    format!("BIOS facultatifs absents : {}", clean_list(files))
}

pub fn tampered(core: &str) -> String {
    let core = clean(core);
    format!(
        "Le core « {core} » a été modifié depuis son installation : il n'est pas chargé. \
         Le réinstaller ?"
    )
}

pub fn game_failed(game: &str, e: &str) -> String {
    format!("« {} » : {}", clean(game), clean(e))
}

/// A launch that failed for a reason the user can act on: what happened + what to do.
pub fn launch_failed(game: &str, e: &str) -> String {
    format!("{} {LAUNCH_FIX}", game_failed(game, e))
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

pub fn covers_queued(n: usize) -> String {
    format!("Jaquettes : {n} à télécharger…")
}

pub fn covers_summary(found: u32, missing: u32, source_broken: bool) -> String {
    let broken = if source_broken {
        ", source injoignable"
    } else {
        ""
    };
    format!("Jaquettes : {found} trouvées, {missing} introuvables{broken}")
}

pub fn dat_downloading(system: &str, done: usize, total: usize) -> String {
    format!("Base de jeux : {} ({}/{total})…", clean(system), done + 1)
}

pub fn dat_download_done(
    updated: usize,
    unchanged: usize,
    failed: &[(String, String)],
    offline: bool,
) -> String {
    let mut s = format!("Base de jeux : {updated} DAT mis à jour, {unchanged} inchangés");
    if !failed.is_empty() {
        let names: Vec<String> = failed
            .iter()
            .map(|(n, e)| format!("{} ({})", clean(n), clean(e)))
            .collect();
        s.push_str(&format!(" · échecs : {}", names.join(", ")));
    }
    if offline {
        s.push_str(" · réseau indisponible, téléchargement interrompu");
    }
    s
}

/// Card and list title of a game (DAT names are untrusted).
pub fn game_title(title: &str) -> String {
    clean(title)
}

pub const BIOS_INSTALL_AND_LAUNCH: &str = "Installer et lancer";
pub const DESKTOP_BUTTON: &str = "Bureau Atari ST";
pub const AMIGA_DESKTOP_BUTTON: &str = "Workbench / AROS";
pub const COMPUTERS_TITLE: &str = "Ordinateurs";
pub const DEFAULT_INPUT_MODE: &str = "Mode par défaut :";
pub const INPUT_MODE_AUTO: &str = "Automatique (selon le système)";
pub const INPUT_MODE_KEYBOARD: &str = "Clavier";
pub const INPUT_MODE_JOYSTICK: &str = "Joystick";
pub const DEFAULT_INPUT_MODE_NOTE: &str = "Automatique : joystick pour l'Amstrad CPC et le Commodore 64, clavier \
                                           pour les autres. Le mode choisi en jeu (touche hôte + J) est retenu \
                                           pour ce jeu.";
pub const AUTO_FAST_LOAD: &str = "Chargement accéléré automatique";
pub const AUTO_FAST_LOAD_NOTE: &str = "Le core accélère l'émulation pendant les chargements de cassette ou de \
                                       disquette (son coupé pendant ce temps).";
pub const CPC_BASIC_BUTTON: &str = "Amstrad CPC (BASIC)";
pub const C64_BASIC_BUTTON: &str = "Commodore 64 (BASIC)";
pub const OPEN_ROM_FOLDER: &str = "Ouvrir le dossier des ROMs";

// ---- 3.0 : BIOS avant lancement, dossier des ROMs, fichiers non classés (design v3.0 §2) -------------------

pub const FALLBACK_CONTINUE: &str = "Continuer avec la ROM intégrée";
pub const USE_FALLBACK: &str = "Utiliser la ROM intégrée";
pub const FALLBACK_NOTE: &str = "Cet avertissement n'est affiché qu'une fois par jeu. Importer le fichier \
                                 d'origine le copie dans le dossier ci-dessus (vérifié par son empreinte).";

pub fn fallback_title(wanted: &str) -> String {
    if wanted.to_ascii_lowercase().starts_with("kick") {
        format!("Pas de Kickstart ({wanted})")
    } else {
        format!("BIOS absent ({wanted})")
    }
}

/// `others`: files of the same table present (a BIOS of another region, design v7.0 §2.1).
pub fn fallback_body(game: &str, label: &str, wanted: &str, others: &[String]) -> String {
    if !crate::ui::text::is_kickstart(wanted) {
        let others: Vec<String> = others.iter().map(|o| clean(o)).collect();
        return format!(
            "« {} » : {}",
            clean(game),
            crate::ui::text::core_fallback_warning(&clean(label), &clean(wanted), &others)
        );
    }
    format!(
        "« {} » attend {wanted}, absent du dossier des BIOS. Le jeu démarrera avec {} : la compatibilité est \
         limitée (beaucoup de jeux demandent le Kickstart d'origine).",
        clean(game),
        clean(label)
    )
}

pub fn wrong_firmware_body(game: &str, wanted: &str) -> String {
    format!(
        "« {} » attend {wanted} : le fichier présent ne correspond à aucune version connue. Le jeu peut ne pas \
         démarrer.",
        clean(game)
    )
}

pub fn unreadable_firmware_body(wanted: &str, error: &str) -> String {
    format!("{wanted} est illisible : {}", clean(error))
}

pub fn game_options_failed(e: &str) -> String {
    format!("Options du jeu non enregistrées : {}", clean(e))
}

pub const FOLDER_OPENED: &str = "Dossier des ROMs ouvert dans le gestionnaire de fichiers.";
pub const FOLDER_CHOOSE: &str = "Quel dossier ouvrir ?";
pub const FOLDER_TITLE: &str = "Dossier des ROMs";
pub const FOLDER_CONTAINER: &str = "emu-rust tourne dans un conteneur Docker : le dossier ne peut pas s'ouvrir \
                                    ici. Déposez vos jeux dans ce dossier du conteneur (par exemple avec \
                                    « docker cp »), ou montez un dossier de votre ordinateur sur /data/roms \
                                    (variable EMU_HOST_ROMS_DIR de compose.yaml), puis lancez « Scanner ».";
pub const FOLDER_NO_MANAGER: &str =
    "Aucun gestionnaire de fichiers n'a pu ouvrir le dossier. Le voici :";
pub const FOLDER_HOST_UNKNOWN: &str =
    "Aucun dossier de l'hôte n'est monté (EMU_HOST_ROMS_DIR absent).";

pub fn folder_host(hint: &str) -> String {
    format!("Sur votre ordinateur, ce dossier est : {}", clean(hint))
}

pub fn unclassified_title(n: usize) -> String {
    format!("{n} fichier(s) non classé(s)")
}

pub const UNCLASSIFIED_HELP: &str = "Ces fichiers ont une extension commune à plusieurs systèmes et ne figurent \
                                     dans aucune base : rangez chacun dans le dossier de son système, puis \
                                     relancez « Scanner ».";
pub const UNCLASSIFIED_SHOW: &str = "Voir les fichiers non classés";

/// One unclassified file and the folders it may go to (`roms/<id>/` of every system accepting its extension).
pub fn unclassified_line(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let ids: Vec<String> = emu_library::systems()
        .iter()
        .filter(|s| s.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)))
        .map(|s| format!("roms/{}/", s.id))
        .collect();
    let advice = if ids.is_empty() {
        "roms/<système>/".to_owned()
    } else {
        ids.join(" ou ")
    };
    format!("{} : rangez ce fichier dans {advice}", path.display())
}

/// Title of a launch without content: « Bureau Atari ST », « Amstrad CPC (BASIC) » for the 8-bit computers.
pub fn desktop_title(system_id: &str, system: &str) -> String {
    match system_id {
        "cpc" | "c64" => format!("{system} (BASIC)"),
        _ => format!("Bureau {system}"),
    }
}

pub fn desktop_needs_core(core: &str) -> String {
    format!("Le core {core} n'est pas installé : l'installer depuis l'écran « Cores ».")
}

// ---- BIOS manquant, générique (design v6.0 §2.1 ; repris du dialogue Mega CD de la 2.1) ----------------

pub const FIRMWARE_RETRY: &str = "Réessayer";
pub const COPY_PATH: &str = "Copier le chemin";
pub const PATH_COPIED: &str = "Chemin copié dans le presse-papiers.";
pub const FIRMWARE_NO_DOWNLOAD: &str = "emu-rust ne télécharge jamais de BIOS commercial.";
pub const FIRMWARE_STILL_MISSING: &str = "Rien de nouveau : vérifiez le nom exact et le dossier.";

/// Region of a game, as in « un jeu Europe ».
pub fn region_name(region: emu_library::DiscRegion) -> &'static str {
    match region {
        emu_library::DiscRegion::Europe => "Europe",
        emu_library::DiscRegion::Usa => "USA",
        emu_library::DiscRegion::Japan => "Japon",
    }
}

/// Order of the region keys of a table in the messages (Europe, USA, Japon, then the rest).
pub fn region_order(key: &str) -> usize {
    ["europe", "usa", "japan"]
        .iter()
        .position(|k| *k == key)
        .unwrap_or(usize::MAX)
}

/// « bios_CD_E.bin (Europe) ».
pub fn file_with_region(file: &str, region: Option<&str>) -> String {
    match region {
        Some(r) => format!("{} ({r})", clean(file)),
        None => clean(file),
    }
}

/// `BIOS manquant : bios_CD_E.bin (Mega CD / Sega CD, Europe)`; `any_of`: the region is unknown, any file of the
/// table will do (no single name in the title).
pub fn firmware_missing_title(
    wanted: &str,
    system: Option<&str>,
    region: Option<&str>,
    any_of: bool,
) -> String {
    let wanted = clean(wanted);
    match (any_of, system, region) {
        (true, Some(system), _) => format!("BIOS manquant ({})", clean(system)),
        (true, None, _) => "BIOS manquant".to_owned(),
        (false, Some(system), Some(region)) => {
            format!("BIOS manquant : {wanted} ({}, {region})", clean(system))
        }
        (false, Some(system), None) => format!("BIOS manquant : {wanted} ({})", clean(system)),
        (false, None, Some(region)) => format!("BIOS manquant : {wanted} ({region})"),
        (false, None, None) => format!("BIOS manquant : {wanted}"),
    }
}

/// Dialog body: the exact name (or every name of the table when the region is unknown), the other files present.
pub fn firmware_missing_body(
    game: &str,
    region: Option<&str>,
    wanted: &str,
    alternatives: &[String],
    others_present: &[String],
) -> String {
    let game = clean(game);
    let wanted = clean(wanted);
    let then =
        "puis choisissez « Réessayer » (ou « Importer un BIOS… » pour le choisir et le vérifier).";
    let mut s = if !alternatives.is_empty() {
        format!(
            "La région de « {game} » n'a pas pu être lue. Copiez votre BIOS dans le dossier ci-dessous, sous \
             l'un des noms exacts {} (majuscules et minuscules comprises), {then}",
            or_list(alternatives)
        )
    } else if let Some(region) = region {
        format!(
            "« {game} » est un jeu {region}. Copiez votre BIOS dans le dossier ci-dessous, sous le nom exact \
             {wanted} (majuscules et minuscules comprises), {then}"
        )
    } else {
        format!(
            "« {game} » a besoin du fichier {wanted}. Copiez-le dans le dossier ci-dessous, sous ce nom exact \
             (majuscules et minuscules comprises), {then}"
        )
    };
    if let (Some(region), false) = (region, others_present.is_empty()) {
        let verb = if others_present.len() == 1 {
            "est présent"
        } else {
            "sont présents"
        };
        let others: Vec<String> = others_present.iter().map(|o| clean(o)).collect();
        s.push_str(&format!(
            "\n{} {verb}, mais ce jeu est {region}.",
            others.join(", ")
        ));
    }
    s
}

/// « a, b ou c ».
fn or_list(items: &[String]) -> String {
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} ou {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

/// The system folder, shown selectable (absolute path, local only).
pub fn system_dir_text(dir: &Path) -> String {
    clean(&dir.display().to_string())
}

pub const PAD_TITLE: &str = "Manette";
pub const PAD_ENABLED: &str = "Activer les manettes";
pub const PAD_DISABLED_NOTE: &str = "Manettes désactivées.";
pub const PAD_UNAVAILABLE: &str = "Manettes indisponibles (option --no-gamepad, compilation sans manette ou service système absent).";
pub const PAD_NONE: &str = "Aucune manette branchée.";
pub const PAD_DEADZONE: &str = "Zone morte du stick";
pub const PAD_TRIGGER: &str = "Seuil des gâchettes (L2 / R2)";
pub const PAD_HOTKEY: &str = "Bouton des raccourcis :";
pub const PAD_HOTKEY_MODE: &str = "PS";
pub const PAD_HOTKEY_SELECT: &str = "Share / Select";
pub const PAD_PAUSE_ON_DISCONNECT: &str = "Mettre le jeu en pause si la manette est débranchée";
pub const PAD_REMAP_TITLE: &str = "Remappage des boutons";
pub const PAD_PS_MODE: &str = "Manette PlayStation par défaut :";
pub const PAD_PS_DIGITAL: &str = "Numérique";
pub const PAD_PS_DUALSHOCK: &str = "DualShock (analogique)";
pub const PAD_PS_MODE_NOTE: &str = "Numérique : le stick gauche fait la croix (jeux d'origine). DualShock : sticks \
                                    analogiques, L3 / R3. Chaque jeu peut changer depuis son menu (« Manette »).";
pub const PAD_REMAP_NOTE: &str = "Choisir un bouton du jeu, puis appuyer sur le bouton de la manette à lui associer \
                                  (Échap : annuler). Pris en compte au prochain lancement d'un jeu.";
pub const PAD_REMAP_SYSTEM: &str = "Système :";
pub const PAD_REMAP_CHANGE: &str = "Changer";
pub const PAD_REMAP_CANCEL: &str = "Annuler";
pub const PAD_REMAP_RESET: &str = "Rétablir les boutons par défaut de ce système";
pub const PAD_REMAP_WAIT: &str = "Appuyez sur un bouton de la manette…";
pub const PAD_REMAP_NONE: &str = "—";
pub const PAD_REMAP_NO_PAD: &str = "Branchez une manette pour changer un bouton.";
pub const PAD_REMAP_MODE: &str = "Le bouton PS sert aux raccourcis : choisissez-en un autre.";

pub const LEARN_TITLE: &str = "Configurer une manette";
pub const LEARN_NOTE: &str =
    "Pour une manette non reconnue ou mal configurée : on vous demande un bouton à la fois.";
pub const LEARN_CONFIGURE: &str = "Configurer";
pub const LEARN_FORGET: &str = "Oublier";
pub const LEARN_LEARNED: &str = "apprise";
pub const LEARN_SHAPE: &str = "Forme de la manette :";
pub const LEARN_START: &str = "Commencer";
pub const LEARN_SKIP: &str = "Passer";
pub const LEARN_BACK: &str = "Revenir";
pub const LEARN_CANCEL: &str = "Annuler";
pub const LEARN_SAVE: &str = "Enregistrer";
pub const LEARN_DONE: &str = "Terminé. Vérifiez la liste, puis enregistrez.";
pub const LEARN_NOT_SET: &str = "non défini";
pub const LEARN_EMPTY: &str = "Aucun bouton appris : rien à enregistrer.";
pub const LEARN_DISCONNECTED: &str = "Manette débranchée : configuration annulée.";
pub const LEARN_SAVED: &str = "Manette enregistrée.";
pub const LEARN_FORGOTTEN: &str = "Configuration de la manette oubliée.";

pub fn learn_layout_label(layout: crate::gamepad::PadLayout) -> &'static str {
    use crate::gamepad::PadLayout as L;
    match layout {
        L::Snes => "Super Nintendo",
        L::Nes => "NES",
        L::MegaDrive3 => "Mega Drive (3 boutons)",
        L::MegaDrive6 => "Mega Drive (6 boutons)",
    }
}

pub fn learn_prompt(label: &str) -> String {
    format!("Appuyez sur : {label}")
}

pub fn learn_progress(done: usize, total: usize) -> String {
    format!("Étape {} sur {total}", (done + 1).min(total))
}

pub fn learn_used(label: &str) -> String {
    format!("Cette entrée est déjà utilisée pour « {label} ». Choisissez-en une autre.")
}

pub const PAD_NOTICE_DISMISS: &str = "Ignorer";

pub fn pad_unrecognized(name: &str) -> String {
    format!("Manette non reconnue : {}", clean(name))
}

pub fn learn_title_for(name: &str) -> String {
    format!("Manette : {}", clean(name))
}

pub fn pad_name(name: &str) -> String {
    clean(name)
}

pub fn pad_line(port: usize, name: &str) -> String {
    format!("Port {} : {}", port + 1, clean(name))
}

pub fn pad_button_label(button: crate::gamepad::PadButton) -> &'static str {
    use crate::gamepad::PadButton as P;
    match button {
        P::South => "Croix",
        P::East => "Rond",
        P::West => "Carré",
        P::North => "Triangle",
        P::L1 => "L1",
        P::R1 => "R1",
        P::L2 => "L2",
        P::R2 => "R2",
        P::Select => "Share",
        P::Start => "Options",
        P::Mode => "PS",
        P::L3 => "L3",
        P::R3 => "R3",
        P::DPadUp => "Croix directionnelle haut",
        P::DPadDown => "Croix directionnelle bas",
        P::DPadLeft => "Croix directionnelle gauche",
        P::DPadRight => "Croix directionnelle droite",
    }
}

// ---- PC-DOS (design v4.0) ---------------------------------------------------------------------------------

pub const DOS_FOLDER_HINT: &str = "P : lancer un autre programme de ce jeu";
pub const DOS_SNAPSHOT_TITLE: &str = "Préparation du jeu…";
pub const DOS_SNAPSHOT_NOTE: &str = "Une copie du dossier est rangée dans le cache (vos fichiers ne sont jamais \
                                     modifiés) ; elle est refaite seulement si le dossier change.";
pub const DOS_SNAPSHOT_CANCELLED: &str = "Préparation du jeu annulée.";
pub const DOS_BUSY: &str = "Un jeu DOS est déjà en préparation.";
pub const DOS_NO_PROGRAM: &str = "Aucun programme (.exe, .com, .bat) à la racine de ce dossier.";
pub const DOS_PROGRAMS_TITLE: &str = "Lancer un autre programme…";
pub const DOS_PROGRAMS_HINT: &str = "Flèches / Tab : choisir · Entrée : valider · Échap : retour. Les programmes \
                                     des sous-dossiers et des images se lancent depuis le menu DOS.";
pub const DOS_ONCE: &str = "Pour cette fois";
pub const DOS_BY_DEFAULT: &str = "Par défaut (à chaque lancement)";
pub const DOS_AUTOBOOT_WINS: &str = "Un programme a été choisi dans le menu DOS (touche Droite) : il reste lancé par défaut. \
                                     Utilisez « Changer de programme de démarrage » dans le menu du jeu.";
pub const DOS_CLEAR_DEFAULT: &str = "Revenir au menu DOS";
pub const DOS_DEFAULT_CLEARED: &str = "Programme par défaut retiré : le menu DOS choisira.";
pub const DOS_CACHE_SECTION: &str = "Jeux DOS";
pub const DOS_CACHE_NOTE: &str = "Copies des jeux en dossier (refaites au besoin). Vos sauvegardes ne sont pas \
                                  touchées.";
pub const DOS_CACHE_CLEARED: &str = "Cache DOS vidé.";

pub fn dos_snapshot_body(game: &str) -> String {
    format!("Copie de « {game} » dans le cache avant le premier lancement.")
}

pub fn dos_default_program(program: &str) -> String {
    format!("Programme par défaut : {program}")
}

pub fn dos_cache_button(size: Option<u64>) -> String {
    match size {
        Some(s) => format!("Vider le cache DOS ({})", crate::ui::text::format_size(s)),
        None => "Vider le cache DOS (calcul…)".to_owned(),
    }
}

pub fn dos_cache_failed(e: &str) -> String {
    format!("Cache DOS non vidé : {e}")
}

pub const APP_TITLE: &str = "Application";
pub const SHOW_TERMINAL: &str = "Afficher le terminal (après redémarrage)";
pub const JOURNAL_LINK: &str = "Ouvrir le journal";

pub const HELP_MARK: &str = "?";
pub const HELP_HINT: &str = "Ouvrir l'aide de cette section";

// ---- Sega CD 32X manual mark (ADR 0024 §2, §4, design v7.5 §3) ----------------------------------------------------

pub const CD32X_TITLE: &str = "CD 32X";
pub const CD32X_MENU_ON: &str = "Ce jeu est un CD 32X";
pub const CD32X_MENU_OFF: &str = "Ce n'est pas un CD 32X";
/// Help line when the selected game may be a CD 32X (unknown content, e.g. every `.chd`).
pub const CD32X_INVITE: &str = "Jeu Sega CD 32X ? Clic droit > Ce jeu est un CD 32X (ou touche X)";
/// Help line of a game that can be marked by hand.
pub const CD32X_KEY_ON: &str = "X : ce jeu est un CD 32X";
/// Help line of a game marked by hand.
pub const CD32X_KEY_OFF: &str = "X : ce n'est pas un CD 32X";

pub fn cd32x_save_failed(game: &str, e: &str) -> String {
    format!(
        "Marque CD 32X de « {} » annulée, réglages non enregistrés : {}",
        clean(game),
        clean(e)
    )
}

/// After marking: the core change, the saves not carried over, same-name games, other discs (ADR 0024 §2, §4).
pub fn cd32x_marked(game: &str, same_name: usize) -> String {
    let mut msg = format!(
        "« {} » sera lancé avec le core picodrive, et non plus avec celui du Mega CD. Ses sauvegardes faites avec \
         genesis_plus_gx (.brm) ne sont pas reprises.",
        clean(game)
    );
    if same_name > 1 {
        msg.push_str(&format!(
            " {same_name} jeux Mega CD portent ce nom de fichier : tous seront lancés avec picodrive."
        ));
    }
    msg.push_str(" Si le jeu a d'autres disques listés à part, marquez-les aussi.");
    msg
}

pub fn cd32x_unmarked(game: &str, same_name: usize) -> String {
    let mut msg = format!(
        "« {} » n'est plus marqué CD 32X : il sera lancé avec le core habituel du Mega CD.",
        clean(game)
    );
    if same_name > 1 {
        msg.push_str(&format!(
            " {same_name} jeux Mega CD portent ce nom de fichier : tous perdent la marque."
        ));
    }
    msg
}

// ---- Systèmes non testés : lancement (ADR 0025 §5, §6, §7) ------------------------------------------------

pub const CUSTOM_ACCEPT_TITLE: &str = "Contenu non vérifié";
pub const CUSTOM_ACCEPT_YES: &str = "Accepter et continuer";
pub const CUSTOM_UNTESTED_TITLE: &str = "Système non testé";
pub const CUSTOM_TRY_AND_LAUNCH: &str = "Essayer puis lancer";
pub const CUSTOM_LAUNCH: &str = "Lancer";
pub const CUSTOM_WORKS_FOR_ME: &str = "Ça marche chez moi";
pub const CUSTOM_TRY_TITLE: &str = "Essai du jeu";
pub const CUSTOM_TRY_NOTE: &str =
    "L'essai vérifie le démarrage seulement. Vos fichiers ne sont pas touchés.";
pub const CUSTOM_TRY_CANCELLED: &str = "Essai annulé : le jeu n'est pas lancé.";
pub const CUSTOM_FAILED_TITLE: &str = "L'essai du jeu a échoué";
pub const CUSTOM_CHANGE_CORE: &str = "Changer de core";
pub const CUSTOM_CHANGE_CORE_HINT: &str =
    "Choisissez un autre core pour ce système dans « Core utilisé », puis relancez le jeu.";
pub const CRASH_NOTICE_TITLE: &str = "Arrêt inattendu";
pub const CUSTOM_BUSY: &str = "Un essai de jeu est déjà en cours.";

pub fn custom_accept_body(core: &str) -> String {
    format!(
        "Le core « {} » n'a pas été examiné : il peut contenir des fichiers dont emu-rust ne connaît pas l'origine. \
         L'utiliser quand même ?",
        clean(core)
    )
}

pub fn custom_untested_body(game: &str, system: &str, core: &str) -> String {
    format!(
        "« {} » est un jeu d'un système non testé ({}, core {}). Rien ne garantit qu'il démarre ni que ses \
         sauvegardes fonctionnent.",
        clean(game),
        clean(system),
        clean(core)
    )
}

pub fn custom_try_body(game: &str) -> String {
    format!(
        "Essai de « {} » en cours (30 secondes au plus)…",
        clean(game)
    )
}

/// Why no dry run was made: the launch goes on after the warning only.
pub fn custom_not_tried(why: &str) -> String {
    format!("Pas d'essai : {why}.")
}

pub const NOT_TRIED_ARCHIVE: &str = "le jeu est dans une archive";
pub const NOT_TRIED_SEVERAL_FILES: &str = "le jeu tient en plusieurs fichiers";
pub const NOT_TRIED_TOO_BIG: &str = "le jeu est trop gros pour être copié";
pub const NOT_TRIED_FAILED: &str = "l'essai n'a pas pu être préparé";

pub fn custom_failed_crashed(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("Le core s'est arrêté pendant l'essai (code {code})."),
        None => "Le core s'est arrêté brutalement pendant l'essai.".to_owned(),
    }
}

pub const CUSTOM_FAILED_TIMEOUT: &str = "Le core n'a pas fini l'essai en 30 secondes.";

pub fn custom_failed_load(message: &str) -> String {
    format!("Le core n'a pas pu charger le jeu : {}", clean(message))
}

pub fn crash_notice(game: &str, core: &str, untested: bool) -> String {
    let base = format!(
        "emu-rust s'est arrêté pendant « {} » avec {} : plantage, arrêt forcé ou coupure.",
        clean(game),
        clean(core)
    );
    if untested {
        format!("{base} Ce core n'est pas testé pour ce système : essayez-en un autre.")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_done_mentions_cautious_scan_only_when_set() {
        let mut r = ScanReport::default();
        assert!(!scan_done(&r).contains(SCAN_CAUTIOUS));
        r.cautious = true;
        assert!(scan_done(&r).contains(SCAN_CAUTIOUS));
    }

    #[test]
    fn scan_done_mentions_marker_failure_only_when_set() {
        let mut r = ScanReport::default();
        assert!(!scan_done(&r).contains(SCAN_MARKER_FAILED));
        r.marker_failed = true;
        assert!(scan_done(&r).contains(SCAN_MARKER_FAILED));
    }

    #[test]
    fn empty_states_say_what_to_do() {
        for hint in [
            EMPTY_NO_FILES_HINT,
            NO_MATCH_HINT,
            EMPTY_FAVORITES_HINT,
            EMPTY_RECENT_HINT,
            EMPTY_CORES_HINT,
        ] {
            assert!(!hint.is_empty());
        }
        assert!(empty_hint(Path::new("/x/roms")).contains("scan"));
        assert!(EMPTY_FAVORITES_HINT.contains('F'));
        assert!(EMPTY_NO_FILES_HINT.contains("rescann"));
        assert!(NO_MATCH_CLEAR.contains("filtre"));
    }

    #[test]
    fn error_messages_carry_an_action() {
        let acts = ["Vérifiez", "Relancez", "réessayez", "rescannez"];
        for msg in [
            scan_failed("x"),
            dat_failed("x"),
            launch_failed("Jeu", "x"),
            DB_UNAVAILABLE.to_owned(),
        ] {
            assert!(acts.iter().any(|a| msg.contains(a)), "{msg}");
        }
    }
}
