//! Window-less core management commands (ADR 0013): progress on stderr, results on stdout, French messages.
//! Exit code 0 on success, 1 on failure (the caller maps `Err`).

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use emu_ui::assets::cores::{
    CoreId, CoreManager, CorePreferences, FirmwareState, FirmwareStatus, Origin,
};
use emu_ui::assets::http::{CancelToken, Progress};
use emu_ui::{Config, DataDirs};

use crate::cli::CoresAction;

/// A catalog older than this is refreshed before an install.
const CATALOG_MAX_AGE_SECS: u64 = 3600;

pub fn open_manager(dirs: &DataDirs, config: &Config) -> Result<Arc<CoreManager>, String> {
    emu_ui::open_core_manager(dirs, config)
}

/// Ids are lower case (`CoreId::new` refuses capitals): `Gambatte` is accepted as `gambatte`.
fn core_id(text: &str) -> Result<CoreId, String> {
    CoreId::new(&text.to_ascii_lowercase())
        .ok_or_else(|| format!("identifiant de core invalide : {text}"))
}

/// Single-line progress on stderr.
fn progress_printer(label: String) -> impl FnMut(Progress) {
    let mut last_percent = None;
    move |p: Progress| {
        let text = match p.total {
            Some(total) if total > 0 => {
                let percent = p.done * 100 / total;
                if last_percent == Some(percent) {
                    return;
                }
                last_percent = Some(percent);
                format!("{percent} %")
            }
            _ => format!("{} Kio", p.done / 1024),
        };
        eprint!("\r{label} : {text}   ");
        let _ = std::io::stderr().flush();
    }
}

fn end_progress() {
    eprintln!();
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn refresh(manager: &CoreManager) -> Result<(), String> {
    let mut progress = progress_printer("Vérification du catalogue".into());
    let result = manager.refresh(&mut progress, &CancelToken::new());
    end_progress();
    let catalog = result.map_err(|e| e.to_string())?;
    println!(
        "Catalogue à jour : {} cores disponibles pour cette plate-forme.",
        catalog.cores.len()
    );
    Ok(())
}

fn refresh_if_stale(manager: &CoreManager) -> Result<(), String> {
    let fresh = manager
        .catalog()
        .is_some_and(|c| now_unix().saturating_sub(c.fetched_unix) <= CATALOG_MAX_AGE_SECS);
    if fresh { Ok(()) } else { refresh(manager) }
}

fn install_one(manager: &CoreManager, id: &CoreId) -> Result<(), String> {
    let mut progress = progress_printer(format!("Téléchargement de {id}"));
    let result = manager.install(id, &mut progress, &CancelToken::new());
    end_progress();
    result.map_err(|e| e.to_string())?;
    println!("{id} : installé.");
    Ok(())
}

fn list(manager: &CoreManager) {
    let installed = manager.installed();
    if installed.is_empty() {
        println!("Aucun core installé. Essayer : emu-rust --install-core recommended");
        return;
    }
    let updates = manager.updates();
    for core in &installed {
        let version = match &core.origin {
            Origin::Buildbot { date, pinned, .. } => {
                format!("{date}{}", if *pinned { " (épinglé)" } else { "" })
            }
            Origin::Manual => "manuel".to_owned(),
        };
        let update = updates
            .iter()
            .find(|u| u.id == core.id)
            .map(|u| format!(" - mise à jour disponible : {}", u.available))
            .unwrap_or_default();
        println!("{}  {version}{update}", core.id);
    }
    if manager.catalog().is_none() {
        println!("(catalogue non vérifié : --refresh-cores pour connaître les mises à jour)");
    }
}

/// "Recommended" = the first preferred core of every system, without duplicates.
fn recommended() -> Vec<CoreId> {
    let prefs = CorePreferences::builtin();
    let mut seen = BTreeSet::new();
    prefs
        .systems()
        .filter_map(|(_, cores)| cores.first())
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect()
}

fn describe(status: &FirmwareStatus) -> String {
    let kind = if status.spec.optional {
        "optionnel"
    } else {
        "requis"
    };
    let state = match &status.state {
        FirmwareState::Ok => "présent".to_owned(),
        FirmwareState::Missing => "absent".to_owned(),
        FirmwareState::WrongHash { .. } => "empreinte inattendue".to_owned(),
        FirmwareState::Unverified => "présent (non vérifiable)".to_owned(),
        FirmwareState::Unreadable(e) => format!("illisible : {e}"),
    };
    format!("  {} ({kind}) : {state}", status.spec.path)
}

fn check_bios(manager: &CoreManager, dirs: &DataDirs, id: Option<&str>) -> Result<bool, String> {
    let ids: Vec<CoreId> = match id {
        Some(text) => vec![core_id(text)?],
        None => manager.installed().into_iter().map(|c| c.id).collect(),
    };
    println!("Dossier des BIOS : {}", dirs.system.display());
    let mut required_problem = false;
    for id in ids {
        let statuses = manager.firmware_status(&id);
        if statuses.is_empty() {
            println!("{id} : aucun BIOS déclaré.");
            continue;
        }
        println!("{id} :");
        for status in &statuses {
            println!("{}", describe(status));
            required_problem |= !status.spec.optional
                && !matches!(status.state, FirmwareState::Ok | FirmwareState::Unverified);
        }
    }
    Ok(!required_problem)
}

/// `Ok(true)` = success, `Ok(false)` = ran but found a problem (exit 1), `Err` = failure.
pub fn run(action: &CoresAction, dirs: &DataDirs, config: &Config) -> Result<bool, String> {
    let manager = open_manager(dirs, config)?;
    match action {
        CoresAction::List => list(&manager),
        CoresAction::Refresh => refresh(&manager)?,
        CoresAction::Install(requested) => {
            let mut ids = Vec::new();
            for text in requested {
                if text == "recommended" {
                    ids.extend(recommended());
                } else {
                    ids.push(core_id(text)?);
                }
            }
            refresh_if_stale(&manager)?;
            let mut failed = false;
            for id in ids {
                if let Err(e) = install_one(&manager, &id) {
                    eprintln!("erreur : {id} : {e}");
                    failed = true;
                }
            }
            return Ok(!failed);
        }
        CoresAction::Update => {
            refresh(&manager)?;
            let updates: Vec<_> = manager
                .updates()
                .into_iter()
                .filter(|u| !u.pinned)
                .collect();
            if updates.is_empty() {
                println!("Tous les cores sont à jour.");
            }
            let mut failed = false;
            for update in updates {
                if let Err(e) = install_one(&manager, &update.id) {
                    eprintln!("erreur : {} : {e}", update.id);
                    failed = true;
                }
            }
            return Ok(!failed);
        }
        CoresAction::Remove(text) => {
            let id = core_id(text)?;
            manager.remove(&id).map_err(|e| e.to_string())?;
            println!("{id} : supprimé (sauvegardes conservées).");
        }
        CoresAction::CheckBios(id) => return check_bios(&manager, dirs, id.as_deref()),
    }
    Ok(true)
}

/// Path of a managed core for `--core ID`: verified against the manifest (never loads a tampered file).
pub fn managed_core_path(
    id: &str,
    dirs: &DataDirs,
    config: &Config,
) -> Result<std::path::PathBuf, String> {
    let manager = open_manager(dirs, config)?;
    let id = core_id(id)?;
    manager
        .verified_path(&id)
        .map_err(|e| format!("{e}\nInstaller le core : emu-rust --install-core {id}"))
}
