//! Switch with move: a custom system replaced by an official one through the tidy (design 8.0 §2.1 and §2.6,
//! ADR 0025 §10; journal version 2): the plan, the run (units moved by the tidy under a version 2 journal), the end
//! (`complete_switch`: rows, definition, `F`) and the check before the undo.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::journal::{self, Content, JournalWriter, Op, State};
use super::sys::{RealSys, Sys};
use super::{TidyOptions, TidyPlan, TidyProgress, TidyReport, exec, plan};
use crate::catalog::{Catalog, catalog};
use crate::custom::{self, is_valid_id_syntax, retired_dir};
use crate::db::{Library, ReassignScope};
use crate::error::{LibraryError, TidyRefusal};
use crate::scan::ScanProgress;

/// A custom system `custom_id` replaced by the official `official_id` (same libretro database).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchMove {
    pub custom_id: String,
    pub official_id: String,
}

/// Choices fixed before any action and written in the journal header (no question at recovery).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchChoices {
    pub keys: KeysChoice,
    pub prior_preference: Option<Vec<String>>,
    /// Opaque to `emu-library`, ≤ 16 Kio.
    pub prior_profile: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeysChoice {
    Copy,
    KeepUser,
    KeepOfficial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SwitchDirection {
    Forward,
    Undo,
}

/// A switch whose end is due (`Library::pending_switch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSwitch {
    pub journal: PathBuf,
    pub switch: SwitchMove,
    pub retired_name: String,
    pub choices: SwitchChoices,
    pub direction: SwitchDirection,
}

impl TidyPlan {
    /// `false` while a unit is blocked, duplicate or conflict, or when there is no unit to move (a switch without
    /// move has no journal: the caller reassigns in place).
    pub fn switchable(&self) -> bool {
        use super::TidyStatus;
        self.switch.is_some()
            && !self.units.is_empty()
            && self.units.iter().all(|u| {
                matches!(u.status, TidyStatus::Sure(_) | TidyStatus::Suggested(_))
                    && u.system.is_some()
                    && (!u.moves.is_empty() || u.whole_dir.is_some())
            })
    }
}

fn io_at(path: &Path, source: io::Error) -> LibraryError {
    LibraryError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn refused(journal: &Path, reason: TidyRefusal) -> LibraryError {
    LibraryError::TidyRefused {
        journal: journal.to_path_buf(),
        reason,
    }
}

/// `after` as the plan's snapshot: the published catalog when equal to it, else the last leaked copy when equal to
/// it (a preview asked again with the same `after` leaks nothing), else a new leaked copy like a publication
/// (ADR 0025 §1).
pub(super) fn snapshot(after: &Catalog) -> &'static Catalog {
    static LAST: std::sync::Mutex<Option<&'static Catalog>> = std::sync::Mutex::new(None);
    if after == catalog() {
        return catalog();
    }
    let mut last = LAST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match *last {
        Some(kept) if kept == after => kept,
        _ => {
            let leaked: &'static Catalog = Box::leak(Box::new(after.clone()));
            *last = Some(leaked);
            leaked
        }
    }
}

/// The switch whose definition must not be created, modified, removed or restored right now: a version 2 journal of
/// `journal_dir`, whatever its root, from its first `B` until its `E` — or its `F` when the end is due — and the
/// same for an undo (ADR 0023, amendment of 2026-11-27: « une bascule est en cours » / « à terminer »). A journal
/// that is closed without a due end (incomplete, done, undone) blocks nothing. The Settings read it before any
/// edit of `custom_id`; the oldest such journal is returned.
pub fn open_switch(options: &TidyOptions) -> Result<Option<SwitchMove>, LibraryError> {
    for (_, content) in journal::load_all(options.journal_dir())? {
        let Content::Journal(j) = content else {
            continue;
        };
        if let Some(h) = &j.switch
            && (j.state.in_progress() || j.end_due().is_some())
        {
            return Ok(Some(h.switch()));
        }
    }
    Ok(None)
}

impl Library {
    /// Preview computed with `after` (definition retired, official active): every unit under
    /// `roms/<custom_id>/` → `roms/<official_id>/`. Read-only, like `plan_tidy`. A row of the custom id that the
    /// official system does not accept is not a unit: it is listed in `TidyPlan::switch_frozen` and stays (ADR 0025
    /// §10). `Err(UnknownSystem)` when `after` has no official system `official_id`, or still has `custom_id`, or when
    /// `options` has no definitions folder (`TidyOptions::with_systems_dir`).
    pub fn plan_switch(
        &self,
        options: &TidyOptions,
        after: &Catalog,
        switch: &SwitchMove,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&ScanProgress),
    ) -> Result<TidyPlan, LibraryError> {
        if options.systems_dir().is_none() {
            return Err(LibraryError::UnknownSystem(switch.custom_id.clone()));
        }
        let official = after
            .system(&switch.official_id)
            .filter(|s| !s.is_custom())
            .ok_or_else(|| LibraryError::UnknownSystem(switch.official_id.clone()))?;
        if switch.custom_id == switch.official_id
            || !is_valid_id_syntax(&switch.custom_id)
            || after.system(&switch.custom_id).is_some()
        {
            return Err(LibraryError::UnknownSystem(switch.custom_id.clone()));
        }
        let mut plan = plan::run_in(
            &self.conn,
            snapshot(after),
            options,
            cancel,
            progress,
            Some(&switch.custom_id),
            Some(switch),
        )?;
        plan.switch_frozen = self.switch_frozen_rows(&switch.custom_id, official)?;
        Ok(plan)
    }

    /// Every unit of the plan (no selection). Unit transactions only call `relocate` (unchanged: paths and
    /// `pinned`, rows kept, so `needs_32x`, favourite, history and covers follow). Writes format 2 with the
    /// switch header; the run ends with `E`, never `F` (`complete_switch`'s). Refusals and `Err` cases as
    /// `apply_tidy`; `Err(Io)` for a plan that is not switchable.
    pub fn apply_switch(
        &mut self,
        options: &TidyOptions,
        plan: &TidyPlan,
        choices: &SwitchChoices,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&TidyProgress),
    ) -> Result<TidyReport, LibraryError> {
        exec::Exec::new(&RealSys, &mut self.conn, options)
            .apply_switch(plan, choices, cancel, progress)
    }

    /// `Some` when a readable journal of `journal_dir`, **whatever its root**, is a switch whose last line is `E`
    /// with all `n` units validated (`Forward`), or `U` after a forward `F` with every forward unit returned
    /// (`Undo`); at most one by construction, else the oldest. The caller applies keys and core preference from
    /// it (user core: active definition, else the retired file, else skipped), then calls `complete_switch`.
    /// Called after `recover_tidy`, **before** `pending_tidy_identities`, which yields nothing for such a journal
    /// until `F` is set.
    pub fn pending_switch(
        &self,
        options: &TidyOptions,
    ) -> Result<Option<PendingSwitch>, LibraryError> {
        for (listed, content) in journal::load_all(options.journal_dir())? {
            let Content::Journal(j) = content else {
                continue;
            };
            if let (Some(direction), Some(h)) = (j.end_due(), &j.switch) {
                return Ok(Some(PendingSwitch {
                    journal: listed.path,
                    switch: h.switch(),
                    retired_name: h.retired_name.clone(),
                    choices: SwitchChoices {
                        keys: h.keys,
                        prior_preference: h.prior_preference.clone(),
                        prior_profile: h.prior_profile.clone(),
                    },
                    direction,
                }));
            }
        }
        Ok(None)
    }

    /// `reassign_system`, then retire (Forward) or restore (Undo) the definition by its recorded name — link then
    /// remove (exclusive copy without links), never overwriting, exact names, `.retired/` created if missing, both
    /// folders synced before `F` —, then `F` (ADR 0023, amendment of 2026-11-27). Idempotent; under the tidy lock;
    /// the journal's root is not compared with `options`. On error the journal stays « end due »: `apply_tidy`,
    /// `apply_switch`, `undo_tidy` and `mark_tidy_identities` answer `TidyRefused { reason:
    /// TidyRefusal::SwitchPending, .. }` for **any** root until it succeeds (retried at each start).
    pub fn complete_switch(
        &mut self,
        options: &TidyOptions,
        pending: &PendingSwitch,
        systems_dir: &Path,
    ) -> Result<(), LibraryError> {
        self.complete_switch_with(&RealSys, options, pending, systems_dir)
    }

    /// [`Library::complete_switch`] through `sys` (fault injection in the tests).
    pub(crate) fn complete_switch_with(
        &mut self,
        sys: &dyn Sys,
        options: &TidyOptions,
        pending: &PendingSwitch,
        systems_dir: &Path,
    ) -> Result<(), LibraryError> {
        let _lock = exec::lock(options.journal_dir())?;
        let journal = journal::read(&pending.journal)?
            .ok_or_else(|| refused(&pending.journal, TidyRefusal::UnknownJournal))?;
        let header = journal
            .switch
            .as_ref()
            .filter(|h| h.switch() == pending.switch && h.retired_name == pending.retired_name)
            .ok_or_else(|| {
                io_at(
                    &pending.journal,
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "ce journal n'est pas celui de la bascule annoncée",
                    ),
                )
            })?;
        match journal.end_due() {
            Some(d) if d == pending.direction => {}
            Some(_) => {
                return Err(io_at(
                    &pending.journal,
                    io::Error::new(io::ErrorKind::InvalidInput, "sens de bascule différent"),
                ));
            }
            None => {
                // Already finished (an earlier call, or a retry after a lost answer): nothing to do.
                let done = match pending.direction {
                    SwitchDirection::Forward => journal.forward_final,
                    SwitchDirection::Undo => journal.undo_final,
                };
                return if done {
                    Ok(())
                } else {
                    Err(refused(&pending.journal, TidyRefusal::NotClosed))
                };
            }
        }
        let (custom_id, official_id) = (header.custom_id.as_str(), header.official_id.as_str());
        let active = systems_dir.join(format!("{custom_id}.toml"));
        let kept_dir = retired_dir(systems_dir);
        let kept = kept_dir.join(&header.retired_name);
        let (from, to, to_dir) = match pending.direction {
            SwitchDirection::Forward => (active, kept, kept_dir.clone()),
            SwitchDirection::Undo => (kept, active, systems_dir.to_path_buf()),
        };
        // A copy cut by a crash leaves a temporary named after the id: never a definition, removed first.
        let orphan = to_dir.join(format!(".{custom_id}.emu-rust-tmp"));
        match sys.remove_file(&orphan) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_at(&orphan, e)),
        }
        // (1) The rows. Forward: the ones the official system accepts; the others keep the retired id, frozen.
        match pending.direction {
            SwitchDirection::Forward => {
                let official = Catalog::official()
                    .system(official_id)
                    .cloned()
                    .ok_or_else(|| LibraryError::UnknownSystem(official_id.to_owned()))?;
                self.reassign_system(custom_id, official_id, ReassignScope::AcceptedBy(&official))?;
            }
            SwitchDirection::Undo => {
                self.reassign_system(official_id, custom_id, ReassignScope::All)?;
            }
        }
        let injected = |e| io_at(&pending.journal, e);
        sys.checkpoint().map_err(injected)?;
        // (2) The definition.
        let present = |p: &Path| custom::exact_entry(p).map_err(|e| io_at(p, e));
        match (pending.direction, present(&from)?, present(&to)?) {
            (SwitchDirection::Forward, false, false) => {
                // An active definition that is gone counts as already retired.
                tracing::info!("switch of {custom_id}: its active definition is already absent");
            }
            (SwitchDirection::Undo, false, false) => {
                return Err(io_at(
                    &to,
                    io::Error::new(io::ErrorKind::NotFound, "fichier retiré introuvable"),
                ));
            }
            _ => {
                if pending.direction == SwitchDirection::Forward {
                    std::fs::create_dir_all(&kept_dir).map_err(|e| io_at(&kept_dir, e))?;
                }
                custom::move_no_overwrite_with(&from, &to, &|a, b| sys.hard_link(a, b), &|p| {
                    sys.remove_file(p)
                })?;
            }
        }
        sys.checkpoint().map_err(injected)?;
        for dir in [&kept_dir, &systems_dir.to_path_buf()] {
            match sys.sync_dir(dir) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_at(dir, e)),
            }
        }
        // (3) `F`.
        JournalWriter::append_to(sys, &pending.journal)?.append(sys, &[Op::Final])
    }

    /// Undo of a switch journal: `Err(TidyRefused { reason: TidyRefusal::SwitchNotReversible, .. })` before any
    /// action when a unit cannot return (a file changed, a place taken, a path under a reserved folder) or, after a
    /// forward `F`, `<systems_dir>/<custom_id>.toml` exists or `<systems_dir>/.retired/<retired name>` is missing
    /// (exact names; ADR 0025 §10.5). `undo_tidy` calls the same check with `TidyOptions::systems_dir` (unknown after
    /// an `F`: refused). Any other journal: only the usual refusals.
    pub fn check_switch_undo(
        &self,
        options: &TidyOptions,
        journal: &Path,
        systems_dir: &Path,
    ) -> Result<(), LibraryError> {
        self.check_switch_undo_in(catalog(), options, journal, systems_dir)
    }

    /// [`Library::check_switch_undo`] reading the reserved folders from `cat`.
    pub(crate) fn check_switch_undo_in(
        &self,
        cat: &Catalog,
        options: &TidyOptions,
        journal: &Path,
        systems_dir: &Path,
    ) -> Result<(), LibraryError> {
        let read =
            journal::read(journal)?.ok_or_else(|| refused(journal, TidyRefusal::UnknownJournal))?;
        if read.end_due().is_some() {
            return Err(refused(journal, TidyRefusal::SwitchPending));
        }
        if read.root != options.roms_root() {
            return Err(LibraryError::TidyRootMismatch {
                journal: journal.to_path_buf(),
            });
        }
        if read.state.in_progress() {
            return Err(refused(journal, TidyRefusal::Interrupted));
        }
        if !matches!(read.state, State::Ended | State::EndedMarked) {
            return Err(refused(journal, TidyRefusal::AlreadyUndone));
        }
        let reserved = exec::undo_reserved(&self.conn, cat, read.switch.as_ref())?;
        exec::switch_undo_check(options, &reserved, journal, &read, Some(systems_dir))
    }
}
