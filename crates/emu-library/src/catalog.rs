//! Dynamic system catalog: official systems plus user definitions (design 8.0 §2.1, ADR 0025 §1).
//! Lot PB1: official systems only for now; user definitions (`Catalog::load`) arrive with lot PC1, the private
//! `database` with lot PB4.
#![allow(unused_variables)]

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

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
        Catalog::from_official(
            crate::systems::parse_catalog(crate::systems::CATALOG).unwrap_or_else(|e| {
                // Unreachable in a tested build; an empty catalog keeps the app usable rather than aborting.
                tracing::error!("embedded systems.toml is invalid: {e}");
                Vec::new()
            }),
        )
    }

    fn from_official(systems: Vec<SystemInfo>) -> Catalog {
        Catalog {
            systems,
            customs: Vec::new(),
            masked: Vec::new(),
            reserved: Vec::new(),
        }
    }

    /// Official systems plus `<user_dir>/*.toml` (≤ 256 regular files); never fails, problems are reported.
    pub fn load(user_dir: &Path) -> CatalogLoad {
        // Definitions are read from `user_dir` by lot PC1; until then the catalog is the official one.
        CatalogLoad {
            catalog: Catalog::official(),
            problems: Vec::new(),
            pending_switches: Vec::new(),
        }
    }

    /// Tests: arbitrary official catalog text (synthetic official system for the switch, ADR 0025 §10).
    pub fn load_with_official(official_toml: &str, user_dir: &Path) -> Result<CatalogLoad, String> {
        Ok(CatalogLoad {
            catalog: Catalog::from_official(crate::systems::parse_catalog(official_toml)?),
            problems: Vec::new(),
            pending_switches: Vec::new(),
        })
    }

    pub fn systems(&self) -> &[SystemInfo] {
        &self.systems
    }

    pub fn system(&self, id: &str) -> Option<&SystemInfo> {
        self.systems.iter().find(|s| s.id == id)
    }

    /// Systems accepting `extension` (case-insensitive, without the dot), in catalog order. Includes the systems
    /// that restrict it to their folder (`SystemInfo::folder_extensions`).
    pub fn for_extension(&self, extension: &str) -> Vec<&SystemInfo> {
        self.systems
            .iter()
            .filter(|s| {
                s.extensions
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(extension))
            })
            .collect()
    }

    pub fn custom(&self, id: &str) -> Option<&CustomSystem> {
        self.customs.iter().find(|c| c.id == id)
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
    let published = PUBLISHED.read().unwrap_or_else(|e| e.into_inner());
    published.unwrap_or_else(embedded)
}

static PUBLISHED: RwLock<Option<&'static Catalog>> = RwLock::new(None);

fn embedded() -> &'static Catalog {
    static EMBEDDED: OnceLock<Catalog> = OnceLock::new();
    EMBEDDED.get_or_init(Catalog::official)
}

/// Publications after which a warning is logged (each one leaks a snapshot of 20 to 50 Kio, ADR 0025 §1).
const PUBLICATION_WARNING: usize = 1_000;

/// Publishes `catalog` for `systems()`, `system()` and `systems_for_extension()`; returns it.
pub fn install_catalog(catalog: Catalog) -> &'static Catalog {
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let mut published = PUBLISHED.write().unwrap_or_else(|e| e.into_inner());
    let current = published.unwrap_or_else(embedded);
    if *current == catalog {
        return current;
    }
    if COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1 == PUBLICATION_WARNING {
        tracing::warn!("{PUBLICATION_WARNING} catalog publications in this process");
    }
    let leaked: &'static Catalog = Box::leak(Box::new(catalog));
    *published = Some(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_catalog_has_the_embedded_systems_in_file_order() {
        let c = Catalog::official();
        let ids: Vec<&str> = c.systems().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids.first(), Some(&"megadrive"));
        assert_eq!(ids.last(), Some(&"3do"));
        assert!(c.customs().next().is_none());
        assert!(c.masked_official().is_empty());
        assert!(c.reserved_folders().is_empty());
    }

    #[test]
    fn lookups_by_id_and_extension() {
        let c = Catalog::official();
        assert_eq!(
            c.system("snes").map(|s| s.name.as_str()),
            Some("Super Nintendo")
        );
        assert!(c.system("nope").is_none());
        assert!(c.custom("snes").is_none());
        let ids: Vec<&str> = c
            .for_extension("GB")
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, ["gb", "gbc"]);
        assert!(c.for_extension("zzz").is_empty());
    }

    #[test]
    fn load_without_definitions_equals_the_official_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = Catalog::load(dir.path());
        assert_eq!(loaded.catalog, Catalog::official());
        assert!(loaded.problems.is_empty());
        assert!(loaded.pending_switches.is_empty());
    }

    #[test]
    fn load_with_official_parses_the_given_text_and_reports_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Catalog::load_with_official("not toml [", dir.path()).is_err());
        let text = "[[system]]\nid = \"toy\"\nname = \"Toy\"\nextensions = [\"toy\"]\ndatabase = \"Toy - Toy\"\ndat = \"no-intro\"\n";
        let loaded = Catalog::load_with_official(text, dir.path()).unwrap();
        assert_eq!(loaded.catalog.systems().len(), 1);
        assert!(loaded.catalog.system("toy").is_some());
    }

    #[test]
    fn the_published_catalog_defaults_to_the_official_one() {
        assert_eq!(*catalog(), Catalog::official());
        assert_eq!(systems_len(), Catalog::official().systems().len());
    }

    fn systems_len() -> usize {
        crate::systems().len()
    }
}
