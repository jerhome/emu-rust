use std::path::PathBuf;

use crate::custom::DefinitionError;

/// Messages are user-facing French. They never quote file contents (DAT lines are reported by number only).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LibraryError {
    #[error("{path} : {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("base de la bibliothèque : {0}")]
    Database(String),
    /// The file was written by a newer emu-rust: never modified.
    #[error("base de la bibliothèque en version {found} (cette version gère jusqu'à {supported})")]
    NewerSchema { found: u32, supported: u32 },
    #[error("fichier DAT invalide (ligne {line}) : {message}")]
    Dat { line: usize, message: String },
    #[error("{path} : fichier trop gros")]
    TooLarge { path: PathBuf },
    #[error("{path} : archive illisible : {message}")]
    Archive { path: PathBuf, message: String },
    #[error("système inconnu : {0}")]
    UnknownSystem(String),
    #[error("opération annulée")]
    Cancelled,
    /// 7.1 tidying: the OS lock of `<data>/rangement/lock` is held by another emu-rust.
    #[error("un rangement des ROMs est déjà en cours dans une autre fenêtre d'emu-rust")]
    TidyBusy,
    /// The journal was written for another ROM root: never replayed, nothing touched.
    #[error("{journal} : journal de rangement d'un autre dossier des ROMs, ignoré")]
    TidyRootMismatch { journal: PathBuf },
    /// `TidyPlan::choose_system`: unknown unit, unit without a choice, or a system it does not list.
    #[error("rangement : « {system} » n'est pas un choix possible pour l'élément {unit}")]
    TidyChoice { unit: u32, system: String },
    /// Unreadable tidy journal: not a regular file, too large, or outside the grammar (French message, never its
    /// raw content). Only this case blocks until the user moves the file away (ADR 0023 §4.8, RV3). A failure to
    /// write a journal is an `Io` error.
    #[error("{path} : journal de rangement illisible : {message}")]
    Journal { path: PathBuf, message: String },
    /// A custom system definition refused by `validate_custom` (8.0, ADR 0025 §2); nothing was written.
    #[error("définition de système : {0}")]
    Definition(DefinitionError),
    /// A readable journal whose state refuses the action (ADR 0023 §4.8, RV3). Nothing was touched.
    #[error("{journal} : {reason}")]
    TidyRefused {
        journal: PathBuf,
        reason: TidyRefusal,
    },
}

/// Why a readable tidy journal refuses an action ([`LibraryError::TidyRefused`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TidyRefusal {
    /// `apply_tidy`, `undo_tidy`: an interrupted run must first be recovered (`recover_tidy`).
    Interrupted,
    /// `undo_tidy`: no tidy run to undo.
    NothingToUndo,
    /// `undo_tidy`: only the newest journal can be undone.
    NotLatest,
    /// `undo_tidy`: the newest journal is already undone.
    AlreadyUndone,
    /// `mark_tidy_identities`: the journal is not closed (`E` or `U` missing).
    NotClosed,
    /// `mark_tidy_identities`: not a journal of this folder, or an empty one.
    UnknownJournal,
    /// A switch end is due: finish it first (`complete_switch`, design 8.0 §2.1).
    SwitchPending,
    /// The undo of this switch journal cannot bring everything back (ADR 0025 §10.5).
    SwitchNotReversible,
}

impl std::fmt::Display for TidyRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Interrupted => "un rangement interrompu doit d'abord être repris",
            Self::NothingToUndo => "aucun rangement à annuler",
            Self::NotLatest => "seul le dernier rangement peut être annulé",
            Self::AlreadyUndone => "ce rangement est déjà annulé",
            Self::NotClosed => "rangement non terminé",
            Self::UnknownJournal => "journal de rangement inconnu",
            Self::SwitchPending => "une bascule de système doit d'abord être terminée",
            Self::SwitchNotReversible => "cette bascule ne peut pas être entièrement annulée",
        })
    }
}
