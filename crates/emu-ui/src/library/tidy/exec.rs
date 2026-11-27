//! Execution side of « Ranger les ROMs… » (FU2b, design v7.1 §3.4.1, `.claude/fb1b-7.1.md`): `apply_tidy` /
//! `undo_tidy` in a worker, then the identity step (`relocate_identities`, then `mark_tidy_identities` only after
//! it succeeded), and the recovery run at start-up and before a direct launch. The sequencing goes through
//! [`TidySteps`] so it is unit-tested without touching the disk.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use emu_library::{
    AbsoluteMoves, Library, LibraryError, TidyJournalInfo, TidyOptions, TidyPlan, TidyProgress,
    TidyReport, TidySection,
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
    /// Saves folder, idem (`saves/<core>/…`: no account name in the report).
    pub saves: Option<PathBuf>,
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
            saves: None,
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
    /// `--save-dir` replaces the usual saves folder: the identity step is left for a later start (rv-fu2b m8),
    /// otherwise it would update the registries of the wrong folder and mark the journal done.
    fn identities_deferred(&self) -> bool;
}

/// French text of a tidy error (TidyBusy, unreadable journal, other root; anything else as is).
pub(crate) fn error_text(e: &LibraryError) -> String {
    match e {
        LibraryError::TidyBusy => text::BUSY.to_owned(),
        LibraryError::Journal { path, message } => text::journal_unreadable(path, message),
        LibraryError::TidyRefused { reason, .. } => text::refused(&reason.to_string()),
        LibraryError::Io { path, source } => text::write_error(path, source),
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
            Err(e) => {
                let error = error_text(&e);
                Self {
                    info: None,
                    // Another root: tidying here would fail on its in-progress journal, or undo the wrong one.
                    blocked: matches!(
                        e,
                        LibraryError::Journal { .. } | LibraryError::TidyRootMismatch { .. }
                    ),
                    error: Some(error),
                }
            }
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
    if steps.identities_deferred() {
        defer_note(out);
        return;
    }
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

fn defer_note(out: &mut Outcome) {
    if !out.messages.iter().any(|m| m == text::IDENTITIES_DEFERRED) {
        out.messages.push(text::IDENTITIES_DEFERRED.to_owned());
    }
}

/// What `recover_tidy` closed, from `TidyReport::closed` (an interrupted undo is now final).
fn recovered_texts(report: &TidyReport) -> Vec<&'static str> {
    let mut v = Vec::new();
    let mut add = |t| {
        if !v.contains(&t) {
            v.push(t);
        }
    };
    for (_, section) in &report.closed {
        add(match section {
            TidySection::Undo => text::UNDO_INTERRUPTED_FINAL,
            TidySection::Tidy => text::RUN_INTERRUPTED_RECOVERED,
        });
    }
    if v.is_empty() {
        v.push(text::INTERRUPTED_RECOVERED);
    }
    v
}

/// `recover_tidy`, then the identity step of every pending journal. Errors are reported, never fatal.
pub(crate) fn finish_pending<S: TidySteps>(steps: &mut S, out: &mut Outcome) {
    match steps.recover() {
        Ok(Some(report)) => {
            out.messages
                .extend(recovered_texts(&report).into_iter().map(str::to_owned));
            if out.report.is_none() {
                out.report = Some(report);
            }
        }
        Ok(None) => {}
        // Another window holds the lock and does this work itself; at start-up, say it is only postponed.
        Err(LibraryError::TidyBusy) => {
            out.messages.push(if out.kind == RunKind::Recovery {
                text::RECOVERY_BUSY.to_owned()
            } else {
                text::BUSY.to_owned()
            });
            return;
        }
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
                if out.report.is_none() {
                    out.messages.push(text::AFTER_ERROR.to_owned());
                }
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
    deferred: bool,
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
    fn identities_deferred(&self) -> bool {
        self.deferred
    }
}

/// The saves folder is not the usual one of the data folder (`--save-dir`).
pub(crate) fn saves_overridden(dirs: &DataDirs) -> bool {
    dirs.saves != DataDirs::under(&dirs.root).saves
}

/// What the worker runs.
#[derive(Debug, Clone)]
pub(crate) enum Job {
    Apply {
        plan: Box<TidyPlan>,
        selected: Vec<u32>,
    },
    Undo {
        journal: PathBuf,
    },
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
            Some(library.apply_tidy(&options, plan.as_ref(), selected, cancel, progress))
        }
        Job::Undo { journal } => Some(library.undo_tidy(&options, journal, cancel, progress)),
        Job::Recovery => None,
    };
    // Canonical like the roms root, so that reported paths and messages can be shown as `saves/…`.
    let saves = dirs
        .saves
        .canonicalize()
        .unwrap_or_else(|_| dirs.saves.clone());
    let mut steps = RealSteps {
        library: &mut library,
        options: &options,
        saves: &saves,
        deferred: saves_overridden(dirs),
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
    for m in &mut out.messages {
        *m = text::relativize_as(&text::relativize(m, options.roms_root()), &saves, "saves");
    }
    out.saves = Some(saves);
    out
}

/// `roms/…` or `saves/…` when `p` is under one of them, otherwise `p`.
pub(crate) fn shown_path(out: &Outcome, p: &Path) -> PathBuf {
    for (base, name) in [(&out.root, "roms"), (&out.saves, "saves")] {
        if let Some(rel) = base.as_deref().and_then(|b| strip_root(p, b)) {
            return Path::new(name).join(rel);
        }
    }
    p.to_path_buf()
}

/// `m` with every absolute form of the roms folder or the saves folder replaced by `roms` / `saves`: plain or
/// Windows verbatim (`\\?\C:\…`, `\\?\UNC\…`), either separator, ASCII case folded on Windows. Safety net for texts
/// that come from other crates and carry a path (the account name must not reach the report or the clipboard).
pub(crate) fn shown_message(out: &Outcome, m: &str) -> String {
    let mut s = m.to_owned();
    for (base, name) in [(&out.root, "roms"), (&out.saves, "saves")] {
        if let Some(b) = base.as_deref() {
            s = replace_root(&s, b, name);
        }
    }
    s
}

fn replace_root(m: &str, base: &Path, name: &str) -> String {
    let mut plain = base.to_string_lossy().replace('\\', "/");
    if let Some(unc) = plain.strip_prefix("//?/UNC/") {
        plain = format!("//{unc}");
    } else if let Some(rest) = plain.strip_prefix("//?/") {
        plain = rest.to_owned();
    }
    let plain = plain.trim_end_matches('/').to_owned();
    if plain.is_empty() {
        return m.to_owned();
    }
    // Same byte length as `m`, so offsets found here are valid in `m`.
    let fold = |s: &str| {
        let s = s.replace('\\', "/");
        if cfg!(windows) {
            s.to_ascii_lowercase()
        } else {
            s
        }
    };
    let hay = fold(m);
    let mut forms = vec![format!("//?/{plain}")];
    if let Some(unc) = plain.strip_prefix("//") {
        forms.push(format!("//?/UNC/{unc}"));
    }
    forms.push(plain);
    let forms: Vec<String> = forms.iter().map(|f| fold(f)).collect();
    let mut out = String::new();
    let mut at = 0;
    while at < m.len() {
        // Earliest match, the longest form first on a tie (verbatim before plain).
        let hit = forms
            .iter()
            .filter_map(|f| {
                let mut from = at;
                while let Some(i) = hay.get(from..)?.find(f.as_str()) {
                    let end = from + i + f.len();
                    if hay[end..].starts_with('/') {
                        return Some((from + i, end));
                    }
                    from = from + i + 1;
                }
                None
            })
            .min_by_key(|&(start, end)| (start, std::cmp::Reverse(end)));
        let Some((start, end)) = hit else { break };
        if !m.is_char_boundary(start) || !m.is_char_boundary(end + 1) {
            break;
        }
        out.push_str(&m[at..start]);
        out.push_str(name);
        out.push('/');
        // The rest of the recognised path is shown with `/` too; it ends at a quote, a line break or « : ».
        let tail = &m[end + 1..];
        let stop = tail
            .find(['\n', '"', '\''])
            .into_iter()
            .chain(tail.find(" : "))
            .min();
        let next_root = forms
            .iter()
            .filter_map(|f| hay[end + 1..].find(f.as_str()))
            .min();
        let stop = end
            + 1
            + stop
                .into_iter()
                .chain(next_root)
                .min()
                .unwrap_or(tail.len());
        out.push_str(&m[end + 1..stop].replace('\\', "/"));
        at = stop;
    }
    out.push_str(&m[at..]);
    out
}

/// One « skipped » line: the path and the message, both relative.
pub(crate) fn skipped_line(out: &Outcome, p: &Path, m: &str) -> String {
    text::error_line(&shown_path(out, p), &shown_message(out, m))
}

/// `p` relative to `base`. Falls back to a textual comparison that ignores the Windows verbatim prefix
/// (`\\?\C:\…`, as `fs::canonicalize` returns it) and the separator kind, because an I/O error text
/// carries the plain form of a path whose root may be verbatim (or the opposite).
fn strip_root(p: &Path, base: &Path) -> Option<PathBuf> {
    if let Ok(rel) = p.strip_prefix(base) {
        return Some(rel.to_path_buf());
    }
    let plain = |x: &Path| {
        let s = x.to_string_lossy().replace('\\', "/");
        // `\\?\UNC\srv\share` is the verbatim form of `\\srv\share`; test it before the plain `\\?\` prefix.
        if let Some(unc) = s.strip_prefix("//?/UNC/") {
            format!("//{unc}")
        } else {
            s.strip_prefix("//?/").map_or(s.clone(), str::to_owned)
        }
    };
    let (p, base) = (plain(p), plain(base));
    let base = base.trim_end_matches('/');
    let rest = if cfg!(windows) && p.len() >= base.len() && p.is_char_boundary(base.len()) {
        // Windows paths are case-insensitive (drive letter `c:` / `C:`).
        p[base.len()..]
            .strip_prefix('/')
            .filter(|_| p[..base.len()].eq_ignore_ascii_case(base))
    } else {
        p.strip_prefix(base)?.strip_prefix('/')
    }?;
    // Non-ASCII case differences are not folded (only noted: file systems fold them differently).
    (!base.is_empty()).then(|| rest.split('/').filter(|c| !c.is_empty()).collect())
}

/// One line of `TidyReport::errors`. A `LibraryError::Io` text starts with the absolute path it is about
/// (`<path> : <cause>`): shown relative like the entry, or dropped when it is the entry itself.
pub(crate) fn error_entry(out: &Outcome, p: &Path, message: &str) -> String {
    let shown = shown_path(out, p);
    let message = match message.split_once(" : ") {
        Some((head, cause)) if Path::new(head).is_absolute() => {
            let head = shown_path(out, Path::new(head));
            if head == shown {
                cause.to_owned()
            } else {
                format!("{} : {cause}", text::path(&head))
            }
        }
        _ => message.to_owned(),
    };
    let line = text::error_line(&shown, &message);
    if is_journal_file(p) {
        format!("{line} — {}", text::KEEP_JOURNAL_HINT)
    } else {
        line
    }
}

/// `journal-<unix>[-k].txt`: a journal entry of `TidyReport::errors` (outside the roms folder).
fn is_journal_file(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("journal-") && n.ends_with(".txt"))
}

/// Nothing to recover when no tidying ever ran (no journal folder) or the roms folder is missing.
pub(crate) fn recovery_needed(dirs: &DataDirs) -> bool {
    dirs.tidy_journals.is_dir() && roms_root(dirs).is_dir() && dirs.library_db.is_file()
}

/// Head of the report, worded for a run or an undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReportLines {
    pub headline: String,
    pub cancelled: Option<&'static str>,
    pub left_title: &'static str,
}

pub(crate) fn report_lines(kind: RunKind, r: &TidyReport) -> ReportLines {
    match kind {
        RunKind::Undo => ReportLines {
            // `moved` is the net list of the whole journal, not what came back: not shown as a count.
            headline: if r.left.is_empty()
                && r.errors.is_empty()
                && r.extra_names.is_empty()
                && !r.cancelled
            {
                text::UNDO_ALL_BACK.to_owned()
            } else {
                text::UNDO_SOME_LEFT.to_owned()
            },
            cancelled: r.cancelled.then_some(text::UNDO_CANCELLED),
            left_title: text::UNDO_LEFT_TITLE,
        },
        RunKind::Tidy | RunKind::Recovery => ReportLines {
            headline: text::moved(r.moved.len(), r.dirs_created, r.dirs_removed),
            cancelled: r.cancelled.then_some(text::CANCELLED),
            left_title: text::LEFT_TITLE,
        },
    }
}

/// One line per extra name left behind (`TidyReport::extra_names`), paths relative to the roms folder.
pub(crate) fn extra_name_lines(out: &Outcome, r: &TidyReport) -> Vec<String> {
    r.extra_names
        .iter()
        .map(|n| text::extra_name(&shown_path(out, &n.remove), &shown_path(out, &n.keep)))
        .collect()
}

/// The whole report as plain text, what « Copier le rapport » puts in the clipboard.
pub(crate) fn report_text(out: &Outcome) -> String {
    let mut lines: Vec<String> = out.messages.iter().map(|m| shown_message(out, m)).collect();
    if let Some(r) = &out.report {
        let l = report_lines(out.kind, r);
        lines.push(l.headline.clone());
        lines.extend(l.cancelled.map(str::to_owned));
        if !r.left.is_empty() {
            lines.push(l.left_title.to_owned());
            lines.extend(
                r.left
                    .iter()
                    .map(|(p, b)| text::left(&shown_path(out, p), b)),
            );
        }
        if !r.extra_names.is_empty() {
            lines.push(text::EXTRA_NAMES_TITLE.to_owned());
            lines.extend(extra_name_lines(out, r));
        }
        if !r.errors.is_empty() {
            lines.push(text::ERRORS_TITLE.to_owned());
            lines.extend(r.errors.iter().map(|(p, m)| error_entry(out, p, m)));
        }
    }
    if !out.skipped.is_empty() {
        lines.push(text::SKIPPED_TITLE.to_owned());
        lines.extend(out.skipped.iter().map(|(p, m)| skipped_line(out, p, m)));
    }
    crate::copyable::joined(&lines)
}

/// Problems of a run, an undo or the start-up recovery, written to the « Journal »: failures as errors, games
/// left in place, save collisions and messages as warnings. Nothing is written for a clean run.
pub(crate) fn log_outcome(out: &Outcome) {
    let errors: Vec<String> = out
        .report
        .iter()
        .flat_map(|r| r.errors.iter().map(|(p, m)| error_entry(out, p, m)))
        .collect();
    for line in recovery_log(out) {
        if errors.contains(&line) {
            tracing::error!("{}", text::journal_line(out.kind, &line));
        } else if line != text::SKIPPED_TITLE {
            tracing::warn!("{}", text::journal_line(out.kind, &line));
        }
    }
}

/// Lines logged after the start-up recovery: messages, what was left or failed, identity collisions.
pub(crate) fn recovery_log(out: &Outcome) -> Vec<String> {
    let mut lines: Vec<String> = out.messages.iter().map(|m| shown_message(out, m)).collect();
    if let Some(r) = &out.report {
        lines.extend(
            r.left
                .iter()
                .map(|(p, b)| text::left(&shown_path(out, p), b)),
        );
        lines.extend(r.errors.iter().map(|(p, m)| error_entry(out, p, m)));
        lines.extend(extra_name_lines(out, r));
    }
    if !out.skipped.is_empty() {
        lines.push(text::SKIPPED_TITLE.to_owned());
        lines.extend(out.skipped.iter().map(|(p, m)| skipped_line(out, p, m)));
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
            // The `LibraryError::Io` text of a sharing violation on a French Windows, as the library reports it.
            format!(
                "{} : Le processus ne peut pas accéder au fichier car ce fichier est utilisé par un autre \
                 processus. (os error 32)",
                root.join("Myst.iso").display()
            ),
        )],
        dirs_created: 3,
        dirs_removed: 0,
        cancelled: false,
        ..TidyReport::default()
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
        end_due: false,
        switch: None,
    })));
    out.root = Some(root);
    out.saves = Some(dirs.saves.clone());
    out
}

pub(crate) type RunTask = Task<TidyProgress, Outcome>;

pub(crate) fn spawn_run(dirs: DataDirs, job: Job, notify: Notify) -> RunTask {
    Task::spawn_pinning_catalog("tidy-run", notify, move |cancel, progress| {
        run_job(&dirs, job, cancel, &mut |p: &TidyProgress| {
            progress(p.clone())
        })
    })
}

/// For the analysis worker: `last_tidy` read off the UI thread.
pub(crate) fn read_last(options: &TidyOptions) -> LastTidy {
    let mut last = LastTidy::from_result(emu_library::last_tidy(options));
    if let Some(m) = &mut last.error {
        *m = text::relativize(m, options.roms_root());
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use emu_library::TidyBlock;

    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        recover: Option<Result<Option<TidyReport>, LibraryError>>,
        pending: Vec<(PathBuf, AbsoluteMoves)>,
        relocate_fails: bool,
        last: Option<TidyJournalInfo>,
        deferred: bool,
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
        fn identities_deferred(&self) -> bool {
            self.deferred
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
            end_due: false,
            switch: None,
        }
    }

    fn report(journal: Option<&str>) -> TidyReport {
        TidyReport {
            journal: journal.map(PathBuf::from),
            moved: vec![(PathBuf::from("/r/a.sfc"), PathBuf::from("/r/snes/a.sfc"))],
            ..TidyReport::default()
        }
    }

    fn closed(section: TidySection) -> TidyReport {
        TidyReport {
            closed: vec![(PathBuf::from("/j/journal-1.txt"), section)],
            ..report(Some("/j/journal-1.txt"))
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
            recover: Some(Ok(Some(closed(TidySection::Tidy)))),
            pending: vec![(PathBuf::from("/j/journal-1.txt"), Vec::new())],
            last: Some(info(false, true)),
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
            recover: Some(Ok(Some(closed(TidySection::Undo)))),
            ..Fake::default()
        };
        let mut out = Outcome::new(RunKind::Recovery);
        finish_pending(&mut f, &mut out);
        assert_eq!(out.messages, [text::UNDO_INTERRUPTED_FINAL]);
    }

    #[test]
    fn recovery_wording_follows_the_closed_sections() {
        // No `closed` entry (older report): wording that fits both cases.
        let mut f = Fake {
            recover: Some(Ok(Some(report(Some("/j/journal-1.txt"))))),
            ..Fake::default()
        };
        let mut out = Outcome::new(RunKind::Recovery);
        finish_pending(&mut f, &mut out);
        assert_eq!(out.messages, [text::INTERRUPTED_RECOVERED]);
        // A run and an undo closed together: both are said, once each.
        let mut both = closed(TidySection::Tidy);
        both.closed
            .push((PathBuf::from("/j/journal-2.txt"), TidySection::Undo));
        both.closed
            .push((PathBuf::from("/j/journal-3.txt"), TidySection::Undo));
        let mut f = Fake {
            recover: Some(Ok(Some(both))),
            ..Fake::default()
        };
        let mut out = Outcome::new(RunKind::Recovery);
        finish_pending(&mut f, &mut out);
        assert_eq!(
            out.messages,
            [
                text::RUN_INTERRUPTED_RECOVERED,
                text::UNDO_INTERRUPTED_FINAL
            ]
        );
    }

    #[test]
    fn recovery_after_a_failure_keeps_the_errors_of_its_report() {
        let mut r = closed(TidySection::Tidy);
        r.errors.push((
            PathBuf::from("/r/journal-2.txt"),
            "journal illisible".into(),
        ));
        let mut f = Fake {
            recover: Some(Ok(Some(r))),
            ..Fake::default()
        };
        let err = LibraryError::Io {
            path: PathBuf::from("/j/journal-1.txt"),
            source: std::io::Error::other("disque plein"),
        };
        let out = after_run(RunKind::Tidy, Err(err), &mut f);
        assert_eq!(out.report.as_ref().unwrap().errors.len(), 1);
        assert!(out.messages[0].contains("Erreur de fichier"));
        assert!(out.messages[0].contains("disque plein"));
        assert!(!out.messages.iter().any(|m| m == text::AFTER_ERROR));
    }

    #[test]
    fn extra_names_are_shown_relative_and_stop_the_all_back_headline() {
        let mut out = Outcome::new(RunKind::Undo);
        out.root = Some(PathBuf::from("/data/roms"));
        let mut r = report(Some("/j/journal-1.txt"));
        assert_eq!(
            report_lines(RunKind::Undo, &r).headline,
            text::UNDO_ALL_BACK
        );
        r.extra_names.push(emu_library::TidyExtraName {
            remove: PathBuf::from("/data/roms/snes/a.sfc"),
            keep: PathBuf::from("/data/roms/a.sfc"),
        });
        assert_eq!(
            report_lines(RunKind::Undo, &r).headline,
            text::UNDO_SOME_LEFT
        );
        let lines = extra_name_lines(&out, &r);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("roms/snes/a.sfc"));
        assert!(lines[0].contains("roms/a.sfc est gardé"));
        assert!(!lines[0].contains("/data"));
        assert!(!lines[0].contains("supprimez"));
        out.report = Some(r);
        assert!(recovery_log(&out).iter().any(|l| l.contains("est gardé")));
    }

    #[test]
    fn write_error_texts_are_neutral_and_relative() {
        let t = text::write_error(
            Path::new(r"\\?\C:\d\roms\a.iso"),
            &std::io::Error::from_raw_os_error(32),
        );
        assert!(
            t.starts_with("Erreur de fichier") && !t.contains("//?/"),
            "{t}"
        );
        assert!(t.contains("ouvert par un autre programme"), "{t}");
        let stale = text::write_error(
            Path::new("/r"),
            &std::io::Error::new(std::io::ErrorKind::InvalidInput, "x"),
        );
        assert!(stale.contains("autre dossier de ROMs") && !stale.contains("Écriture"));
        assert_eq!(
            text::relativize("C:/d/roms/a.iso : x", Path::new(r"\\?\C:\d\roms")),
            "C:/d/roms/a.iso : x".replace("C:/d/roms/", "roms/")
        );
        let j = error_entry(
            &Outcome::new(RunKind::Recovery),
            Path::new("/j/journal-9.txt"),
            "illisible",
        );
        assert!(
            j.contains("déplacez ce journal") && j.contains("gardez-le"),
            "{j}"
        );
        let t = text::block(&TidyBlock::NotWritable { dir: "a".into() });
        assert!(t.contains("ouvert dans un autre programme"), "{t}");
    }

    #[test]
    fn block_texts_for_read_only_and_not_writable() {
        for b in [
            TidyBlock::ReadOnlyFile {
                path: PathBuf::from("Sonic.md"),
            },
            TidyBlock::NotWritable {
                dir: PathBuf::from("."),
            },
            TidyBlock::NotWritable {
                dir: PathBuf::from("snes"),
            },
        ] {
            let t = text::block(&b);
            assert!(t.contains("jamais"), "{t}");
            assert!(t.contains("relancez"), "{t}");
            assert!(!t.contains("supprimez"), "{t}");
        }
        let t = text::block(&TidyBlock::NotWritable {
            dir: PathBuf::from("."),
        });
        assert!(
            t.contains("le dossier roms") && !t.contains("roms/."),
            "{t}"
        );
        let t = text::block(&TidyBlock::ReadOnlyFile {
            path: PathBuf::from("a/Sonic.md"),
        });
        assert!(t.contains("roms/a/Sonic.md"), "{t}");
    }

    #[test]
    fn busy_at_start_up_postpones_the_recovery() {
        let mut f = Fake {
            recover: Some(Err(LibraryError::TidyBusy)),
            pending: vec![(PathBuf::from("/j/journal-1.txt"), Vec::new())],
            ..Fake::default()
        };
        let mut out = Outcome::new(RunKind::Recovery);
        finish_pending(&mut f, &mut out);
        assert_eq!(f.calls, ["recover"]);
        assert_eq!(out.messages, [text::RECOVERY_BUSY]);
    }

    #[test]
    fn save_dir_override_defers_the_identity_step() {
        let mut f = Fake {
            pending: vec![
                (PathBuf::from("/j/journal-1.txt"), Vec::new()),
                (PathBuf::from("/j/journal-2.txt"), Vec::new()),
            ],
            deferred: true,
            ..Fake::default()
        };
        let mut out = Outcome::new(RunKind::Recovery);
        finish_pending(&mut f, &mut out);
        assert_eq!(f.calls, ["recover", "pending"]);
        assert_eq!(out.messages, [text::IDENTITIES_DEFERRED]);
        let mut f = Fake {
            deferred: true,
            ..Fake::default()
        };
        let out = after_run(RunKind::Tidy, Ok(report(Some("/j/journal-1.txt"))), &mut f);
        assert_eq!(f.calls, ["last"]);
        assert_eq!(out.messages, [text::IDENTITIES_DEFERRED]);
    }

    /// An absolute path of the platform (a drive letter on Windows) from a `/` path.
    fn abs(p: &str) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!("C:{}", p.replace('/', "\\")))
        } else {
            PathBuf::from(p)
        }
    }

    #[test]
    fn strip_root_ignores_the_verbatim_prefix_and_separator_kind() {
        let rel = |p: &str, b: &str| strip_root(Path::new(p), Path::new(b));
        assert_eq!(
            rel("//?/C:/r/roms/a.bin", "C:/r/roms"),
            Some(PathBuf::from("a.bin"))
        );
        assert_eq!(
            rel("C:/r/roms/a.bin", "//?/C:/r/roms"),
            Some(PathBuf::from("a.bin"))
        );
        assert_eq!(
            rel(r"C:\r\roms\sub\a.bin", r"\\?\C:\r\roms"),
            Some(["sub", "a.bin"].iter().collect())
        );
        assert_eq!(
            rel(r"\\?\UNC\srv\share\roms\a.bin", r"\\srv\share\roms"),
            Some(PathBuf::from("a.bin"))
        );
        assert_eq!(
            rel(r"\\srv\share\roms\sub\a.bin", r"\\?\UNC\srv\share\roms"),
            Some(["sub", "a.bin"].iter().collect())
        );
        assert_eq!(rel("C:/r/romsX/a.bin", "C:/r/roms"), None);
        assert_eq!(rel("C:/other/a.bin", "//?/C:/r/roms"), None);
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_root_with_plain_io_text_does_not_leak_the_absolute_path() {
        let mut out = Outcome::new(RunKind::Tidy);
        out.root = Some(PathBuf::from(r"\\?\C:\data\roms"));
        let entry = Path::new(r"\\?\C:\data\roms\Myst.iso");
        let line = error_entry(
            &out,
            entry,
            r"C:\data\roms\Myst.bin : accès refusé (os error 5)",
        );
        assert_eq!(
            line,
            "roms/Myst.iso : roms/Myst.bin : accès refusé (os error 5)"
        );
    }

    #[test]
    fn saves_override_is_detected_against_the_data_folder() {
        let mut dirs = DataDirs::under(&abs("/data"));
        assert!(!saves_overridden(&dirs));
        dirs.saves = abs("/elsewhere/saves");
        assert!(saves_overridden(&dirs));
    }

    #[test]
    fn error_entries_drop_the_absolute_path_of_the_io_text() {
        let mut out = Outcome::new(RunKind::Tidy);
        let root = abs("/home/u/data/roms");
        out.root = Some(root.clone());
        let entry = root.join("Myst.iso");
        let entry = entry.as_path();
        let io = |name: &str, cause: &str| format!("{} : {cause}", root.join(name).display());
        let line = error_entry(
            &out,
            entry,
            &io("Myst.iso", "fichier utilisé (os error 32)"),
        );
        assert!(line.starts_with("roms/Myst.iso : fichier utilisé (os error 32)"));
        assert!(line.contains(text::SHARING_HINT));
        assert!(!line.contains("home/u"));
        let line = error_entry(&out, entry, &io("Myst.bin", "accès refusé (os error 13)"));
        assert_eq!(
            line,
            "roms/Myst.iso : roms/Myst.bin : accès refusé (os error 13)"
        );
        let line = error_entry(&out, entry, "aucun système choisi : laissé en place");
        assert_eq!(
            line,
            "roms/Myst.iso : aucun système choisi : laissé en place"
        );
    }

    #[test]
    fn demo_report_shows_a_text_the_library_produces() {
        let dirs = DataDirs::under(&abs("/data"));
        let out = demo_outcome(&dirs);
        let r = out.report.as_ref().unwrap();
        let (p, m) = &r.errors[0];
        let line = error_entry(&out, p, m);
        assert!(line.contains(text::SHARING_HINT));
        assert!(!line.contains("data/"));
        assert!(!line.contains("data\\"));
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
        assert!(other.blocked);
        assert!(unreadable.error.as_deref().unwrap().contains("déplacez-le"));
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
    fn error_without_recovery_report_says_games_stay_tidied() {
        let mut f = Fake::default();
        let err = LibraryError::Journal {
            path: PathBuf::from("/j/journal-1.txt"),
            message: "écriture impossible".into(),
        };
        let out = after_run(RunKind::Tidy, Err(err), &mut f);
        assert_eq!(out.messages.last().unwrap(), text::AFTER_ERROR);
    }

    #[test]
    fn saves_folder_is_relativized_in_messages() {
        let m = text::relativize_as(
            "Sauvegardes non mises à jour : /home/u/data/saves/snes9x/x illisible",
            Path::new("/home/u/data/saves"),
            "saves",
        );
        assert_eq!(m, "Sauvegardes non mises à jour : saves/snes9x/x illisible");
    }

    #[test]
    fn report_text_has_every_section() {
        let mut out = Outcome::new(RunKind::Tidy);
        out.root = Some(PathBuf::from("/d/roms"));
        out.saves = Some(PathBuf::from("/d/saves"));
        out.messages.push("note".into());
        out.skipped
            .push((PathBuf::from("/d/saves/a/.reg"), "illisible".into()));
        let t = report_text(&out);
        assert!(t.contains("note") && t.contains(text::SKIPPED_TITLE), "{t}");
        assert!(t.contains("saves/a/.reg") && !t.contains("/d/saves"), "{t}");
    }

    #[test]
    fn skipped_message_with_a_verbatim_path_is_relative() {
        let mut out = Outcome::new(RunKind::Tidy);
        out.root = Some(abs("/Users/u/data/roms"));
        out.saves = Some(abs("/Users/u/data/saves"));
        let m = if cfg!(windows) {
            r"Sauvegarde \\?\C:\Users\u\data\roms\c64\Maniac Mansion.zip déjà prise"
        } else {
            "Sauvegarde /Users/u/data/roms/c64/Maniac Mansion.zip déjà prise"
        };
        out.skipped
            .push((abs("/Users/u/data/saves/c64/.reg"), m.into()));
        let t = report_text(&out);
        assert!(t.contains("roms/c64/Maniac Mansion.zip"), "{t}");
        assert!(!t.contains("?") && !t.contains("Users"), "{t}");
        let line = skipped_line(&out, &out.skipped[0].0, m);
        assert!(!line.contains("Users"), "{line}");
    }

    #[test]
    fn shown_message_handles_plain_unc_and_case_forms() {
        let mut out = Outcome::new(RunKind::Tidy);
        out.root = Some(PathBuf::from("/d/roms"));
        assert_eq!(
            shown_message(&out, "a /d/roms/x et /d/roms/y"),
            "a roms/x et roms/y"
        );
        assert_eq!(shown_message(&out, "/d/romsX/x"), "/d/romsX/x");
        out.root = Some(PathBuf::from(r"\\?\UNC\srv\share\roms"));
        assert_eq!(
            shown_message(&out, r"\\srv\share\roms\a et \\?\UNC\srv\share\roms\b"),
            "roms/a et roms/b"
        );
        if cfg!(windows) {
            out.root = Some(PathBuf::from(r"C:\D\Roms"));
            assert_eq!(shown_message(&out, r"\\?\c:\d\roms\x"), "roms/x");
        }
    }

    #[test]
    fn report_paths_are_relative_to_roms_and_saves() {
        let mut out = Outcome::new(RunKind::Tidy);
        out.root = Some(PathBuf::from("/home/u/data/roms"));
        out.saves = Some(PathBuf::from("/home/u/data/saves"));
        assert_eq!(
            shown_path(&out, Path::new("/home/u/data/roms/snes/a.sfc")),
            Path::new("roms/snes/a.sfc")
        );
        assert_eq!(
            shown_path(&out, Path::new("/home/u/data/saves/snes9x/.emu-rust-games")),
            Path::new("saves/snes9x/.emu-rust-games")
        );
        assert_eq!(
            shown_path(&out, Path::new("/elsewhere/x")),
            Path::new("/elsewhere/x")
        );
    }

    #[test]
    fn undo_report_has_its_own_wording() {
        let mut r = report(Some("/j/journal-1.txt"));
        let undo = report_lines(RunKind::Undo, &r);
        assert_eq!(undo.headline, text::UNDO_ALL_BACK);
        assert_eq!(undo.left_title, text::UNDO_LEFT_TITLE);
        r.cancelled = true;
        let undo = report_lines(RunKind::Undo, &r);
        assert_eq!(undo.cancelled, Some(text::UNDO_CANCELLED));
        assert_eq!(undo.headline, text::UNDO_SOME_LEFT);
        r.cancelled = false;
        r.errors
            .push((PathBuf::from("/r/b.sfc"), "accès refusé".into()));
        assert_eq!(
            report_lines(RunKind::Undo, &r).headline,
            text::UNDO_SOME_LEFT
        );
        r.cancelled = true;
        let tidy = report_lines(RunKind::Tidy, &r);
        assert_eq!(tidy.cancelled, Some(text::CANCELLED));
        assert!(tidy.headline.contains("Fichiers déplacés : 1"));
    }

    #[test]
    fn undo_confirmation_recalls_date_and_files() {
        let t = text::undo_confirm(4, 1_763_840_000);
        // Local time: the day may shift with the zone, not the month (2025-11-22 19:33 UTC).
        assert!(t.contains("11/2025 à "));
        assert!(t.contains("4 fichier(s)"));
        assert_eq!(text::apply(3), "Ranger 3 jeu(x)");
        assert_eq!(text::CLOSE_WITHOUT_TIDY, "Fermer sans ranger");
        assert!(text::UNDO.ends_with('…'));
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
        assert!(t.contains("ligne 3 invalide"));
        assert!(t.contains("illisible") && t.contains("déplacez-le"));
        assert!(!t.contains("supprimez"));
        let t = error_text(&LibraryError::TidyRefused {
            journal: PathBuf::from("/j/journal-5.txt"),
            reason: emu_library::TidyRefusal::AlreadyUndone,
        });
        assert!(t.contains("ce rangement est déjà annulé"));
        assert!(!t.contains("illisible"));
        let t = error_text(&LibraryError::Io {
            path: PathBuf::from("/j/journal-5.txt"),
            source: std::io::Error::other("écriture du journal interrompue"),
        });
        assert!(t.starts_with("Erreur de fichier"));
        assert!(!t.contains("illisible"));
        let t = error_text(&LibraryError::TidyRootMismatch {
            journal: PathBuf::from("/data/rangement/journal-6.txt"),
        });
        assert!(t.contains("autre dossier des ROMs"));
        // Removing it would lose the record of where the files went.
        assert!(!t.contains("supprimez"));
        assert!(t.contains("Lancez d'abord emu-rust sur l'autre dossier"));
    }
}
