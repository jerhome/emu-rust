//! Dynamic system catalog: official systems plus user definitions (design 8.0 §2.1, ADR 0025 §1): an immutable
//! snapshot, read from the definitions folder by `Catalog::load`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use crate::custom::{
    self, Context, CustomSystem, DefinitionError, MAX_DEFINITION_FILES, MAX_RETIRED_READ,
};

/// Names of `*.toml` kept from the definitions folder before the limit of [`MAX_DEFINITION_FILES`]: the extra ones
/// are reported (and their folders reserved), so a folder the user filled with a million files is not walked to
/// the end of what is kept.
const MAX_DEFINITION_LISTING: usize = MAX_RETIRED_READ * 4;
use crate::systems::SystemInfo;

/// Officials active in the order of `systems.toml`, then custom systems sorted by id. `PartialEq`: a publication
/// is skipped when the catalog equals the current one.
#[derive(Debug, Clone)]
pub struct Catalog {
    systems: Vec<SystemInfo>,
    customs: Vec<CustomSystem>,
    masked: Vec<SystemInfo>,
    reserved: Vec<String>,
    conflicted: Vec<String>,
}

impl PartialEq for Catalog {
    fn eq(&self, other: &Self) -> bool {
        self.systems == other.systems
            && self.customs == other.customs
            && self.masked == other.masked
            && self.reserved == other.reserved
            && self.conflicted == other.conflicted
    }
}

impl Catalog {
    pub fn official() -> Catalog {
        Catalog {
            systems: Catalog::official_systems(),
            customs: Vec::new(),
            masked: Vec::new(),
            reserved: Vec::new(),
            conflicted: Vec::new(),
        }
    }

    /// Official systems plus `<user_dir>/*.toml` (≤ 256 regular files, in name order); never fails, problems are
    /// reported. A missing or unreadable folder is an empty one.
    pub fn load(user_dir: &Path) -> CatalogLoad {
        Catalog::build(Catalog::official_systems(), user_dir, None)
    }

    /// The catalog « after » of a switch with move (`plan_switch`'s `after`): [`Catalog::load`] as if
    /// `<user_dir>/<custom_id>.toml` (exact name) were not there; the file is not touched.
    pub fn load_without(user_dir: &Path, custom_id: &str) -> CatalogLoad {
        Catalog::build(Catalog::official_systems(), user_dir, Some(custom_id))
    }

    /// Tests: arbitrary official catalog text (synthetic official system for the switch, ADR 0025 §10).
    pub fn load_with_official(official_toml: &str, user_dir: &Path) -> Result<CatalogLoad, String> {
        Ok(Catalog::build(
            crate::systems::parse_catalog(official_toml)?,
            user_dir,
            None,
        ))
    }

    fn official_systems() -> Vec<SystemInfo> {
        crate::systems::parse_catalog(crate::systems::CATALOG).unwrap_or_else(|e| {
            // Unreachable in a tested build; an empty catalog keeps the app usable rather than aborting.
            tracing::error!("embedded systems.toml is invalid: {e}");
            Vec::new()
        })
    }

    fn build(officials: Vec<SystemInfo>, user_dir: &Path, skip: Option<&str>) -> CatalogLoad {
        let mut problems = Vec::new();
        let officials_ids: BTreeSet<String> = officials.iter().map(|o| o.id.clone()).collect();
        let mut ctx = Context::officials(officials.iter());
        let mut loaded: Vec<(SystemInfo, CustomSystem)> = Vec::new();
        let (files, ignored) = definition_files(user_dir, skip);
        problems.extend(ignored.into_iter().map(|(id, file)| DefinitionProblem {
            file,
            id,
            error: DefinitionError::TooManyDefinitions,
        }));
        for (stem, path) in files {
            let loaded_draft = custom::load_definition(&path, &stem, &ctx).and_then(|draft| {
                // An official id and the database of another official system: two masked officials for one
                // definition, never accepted (the other official would vanish without a word).
                let by_id = officials.iter().find(|o| o.id == draft.id);
                let by_database = draft
                    .database
                    .as_deref()
                    .and_then(|d| officials.iter().find(|o| o.libretro_name() == Some(d)));
                match (by_id, by_database) {
                    (Some(a), Some(b)) if a.id != b.id => Err(DefinitionError::DuplicateDatabase),
                    _ => Ok(draft),
                }
            });
            match loaded_draft {
                Ok(draft) => {
                    let system = custom::build_system(&draft);
                    ctx.add_custom(&system);
                    loaded.push((system, custom::build_custom(&draft, &path)));
                }
                Err(error) => problems.push(DefinitionProblem {
                    file: path,
                    id: stem,
                    error,
                }),
            }
        }
        loaded.sort_by(|a, b| a.0.id.cmp(&b.0.id));

        // A valid definition always stays active; the officials it conflicts with (same id or same database) are
        // masked (ADR 0025 §10).
        let mut masked_ids: BTreeSet<String> = BTreeSet::new();
        let mut pending_switches = Vec::new();
        for (system, _) in &loaded {
            let by_id = officials.iter().find(|o| o.id == system.id);
            let by_database = system
                .libretro_name()
                .and_then(|d| officials.iter().find(|o| o.libretro_name() == Some(d)));
            masked_ids.extend(by_id.iter().chain(by_database.iter()).map(|o| o.id.clone()));
            let (official, kind) = match (by_id, by_database) {
                (Some(o), _) if o.libretro_name() == system.libretro_name() => {
                    (o, SwitchKind::InPlace)
                }
                (Some(o), _) => (o, SwitchKind::Uncertain),
                (None, Some(o)) => (o, SwitchKind::NeedsMove),
                (None, None) => continue,
            };
            pending_switches.push(SwitchProposal {
                custom_id: system.id.clone(),
                official: official.clone(),
                kind,
            });
        }
        let (masked, active): (Vec<SystemInfo>, Vec<SystemInfo>) = officials
            .into_iter()
            .partition(|o| masked_ids.contains(&o.id));
        let mut systems = active;
        systems.extend(loaded.iter().map(|(s, _)| s.clone()));

        // Folders the tidy leaves alone: retired definitions, definitions in error, masked officials, minus the
        // ids that are active (ADR 0025 §12).
        let retired = custom::retired_customs(user_dir);
        let mut reserved: BTreeSet<String> = retired.iter().map(|r| r.id.clone()).collect();
        reserved.extend(problems.iter().map(|p| p.id.clone()));
        reserved.extend(masked.iter().map(|s| s.id.clone()));
        for s in &systems {
            reserved.remove(&s.id);
        }
        // Ids whose rows belong to two catalogs at once: an official masked by a definition of the same id, or an
        // official id whose definition is in error. The scan freezes the rows the active system does not accept.
        // (c) An active official whose `.retired/` keeps a definition `Replaced` by it (a switch accepted in place or
        // uncertain): the rows the definition alone accepted are frozen, not lost (ADR 0025 §10).
        let replaced = retired
            .iter()
            .filter(|r| r.why == custom::Retirement::Replaced)
            .map(|r| r.id.clone())
            .filter(|id| systems.iter().any(|s| !s.is_custom() && s.id == *id));
        let conflicted: BTreeSet<String> = masked
            .iter()
            .map(|o| o.id.clone())
            .filter(|id| systems.iter().any(|s| s.is_custom() && s.id == *id))
            .chain(problems.iter().map(|p| p.id.clone()))
            .filter(|id| officials_ids.contains(id))
            .chain(replaced)
            .collect();
        CatalogLoad {
            catalog: Catalog {
                systems,
                customs: loaded.into_iter().map(|(_, c)| c).collect(),
                masked,
                reserved: reserved.into_iter().collect(),
                conflicted: conflicted.into_iter().collect(),
            },
            problems,
            pending_switches,
        }
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

    /// Official ids whose games may sit under another definition of the system: an official masked by a definition
    /// of the same id, an official id whose definition is in error, or an active official whose `.retired/` keeps a
    /// `Replaced` definition of that id. The scan neither purges nor reclassifies
    /// the rows of such an id that the active system does not accept (ADR 0025 §13).
    pub fn is_conflicted(&self, id: &str) -> bool {
        self.conflicted.iter().any(|c| c == id)
    }
}

/// `(stem, path)` of a definition file.
type DefinitionFile = (String, PathBuf);

/// The `*.toml` entries of `dir` in name order: the first [`MAX_DEFINITION_FILES`] to read, and the
/// others (reported as problems, at most [`MAX_DEFINITION_LISTING`] kept: the smallest names, whatever the order the
/// file system lists them in). Directories and links are listed too: reading them reports `NotRegularFile`.
fn definition_files(dir: &Path, skip: Option<&str>) -> (Vec<DefinitionFile>, Vec<DefinitionFile>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    "cannot read the custom systems folder {}: {e}",
                    dir.display()
                );
            }
            return (Vec::new(), Vec::new());
        }
    };
    // Sorted set cut at its largest name: bounded memory, and the same files kept for any listing order.
    let mut names: BTreeSet<DefinitionFile> = BTreeSet::new();
    for e in entries.flatten() {
        let Ok(name) = e.file_name().into_string() else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".toml").filter(|s| !s.is_empty()) else {
            continue;
        };
        if skip == Some(stem) {
            continue;
        }
        names.insert((stem.to_owned(), e.path()));
        if names.len() > MAX_DEFINITION_LISTING {
            names.pop_last();
        }
    }
    let mut files: Vec<DefinitionFile> = names.into_iter().collect();
    let ignored = if files.len() > MAX_DEFINITION_FILES {
        tracing::warn!(
            "{} definitions in {}: only the first {MAX_DEFINITION_FILES} are read",
            files.len(),
            dir.display()
        );
        files.split_off(MAX_DEFINITION_FILES)
    } else {
        Vec::new()
    };
    (files, ignored)
}

/// Number of `*.toml` entries of `dir` that count against [`MAX_DEFINITION_FILES`].
pub(crate) fn definition_count(dir: &Path) -> usize {
    let (files, ignored) = definition_files(dir, None);
    files.len() + ignored.len()
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
    use crate::custom::{MAX_DEFINITION_FILES, MAX_DEFINITION_SIZE};

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

    fn def(id: &str, database: Option<&str>, ext: &str) -> String {
        let database = database.map_or(String::new(), |d| format!("database = \"{d}\"\n"));
        format!(
            "format = 1\nrender = \"software\"\n\n[system]\nid = \"{id}\"\nname = \"Sys {id}\"\n{database}\
             extensions = [\"{ext}\"]\ndat = []\n\n[cores]\nlist = [\"demo_core\"]\nignored_firmware = []\n\n\
             [origin]\ncore = \"demo_core\"\ncreated_by = \"8.0.0\"\ncreated_unix = 1\n"
        )
    }

    const SNES_DATABASE: &str = "Nintendo - Super Nintendo Entertainment System";

    fn put(dir: &Path, file: &str, text: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(file), text).unwrap();
    }

    fn ids(c: &Catalog) -> Vec<String> {
        c.systems().iter().map(|s| s.id.clone()).collect()
    }

    #[test]
    fn load_without_ignores_one_definition_by_its_exact_name_and_touches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        put(
            tmp.path(),
            "mysnes.toml",
            &def("mysnes", Some(SNES_DATABASE), "xyz"),
        );
        put(tmp.path(), "other.toml", &def("other", None, "oth"));
        let with = Catalog::load(tmp.path()).catalog;
        assert!(with.system("mysnes").is_some() && with.masked_official().len() == 1);
        let without = Catalog::load_without(tmp.path(), "mysnes");
        assert!(without.problems.is_empty() && without.pending_switches.is_empty());
        let c = without.catalog;
        assert!(c.system("mysnes").is_none() && c.system("snes").is_some());
        assert!(c.masked_official().is_empty());
        assert!(c.custom("other").is_some());
        // The file is still there; another id, a different case or a missing id change nothing.
        assert!(tmp.path().join("mysnes.toml").is_file());
        for id in ["nothing", "MYSNES", "mysnes.toml"] {
            assert_eq!(Catalog::load_without(tmp.path(), id).catalog, with, "{id}");
        }
    }

    #[test]
    fn load_without_skips_the_definition_before_the_limit_of_files() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..=MAX_DEFINITION_FILES {
            put(tmp.path(), &format!("a{i:04}.toml"), "x");
        }
        // One file too many: reported. Without the first of them, the last one fits.
        let with = Catalog::load(tmp.path());
        assert_eq!(
            with.problems
                .iter()
                .filter(|p| p.error == DefinitionError::TooManyDefinitions)
                .count(),
            1
        );
        let without = Catalog::load_without(tmp.path(), "a0000");
        assert!(
            !without
                .problems
                .iter()
                .any(|p| p.error == DefinitionError::TooManyDefinitions)
        );
    }

    #[test]
    fn a_missing_directory_or_a_plain_file_gives_the_official_catalog() {
        let tmp = tempfile::tempdir().unwrap();
        for dir in [tmp.path().join("nowhere"), {
            let f = tmp.path().join("file");
            std::fs::write(&f, "x").unwrap();
            f
        }] {
            let loaded = Catalog::load(&dir);
            assert_eq!(loaded.catalog, Catalog::official());
            assert!(loaded.problems.is_empty() && loaded.pending_switches.is_empty());
        }
    }

    #[test]
    fn customs_follow_the_officials_sorted_by_id() {
        let tmp = tempfile::tempdir().unwrap();
        put(tmp.path(), "zeta.toml", &def("zeta", None, "zz1"));
        put(
            tmp.path(),
            "alpha.toml",
            &def("alpha", Some("Demo - Alpha"), "aa1"),
        );
        let loaded = Catalog::load(tmp.path());
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        let ids = ids(&loaded.catalog);
        let officials = Catalog::official().systems().len();
        assert_eq!(&ids[..officials], &ids_of_official()[..]);
        assert_eq!(&ids[officials..], ["alpha", "zeta"]);
        assert_eq!(
            loaded
                .catalog
                .customs()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert!(loaded.catalog.masked_official().is_empty());
        assert!(loaded.catalog.reserved_folders().is_empty());
        // Deterministic: the same folder gives an equal catalog (a publication is then skipped).
        assert_eq!(Catalog::load(tmp.path()).catalog, loaded.catalog);
    }

    fn ids_of_official() -> Vec<String> {
        ids(&Catalog::official())
    }

    #[test]
    fn only_regular_toml_files_are_definitions() {
        let tmp = tempfile::tempdir().unwrap();
        put(tmp.path(), "README.txt", "notes");
        put(tmp.path(), "good.toml", &def("good", None, "gd1"));
        put(
            &tmp.path().join(".retired"),
            "gone-1.toml",
            &def("gone", None, "gn1"),
        );
        std::fs::create_dir(tmp.path().join("dir.toml")).unwrap();
        let loaded = Catalog::load(tmp.path());
        let problems: Vec<(String, DefinitionError)> = loaded
            .problems
            .iter()
            .map(|p| (p.id.clone(), p.error.clone()))
            .collect();
        assert_eq!(
            problems,
            [("dir".to_owned(), DefinitionError::NotRegularFile)]
        );
        assert_eq!(loaded.problems[0].file, tmp.path().join("dir.toml"));
        assert_eq!(
            loaded
                .catalog
                .customs()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            ["good"]
        );
    }

    #[test]
    fn the_size_limit_is_64_kio() {
        let tmp = tempfile::tempdir().unwrap();
        let base = def("fits", None, "ft1");
        let pad = |n: usize| format!("{base}#{}\n", "x".repeat(n - base.len() - 2));
        put(tmp.path(), "fits.toml", &pad(MAX_DEFINITION_SIZE as usize));
        let mut over = def("over", None, "ov1");
        let n = MAX_DEFINITION_SIZE as usize + 1 - over.len() - 2;
        over = format!("{over}#{}\n", "x".repeat(n));
        assert_eq!(over.len() as u64, MAX_DEFINITION_SIZE + 1);
        put(tmp.path(), "over.toml", &over);
        let loaded = Catalog::load(tmp.path());
        assert!(loaded.catalog.custom("fits").is_some());
        assert_eq!(loaded.problems.len(), 1);
        assert_eq!(loaded.problems[0].id, "over");
        assert_eq!(loaded.problems[0].error, DefinitionError::TooLarge);
    }

    #[test]
    fn at_most_256_definitions_are_read_in_name_order() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..300 {
            let id = format!("s{i:03}");
            put(
                tmp.path(),
                &format!("{id}.toml"),
                &def(&id, Some(&format!("Demo - N{i:03}")), "ex1"),
            );
        }
        let loaded = Catalog::load(tmp.path());
        assert_eq!(loaded.catalog.customs().count(), MAX_DEFINITION_FILES);
        assert!(loaded.catalog.custom("s255").is_some());
        assert!(loaded.catalog.custom("s256").is_none());
        // Each file beyond the limit is reported, and its folder is left alone by the tidy.
        assert_eq!(loaded.problems.len(), 300 - MAX_DEFINITION_FILES);
        assert!(
            loaded
                .problems
                .iter()
                .all(|p| p.error == DefinitionError::TooManyDefinitions)
        );
        assert_eq!(loaded.problems[0].id, "s256");
        assert_eq!(loaded.problems[0].file, tmp.path().join("s256.toml"));
        let reserved = loaded.catalog.reserved_folders();
        assert_eq!(reserved.len(), 300 - MAX_DEFINITION_FILES);
        assert!(reserved.iter().any(|r| r == "s256") && reserved.iter().any(|r| r == "s299"));
    }

    #[test]
    fn the_names_kept_past_the_limit_do_not_depend_on_the_listing_order() {
        let tmp = tempfile::tempdir().unwrap();
        for i in (0..MAX_DEFINITION_LISTING + 10).rev() {
            std::fs::write(tmp.path().join(format!("x{i:05}.toml")), "x").unwrap();
        }
        let (files, ignored) = definition_files(tmp.path(), None);
        assert_eq!(files.len(), MAX_DEFINITION_FILES);
        assert_eq!(files.len() + ignored.len(), MAX_DEFINITION_LISTING);
        assert_eq!(files[0].0, "x00000");
        assert_eq!(
            ignored.last().map(|i| i.0.as_str()),
            Some(format!("x{:05}", MAX_DEFINITION_LISTING - 1).as_str())
        );
    }

    #[test]
    fn an_official_id_with_the_database_of_another_official_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        // id of snes, database of the Mega Drive: two officials would be masked by one file.
        put(
            tmp.path(),
            "snes.toml",
            &def("snes", Some("Sega - Mega Drive - Genesis"), "sf1"),
        );
        let loaded = Catalog::load(tmp.path());
        assert_eq!(loaded.problems.len(), 1);
        assert_eq!(loaded.problems[0].error, DefinitionError::DuplicateDatabase);
        assert!(loaded.pending_switches.is_empty());
        assert!(loaded.catalog.masked_official().is_empty());
        assert!(!loaded.catalog.system("snes").unwrap().is_custom());
        assert!(loaded.catalog.system("megadrive").is_some());
        // The id of a definition in error that is an official one is in conflict; so is a masked official.
        assert!(loaded.catalog.is_conflicted("snes"));
        assert!(!loaded.catalog.is_conflicted("megadrive"));
    }

    #[test]
    fn conflicted_ids_are_the_officials_sharing_their_id_with_a_definition() {
        let tmp = tempfile::tempdir().unwrap();
        put(
            tmp.path(),
            "nes.toml",
            &def("nes", Some("Demo - Other"), "ne1"),
        );
        put(
            tmp.path(),
            "mymd.toml",
            &def("mymd", Some("Sega - Mega Drive - Genesis"), "md1"),
        );
        put(tmp.path(), "gb.toml", "not toml [");
        let loaded = Catalog::load(tmp.path());
        let c = &loaded.catalog;
        assert!(c.is_conflicted("nes") && c.is_conflicted("gb"));
        // Masked through its database only: its rows are those of an absent system, not a conflict.
        assert!(!c.is_conflicted("megadrive") && !c.is_conflicted("mymd"));
        assert!(!c.is_conflicted("snes"));
    }

    #[test]
    fn an_active_official_with_a_replaced_definition_of_its_id_is_conflicted() {
        let tmp = tempfile::tempdir().unwrap();
        let retired = tmp.path().join(".retired");
        let text = def("snes", Some("Demo - Other"), "ne1");
        put(&retired, "snes-100.replaced.toml", &text);
        put(
            &retired,
            "gb-100.toml",
            &def("gb", Some("Demo - Other"), "ne1"),
        );
        put(
            &retired,
            "mine-100.replaced.toml",
            &def("mine", None, "mn1"),
        );
        let c = Catalog::load(tmp.path()).catalog;
        // (c): Replaced only, official id only; a Removed one or a non-official id does not count.
        assert!(c.is_conflicted("snes"));
        assert!(!c.is_conflicted("gb") && !c.is_conflicted("mine"));
        // A definition active again under that id is the ordinary masked case, still conflicted.
        put(tmp.path(), "snes.toml", &text);
        assert!(Catalog::load(tmp.path()).catalog.is_conflicted("snes"));
        // The folder `roms/snes/` stays an active system folder, not a reserved one.
        assert!(!c.reserved_folders().contains(&"snes".to_owned()));
    }

    #[test]
    fn the_first_file_by_name_owns_a_shared_database() {
        let tmp = tempfile::tempdir().unwrap();
        put(
            tmp.path(),
            "second.toml",
            &def("second", Some("Demo - Same"), "ex1"),
        );
        put(
            tmp.path(),
            "first.toml",
            &def("first", Some("Demo - Same"), "ex1"),
        );
        let loaded = Catalog::load(tmp.path());
        assert!(loaded.catalog.custom("first").is_some());
        assert!(loaded.catalog.custom("second").is_none());
        assert_eq!(loaded.problems.len(), 1);
        assert_eq!(
            (loaded.problems[0].id.as_str(), &loaded.problems[0].error),
            ("second", &DefinitionError::DuplicateDatabase)
        );
        // The folder of a definition in error is reserved: its games stay where they are.
        assert_eq!(loaded.catalog.reserved_folders(), ["second"]);
    }

    #[test]
    fn a_system_masks_the_official_it_conflicts_with() {
        let tmp = tempfile::tempdir().unwrap();
        // Same id, same database: in place.
        put(
            tmp.path(),
            "snes.toml",
            &def(
                "snes",
                Some("Nintendo - Super Nintendo Entertainment System"),
                "sf1",
            ),
        );
        // Same database, other id: needs a move.
        put(
            tmp.path(),
            "mymd.toml",
            &def("mymd", Some("Sega - Mega Drive - Genesis"), "md1"),
        );
        // Same id, other database: uncertain.
        put(
            tmp.path(),
            "nes.toml",
            &def("nes", Some("Demo - Another Machine"), "ne1"),
        );
        // Same id, no database: uncertain.
        put(tmp.path(), "gb.toml", &def("gb", None, "gb1"));
        let loaded = Catalog::load(tmp.path());
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        let kinds: Vec<(&str, &str, SwitchKind)> = loaded
            .pending_switches
            .iter()
            .map(|p| (p.custom_id.as_str(), p.official.id.as_str(), p.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("gb", "gb", SwitchKind::Uncertain),
                ("mymd", "megadrive", SwitchKind::NeedsMove),
                ("nes", "nes", SwitchKind::Uncertain),
                ("snes", "snes", SwitchKind::InPlace),
            ]
        );
        assert!(
            loaded
                .pending_switches
                .iter()
                .all(|p| !p.official.is_custom())
        );
        // The definitions stay active; the officials are hidden (masked), not removed from the data.
        let masked: Vec<&str> = loaded
            .catalog
            .masked_official()
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(masked, ["megadrive", "snes", "nes", "gb"]);
        for id in ["snes", "nes", "gb"] {
            assert!(loaded.catalog.system(id).unwrap().is_custom(), "{id}");
        }
        assert!(loaded.catalog.system("megadrive").is_none());
        assert!(loaded.catalog.system("mymd").is_some());
        assert_eq!(
            ids(&loaded.catalog).len(),
            Catalog::official().systems().len()
        );
        // Only the id of a masked official without definition of its id is reserved: the active ids never are.
        assert_eq!(loaded.catalog.reserved_folders(), ["megadrive"]);
    }

    #[test]
    fn the_folders_of_retired_and_broken_systems_are_reserved_unless_the_id_is_active() {
        let tmp = tempfile::tempdir().unwrap();
        let retired = tmp.path().join(".retired");
        put(&retired, "gone-1700000000.toml", &def("gone", None, "gn1"));
        put(
            &retired,
            "old-1700000001.replaced.toml",
            &def("old", None, "ol1"),
        );
        // A retired file of an id that is active again, and of an official id: never reserved.
        put(
            &retired,
            "alive-1700000002.toml",
            &def("alive", None, "al1"),
        );
        put(
            &retired,
            "megadrive-1700000003.replaced.toml",
            &def("megadrive", None, "md1"),
        );
        put(&retired, "notaname.bak", "x");
        put(tmp.path(), "alive.toml", &def("alive", None, "al1"));
        put(tmp.path(), "broken.toml", "not toml [");
        put(tmp.path(), "megadrive.toml", "not toml [");
        let loaded = Catalog::load(tmp.path());
        assert_eq!(loaded.problems.len(), 2);
        assert_eq!(loaded.catalog.reserved_folders(), ["broken", "gone", "old"]);
        assert!(
            !loaded
                .catalog
                .reserved_folders()
                .iter()
                .any(|f| f == "megadrive" || f == "alive")
        );
    }

    #[test]
    fn at_most_1024_retired_names_are_read() {
        let tmp = tempfile::tempdir().unwrap();
        let retired = tmp.path().join(".retired");
        std::fs::create_dir_all(&retired).unwrap();
        for i in 0..MAX_RETIRED_READ + 76 {
            std::fs::write(retired.join(format!("r{i:04}-1.toml")), "x").unwrap();
        }
        let reserved = Catalog::load(tmp.path()).catalog.reserved_folders().len();
        assert_eq!(reserved, MAX_RETIRED_READ);
    }

    #[test]
    fn a_synthetic_official_catalog_drives_the_switch_kinds() {
        let tmp = tempfile::tempdir().unwrap();
        let official = "[[system]]\nid = \"synth\"\nname = \"Synthetic\"\nextensions = [\"syn\"]\n\
                        database = \"Synthetic - Official - Console\"\n";
        put(
            tmp.path(),
            "synth.toml",
            &def("synth", Some("Synthetic - Official - Console"), "sy1"),
        );
        put(
            tmp.path(),
            "other.toml",
            &def("other", Some("Demo - Free"), "ot1"),
        );
        let loaded = Catalog::load_with_official(official, tmp.path()).unwrap();
        assert_eq!(loaded.pending_switches.len(), 1);
        assert_eq!(loaded.pending_switches[0].kind, SwitchKind::InPlace);
        // The custom `synth` replaces the official one (masked); customs are sorted by id.
        assert_eq!(ids(&loaded.catalog), ["other", "synth"]);
        assert!(loaded.catalog.system("synth").unwrap().is_custom());
    }
}
