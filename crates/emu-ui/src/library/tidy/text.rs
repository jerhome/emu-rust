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
pub const APPLY_LATER: &str = "Le déplacement des fichiers arrive à l'étape suivante du développement : cet \
                               écran ne montre pour l'instant que l'aperçu.";
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
