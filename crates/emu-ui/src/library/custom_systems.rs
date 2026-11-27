//! Custom systems in the library (ADR 0025 §2, §9, §10): the « Systèmes non testés » section of the settings
//! (edit, delete, restore, forget), the problems of the definitions, the publication of the catalog after a change
//! (deferred while a scan or a tidying runs), and what follows the addition of a system.

use std::path::Path;

use egui::{Button, CollapsingHeader, Key, RichText, TextEdit, Ui};
use emu_assets::cores::CoreId;
use emu_assets::jobs::{Job, JobEvent};
use emu_library::{
    Catalog, CatalogLoad, CustomSystemDraft, DefinitionError, DefinitionProblem, ForgetDats,
    ForgetScope, LibraryError, RetiredSystem, Retirement, SwitchKind, SwitchMove, SwitchProposal,
    TidyOptions, custom_games_on_disk, normalize_name, open_switch, restore_custom, retire_custom,
    retired_customs, validate_custom, write_custom,
};

use super::settings::help_anchor;
use super::worker::FoundDat;
use super::{Dialog, Level, LibraryUi, notify, text, weak};
use crate::cores_ui;

/// Files of the games found under `roms/<id>/` that the refusal lists at most.
const GAMES_LISTED: usize = 20;

/// A definition problem line, or the group of files ignored beyond the bound.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct ProblemSummary {
    /// One line per refused definition (« fichier : raison »).
    pub lines: Vec<String>,
    /// File names beyond the bound: one line in the page, the names in a folded list.
    pub too_many: Vec<String>,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub(super) fn summarize_problems(problems: &[DefinitionProblem]) -> ProblemSummary {
    let mut summary = ProblemSummary::default();
    for problem in problems {
        let name = file_name(&problem.file);
        if matches!(problem.error, DefinitionError::TooManyDefinitions) {
            summary.too_many.push(name);
        } else {
            summary
                .lines
                .push(text::definition_problem(&name, &problem.error.to_string()));
        }
    }
    summary
}

/// A retired definition as the page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RetiredRow {
    pub retired: RetiredSystem,
    /// Name of the official system that replaced it (`Replaced` only, read from the retired file).
    pub replaced_by: Option<String>,
    /// « Restaurer »: only a definition the user removed, and only while no definition has its id again.
    pub can_restore: bool,
    /// « Oublier ses jeux »: only for a definition the user removed (a replaced one has no button, ADR 0025 §9),
    /// and not while an active system has this id (an official one: its rows are the official's own, they must
    /// never be forgotten here).
    pub can_forget: bool,
}

pub(super) fn retired_rows(retired: Vec<RetiredSystem>, catalog: &Catalog) -> Vec<RetiredRow> {
    retired
        .into_iter()
        .map(|retired| {
            let replaced_by = match retired.why {
                Retirement::Replaced => replaced_by_name(&retired.file, catalog),
                _ => None,
            };
            RetiredRow {
                can_restore: retired.why == Retirement::Removed
                    && catalog.custom(&retired.id).is_none(),
                can_forget: retired.why == Retirement::Removed
                    && catalog.system(&retired.id).is_none(),
                replaced_by,
                retired,
            }
        })
        .collect()
}

/// The official system that has the `database` of a retired definition (the file is read once, bounded).
fn replaced_by_name(file: &Path, catalog: &Catalog) -> Option<String> {
    use std::io::Read as _;
    let mut raw = String::new();
    std::fs::File::open(file)
        .ok()?
        .take(emu_library::MAX_DEFINITION_SIZE)
        .read_to_string(&mut raw)
        .ok()?;
    let table: toml::Table = raw.parse().ok()?;
    let database = table.get("database")?.as_str()?;
    catalog
        .systems()
        .iter()
        .chain(catalog.masked_official())
        .find(|s| !s.is_custom() && s.libretro_name() == Some(database))
        .map(|s| s.name.clone())
}

/// How a definition is deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DeleteKind {
    /// Rows hidden, definition retired.
    Plain,
    /// Same id as an official system, other database: its rows are the official's. Carries that system's name.
    Uncertain { official: String },
}

pub(super) fn delete_kind(switches: &[SwitchProposal], id: &str) -> DeleteKind {
    switches
        .iter()
        .find(|p| p.custom_id == id && matches!(p.kind, SwitchKind::Uncertain))
        .map_or(DeleteKind::Plain, |p| DeleteKind::Uncertain {
            official: p.official.name.clone(),
        })
}

/// The refusal text when a switch journal blocks `id`, or when the journals cannot be read (nothing is changed
/// then). `None`: the definition may be touched.
pub(super) fn switch_refusal(
    open: Result<Option<SwitchMove>, LibraryError>,
    id: &str,
) -> Option<String> {
    match open {
        Ok(Some(switch)) if switch.custom_id == id => Some(text::custom_switch_open(id)),
        Ok(_) => None,
        Err(e) => Some(text::custom_switch_unreadable(&e.to_string())),
    }
}

/// `Err((files, more))` while games of the definition are still under `roms/<id>/` (relative to the root, sorted):
/// the deletion of an uncertain conflict is refused (ADR 0025 §9). The disk is read, never the rows.
pub(super) fn uncertain_delete_check(
    roms_root: &Path,
    catalog: &Catalog,
    id: &str,
    limit: usize,
) -> Result<(), (Vec<String>, bool)> {
    let found = custom_games_on_disk(roms_root, catalog, id, limit + 1)
        .map_err(|e| (vec![e.to_string()], false))?;
    if found.is_empty() {
        return Ok(());
    }
    let more = found.len() > limit;
    let files = found
        .iter()
        .take(limit)
        .map(|f| f.strip_prefix(roms_root).unwrap_or(f).display().to_string())
        .collect();
    Err((files, more))
}

/// Edited text fields to a definition draft: the name normalized, extensions lower case without dots, numbers
/// empty or positive.
pub(super) fn apply_edit(
    mut draft: CustomSystemDraft,
    name: &str,
    extensions: &str,
    screenscraper: &str,
    igdb: &str,
) -> Result<CustomSystemDraft, String> {
    let number = |raw: &str| -> Result<Option<u32>, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Ok(None);
        }
        raw.parse::<u32>()
            .map(Some)
            .map_err(|_| text::EDIT_NUMBER_INVALID.to_owned())
    };
    draft.screenscraper = number(screenscraper)?;
    draft.igdb = number(igdb)?;
    draft.name = normalize_name(name);
    let mut list: Vec<String> = Vec::new();
    for ext in extensions.split([',', ';', ' ']) {
        let ext = ext.trim().trim_start_matches('.').to_lowercase();
        if !ext.is_empty() && !list.contains(&ext) {
            list.push(ext);
        }
    }
    draft.extensions = list;
    if draft.cores.is_empty() {
        return Err(text::EDIT_NO_CORE.to_owned());
    }
    Ok(draft)
}

/// What the page shows, read from the disk when it opens and after each action (not at every frame).
pub(super) struct View {
    pub(super) load: CatalogLoad,
    pub(super) summary: ProblemSummary,
    pub(super) retired: Vec<RetiredRow>,
}

/// What happens once a due publication of the catalog has gone through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AfterPublish {
    /// Systems were added: their folders, a scan, the notice.
    Added {
        names: Vec<String>,
        ids: Vec<String>,
        /// Folders `roms/<id>/` that were already there (their files are scanned).
        existing: Vec<String>,
    },
    /// The definition now lists the DAT sources of the search: import the files it saved (no request).
    ImportDats {
        id: String,
        database: String,
        found: Vec<FoundDat>,
    },
}

/// Confirmation and information dialogs of the custom systems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Confirm {
    Delete {
        id: String,
        name: String,
    },
    /// Same id as an official system, other database: the deletion forgets the games first.
    DeleteUncertain {
        id: String,
        name: String,
        official: String,
    },
    /// Games remain under `roms/<id>/`: listed, nothing is done.
    Blocked {
        id: String,
        files: Vec<String>,
        more: bool,
    },
    Forget {
        id: String,
    },
    /// A system was added.
    Added {
        names: Vec<String>,
        ids: Vec<String>,
        existing: Vec<String>,
    },
}

enum EditJob {
    /// Extensions read again from the core.
    Recalc(Job<Result<Vec<String>, String>>),
    /// DAT sources found for the database name.
    Search(Job<Result<Vec<FoundDat>, String>>),
}

pub(super) struct Edit {
    draft: CustomSystemDraft,
    pub(super) name: String,
    pub(super) extensions: String,
    screenscraper: String,
    pub(super) igdb: String,
    /// BIOS files the cores announce, plus those already ignored: « ne plus avertir ».
    firmware: Vec<String>,
    pub(super) error: Option<String>,
    focus: bool,
    job: Option<EditJob>,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) view: Option<View>,
    /// A publication of the catalog is due: asked again at every frame until no scan or tidying pins it.
    publish_due: bool,
    after: Vec<AfterPublish>,
    pub(super) edit: Option<Edit>,
}

enum Action {
    Edit(String),
    Delete(String),
    Restore(String),
    Forget(String),
    OpenFolder,
}

fn now_unix() -> u64 {
    u64::try_from(super::now_unix()).unwrap_or(0)
}

impl LibraryUi {
    /// Re-reads the definitions and the retired ones from the disk.
    pub(super) fn refresh_custom_systems(&mut self) {
        let load = Catalog::load(&self.dirs.custom_systems);
        let summary = summarize_problems(&load.problems);
        let retired = retired_rows(retired_customs(&self.dirs.custom_systems), &load.catalog);
        self.custom_systems.view = Some(View {
            load,
            summary,
            retired,
        });
    }

    /// A definition was written, retired or restored (or « Recalculer »): the catalog is published as soon as no
    /// scan or tidying reads it, and `after` runs then.
    pub(super) fn request_catalog_publication(&mut self, after: Option<AfterPublish>) {
        self.custom_systems.publish_due = true;
        self.custom_systems.after.extend(after);
        self.refresh_custom_systems();
    }

    pub(super) fn publication_waits(&self) -> bool {
        self.custom_systems.publish_due
    }

    /// Once per frame: publishes a due catalog when nothing pins it, then refreshes what depends on it.
    pub(super) fn poll_catalog_publication(&mut self, ctx: &egui::Context) {
        if !self.custom_systems.publish_due {
            return;
        }
        // A unit test never publishes the catalog of the process: the cases that do live in `tests/`.
        if cfg!(test) {
            return;
        }
        let Some(load) = crate::try_publish_catalog(&self.dirs) else {
            return;
        };
        self.custom_systems.publish_due = false;
        drop(load);
        self.cores
            .set_preferences(cores_ui::preferences(&self.config.cores));
        self.forget_firmware_banner();
        if let Some(system) = self.filter.system.clone()
            && emu_library::system(&system).is_none()
        {
            self.filter.system = None;
        }
        self.reload_games();
        self.refresh_settings_cache();
        for after in std::mem::take(&mut self.custom_systems.after) {
            self.after_publication(after, ctx);
        }
    }

    fn after_publication(&mut self, after: AfterPublish, ctx: &egui::Context) {
        match after {
            AfterPublish::ImportDats {
                id,
                database,
                found,
            } => self.start_found_dats_import(ctx, id, database, found),
            AfterPublish::Added {
                names,
                ids,
                existing,
            } => {
                let roms = self.dirs.root.join("roms");
                if let Err(e) = emu_library::create_system_folders(&roms) {
                    tracing::warn!("dossiers des systèmes non créés : {e}");
                }
                self.start_scan(ctx);
                self.open_dialog(Dialog::CustomSystems(Confirm::Added {
                    names,
                    ids,
                    existing,
                }));
            }
        }
    }

    /// The files of the search are on the disk: imported at once, or by the next DAT update when one is running.
    fn start_found_dats_import(
        &mut self,
        ctx: &egui::Context,
        id: String,
        database: String,
        found: Vec<FoundDat>,
    ) {
        if self.dat_work_pending() {
            self.set_status(text::SEARCH_IMPORT_LATER.to_owned(), Level::Info);
            return;
        }
        if self.capture || self.db.is_none() {
            return;
        }
        self.dat_progress = None;
        self.dat_download = Some(super::worker::spawn_found_dats_import(
            self.dirs.library_db.clone(),
            self.dirs.database.clone(),
            id,
            database,
            found,
            notify(ctx),
        ));
    }

    /// A DAT update or import is running, or the search / its publication has not ended: a test waits for it.
    pub(super) fn dat_work_pending(&self) -> bool {
        self.dat.is_some()
            || self.dat_download.is_some()
            || self.publication_waits()
            || self
                .custom_systems
                .edit
                .as_ref()
                .is_some_and(|e| e.job.is_some())
    }

    /// Writes the definitions of a system the user added from the cores screen (ADR 0025 §9). The catalog is
    /// published later (`poll_catalog_publication`), then the folders, the scan and the notice follow.
    pub(super) fn add_custom_systems(&mut self, drafts: Vec<CustomSystemDraft>) {
        if self.capture {
            return;
        }
        let catalog = Catalog::load(&self.dirs.custom_systems).catalog;
        // Every id is checked before the first file is written: a refusal leaves nothing half done.
        if !drafts.iter().all(|draft| self.switch_allows(&draft.id)) {
            return;
        }
        let (mut names, mut ids, mut failure) = (Vec::new(), Vec::new(), None);
        for draft in drafts {
            match write_custom(&self.dirs.custom_systems, &draft, &catalog) {
                Ok(_) => {
                    names.push(draft.name);
                    ids.push(draft.id);
                }
                Err(e) => failure = failure.or(Some(e.to_string())),
            }
        }
        if let Some(e) = failure {
            self.set_status(text::add_failed(&e), Level::Warn);
        }
        if !ids.is_empty() {
            let roms = self.dirs.root.join("roms");
            let existing = ids
                .iter()
                .filter(|id| roms.join(id).exists())
                .cloned()
                .collect();
            self.request_catalog_publication(Some(AfterPublish::Added {
                names,
                ids,
                existing,
            }));
        }
    }

    /// `true` when no switch journal blocks the definition `id`; otherwise the refusal is on the status line.
    fn switch_allows(&mut self, id: &str) -> bool {
        // `TidyOptions` needs an existing folder as its root; only the journal folder is read here.
        let open = TidyOptions::new(&self.dirs.root, &self.dirs.tidy_journals)
            .and_then(|options| open_switch(&options));
        match switch_refusal(open, id) {
            None => true,
            Some(message) => {
                self.set_status(message, Level::Warn);
                false
            }
        }
    }

    /// Scan, tidying or a DAT task is writing the base: forgetting games waits for its end.
    fn library_busy(&mut self) -> bool {
        let busy = self.scan.is_some()
            || self.tidy.busy()
            || self.dat.is_some()
            || self.dat_download.is_some();
        if busy {
            self.set_status(text::CUSTOM_BUSY_TASK.to_owned(), Level::Warn);
        }
        busy
    }

    pub(super) fn can_forget_retired(&self, id: &str) -> bool {
        self.custom_systems
            .view
            .as_ref()
            .and_then(|v| v.retired.iter().find(|r| r.retired.id == id))
            .is_some_and(|r| r.can_forget)
    }

    /// For the tests: `(id, can_restore, can_forget)` of every retired definition.
    pub(super) fn retired_buttons(&self) -> Vec<(String, bool, bool)> {
        self.custom_systems
            .view
            .as_ref()
            .map_or_else(Vec::new, |v| {
                v.retired
                    .iter()
                    .map(|r| (r.retired.id.clone(), r.can_restore, r.can_forget))
                    .collect()
            })
    }

    /// For the tests: the problem lines and the files grouped under the « N définitions ignorées » line.
    pub(super) fn problem_lines(&self) -> (Vec<String>, Vec<String>) {
        self.custom_systems
            .view
            .as_ref()
            .map_or_else(Default::default, |v| {
                (v.summary.lines.clone(), v.summary.too_many.clone())
            })
    }

    fn custom_name(&self, id: &str) -> String {
        self.custom_systems
            .view
            .as_ref()
            .and_then(|v| v.load.catalog.system(id))
            .map_or_else(|| id.to_owned(), |s| s.name.clone())
    }

    pub(super) fn ask_delete(&mut self, id: &str) {
        if self.capture || !self.switch_allows(id) {
            return;
        }
        let Some(view) = &self.custom_systems.view else {
            return;
        };
        let name = self.custom_name(id);
        let confirm = match delete_kind(&view.load.pending_switches, id) {
            DeleteKind::Plain => Confirm::Delete {
                id: id.to_owned(),
                name,
            },
            DeleteKind::Uncertain { official } => Confirm::DeleteUncertain {
                id: id.to_owned(),
                name,
                official,
            },
        };
        self.open_dialog(Dialog::CustomSystems(confirm));
    }

    /// Retires the definition (never deletes it): the games stay in the base, hidden.
    fn delete_custom(&mut self, id: &str, name: &str) {
        if !self.switch_allows(id) {
            return;
        }
        match retire_custom(
            &self.dirs.custom_systems,
            id,
            Retirement::Removed,
            now_unix(),
        ) {
            Ok(_) => {
                self.set_status(text::custom_deleted(name), Level::Info);
                self.request_catalog_publication(None);
            }
            Err(e) => self.set_status(text::custom_write_failed(&e.to_string()), Level::Warn),
        }
    }

    /// Uncertain conflict: refused while games remain under `roms/<id>/`; otherwise forgets the rows the
    /// definition claims (the official system's own are kept), then retires it.
    fn delete_uncertain(&mut self, id: &str, name: &str) {
        if self.library_busy() || !self.switch_allows(id) {
            return;
        }
        let Some(view) = &self.custom_systems.view else {
            return;
        };
        let roms = self.dirs.root.join("roms");
        if let Err((files, more)) =
            uncertain_delete_check(&roms, &view.load.catalog, id, GAMES_LISTED)
        {
            self.open_dialog(Dialog::CustomSystems(Confirm::Blocked {
                id: id.to_owned(),
                files,
                more,
            }));
            return;
        }
        let catalog = &view.load.catalog;
        let Some(db) = self.db.as_mut() else {
            self.set_status(text::CUSTOM_NO_LIBRARY.to_owned(), Level::Warn);
            return;
        };
        if let Err(e) = db.forget_system(id, ForgetDats::Keep, ForgetScope::ClaimedBy(catalog)) {
            self.set_status(text::custom_write_failed(&e.to_string()), Level::Warn);
            return;
        }
        self.delete_custom(id, name);
    }

    pub(super) fn restore_custom_system(&mut self, id: &str) {
        if self.capture || !self.switch_allows(id) {
            return;
        }
        let Some(view) = &self.custom_systems.view else {
            return;
        };
        let Some(row) = view
            .retired
            .iter()
            .find(|r| r.retired.id == id && r.can_restore)
        else {
            return;
        };
        match restore_custom(
            &self.dirs.custom_systems,
            &row.retired.file,
            &view.load.catalog,
        ) {
            Ok(_) => {
                self.set_status(text::custom_restored(id), Level::Info);
                self.request_catalog_publication(None);
            }
            Err(e) => self.set_status(text::custom_write_failed(&e.to_string()), Level::Warn),
        }
    }

    fn forget_retired(&mut self, id: &str) {
        if !self.switch_allows(id) || self.library_busy() {
            return;
        }
        if !self.can_forget_retired(id) {
            return;
        }
        let Some(db) = self.db.as_mut() else {
            self.set_status(text::CUSTOM_NO_LIBRARY.to_owned(), Level::Warn);
            return;
        };
        match db.forget_system(id, ForgetDats::Remove, ForgetScope::All) {
            Ok(()) => {
                self.set_status(text::custom_forgotten(id), Level::Info);
                self.reload_games();
                self.refresh_custom_systems();
            }
            Err(e) => self.set_status(text::custom_write_failed(&e.to_string()), Level::Warn),
        }
    }

    pub(super) fn open_edit(&mut self, id: &str) {
        if self.capture || !self.switch_allows(id) {
            return;
        }
        let Some(view) = &self.custom_systems.view else {
            return;
        };
        let Some(draft) = CustomSystemDraft::from_custom(&view.load.catalog, id) else {
            return;
        };
        let mut firmware: Vec<String> = Vec::new();
        if let Some(manager) = self.cores.manager() {
            for core in &draft.cores {
                if let Some(info) = CoreId::new(core).and_then(|c| manager.info(&c)) {
                    for file in &info.firmware {
                        if !firmware.contains(&file.path) {
                            firmware.push(file.path.clone());
                        }
                    }
                }
            }
        }
        for ignored in &draft.ignored_firmware {
            if !firmware.contains(ignored) {
                firmware.push(ignored.clone());
            }
        }
        self.custom_systems.edit = Some(Edit {
            name: draft.name.clone(),
            extensions: draft.extensions.join(", "),
            screenscraper: draft
                .screenscraper
                .map(|n| n.to_string())
                .unwrap_or_default(),
            igdb: draft.igdb.map(|n| n.to_string()).unwrap_or_default(),
            draft,
            firmware,
            error: None,
            focus: true,
            job: None,
        });
    }

    /// « Enregistrer » of the edit form: validated, written, published.
    pub(super) fn save_edit(&mut self, then: Option<AfterPublish>) {
        let Some(edit) = &mut self.custom_systems.edit else {
            return;
        };
        let id = edit.draft.id.clone();
        let built = apply_edit(
            edit.draft.clone(),
            &edit.name,
            &edit.extensions,
            &edit.screenscraper,
            &edit.igdb,
        );
        let draft = match built {
            Ok(draft) => draft,
            Err(message) => {
                edit.error = Some(message);
                return;
            }
        };
        if !self.switch_allows(&id) {
            return;
        }
        let Some(view) = &self.custom_systems.view else {
            return;
        };
        let written = validate_custom(&draft, &view.load.catalog, false)
            .map_err(LibraryError::Definition)
            .and_then(|()| write_custom(&self.dirs.custom_systems, &draft, &view.load.catalog));
        match written {
            Ok(_) => {
                self.set_status(text::custom_saved(&draft.name), Level::Info);
                self.custom_systems.edit = None;
                self.request_catalog_publication(then);
            }
            Err(e) => {
                if let Some(edit) = &mut self.custom_systems.edit {
                    edit.error = Some(text::edit_validation(&e.to_string()));
                }
            }
        }
    }

    // ---- jobs of the edit form ---------------------------------------------------------------------------

    /// « Recalculer depuis le core »: the extensions the installed core reports (a probe in a child process).
    fn start_recalc(&mut self, ctx: &egui::Context) {
        let Some(edit) = &mut self.custom_systems.edit else {
            return;
        };
        let Some(manager) = self.cores.manager().cloned() else {
            edit.error = Some(text::RECALC_NEEDS_CORE.to_owned());
            return;
        };
        let Some(core) = edit.draft.cores.first().and_then(|c| CoreId::new(c)) else {
            return;
        };
        if !manager.installed().iter().any(|c| c.id == core) {
            edit.error = Some(text::RECALC_NEEDS_CORE.to_owned());
            return;
        }
        edit.error = None;
        let cache = self.dirs.cache_dir();
        let system_dir = self.dirs.system.clone();
        edit.job = Some(EditJob::Recalc(Job::spawn(
            "recalc",
            notify(ctx),
            move |_, cancel| {
                let path = manager.verified_path(&core).map_err(|e| e.to_string())?;
                let command =
                    crate::dry_run::DryRunCommand::current_exe().map_err(|e| e.to_string())?;
                let verdict =
                    crate::custom::probe_core(&command, &cache, &system_dir, &path, &|| {
                        cancel.is_cancelled()
                    });
                match crate::custom::probe_outcome(verdict) {
                    Ok(report) => Ok(emu_library::cartridge_extensions(&report.extensions)),
                    Err(crate::custom::ProbeFailure::HardwareRender) => {
                        Err(cores_ui::text::hardware_render(core.as_str()))
                    }
                    Err(
                        crate::custom::ProbeFailure::NotRun(why)
                        | crate::custom::ProbeFailure::Failed(why),
                    ) => Err(why),
                    Err(crate::custom::ProbeFailure::Cancelled) => Err(String::new()),
                }
            },
        )));
    }

    /// « Chercher la base d'identification »: one request per source at most (ADR 0025 §9).
    pub(super) fn start_search(&mut self, ctx: &egui::Context) {
        let Some(edit) = &mut self.custom_systems.edit else {
            return;
        };
        let Some(database) = edit.draft.database.clone() else {
            return;
        };
        edit.error = None;
        let dest = self.dirs.database.clone();
        let base = self.dat_base_url.clone();
        let http = self.dat_http.clone();
        edit.job = Some(EditJob::Search(Job::spawn(
            "dat-search",
            notify(ctx),
            move |_, cancel| {
                let client = emu_assets::http::HttpClient::new(http).map_err(|e| e.to_string())?;
                super::worker::search_dats(&client, &base, &dest, &database, &|| {
                    cancel.is_cancelled()
                })
            },
        )));
    }

    /// Once per frame: the result of a job of the edit form.
    pub(super) fn poll_custom_jobs(&mut self) {
        let Some(edit) = &mut self.custom_systems.edit else {
            return;
        };
        let Some(job) = &mut edit.job else {
            return;
        };
        let mut result = None;
        let (recalc, polled) = match job {
            EditJob::Recalc(job) => (
                true,
                job.poll()
                    .into_iter()
                    .find_map(|e| match e {
                        JobEvent::Done(r) => Some(r.map(Found::Extensions)),
                        JobEvent::Progress(_) => None,
                    })
                    .or_else(|| {
                        job.is_finished()
                            .then(|| Err(text::recalc_failed(text::TASK_INTERRUPTED)))
                    }),
            ),
            EditJob::Search(job) => (
                false,
                job.poll()
                    .into_iter()
                    .find_map(|e| match e {
                        JobEvent::Done(r) => Some(r.map(Found::Sources)),
                        JobEvent::Progress(_) => None,
                    })
                    .or_else(|| {
                        job.is_finished()
                            .then(|| Err(text::search_failed(text::TASK_INTERRUPTED)))
                    }),
            ),
        };
        if let Some(polled) = polled {
            result = Some((recalc, polled));
        }
        let Some((recalc, outcome)) = result else {
            return;
        };
        edit.job = None;
        match outcome {
            Ok(Found::Extensions(list)) => {
                edit.extensions = list.join(", ");
                let name = edit.name.clone();
                self.set_status(text::recalc_done(&name, &list), Level::Info);
            }
            Ok(Found::Sources(sources)) if sources.is_empty() => {
                self.set_status(text::SEARCH_NOT_FOUND.to_owned(), Level::Info);
            }
            Ok(Found::Sources(sources)) => {
                // The import needs the published definition listing the sources: the form is saved as it stands.
                edit.draft.dats = sources.iter().map(|s| s.0).collect();
                let ids: Vec<&str> = sources.iter().map(|s| s.0.id()).collect();
                let message = text::search_found(&ids);
                let id = edit.draft.id.clone();
                let database = edit.draft.database.clone().unwrap_or_default();
                self.save_edit(Some(AfterPublish::ImportDats {
                    id,
                    database,
                    found: sources,
                }));
                // Kept open (and its error shown) when the form could not be saved.
                if self.custom_systems.edit.is_none() {
                    self.set_status(message, Level::Info);
                }
            }
            // The user gave up (empty message): nothing to say.
            Err(message) if message.is_empty() => {}
            Err(message) => {
                let shown = if recalc {
                    text::recalc_failed(&message)
                } else {
                    message
                };
                if let Some(edit) = &mut self.custom_systems.edit {
                    edit.error = Some(shown);
                }
            }
        }
    }

    // ---- the page --------------------------------------------------------------------------------------------

    pub(super) fn custom_systems_section(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        self.section_title(ui, text::CUSTOM_SYSTEMS_TITLE, help_anchor::CUSTOM);
        weak(ui, text::CUSTOM_SYSTEMS_NOTE);
        if self.publication_waits() {
            weak(ui, text::CUSTOM_WAITING_PUBLICATION);
        }
        let Some(view) = &self.custom_systems.view else {
            return;
        };
        let mut action = None;
        let busy_edit = self.custom_systems.edit.is_some();
        let rows: Vec<(String, String, Vec<String>)> = view
            .load
            .catalog
            .customs()
            .map(|c| {
                let name = view
                    .load
                    .catalog
                    .system(&c.id)
                    .map_or_else(|| c.id.clone(), |s| s.name.clone());
                (c.id.clone(), name, c.cores.clone())
            })
            .collect();
        if rows.is_empty() {
            weak(ui, text::CUSTOM_SYSTEMS_NONE);
        }
        for (id, name, cores) in &rows {
            ui.horizontal_wrapped(|ui| {
                ui.label(text::custom_system_line(name, id, cores));
                if ui
                    .add_enabled(!busy_edit && !self.capture, Button::new(text::CUSTOM_EDIT))
                    .clicked()
                {
                    action = Some(Action::Edit(id.clone()));
                }
                if ui
                    .add_enabled(!self.capture, Button::new(text::CUSTOM_DELETE))
                    .clicked()
                {
                    action = Some(Action::Delete(id.clone()));
                }
            });
        }
        if !view.summary.lines.is_empty() || !view.summary.too_many.is_empty() {
            ui.label(RichText::new(text::CUSTOM_PROBLEMS_TITLE).strong());
            for line in &view.summary.lines {
                crate::copyable::message(ui, super::WARNING, line);
            }
            if !view.summary.too_many.is_empty() {
                ui.colored_label(
                    super::WARNING,
                    text::too_many_definitions(
                        view.summary.too_many.len(),
                        emu_library::MAX_DEFINITION_FILES,
                    ),
                );
                weak(ui, text::TOO_MANY_HINT);
                CollapsingHeader::new(text::TOO_MANY_LIST)
                    .id_salt("custom-too-many")
                    .default_open(false)
                    .show(ui, |ui| {
                        for name in &view.summary.too_many {
                            ui.label(RichText::new(name).monospace());
                        }
                    });
            }
        }
        if !view.retired.is_empty() {
            ui.label(RichText::new(text::CUSTOM_RETIRED_TITLE).strong());
            for row in &view.retired {
                ui.horizontal_wrapped(|ui| {
                    let id = &row.retired.id;
                    match row.retired.why {
                        Retirement::Replaced => {
                            ui.label(text::retired_replaced_line(id, row.replaced_by.as_deref()));
                        }
                        _ => {
                            ui.label(text::retired_removed_line(id));
                        }
                    }
                    if row.retired.why == Retirement::Removed
                        && ui
                            .add_enabled(
                                row.can_restore && !self.capture,
                                Button::new(text::CUSTOM_RESTORE),
                            )
                            .clicked()
                    {
                        action = Some(Action::Restore(id.clone()));
                    }
                    if row.can_forget
                        && ui
                            .add_enabled(!self.capture, Button::new(text::CUSTOM_FORGET))
                            .clicked()
                    {
                        action = Some(Action::Forget(id.clone()));
                    }
                    if row.retired.why == Retirement::Replaced {
                        weak(ui, text::RETIRED_REPLACED_NOTE);
                    }
                });
            }
        }
        if ui.button(text::CUSTOM_OPEN_FOLDER).clicked() {
            action = Some(Action::OpenFolder);
        }
        self.edit_form(ui, ctx);
        match action {
            Some(Action::Edit(id)) => self.open_edit(&id),
            Some(Action::Delete(id)) => self.ask_delete(&id),
            Some(Action::Restore(id)) => self.restore_custom_system(&id),
            Some(Action::Forget(id)) => {
                self.open_dialog(Dialog::CustomSystems(Confirm::Forget { id }));
            }
            Some(Action::OpenFolder) => {
                let dir = self.dirs.custom_systems.clone();
                if !self.capture {
                    let _ = std::fs::create_dir_all(&dir);
                }
                self.open_folder(dir);
            }
            None => {}
        }
    }

    fn edit_form(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let Some(edit) = &mut self.custom_systems.edit else {
            return;
        };
        enum Pick {
            Save,
            Cancel,
            Recalc,
            Search,
        }
        let mut pick = None;
        let working = edit.job.is_some();
        let escape = self.dialog.is_none() && ui.input(|i| i.key_pressed(Key::Escape));
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text::EDIT_TITLE).strong());
            let focus = std::mem::take(&mut edit.focus);
            egui::Grid::new("custom-edit")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label(text::EDIT_NAME);
                    let first = ui.add(TextEdit::singleline(&mut edit.name).desired_width(320.0));
                    if focus {
                        first.request_focus();
                    }
                    ui.end_row();
                    ui.label(text::EDIT_EXTENSIONS);
                    ui.add(TextEdit::singleline(&mut edit.extensions).desired_width(320.0));
                    ui.end_row();
                    ui.label(text::EDIT_SCREENSCRAPER);
                    ui.add(TextEdit::singleline(&mut edit.screenscraper).desired_width(120.0));
                    ui.end_row();
                    ui.label(text::EDIT_IGDB);
                    ui.add(TextEdit::singleline(&mut edit.igdb).desired_width(120.0));
                    ui.end_row();
                });
            ui.label(text::EDIT_CORES);
            let count = edit.draft.cores.len();
            let mut swap = None;
            let mut remove = None;
            for (i, core) in edit.draft.cores.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(core).monospace());
                    if ui.add_enabled(i > 0, Button::new(text::EDIT_UP)).clicked() {
                        swap = Some((i - 1, i));
                    }
                    if ui
                        .add_enabled(i + 1 < count, Button::new(text::EDIT_DOWN))
                        .clicked()
                    {
                        swap = Some((i, i + 1));
                    }
                    if ui
                        .add_enabled(count > 1, Button::new(text::EDIT_REMOVE_CORE))
                        .clicked()
                    {
                        remove = Some(i);
                    }
                });
            }
            if let Some((a, b)) = swap {
                edit.draft.cores.swap(a, b);
            }
            if let Some(i) = remove {
                edit.draft.cores.remove(i);
            }
            if !edit.firmware.is_empty() {
                ui.label(text::EDIT_FIRMWARE);
                for file in &edit.firmware {
                    let mut ignored = edit.draft.ignored_firmware.contains(file);
                    if ui
                        .checkbox(&mut ignored, RichText::new(file).monospace())
                        .changed()
                    {
                        if ignored {
                            edit.draft.ignored_firmware.push(file.clone());
                        } else {
                            edit.draft.ignored_firmware.retain(|f| f != file);
                        }
                    }
                }
            }
            if edit.draft.database.is_none() {
                weak(ui, text::EDIT_NO_DATABASE);
            }
            if let Some(error) = &edit.error {
                crate::copyable::message(ui, super::ERROR, error);
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!working, Button::new(text::EDIT_SAVE))
                    .clicked()
                {
                    pick = Some(Pick::Save);
                }
                if ui.button(text::EDIT_CANCEL).clicked() {
                    pick = Some(Pick::Cancel);
                }
                if ui
                    .add_enabled(!working, Button::new(text::EDIT_RECALC))
                    .clicked()
                {
                    pick = Some(Pick::Recalc);
                }
                if ui
                    .add_enabled(
                        !working && edit.draft.database.is_some(),
                        Button::new(text::EDIT_SEARCH_DAT),
                    )
                    .clicked()
                {
                    pick = Some(Pick::Search);
                }
                if working {
                    ui.spinner();
                    ui.label(match edit.job {
                        Some(EditJob::Recalc(_)) => text::RECALC_RUNNING,
                        _ => text::SEARCH_RUNNING,
                    });
                }
            });
        });
        if escape && pick.is_none() && !working {
            pick = Some(Pick::Cancel);
        }
        match pick {
            Some(Pick::Save) => self.save_edit(None),
            Some(Pick::Cancel) => self.custom_systems.edit = None,
            Some(Pick::Recalc) => self.start_recalc(ctx),
            Some(Pick::Search) => self.start_search(ctx),
            None => {}
        }
    }

    /// Dialogs of [`Confirm`]. The safe choice has the focus: « Annuler », « OK ».
    pub(super) fn custom_systems_dialog(&mut self, ctx: &egui::Context) {
        let Some(Dialog::CustomSystems(confirm)) = &self.dialog else {
            return;
        };
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let focus = std::mem::take(&mut self.focus_dialog);
        let mut pick = escape.then_some(ConfirmPick::Cancel);
        egui::Modal::new(egui::Id::unique("library-custom-systems")).show(ctx, |ui| {
            ui.set_max_width(560.0);
            let (title, body, yes, cancel_label, folder) = match confirm {
                Confirm::Delete { name, .. } => (
                    text::CONFIRM_DELETE_TITLE,
                    text::confirm_delete_body(name),
                    Some(text::CONFIRM_DELETE_YES),
                    text::CONFIRM_CANCEL,
                    false,
                ),
                Confirm::DeleteUncertain { name, official, .. } => (
                    text::CONFIRM_UNCERTAIN_TITLE,
                    text::confirm_uncertain_body(name, official),
                    Some(text::CONFIRM_UNCERTAIN_YES),
                    text::CONFIRM_CANCEL,
                    false,
                ),
                Confirm::Blocked { id, files, more } => (
                    text::UNCERTAIN_GAMES_TITLE,
                    text::uncertain_games_body(id, files, *more),
                    None,
                    text::ADDED_OK,
                    true,
                ),
                Confirm::Forget { id } => (
                    text::CONFIRM_FORGET_TITLE,
                    text::confirm_forget_body(id),
                    Some(text::CONFIRM_FORGET_YES),
                    text::CONFIRM_CANCEL,
                    false,
                ),
                Confirm::Added {
                    names,
                    ids,
                    existing,
                } => (
                    text::ADDED_TITLE,
                    text::added_body(names, ids, existing),
                    None,
                    text::ADDED_OK,
                    true,
                ),
            };
            ui.heading(title);
            ui.add(egui::Label::new(&body).selectable(true));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                // The safe choice first: it holds the focus.
                let cancel = ui.button(cancel_label);
                if focus {
                    cancel.request_focus();
                }
                if cancel.clicked() {
                    pick = Some(ConfirmPick::Cancel);
                }
                if folder && ui.button(text::OPEN_GAMES_FOLDER).clicked() {
                    pick = Some(ConfirmPick::Folder);
                }
                if let Some(label) = yes
                    && ui.button(label).clicked()
                {
                    pick = Some(ConfirmPick::Yes);
                }
            });
        });
        if let Some(pick) = pick {
            self.resolve_confirm(pick);
        }
    }

    /// The answer to the open dialog of [`Confirm`].
    pub(super) fn resolve_confirm(&mut self, pick: ConfirmPick) {
        let Some(Dialog::CustomSystems(confirm)) = self.dialog.take() else {
            return;
        };
        self.focus = Some(super::Focus::Grid);
        match (confirm, pick) {
            (Confirm::Delete { id, name }, ConfirmPick::Yes) => self.delete_custom(&id, &name),
            (Confirm::DeleteUncertain { id, name, .. }, ConfirmPick::Yes) => {
                self.delete_uncertain(&id, &name);
            }
            (Confirm::Forget { id }, ConfirmPick::Yes) => self.forget_retired(&id),
            (Confirm::Blocked { id, .. }, ConfirmPick::Folder) => {
                self.open_folder(self.dirs.root.join("roms").join(id));
            }
            (Confirm::Added { ids, .. }, ConfirmPick::Folder) => {
                let roms = self.dirs.root.join("roms");
                match ids.as_slice() {
                    [only] => self.open_folder(roms.join(only)),
                    _ => self.open_folder(roms),
                }
            }
            _ => {}
        }
    }
}

/// The button pressed in a dialog of [`Confirm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConfirmPick {
    Yes,
    Cancel,
    Folder,
}

/// What a job of the edit form found.
enum Found {
    Extensions(Vec<String>),
    Sources(Vec<FoundDat>),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn problem(file: &str, error: DefinitionError) -> DefinitionProblem {
        DefinitionProblem {
            file: PathBuf::from("systems").join(file),
            id: file.trim_end_matches(".toml").to_owned(),
            error,
        }
    }

    fn retired(id: &str, why: Retirement) -> RetiredSystem {
        RetiredSystem {
            id: id.to_owned(),
            file: PathBuf::from(".retired").join(format!("{id}-1.toml")),
            why,
            retired_unix: 1,
        }
    }

    #[test]
    fn the_definitions_beyond_the_bound_are_one_group_and_the_others_keep_their_line() {
        let mut problems = vec![problem("broken.toml", DefinitionError::NoCore)];
        for i in 0..44 {
            problems.push(problem(
                &format!("late{i:02}.toml"),
                DefinitionError::TooManyDefinitions,
            ));
        }
        let summary = summarize_problems(&problems);
        assert_eq!(summary.lines.len(), 1);
        assert!(summary.lines[0].starts_with("broken.toml : "));
        assert_eq!(summary.too_many.len(), 44);
        assert_eq!(summary.too_many[0], "late00.toml");
        assert_eq!(
            text::too_many_definitions(summary.too_many.len(), 256),
            "44 définitions ignorées au-delà de 256"
        );
        assert_eq!(
            text::too_many_definitions(1, 256),
            "1 définition ignorée au-delà de 256"
        );
    }

    #[test]
    fn restore_is_offered_for_a_removed_definition_only() {
        let catalog = Catalog::official();
        let rows = retired_rows(
            vec![
                retired("gone", Retirement::Removed),
                retired("swapped", Retirement::Replaced),
            ],
            &catalog,
        );
        assert!(rows[0].can_restore, "a removed definition comes back");
        assert!(!rows[1].can_restore, "a replaced one is never offered");
    }

    #[test]
    fn forgetting_the_games_of_an_active_id_is_never_offered() {
        // `snes` is an official id: a retired definition of that id shares the official's rows.
        let rows = retired_rows(
            vec![
                retired("snes", Retirement::Removed),
                retired("gone", Retirement::Removed),
            ],
            &Catalog::official(),
        );
        assert!(!rows[0].can_forget);
        assert!(rows[1].can_forget);
    }

    fn proposal(id: &str, kind: SwitchKind) -> SwitchProposal {
        let official = Catalog::official().system("snes").unwrap().clone();
        SwitchProposal {
            custom_id: id.to_owned(),
            official,
            kind,
        }
    }

    #[test]
    fn only_an_uncertain_conflict_asks_to_forget_the_games() {
        let switches = [
            proposal("snes", SwitchKind::Uncertain),
            proposal("megadrive", SwitchKind::InPlace),
        ];
        assert_eq!(
            delete_kind(&switches, "snes"),
            DeleteKind::Uncertain {
                official: Catalog::official().system("snes").unwrap().name.clone()
            }
        );
        assert_eq!(delete_kind(&switches, "megadrive"), DeleteKind::Plain);
        assert_eq!(delete_kind(&switches, "other"), DeleteKind::Plain);
    }

    #[test]
    fn the_notice_of_an_addition_names_the_folder_and_says_when_it_was_already_there() {
        let names = ["Acme".to_owned()];
        let ids = ["acme".to_owned()];
        let fresh = text::added_body(&names, &ids, &[]);
        assert!(
            fresh.contains("roms/acme/") && !fresh.contains("existait"),
            "{fresh}"
        );
        let known = text::added_body(&names, &ids, &ids);
        assert!(known.contains("existait déjà"), "{known}");
    }
}
