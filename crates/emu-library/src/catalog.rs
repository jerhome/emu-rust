//! Dynamic system catalog: official systems plus user definitions (design 8.0 §2.1, ADR 0025 §1).
//! Contract stubs (PX0): the bodies belong to lot PB1 (and PB4 for the private `database`).
#![allow(unused_variables)]

use std::path::{Path, PathBuf};

use crate::custom::{CustomSystem, DefinitionError};
use crate::systems::SystemInfo;

/// Officials active in the order of `systems.toml`, then custom systems sorted by id. `PartialEq`: a publication
/// is skipped when the catalog equals the current one.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    systems: Vec<SystemInfo>,
    customs: Vec<CustomSystem>,
    masked: Vec<SystemInfo>,
    reserved: Vec<String>,
}

impl Catalog {
    pub fn official() -> Catalog {
        todo!("lot PB1")
    }

    /// Official systems plus `<user_dir>/*.toml` (≤ 256 regular files); never fails, problems are reported.
    pub fn load(user_dir: &Path) -> CatalogLoad {
        todo!("lot PB1")
    }

    /// Tests: arbitrary official catalog text (synthetic official system for the switch, ADR 0025 §10).
    pub fn load_with_official(official_toml: &str, user_dir: &Path) -> Result<CatalogLoad, String> {
        todo!("lot PB1")
    }

    pub fn systems(&self) -> &[SystemInfo] {
        &self.systems
    }

    pub fn system(&self, id: &str) -> Option<&SystemInfo> {
        todo!("lot PB1")
    }

    pub fn for_extension(&self, extension: &str) -> Vec<&SystemInfo> {
        todo!("lot PB1")
    }

    pub fn custom(&self, id: &str) -> Option<&CustomSystem> {
        todo!("lot PB1")
    }

    pub fn customs(&self) -> impl Iterator<Item = &CustomSystem> {
        self.customs.iter()
    }

    /// Official systems hidden by an active custom definition (ADR 0025 §10).
    pub fn masked_official(&self) -> &[SystemInfo] {
        &self.masked
    }

    /// Ids whose `roms/<id>/` the tidy neither takes from nor puts into (ADR 0025 §12): (retired definitions —
    /// names of `.retired/`, ≤ 1 024 read —, definitions in error, masked officials) minus the active ids of this
    /// catalog. The tidy adds the ids of `game.system` absent from the catalog; `plan_switch` exempts its source.
    pub fn reserved_folders(&self) -> &[String] {
        &self.reserved
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogLoad {
    pub catalog: Catalog,
    pub problems: Vec<DefinitionProblem>,
    pub pending_switches: Vec<SwitchProposal>,
}

/// `id` = file stem, always known (rows of an invalid definition are never touched by the scan anyway).
#[derive(Debug, Clone, PartialEq)]
pub struct DefinitionProblem {
    pub file: PathBuf,
    pub id: String,
    pub error: DefinitionError,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchProposal {
    pub custom_id: String,
    pub official: SystemInfo,
    pub kind: SwitchKind,
}

/// `InPlace`: same id and database. `NeedsMove`: same database, other id (switch through the tidy, ADR 0025
/// §10). `Uncertain`: same id, other or no database (asks « same machine? »).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SwitchKind {
    InPlace,
    NeedsMove,
    Uncertain,
}

/// The published catalog (ADR 0025 §1); the embedded official one until `install_catalog` is called.
pub fn catalog() -> &'static Catalog {
    todo!("lot PB1")
}

/// Publishes `catalog` for `systems()`, `system()` and `systems_for_extension()`; returns it.
pub fn install_catalog(catalog: Catalog) -> &'static Catalog {
    todo!("lot PB1")
}
