//! Execution side of « Ranger les ROMs… » (FU2b, design v7.1 §3.4.1, `.claude/fb1b-7.1.md`): `apply_tidy` /
//! `undo_tidy` in a worker, then the identity step (`relocate_identities`, then `mark_tidy_identities` only after
//! it succeeded), and the recovery run at start-up and before a direct launch. The sequencing goes through
//! [`TidySteps`] so it is unit-tested without touching the disk.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use emu_library::{
    AbsoluteMoves, Library, LibraryError, TidyJournalInfo, TidyOptions, TidyPlan, TidyProgress,
    TidyReport,
};

use super::text;
use crate::config::DataDirs;
use crate::library::worker::{Notify, Task};

/// What the worker ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunKind {
    Tidy,
    Undo,
    /// Start-up / pre-launch replay of an interrupted run.
    Recovery,
}

/// Everything the report screen shows. Strings are French and already sanitized where they come from paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub kind: RunKind,
    /// Report of the run, or of `recover_tidy` after a failed run.
    pub report: Option<TidyReport>,
    /// `RelocateReport::skipped` of every identity step (identity collisions, unreadable registries).
    pub skipped: Vec<(PathBuf, String)>,
    /// Errors and notes, in order.
    pub messages: Vec<String>,
    /// `last_tidy` after the run: drives « Annuler le dernier rangement ».
    pub last: LastTidy,
    /// Canonical roms root, to show paths relative to it.
    pub root: Option<PathBuf>,
}

impl Outcome {
    pub(crate) fn new(kind: RunKind) -> Self {
        Self {
            kind,
            report: None,
            skipped: Vec::new(),
            messages: Vec::new(),
            last: LastTidy::default(),
            root: None,
        }
    }
}

/// The library and runtime calls of the sequence.
pub(crate) trait TidySteps {
    fn recover(&mut self) -> Result<Option<TidyReport>, LibraryError>;
    fn pending(&mut self) -> Result<Vec<(PathBuf, AbsoluteMoves)>, LibraryError>;
    /// `emu_runtime::relocate_identities` on the saves folder; `Ok(skipped)`.
    fn relocate(&mut self, moves: &AbsoluteMoves) -> Result<Vec<(PathBuf, String)>, String>;
    fn mark(&mut self, journal: &Path) -> Result<(), LibraryError>;
    fn last(&mut self) -> Result<Option<TidyJournalInfo>, LibraryError>;
}

/// French text of a tidy error (TidyBusy, unreadable journal, other root; anything else as is).
pub(crate) fn error_text(e: &LibraryError) -> String {
    match e {
        LibraryError::TidyBusy => text::BUSY.to_owned(),
        LibraryError::Journal { path, message } => text::journal_unreadable(path, message),
        LibraryError::TidyRootMismatch { journal } => text::other_root(journal),
        e => crate::ui::text::sanitize_core_text(&e.to_string()),
    }
}

/// `last_tidy`, read off the UI thread.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LastTidy {
    pub info: Option<TidyJournalInfo>,
    /// French text of the `last_tidy` error.
    pub error: Option<String>,
    /// An unreadable journal blocks tidying and undoing until the file is removed.
    pub blocked: bool,
}

impl LastTidy {
    pub(crate) fn from_result(r: Result<Option<TidyJournalInfo>, LibraryError>) -> Self {
        match r {
            Ok(info) => Self {
                info,
                ..Self::default()
            },
            Err(e) => Self {
                info: None,
                blocked: matches!(e, LibraryError::Journal { .. }),
                error: Some(error_text(&e)),
            },
        }
    }

    /// « Annuler le dernier rangement » is offered for this journal (newest, closed, not undone, this root).
    pub(crate) fn undo_target(&self) -> Option<&Path> {
        self.info
            .as_ref()
            .filter(|i| i.can_undo())
            .map(|i| i.path.as_path())
    }
}

/// « Ranger » is enabled: the preview is settled (no choice pending), something is checked, nothing blocks.
pub(crate) fn can_apply(updating: usize, checked: usize, blocked: bool) -> bool {
    updating == 0 && checked > 0 && !blocked
}

/// Identity step of one closed journal: relocate, then mark only when relocate succeeded (otherwise the journal
/// stays pending and the next start replays it).
fn identities<S: TidySteps>(
    steps: &mut S,
    journal: &Path,
    moves: &AbsoluteMoves,
    out: &mut Outcome,
) {
    match steps.relocate(moves) {
        Ok(skipped) => {
            out.skipped.extend(skipped);
            if let Err(e) = steps.mark(journal) {
                out.messages.push(error_text(&e));
            }
        }
        Err(e) => out.messages.push(text::relocate_failed(&e)),
    }
}

/// `recover_tidy`, then the identity step of every pending journal. Errors are reported, never fatal.
pub(crate) fn finish_pending<S: TidySteps>(steps: &mut S, out: &mut Outcome) {
    match steps.recover() {
        Ok(Some(report)) => {
            let undone = steps.last().ok().flatten().is_some_and(|i| i.undone);
            out.messages.push(if undone {
                text::UNDO_INTERRUPTED_FINAL.to_owned()
            } else {
                text::RUN_INTERRUPTED_RECOVERED.to_owned()
            });
            if out.report.is_none() {
                out.report = Some(report);
            }
        }
        Ok(None) => {}
        Err(e) => out.messages.push(error_text(&e)),
    }
    match steps.pending() {
        Ok(list) => {
            for (journal, moves) in list {
                identities(steps, &journal, &moves, out);
            }
        }
        Err(e) => out.messages.push(error_text(&e)),
    }
}

/// After `apply_tidy` / `undo_tidy`: identity step of its journal, or recovery after an `Err` (the partial report
/// is lost: `recover_tidy` closes the journal and gives its own).
pub(crate) fn after_run<S: TidySteps>(
    kind: RunKind,
    result: Result<TidyReport, LibraryError>,
    steps: &mut S,
) -> Outcome {
    let mut out = Outcome::new(kind);
    match result {
        Ok(report) => {
            if let Some(journal) = &report.journal {
                identities(steps, journal, &report.moved, &mut out);
            }
            out.report = Some(report);
        }
        Err(e) => {
            out.messages.push(error_text(&e));
            let busy = matches!(e, LibraryError::TidyBusy);
            // Another window holds the lock: its own run recovers; nothing to replay here.
            if !busy {
                finish_pending(steps, &mut out);
            }
        }
    }
    out.last = LastTidy::from_result(steps.last());
    out
}

/// The real steps: one library connection, the saves folder of the identity registries.
struct RealSteps<'a> {
    library: &'a mut Library,
    options: &'a TidyOptions,
    saves: &'a Path,
}

impl TidySteps for RealSteps<'_> {
    fn recover(&mut self) -> Result<Option<TidyReport>, LibraryError> {
        self.library.recover_tidy(self.options)
    }
    fn pending(&mut self) -> Result<Vec<(PathBuf, AbsoluteMoves)>, LibraryError> {
        self.library.pending_tidy_identities(self.options)
    }
    fn relocate(&mut self, moves: &AbsoluteMoves) -> Result<Vec<(PathBuf, String)>, String> {
        emu_runtime::relocate_identities(self.saves, moves)
            .map(|r| r.skipped)
            .map_err(|e| e.to_string())
    }
    fn mark(&mut self, journal: &Path) -> Result<(), LibraryError> {
        self.library.mark_tidy_identities(self.options, journal)
    }
    fn last(&mut self) -> Result<Option<TidyJournalInfo>, LibraryError> {
        emu_library::last_tidy(self.options)
    }
}

/// What the worker runs.
#[derive(Debug, Clone)]
pub(crate) enum Job {
    Apply { plan: TidyPlan, selected: Vec<u32> },
    Undo { journal: PathBuf },
    Recovery,
}

impl Job {
    fn kind(&self) -> RunKind {
        match self {
            Job::Apply { .. } => RunKind::Tidy,
            Job::Undo { .. } => RunKind::Undo,
            Job::Recovery => RunKind::Recovery,
        }
    }
}

fn roms_root(dirs: &DataDirs) -> PathBuf {
    dirs.root.join("roms")
}

/// Runs `job` on this thread (worker side, or before the window exists).
pub(crate) fn run_job(
    dirs: &DataDirs,
    job: Job,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&TidyProgress),
) -> Outcome {
    let kind = job.kind();
    let fail = |msg: String| {
        let mut out = Outcome::new(kind);
        out.messages.push(msg);
        out
    };
    let options = match TidyOptions::new(&roms_root(dirs), &dirs.tidy_journals) {
        Ok(o) => o,
        Err(e) => return fail(error_text(&e)),
    };
    let mut library = match Library::open_existing(&dirs.library_db) {
        Ok(l) => l,
        Err(e) => return fail(error_text(&e)),
    };
    let result = match &job {
        Job::Apply { plan, selected } => {
            Some(library.apply_tidy(&options, plan, selected, cancel, progress))
        }
        Job::Undo { journal } => Some(library.undo_tidy(&options, journal, cancel, progress)),
        Job::Recovery => None,
    };
    let mut steps = RealSteps {
        library: &mut library,
        options: &options,
        saves: &dirs.saves,
    };
    let mut out = match result {
        Some(result) => after_run(kind, result, &mut steps),
        None => {
            let mut out = Outcome::new(kind);
            finish_pending(&mut steps, &mut out);
            out.last = LastTidy::from_result(steps.last());
            out
        }
    };
    out.root = Some(options.roms_root().to_path_buf());
    out
}

/// Nothing to recover when no tidying ever ran (no journal folder) or the roms folder is missing.
pub(crate) fn recovery_needed(dirs: &DataDirs) -> bool {
    dirs.tidy_journals.is_dir() && roms_root(dirs).is_dir() && dirs.library_db.is_file()
}

/// Lines logged after the start-up recovery: messages, what was left or failed, identity collisions.
pub(crate) fn recovery_log(out: &Outcome) -> Vec<String> {
    let mut lines = out.messages.clone();
    if let Some(r) = &out.report {
        lines.extend(r.left.iter().map(|(p, b)| text::left(p, b)));
        lines.extend(r.errors.iter().map(|(p, m)| text::error_line(p, m)));
    }
    if !out.skipped.is_empty() {
        lines.push(text::SKIPPED_TITLE.to_owned());
        lines.extend(out.skipped.iter().map(|(p, m)| text::error_line(p, m)));
    }
    lines
}

/// Sample report of `--screen library-tidy-report` (paths under the data folder; nothing exists or moves).
pub(crate) fn demo_outcome(dirs: &DataDirs) -> Outcome {
    let root = roms_root(dirs);
    let mut out = Outcome::new(RunKind::Tidy);
    out.report = Some(TidyReport {
        journal: Some(dirs.tidy_journals.join("journal-1763840000.txt")),
        moved: vec![
            (root.join("Sonic.md"), root.join("megadrive/Sonic.md")),
            (root.join("Zelda.sfc"), root.join("snes/Zelda.sfc")),
            (root.join("Crash.cue"), root.join("psx/Crash/Crash.cue")),
            (root.join("Crash.bin"), root.join("psx/Crash/Crash.bin")),
        ],
        left: vec![
            (
                root.join("Tetris.gb"),
                emu_library::TidyBlock::Conflict {
                    existing: PathBuf::from("gb/Tetris.gb"),
                },
            ),
            (root.join("Doom"), emu_library::TidyBlock::ChangedSincePlan),
        ],
        errors: vec![(
            root.join("Myst.iso"),
            "accès refusé : le fichier est ouvert par un autre programme".to_owned(),
        )],
        dirs_created: 3,
        dirs_removed: 0,
        cancelled: false,
    });
    out.skipped.push((
        dirs.saves.join("snes9x/.emu-rust-games"),
        "snes/Zelda.sfc est déjà le chemin d'un autre jeu : sauvegarde laissée sous l'ancien chemin".to_owned(),
    ));
    out.last = LastTidy::from_result(Ok(Some(TidyJournalInfo {
        path: dirs.tidy_journals.join("journal-1763840000.txt"),
        created_unix: 1_763_840_000,
        moved: 4,
        undone: false,
        complete: true,
        identities_done: true,
    })));
    out.root = Some(root);
    out
}

pub(crate) type RunTask = Task<TidyProgress, Outcome>;

pub(crate) fn spawn_run(dirs: DataDirs, job: Job, notify: Notify) -> RunTask {
    Task::spawn("tidy-run", notify, move |cancel, progress| {
        run_job(&dirs, job, cancel, &mut |p: &TidyProgress| {
            progress(p.clone())
        })
    })
}

/// For the analysis worker: `last_tidy` read off the UI thread.
pub(crate) fn read_last(options: &TidyOptions) -> LastTidy {
    LastTidy::from_result(emu_library::last_tidy(options))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        recover: Option<Result<Option<TidyReport>, LibraryError>>,
        pending: Vec<(PathBuf, AbsoluteMoves)>,
        relocate_fails: bool,
        last: Option<TidyJournalInfo>,
    }

    impl TidySteps for Fake {
        fn recover(&mut self) -> Result<Option<TidyReport>, LibraryError> {
            self.calls.push("recover".into());
            self.recover.take().unwrap_or(Ok(None))
        }
        fn pending(&mut self) -> Result<Vec<(PathBuf, AbsoluteMoves)>, LibraryError> {
            self.calls.push("pending".into());
            Ok(std::mem::take(&mut self.pending))
        }
        fn relocate(&mut self, moves: &AbsoluteMoves) -> Result<Vec<(PathBuf, String)>, String> {
            self.calls.push(format!("relocate {}", moves.len()));
            if self.relocate_fails {
                Err("registre verrouillé".into())
            } else {
                Ok(vec![(PathBuf::from("/s/a"), "collision".into())])
            }
        }
        fn mark(&mut self, journal: &Path) -> Result<(), LibraryError> {
            self.calls.push(format!("mark {}", journal.display()));
            Ok(())
        }
        fn last(&mut self) -> Result<Option<TidyJournalInfo>, LibraryError> {
            self.calls.push("last".into());
            Ok(self.last.clone())
        }
    }

    fn info(undone: bool, complete: bool) -> TidyJournalInfo {
        TidyJournalInfo {
            path: PathBuf::from("/j/journal-1.txt"),
            created_unix: 1,
            moved: 2,
            undone,
            complete,
            identities_done: false,
        }
    }

    fn report(journal: Option<&str>) -> TidyReport {
        TidyReport {
            journal: journal.map(PathBuf::from),
            moved: vec![(PathBuf::from("/r/a.sfc"), PathBuf::from("/r/snes/a.sfc"))],
            ..TidyReport::default()
        }
    }

    #[test]
    fn apply_relocates_then_marks_its_journal() {
        let mut f = Fake::default();
        let out = after_run(RunKind::Tidy, Ok(report(Some("/j/journal-1.txt"))), &mut f);
        assert_eq!(f.calls, ["relocate 1", "mark /j/journal-1.txt", "last"]);
        assert_eq!(out.skipped.len(), 1);
        assert!(out.messages.is_empty());
    }

    #[test]
    fn no_journal_means_no_identity_step() {
        let mut f = Fake::default();
        after_run(RunKind::Tidy, Ok(report(None)), &mut f);
        assert_eq!(f.calls, ["last"]);
    }

    #[test]
    fn failed_relocate_does_not_mark() {
        let mut f = Fake {
            relocate_fails: true,
            ..Fake::default()
        };
        let out = after_run(RunKind::Undo, Ok(report(Some("/j/journal-1.txt"))), &mut f);
        assert_eq!(f.calls, ["relocate 1", "last"]);
        assert_eq!(out.messages.len(), 1);
    }

    #[test]
    fn error_recovers_then_finishes_pending() {
        let mut f = Fake {
            recover: Some(Ok(Some(report(Some("/j/journal-1.txt"))))),
            pending: vec![(PathBuf::from("/j/journal-1.txt"), Vec::new())],
            ..Fake::default()
        };
        let err = LibraryError::Journal {
            path: PathBuf::from("/j/journal-1.txt"),
            message: "écriture impossible".into(),
        };
        let out = after_run(RunKind::Tidy, Err(err), &mut f);
        assert_eq!(
            f.calls,
            [
                "recover",
                "last",
                "pending",
                "relocate 0",
                "mark /j/journal-1.txt",
                "last"
            ]
        );
        assert!(out.report.is_some());
        assert!(out.messages[1].contains(text::RUN_INTERRUPTED_RECOVERED));
    }

    #[test]
    fn busy_neither_recovers_nor_relocates() {
        let mut f = Fake::default();
        let out = after_run(RunKind::Tidy, Err(LibraryError::TidyBusy), &mut f);
        assert_eq!(f.calls, ["last"]);
        assert_eq!(out.messages, [text::BUSY]);
    }

    #[test]
    fn interrupted_undo_is_reported_as_final() {
        let mut f = Fake {
            recover: Some(Ok(Some(report(Some("/j/journal-1.txt"))))),
            last: Some(info(true, true)),
            ..Fake::default()
        };
        let mut out = Outcome::new(RunKind::Recovery);
        finish_pending(&mut f, &mut out);
        assert_eq!(out.messages, [text::UNDO_INTERRUPTED_FINAL]);
    }

    #[test]
    fn recovery_errors_do_not_stop_the_pending_step() {
        let mut f = Fake {
            recover: Some(Err(LibraryError::TidyRootMismatch {
                journal: PathBuf::from("/j/journal-9.txt"),
            })),
            pending: vec![(PathBuf::from("/j/journal-1.txt"), Vec::new())],
            ..Fake::default()
        };
        let mut out = Outcome::new(RunKind::Recovery);
        finish_pending(&mut f, &mut out);
        assert_eq!(
            f.calls,
            ["recover", "pending", "relocate 0", "mark /j/journal-1.txt"]
        );
        assert!(out.messages[0].contains("autre dossier des ROMs"));
    }

    #[test]
    fn undo_is_offered_only_for_a_closed_journal_not_undone() {
        let last = |i| LastTidy::from_result(Ok(i));
        assert!(last(Some(info(false, true))).undo_target().is_some());
        assert!(last(Some(info(true, true))).undo_target().is_none());
        assert!(last(Some(info(false, false))).undo_target().is_none());
        assert!(last(None).undo_target().is_none());
        let unreadable = LastTidy::from_result(Err(LibraryError::Journal {
            path: PathBuf::from("/j/journal-2.txt"),
            message: "x".into(),
        }));
        assert!(unreadable.undo_target().is_none());
        assert!(unreadable.blocked);
        let other = LastTidy::from_result(Err(LibraryError::TidyRootMismatch {
            journal: PathBuf::from("/j/journal-2.txt"),
        }));
        assert!(other.undo_target().is_none());
        assert!(!other.blocked);
    }

    #[test]
    fn apply_needs_a_settled_preview() {
        assert!(can_apply(0, 3, false));
        assert!(!can_apply(1, 3, false));
        assert!(!can_apply(0, 0, false));
        assert!(!can_apply(0, 3, true));
    }

    #[test]
    fn recovery_log_lists_messages_left_and_collisions() {
        let mut out = Outcome::new(RunKind::Recovery);
        out.messages.push(text::UNDO_INTERRUPTED_FINAL.to_owned());
        let mut r = report(Some("/j/journal-1.txt"));
        r.left.push((
            PathBuf::from("/r/b.sfc"),
            emu_library::TidyBlock::ChangedSincePlan,
        ));
        out.report = Some(r);
        out.skipped
            .push((PathBuf::from("/s/x"), "chemin déjà pris".into()));
        let log = recovery_log(&out);
        assert_eq!(log[0], text::UNDO_INTERRUPTED_FINAL);
        assert!(log[1].contains("modifié depuis l'aperçu"));
        assert_eq!(log[2], text::SKIPPED_TITLE);
        assert!(log[3].contains("chemin déjà pris"));
    }

    #[test]
    fn error_texts_are_french() {
        assert_eq!(error_text(&LibraryError::TidyBusy), text::BUSY);
        assert!(text::BUSY.contains("une autre fenêtre d'emu-rust range déjà"));
        let t = error_text(&LibraryError::Journal {
            path: PathBuf::from("/data/rangement/journal-5.txt"),
            message: "ligne 3 invalide".into(),
        });
        assert!(t.contains("/data/rangement/journal-5.txt"));
        assert!(t.contains("retiré"));
        let t = error_text(&LibraryError::TidyRootMismatch {
            journal: PathBuf::from("/data/rangement/journal-6.txt"),
        });
        assert!(t.contains("autre dossier des ROMs"));
    }
}
