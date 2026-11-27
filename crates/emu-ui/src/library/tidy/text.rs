//! User-facing strings of « Ranger les ROMs… » (ADR 0023 §4), in one place for a future translation.

use std::path::Path;

use emu_library::{TidyBlock, TidyReason};

fn clean(s: &str) -> String {
    crate::ui::text::sanitize_core_text(s)
}

/// Paths of the user's files: shown with `/`, sanitized (file names are untrusted text).
pub fn path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    // Windows verbatim prefix (`\\?\C:\…`): noise for the reader.
    clean(s.strip_prefix("//?/").unwrap_or(&s))
}

/// `roms/<p>` for a path relative to the roms root; `roms` for the root itself (`.` or empty).
pub fn root_path(p: &Path) -> String {
    let p = path(p);
    if p.is_empty() || p == "." {
        "le dossier roms".to_owned()
    } else {
        format!("roms/{p}")
    }
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
pub const CLOSE_WITHOUT_TIDY: &str = "Fermer sans ranger";
pub const STOP: &str = "Arrêter l'analyse";
pub const RETRY: &str = "Analyser de nouveau";
pub fn apply(units: usize) -> String {
    format!("Ranger {units} jeu(x)")
}
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

/// Folders of a retired or broken system (ADR 0025 §12): nothing is taken from them nor put in them.
pub fn reserved_folders(folders: &[String]) -> String {
    let list = folders
        .iter()
        .map(|f| format!("roms/{}/", clean(f)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Dossier d'un système retiré ou en erreur : laissé tel quel ({list}). Rien n'y est pris, rien n'y est posé."
    )
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
        TidyBlock::ReadOnlyFile { path: p } => format!(
            "fichier en lecture seule : {}. emu-rust ne modifie jamais cet attribut : retirez-le vous-même \
             (Propriétés du fichier, décocher « Lecture seule »), puis relancez le rangement",
            root_path(p)
        ),
        TidyBlock::NotWritable { dir } => format!(
            "écriture refusée dans {} (droits du dossier, disque protégé en écriture, ou un fichier de ce \
             dossier est ouvert dans un autre programme). emu-rust ne modifie jamais ces protections : \
             levez-les vous-même, puis relancez le rangement",
            root_path(dir)
        ),
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

/// A write failure of the library (journal included): `LibraryError::Io`.
pub fn write_error(p: &Path, source: &std::io::Error) -> String {
    if source.kind() == std::io::ErrorKind::InvalidInput {
        return STALE_PREVIEW.to_owned();
    }
    let cause = source.to_string();
    let line = format!("Erreur de fichier : {} ({})", path(p), clean(&cause));
    match io_hint(&cause) {
        Some(hint) => format!("{line} — {hint}"),
        None => line,
    }
}

/// The library refuses a plan made for another roms folder (`ErrorKind::InvalidInput`).
pub const STALE_PREVIEW: &str = "Cet aperçu a été fait pour un autre dossier de ROMs : fermez-le et refaites \
                                 l'aperçu.";

/// Replaces the absolute roms folder (as shown) by `roms` in a message built from library paths.
pub fn relativize(message: &str, root: &Path) -> String {
    relativize_as(message, root, "roms")
}

/// Same for another folder shown under `name` (`saves`).
pub fn relativize_as(message: &str, root: &Path, name: &str) -> String {
    let root = path(root);
    if root.is_empty() {
        return message.to_owned();
    }
    message.replace(&format!("{root}/"), &format!("{name}/"))
}

/// Advice added to the line of an unreadable journal that is still in the journal folder.
pub const KEEP_JOURNAL_HINT: &str = "déplacez ce journal hors du dossier des journaux et gardez-le : il permet \
                                     de retrouver où sont allés les fichiers";

/// `TidyRefused`: the reason text of the library, said as is (it is already neutral French).
pub fn refused(reason: &str) -> String {
    format!("Action impossible : {}.", clean(reason))
}

/// Unreadable journal (`LibraryError::Journal`): it blocks tidying and undoing until it is moved away.
pub fn journal_unreadable(p: &Path, message: &str) -> String {
    format!(
        "Journal de rangement illisible : {} ({}). Il bloque le rangement et son annulation tant qu'il reste \
         dans le dossier des journaux : déplacez-le ailleurs et gardez-le, il permet de retrouver où sont \
         allés les fichiers.",
        path(p),
        clean(message)
    )
}

pub fn other_root(journal: &Path) -> String {
    format!(
        "Le journal {} concerne un autre dossier des ROMs : il n'est ni rejoué ni annulable ici, et il bloque \
         le rangement de ce dossier. Lancez d'abord emu-rust sur l'autre dossier des ROMs pour qu'il termine \
         ce rangement ; si cet autre dossier n'existe plus, déplacez ce journal hors du dossier des journaux \
         (gardez-le : il permet de retrouver où sont allés les fichiers).",
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

/// `recover_tidy` closed a journal but `last_tidy` could not say whether it was a run or an undo.
pub const INTERRUPTED_RECOVERED: &str = "Un rangement ou une annulation précédente avait été interrompu : la \
                                         reprise est faite. Les jeux déjà traités restent où ils sont ; \
                                         consultez le dossier des ROMs si besoin.";
/// Start-up recovery while another window holds the tidying lock.
pub const RECOVERY_BUSY: &str = "Reprise du rangement interrompu reportée : une autre fenêtre d'emu-rust range \
                                 déjà les ROMs. Elle sera faite au prochain démarrage.";
/// `--save-dir` points elsewhere: the identity registries of the usual saves folder are not touched.
pub const IDENTITIES_DEFERRED: &str = "Sauvegardes non mises à jour pour les jeux rangés : un autre dossier de \
                                       sauvegardes est utilisé (--save-dir). Ce sera fait au prochain démarrage \
                                       sans --save-dir.";

pub const APPLY_UPDATING: &str = "« Ranger » sera disponible une fois l'aperçu à jour.";
pub const UNDO: &str = "Annuler le dernier rangement…";
pub const UNDO_CONFIRM_YES: &str = "Oui, annuler ce rangement";
pub const UNDO_CONFIRM_NO: &str = "Non, garder le rangement";

pub fn undo_confirm(moved: u64, unix: i64) -> String {
    format!(
        "Annuler le rangement du {} ({moved} fichier(s) déplacé(s)) ? Les jeux reviennent à leur place \
         d'origine. Une seule annulation est possible, et elle ne peut pas être arrêtée en cours de route.",
        local_date(unix)
    )
}
pub const NEW_ANALYSIS: &str = "Nouvelle analyse";
pub const STOP_RUN: &str = "Arrêter après le jeu en cours";

pub fn last_tidy(moved: u64, unix: i64) -> String {
    let date = local_date(unix);
    format!("Dernier rangement : {date}, {moved} fichier(s) déplacé(s).")
}

/// `JJ/MM/AAAA à HH:MM`, local time (the journal stores Unix seconds); UTC when the local offset is unknown.
fn local_date(unix: i64) -> String {
    use chrono::{Local, TimeZone, Utc};
    match Local.timestamp_opt(unix, 0).single() {
        Some(t) => t.format("%d/%m/%Y à %H:%M").to_string(),
        None => Utc.timestamp_opt(unix, 0).single().map_or_else(
            || unix.to_string(),
            |t| t.format("%d/%m/%Y à %H:%M UTC").to_string(),
        ),
    }
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
pub const UNDO_CANCELLED: &str = "Annulation interrompue : elle est définitive. Les jeux qu'elle n'avait pas \
                                  encore remis en place restent rangés ; déplacez-les à la main si besoin.";
pub const UNDO_ALL_BACK: &str =
    "Annulation terminée : les jeux sont revenus à leur place d'origine.";
pub const UNDO_SOME_LEFT: &str = "Annulation terminée, sauf pour les jeux ci-dessous : ils restent rangés \
                                  (déplacez-les à la main si besoin).";
pub const UNDO_LEFT_TITLE: &str = "Restés rangés :";
pub const AFTER_ERROR: &str = "Les jeux déjà traités restent rangés.";
pub const EXIT_UNFINISHED: &str =
    "Fenêtre fermée pendant le rangement : il sera terminé au prochain démarrage.";
pub const RUN_INTERRUPTED: &str = "Tâche interrompue : relancez emu-rust pour terminer la reprise.";

pub fn moved(files: usize, dirs_created: u64, dirs_removed: u64) -> String {
    format!(
        "Fichiers déplacés : {files} · dossiers créés : {dirs_created} · dossiers retirés : {dirs_removed}"
    )
}

pub const LEFT_TITLE: &str = "Laissés en place :";
pub const EXTRA_NAMES_TITLE: &str = "Deux noms pour le même fichier :";

/// `remove` and `keep` are already shown relative (`roms/…`). Same file, so nothing is lost either way.
pub fn extra_name(remove: &Path, keep: &Path) -> String {
    format!(
        "{} n'a pas pu être retiré (lecture seule ou écriture refusée) ; emu-rust ne change jamais ces \
         protections. {} est gardé : levez la protection de {} vous-même (Propriétés, « Lecture seule »), \
         puis retirez ce nom.",
        path(remove),
        path(keep),
        path(remove)
    )
}

pub const ERRORS_TITLE: &str = "Erreurs :";
pub const SKIPPED_TITLE: &str = "Sauvegardes non mises à jour :";

/// A problem of a run written to the « Journal », prefixed with what was running.
pub fn journal_line(kind: super::exec::RunKind, line: &str) -> String {
    let what = match kind {
        super::exec::RunKind::Tidy => "Rangement des ROMs",
        super::exec::RunKind::Undo => "Annulation du rangement",
        super::exec::RunKind::Recovery => "Reprise du rangement",
        super::exec::RunKind::Switch => "Bascule de système",
    };
    format!("{what} : {line}")
}

/// The analysis of « Ranger les ROMs… » failed (written to the « Journal »).
pub fn journal_analysis(e: &str) -> String {
    format!("Analyse du rangement : {e}")
}

pub fn left(p: &Path, b: &TidyBlock) -> String {
    format!("{} : {}", path(p), block(b))
}

pub fn error_line(p: &Path, message: &str) -> String {
    match io_hint(message) {
        Some(hint) => format!("{} : {} — {hint}", path(p), clean(message)),
        None => format!("{} : {}", path(p), clean(message)),
    }
}

/// Windows sharing / lock violation (`ERROR_SHARING_VIOLATION` 32, `ERROR_LOCK_VIOLATION` 33), recognized in the
/// `io::Error` text the library reports (`… (os error 32)`): its system wording does not say what to do. Those
/// codes cannot come out of a rename or a hard link on Unix (EPIPE, EDOM), so the check is not `cfg`-gated.
fn io_hint(message: &str) -> Option<&'static str> {
    let code = message
        .trim_end()
        .strip_suffix(')')?
        .rsplit_once("(os error ")?
        .1;
    matches!(code, "32" | "33").then_some(SHARING_HINT)
}

pub const SHARING_HINT: &str = "fichier ouvert par un autre programme : fermez-le, puis relancez le rangement \
                                (le jeu est resté à sa place)";

// ---- switch to an official system (ADR 0025 §10) ------------------------------------------------------------

pub const SWITCH_TITLE: &str = "Basculer vers le système officiel";
pub const SWITCH_ANALYSING: &str = "Analyse de la bascule… ";
pub const SWITCH_START: &str = "Basculer";
pub const SWITCH_START_RECOMMENDED: &str = "Basculer (recommandé)";
pub const SWITCH_LATER: &str = "Plus tard";
pub const SWITCH_NOT_THIS_ONE: &str = "Non, ce n'est pas la même machine";
pub const SWITCH_UNCERTAIN_YES: &str = "Oui, basculer";
pub const SWITCH_REPORT: &str = "Rapport de la bascule";
pub const SWITCH_BANNER_DISMISS: &str = "Fermer";
pub const SWITCH_SETTINGS_BUTTON: &str = "Basculer…";
pub const SWITCH_KEYS_TITLE: &str = "Vos touches et celles du système officiel sont différentes :";
pub const SWITCH_KEYS_USER: &str = "Garder vos touches";
pub const SWITCH_KEYS_OFFICIAL: &str = "Garder celles du système officiel";
pub const SWITCH_CORE_NOTE: &str = "Votre core reste le premier choix pour ce système ; le core recommandé reste \
                                    disponible dans l'écran Cores.";
pub const SWITCH_BLOCKED_TITLE: &str = "Jeux à régler avant de basculer :";
pub const SWITCH_BLOCKED_HINT: &str = "La bascule n'est pas proposée tant qu'il en reste : réglez-les (déplacez, \
                                       renommez ou supprimez ces fichiers), puis relancez la bascule.";
pub const SWITCH_NO_OFFICIAL: &str = "Système officiel introuvable : bascule impossible.";
pub const SWITCH_JOURNAL_OPEN: &str = "Une bascule est déjà en cours : terminez-la d'abord.";

pub fn switch_banner(official: &str) -> String {
    format!("emu-rust prend désormais en charge {official} officiellement.")
}

pub fn switch_banner_uncertain(name: &str) -> String {
    format!(
        "Une machine officielle porte le même nom de dossier que « {name} » : est-ce la même machine ?"
    )
}

pub fn switch_intro(official: &str, kind: emu_library::SwitchKind) -> String {
    match kind {
        emu_library::SwitchKind::NeedsMove => format!(
            "Vos jeux de ce système seront rangés dans le dossier de {official} par le rangement, puis le système \
             personnalisé sera retiré."
        ),
        emu_library::SwitchKind::Uncertain => format!(
            "Le dossier est le même que celui de {official} : si c'est la même machine, vos jeux, favoris et \
             jaquettes restent tels quels et le système personnalisé est retiré."
        ),
        _ => format!(
            "Le dossier est déjà celui de {official} : aucun fichier n'est déplacé ; vos jeux, favoris et jaquettes \
             restent tels quels et le système personnalisé est retiré."
        ),
    }
}

pub const SWITCH_DEFINITIVE: &str = "Cette bascule est définitive : le système personnalisé retiré ne se restaure \
                                     pas.";
pub const SWITCH_UNDOABLE: &str = "Annulable tant que vous ne rangez rien d'autre : « Annuler le dernier \
                                   rangement » remet les jeux en place et restaure le système personnalisé. Les \
                                   jaquettes renommées ne sont pas défaites (elles se retéléchargent sur action).";

pub fn switch_units(units: usize, files: usize) -> String {
    format!("{units} jeu(x) à déplacer ({files} fichier(s)).")
}

pub fn switch_frozen(n: usize, official: &str) -> String {
    format!(
        "{n} jeu(x) que {official} ne reconnaît pas : gardés, masqués, jusqu'à « Oublier ses jeux » (Réglages)."
    )
}

pub fn switch_ignored(n: usize, custom_id: &str, official: &str) -> String {
    format!("{n} fichier(s) restent dans roms/{custom_id}/ : {official} ne les reconnaît pas.")
}

pub fn switch_done(official: &str) -> String {
    format!("Bascule faite : {official} est maintenant pris en charge officiellement.")
}

pub fn switch_undone(custom: &str) -> String {
    format!("Bascule annulée : le système personnalisé « {custom} » est de retour.")
}

pub fn switch_incomplete(n: usize, custom_id: &str) -> String {
    format!(
        "Bascule incomplète : {n} jeu(x) restent dans roms/{custom_id}/ ; résolvez-les puis relancez la bascule."
    )
}

pub fn switch_end_failed(reason: &str) -> String {
    format!(
        "La fin de la bascule n'a pas pu se faire : {reason}. Elle sera retentée au prochain démarrage ; aucun \
         rangement ni aucune annulation n'est possible d'ici là."
    )
}

pub const SWITCH_CONFIG_LOCKED: &str =
    "le fichier des réglages est invalide : il n'est pas réécrit";
pub const SWITCH_CONFIG_TOO_LARGE: &str = "le fichier des réglages est trop gros";

pub fn switch_settings_failed(reason: &str) -> String {
    format!(
        "Touches et core non repris ({reason}) : la fin de la bascule est repoussée au prochain démarrage."
    )
}

pub const SWITCH_INTERRUPTED: &str =
    "Bascule interrompue : relancez la bascule pour déplacer les jeux restants.";
pub fn switch_end_banner(custom_id: &str, block: super::switch::EndBlock, retired: &str) -> String {
    use super::switch::EndBlock;
    let why = match block {
        EndBlock::PlaceTaken => format!("systems/{custom_id}.toml existe déjà"),
        EndBlock::RetiredMissing => {
            format!(
                "fichier retiré introuvable : remettez systems/.retired/{retired} puis relancez"
            )
        }
        EndBlock::Other => "voir le Journal".to_owned(),
    };
    format!("Bascule de « {custom_id} » à terminer : {why}.")
}

pub const SWITCH_END_DUE: &str = "Une bascule attend sa fin : relancez-la ci-dessous.";
pub const SWITCH_END_RETRY: &str = "Terminer la bascule";
pub const SWITCH_REMOVE_AND_END: &str = "Retirer ce système et terminer la bascule";

pub const SWITCH_UNDO: &str = "Annuler la bascule…";
pub const SWITCH_UNDO_REFUSED_UNIT: &str = "Annulation de la bascule impossible : un jeu ne peut pas revenir (fichier \
                                            changé ou place d'origine occupée).";
pub const SWITCH_UNDO_REFUSED_MISSING: &str = "Annulation de la bascule impossible : le fichier retiré du système \
                                               personnalisé est introuvable, remettez-le dans systems/.retired/.";

pub fn switch_undo_recreated(id: &str) -> String {
    format!(
        "Annulation de la bascule impossible : le système personnalisé a été recréé, supprimez d'abord le \
         système ({}).",
        clean(id)
    )
}

pub fn switch_undo_confirm(moved: u64, unix: i64) -> String {
    format!(
        "Annuler la bascule du {} ({moved} fichier(s) déplacé(s)) ? Les jeux reviennent à leur place d'origine et \
         le système personnalisé est restauré. Une seule annulation est possible, et elle ne peut pas être \
         arrêtée en cours de route.",
        local_date(unix)
    )
}

pub fn and_more(n: usize) -> String {
    format!("… et {n} autre(s)")
}
