//! User-facing strings of « Ranger les ROMs… » (ADR 0023 §4), in one place for a future translation.

use std::path::Path;

use emu_library::{TidyBlock, TidyReason};

fn clean(s: &str) -> String {
    crate::ui::text::sanitize_core_text(s)
}

/// Paths of the user's files: shown with `/`, sanitized (file names are untrusted text).
pub fn path(p: &Path) -> String {
    clean(&p.to_string_lossy().replace('\\', "/"))
}

pub const CREATE_FOLDERS: &str = "Créer les dossiers des systèmes";
pub const OPEN_TIDY: &str = "Ranger les ROMs…";
pub const TIDY_SETTINGS_NOTE: &str = "Range le dossier des ROMs d'emu-rust (pas les autres dossiers) : un aperçu \
                                      est montré, rien n'est déplacé sans votre accord.";

pub fn folders_created(n: usize) -> String {
    match n {
        0 => "Tous les dossiers des systèmes existent déjà.".to_owned(),
        1 => "1 dossier de système créé.".to_owned(),
        n => format!("{n} dossiers de systèmes créés."),
    }
}

pub fn folders_failed(e: &str) -> String {
    format!("Dossiers non créés : {e}")
}

pub const TITLE: &str = "Ranger les ROMs";
pub const BACK: &str = "Retour";
pub const CANCEL: &str = "Annuler";
pub const STOP: &str = "Arrêter l'analyse";
pub const RETRY: &str = "Analyser de nouveau";
pub const APPLY: &str = "Ranger";
pub const INTRO: &str = "Aperçu : rien n'a encore été déplacé. Les fichiers sont déplacés sans jamais être \
                         renommés, écrasés ni supprimés ; un jeu déjà dans la bibliothèque garde ses favoris, \
                         sauvegardes et jaquettes.";
pub const SECOND_INSTANCE: &str = "Fermez toute autre fenêtre d'emu-rust avant de ranger.";
pub const HINT: &str =
    "Tab : élément suivant · Espace / Entrée : cocher ou choisir · Échap : retour";

pub const REFUSED_SCAN: &str = "Rangement impossible pendant un scan : attendez la fin du scan.";
pub const REFUSED_DOWNLOAD: &str =
    "Rangement impossible pendant un téléchargement : attendez qu'il se termine.";
pub const REFUSED_GAME: &str = "Rangement impossible pendant une partie.";
pub const REFUSED_NO_DB: &str = "Rangement impossible : la bibliothèque est indisponible.";

pub fn analysing(files: u64) -> String {
    format!("Analyse du dossier des ROMs… {files} fichiers examinés")
}

pub const ANALYSIS_STOPPED: &str = "Analyse arrêtée.";
pub const CHOICE_INTERRUPTED: &str =
    "Choix non appliqué : la mise à jour de l'aperçu a été interrompue.";

/// A system choice is being applied (collisions may need large files to be read).
pub fn updating(pending: usize) -> String {
    match pending {
        0 | 1 => "Mise à jour de l'aperçu…".to_owned(),
        n => format!("Mise à jour de l'aperçu… ({n} choix en cours)"),
    }
}

pub fn no_roms_folder(p: &Path) -> String {
    format!(
        "Aucun dossier de ROMs : {} n'existe pas encore. Créez-le (« Créer les dossiers des systèmes ») et \
         placez-y vos jeux.",
        path(p)
    )
}

pub fn analysis_failed(e: &str) -> String {
    format!("Analyse impossible : {e}")
}

pub const NOTHING_TO_DO: &str = "Rien à ranger : tout est déjà dans le dossier de son système.";

pub fn summary(units: usize, files: usize, bytes: u64, dirs: usize) -> String {
    format!(
        "Cochés : {units} jeu(x), {files} fichier(s), {} · {dirs} dossier(s) de destination",
        crate::ui::text::format_size(bytes)
    )
}

pub fn counts(already: u64, ignored: u64) -> String {
    format!("Déjà rangés : {already} · Ignorés (pas des jeux) : {ignored}")
}

pub const GROUP_CHOOSE: &str = "Système à choisir";
pub const GROUP_LEFT: &str = "Laissés en place";

pub fn group_system(name: &str, id: &str, n: usize) -> String {
    format!("{} (roms/{id}/) — {n}", clean(name))
}

pub fn group_count(label: &str, n: usize) -> String {
    format!("{label} — {n}")
}

pub fn system_name(id: &str) -> String {
    emu_library::system(id).map_or_else(|| clean(id), |s| clean(&s.name))
}

pub fn destination(to: &Path) -> String {
    format!("vers roms/{}", path(to))
}

pub fn reason(r: TidyReason) -> &'static str {
    match r {
        TidyReason::Library => "déjà dans la bibliothèque",
        TidyReason::Header => "reconnu par son en-tête",
        TidyReason::DatHash => "reconnu par son empreinte (DAT)",
        TidyReason::ArchiveContent => "d'après le contenu de l'archive",
        TidyReason::DatName => "d'après son nom (DAT) : à vérifier",
        TidyReason::Extension => "d'après son extension",
        TidyReason::DosFolder => "dossier avec un programme : jeu DOS probable, à vérifier",
    }
}

pub const SURE: &str = "sûr";
pub const SUGGESTED: &str = "suggéré, décoché";
pub const AMBIGUOUS: &str = "plusieurs systèmes possibles : choisissez";
pub const UNKNOWN: &str = "système inconnu : laissé en place";

pub fn resolved(system: &str) -> String {
    format!("résolu : {}", system_name(system))
}

pub const OTHER_SYSTEM: &str = "Autre système…";

pub fn system_change(from: &str, to: &str) -> String {
    format!(
        "Attention : ce jeu passe de {} à {} ; le core change, ses sauvegardes et états ne suivront pas.",
        system_name(from),
        system_name(to)
    )
}

pub fn block(b: &TidyBlock) -> String {
    match b {
        TidyBlock::Conflict { existing } => {
            format!("un autre fichier occupe déjà roms/{}", path(existing))
        }
        TidyBlock::Duplicate { existing } => {
            format!(
                "doublon : le même fichier est déjà dans roms/{}",
                path(existing)
            )
        }
        TidyBlock::MissingReference { name } => {
            format!("fichier cité introuvable : {}", clean(name))
        }
        TidyBlock::OutsideReference { name } => format!(
            "référence hors du dossier du jeu ou absolue : {}",
            clean(name)
        ),
        TidyBlock::SharedTrack { name } => {
            format!("piste utilisée par deux jeux : {}", clean(name))
        }
        TidyBlock::UngroupedTracks => "pistes sans fichier .cue impossibles à regrouper".to_owned(),
        TidyBlock::Symlink => "lien symbolique : jamais suivi".to_owned(),
        TidyBlock::DiscInArchive => "image de CD dans une archive : non prise en charge".to_owned(),
        TidyBlock::DestinationNotPlainDir { dir } => format!(
            "roms/{} n'est pas un vrai dossier (lien ou fichier)",
            path(dir)
        ),
        TidyBlock::OtherVolume => {
            "sur un autre disque : pas de copie entre disques en 7.1".to_owned()
        }
        TidyBlock::NoHardLinks => {
            "système de fichiers sans liens physiques (FAT, exFAT)".to_owned()
        }
        TidyBlock::NonUtf8Path => "nom de fichier illisible (caractères non UTF-8)".to_owned(),
        TidyBlock::PathTooLong => "chemin de destination trop long pour Windows".to_owned(),
        TidyBlock::FolderNamedLikeSystem { name } => format!(
            "le dossier à créer porterait le nom d'un système : {}",
            clean(name)
        ),
        TidyBlock::ChangedSincePlan => "modifié depuis l'aperçu".to_owned(),
    }
}

pub const BULK_TITLE: &str = "Choix par extension (tous les fichiers encore à choisir) :";

pub fn bulk_line(extension: &str, n: usize) -> String {
    format!(".{} ({n}) :", clean(extension))
}

pub fn bulk_done(n: u32, extension: &str, system: &str) -> String {
    format!(
        "{n} fichier(s) .{} rangé(s) vers {}",
        clean(extension),
        system_name(system)
    )
}

pub const BIOS_TITLE: &str = "BIOS reconnus (laissés en place) :";

pub fn bios_found(p: &Path, names: &[String]) -> String {
    let names: Vec<String> = names.iter().map(|n| clean(n)).collect();
    format!("{} : BIOS connu ({})", path(p), names.join(", "))
}

pub const BIOS_IMPORT: &str = "Importer ce BIOS…";
pub const BIOS_UNIT: &str = "BIOS connu : utilisez « Importer ce BIOS… » plutôt que le rangement";

// Execution, report, undo and recovery (FU2b).

pub const BUSY: &str = "Rangement impossible : une autre fenêtre d'emu-rust range déjà les ROMs. Réessayez quand \
                        elle aura fini.";

pub fn journal_unreadable(p: &Path, message: &str) -> String {
    format!(
        "Journal de rangement illisible : {} ({}). Il bloque le rangement et son annulation tant que ce fichier \
         n'est pas retiré du dossier (déplacez-le ailleurs ou supprimez-le).",
        path(p),
        clean(message)
    )
}

pub fn other_root(journal: &Path) -> String {
    format!(
        "Le journal {} concerne un autre dossier des ROMs : il n'est ni rejoué ni annulable ici.",
        path(journal)
    )
}

pub fn relocate_failed(e: &str) -> String {
    format!(
        "Sauvegardes non mises à jour : {}. Ce sera retenté au prochain démarrage.",
        clean(e)
    )
}

pub const UNDO_INTERRUPTED_FINAL: &str = "L'annulation précédente a été interrompue : elle est définitive. Les jeux \
                                          qu'elle n'avait pas encore remis en place restent rangés ; déplacez-les \
                                          à la main si besoin.";
pub const RUN_INTERRUPTED_RECOVERED: &str = "Le rangement précédent a été interrompu : le jeu en cours de \
                                             déplacement a été remis à sa place, les autres restent rangés.";

pub const APPLY_UPDATING: &str = "« Ranger » sera disponible une fois l'aperçu à jour.";
pub const UNDO: &str = "Annuler le dernier rangement";
pub const NEW_ANALYSIS: &str = "Nouvelle analyse";
pub const STOP_RUN: &str = "Arrêter après le jeu en cours";

pub fn last_tidy(moved: u64, unix: i64) -> String {
    let date = chrono_free_date(unix);
    format!("Dernier rangement : {date}, {moved} fichier(s) déplacé(s).")
}

/// `AAAA-MM-JJ HH:MM` UTC without a date crate (civil-from-days).
fn chrono_free_date(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        secs / 3600,
        secs % 3600 / 60
    )
}

pub fn running(kind_undo: bool, done: u64, total: u64) -> String {
    if kind_undo {
        format!("Annulation du rangement… {done} / {total} jeu(x)")
    } else {
        format!("Rangement… {done} / {total} jeu(x)")
    }
}

pub const REPORT_TIDY: &str = "Rapport du rangement";
pub const REPORT_UNDO: &str = "Rapport de l'annulation";
pub const REPORT_RECOVERY: &str = "Reprise d'un rangement interrompu";
pub const CANCELLED: &str = "Arrêté à votre demande : les jeux déjà traités le restent.";
pub const RUN_INTERRUPTED: &str = "Tâche interrompue : relancez emu-rust pour terminer la reprise.";

pub fn moved(files: usize, dirs_created: u64, dirs_removed: u64) -> String {
    format!(
        "Fichiers déplacés : {files} · dossiers créés : {dirs_created} · dossiers retirés : {dirs_removed}"
    )
}

pub const LEFT_TITLE: &str = "Laissés en place :";
pub const ERRORS_TITLE: &str = "Erreurs :";
pub const SKIPPED_TITLE: &str =
    "Sauvegardes non rattachées (le chemin est déjà pris par un autre jeu) :";

pub fn left(p: &Path, b: &TidyBlock) -> String {
    format!("{} : {}", path(p), block(b))
}

pub fn error_line(p: &Path, message: &str) -> String {
    format!("{} : {}", path(p), clean(message))
}
