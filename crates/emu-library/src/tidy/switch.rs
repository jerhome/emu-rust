//! Switch with move: a custom system replaced by an official one through the tidy (design 8.0 §2.1 and §2.6,
//! ADR 0025 §10; journal version 2). Contract stubs: the bodies belong to lot PB3.
#![allow(unused_variables)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::catalog::Catalog;
use crate::db::Library;
use crate::error::LibraryError;
use crate::scan::ScanProgress;

use super::{TidyOptions, TidyPlan, TidyProgress, TidyReport};

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
    /// `false` while a unit is blocked, duplicate or conflict.
    pub fn switchable(&self) -> bool {
        todo!("lot PB3")
    }
}

impl Library {
    /// Preview computed with `after` (definition retired, official active): every unit under
    /// `roms/<custom_id>/` → `roms/<official_id>/`. Read-only, like `plan_tidy`.
    pub fn plan_switch(
        &self,
        options: &TidyOptions,
        after: &Catalog,
        switch: &SwitchMove,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&ScanProgress),
    ) -> Result<TidyPlan, LibraryError> {
        todo!("lot PB3")
    }

    /// Every unit of the plan (no selection). Unit transactions only call `relocate`. Writes journal format 2
    /// with the switch header. Refusals and `Err` cases as `apply_tidy`.
    pub fn apply_switch(
        &mut self,
        options: &TidyOptions,
        plan: &TidyPlan,
        choices: &SwitchChoices,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&TidyProgress),
    ) -> Result<TidyReport, LibraryError> {
        todo!("lot PB3")
    }

    /// `Some` when the newest journal is a switch whose end is due (design 8.0 §2.6); called after
    /// `recover_tidy`, **before** `pending_tidy_identities`.
    pub fn pending_switch(
        &self,
        options: &TidyOptions,
    ) -> Result<Option<PendingSwitch>, LibraryError> {
        todo!("lot PB3")
    }

    /// `reassign_system`, then retire (Forward) or restore (Undo) the definition by its recorded name, then
    /// `F`. Idempotent; under the tidy lock. On error the journal stays « end due »: `apply_tidy`,
    /// `apply_switch` and `undo_tidy` answer `TidyRefusal::SwitchPending` until it succeeds.
    pub fn complete_switch(
        &mut self,
        options: &TidyOptions,
        pending: &PendingSwitch,
        systems_dir: &Path,
    ) -> Result<(), LibraryError> {
        todo!("lot PB3")
    }

    /// Refuses the undo of a switch journal before any action (`TidyRefusal::SwitchNotReversible`). `undo_tidy`
    /// keeps its signature and calls the same check.
    pub fn check_switch_undo(
        &self,
        options: &TidyOptions,
        journal: &Path,
        systems_dir: &Path,
    ) -> Result<(), LibraryError> {
        todo!("lot PB3")
    }
}
