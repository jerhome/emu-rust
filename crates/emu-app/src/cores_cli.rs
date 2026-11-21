//! Window-less core management commands (ADR 0013): progress on stderr, results on stdout, French messages.
//! Exit code 0 on success, 1 on failure (the caller maps `Err`).

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use emu_ui::assets::bios::{
    BiosError, FreeBiosState, ReplacePolicy, builtin_free_bios, find_free_bios,
};
use emu_ui::assets::bios::{ReplacePolicy as ImportPolicy, TargetState};
use emu_ui::assets::cores::{
    CoreId, CoreManager, CorePreferences, FirmwareCheck, FirmwareState, FirmwareStatus,
    FirmwareVariant, KnownFirmware, Origin, check_core_firmware,
};
use emu_ui::assets::http::{CancelToken, Progress};
use emu_ui::{BiosIdentified, ROM_KEY_NAME};
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
        FirmwareState::Free { label } => format!("BIOS libre : {label}"),
        FirmwareState::Unreadable(e) => format!("illisible : {e}"),
        FirmwareState::CoreFallback { label } => fallback_text(label),
        FirmwareState::Builtin { label } => format!("intégré au core : {label}"),
    };
    format!("  {} ({kind}) : {state}", status.spec.path)
}

fn fallback_text(label: &str) -> String {
    format!(
        "absent : {label} utilisé à la place (compatibilité limitée, original recommandé ; non bloquant)"
    )
}

/// The system whose `required_firmware` table applies to `id` (same rule as the UI launch).
fn table_system(prefs: &CorePreferences, id: &CoreId) -> Option<String> {
    let systems: Vec<&str> = prefs
        .systems()
        .filter(|(_, cores)| cores.contains(id))
        .map(|(s, _)| s)
        .collect();
    systems
        .iter()
        .find(|s| prefs.required_firmware(id, s).is_some())
        .or_else(|| systems.first())
        .map(|s| (*s).to_owned())
}

/// One line of `--check-bios` for a core with a table or a built-in ROM: the variant it was checked for
/// (`None`: the table's default), and the result.
struct RequiredLine {
    region: Option<String>,
    check: FirmwareCheck,
}

/// The single 3.0 check (`check_core_firmware`), when the core has a table or a built-in ROM: once per region for a
/// `variant = "region"` table (Mega CD: the BIOS of each region, design v6.0 §3.5), the default variant otherwise.
fn required_checks(dirs: &DataDirs, prefs: &CorePreferences, id: &CoreId) -> Vec<RequiredLine> {
    let Some(system) = table_system(prefs, id) else {
        return Vec::new();
    };
    let regions: Vec<Option<String>> = match prefs.required_firmware(id, &system) {
        Some(req) if req.variant == FirmwareVariant::Region => {
            let mut keys: Vec<String> = req.files.keys().cloned().collect();
            keys.sort_by_key(|k| region_order(k));
            keys.into_iter().map(Some).collect()
        }
        _ => vec![None],
    };
    let known = KnownFirmware::builtin();
    let mut out = Vec::new();
    for region in regions {
        match check_core_firmware(&dirs.system, prefs, id, &system, region.as_deref(), &known) {
            Some(Ok(check)) => out.push(RequiredLine { region, check }),
            Some(Err(e)) => {
                eprintln!("{id} : {e}");
                return Vec::new();
            }
            None => return Vec::new(),
        }
    }
    out
}

/// Europe, USA, Japon, then the rest (same order as the library dialog).
fn region_order(key: &str) -> usize {
    ["europe", "usa", "japan"]
        .iter()
        .position(|k| *k == key)
        .unwrap_or(usize::MAX)
}

fn region_label(key: &str) -> &str {
    match key {
        "europe" => "Europe",
        "usa" => "USA",
        "japan" => "Japon",
        other => other,
    }
}

fn describe_required(line: &RequiredLine) -> String {
    let check = &line.check;
    let wanted = check.wanted.as_deref().unwrap_or("-");
    let state = match &check.state {
        FirmwareState::Ok => "présent".to_owned(),
        FirmwareState::Missing => "absent".to_owned(),
        FirmwareState::WrongHash { .. } => "empreinte inattendue".to_owned(),
        FirmwareState::Unverified => "présent (non vérifiable)".to_owned(),
        FirmwareState::Free { label } => format!("BIOS libre : {label}"),
        FirmwareState::Unreadable(e) => format!("illisible : {e}"),
        FirmwareState::CoreFallback { label } => fallback_text(label),
        FirmwareState::Builtin { label } => return format!("  ROM intégrée au core : {label}"),
    };
    match &line.region {
        Some(region) => format!("  région {} : {wanted} : {state}", region_label(region)),
        None => format!("  modèle par défaut : {wanted} : {state}"),
    }
}

fn usable(check: &FirmwareCheck) -> bool {
    !matches!(
        check.state,
        FirmwareState::Missing | FirmwareState::WrongHash { .. } | FirmwareState::Unreadable(_)
    )
}

/// A region table is a problem only when no region's file is usable (a game of an unknown region launches with
/// any of them, design v6.0 §2.2); any other table when its file is not.
fn required_is_problem(lines: &[RequiredLine]) -> bool {
    if lines.iter().any(|l| l.region.is_some()) {
        !lines.iter().any(|l| usable(&l.check))
    } else {
        lines.iter().any(|l| !usable(&l.check))
    }
}

fn check_bios(manager: &CoreManager, dirs: &DataDirs, id: Option<&str>) -> Result<bool, String> {
    let ids: Vec<CoreId> = match id {
        Some(text) => vec![core_id(text)?],
        None => manager.installed().into_iter().map(|c| c.id).collect(),
    };
    println!("Dossier des BIOS : {}", dirs.system.display());
    let prefs = CorePreferences::builtin();
    let mut required_problem = false;
    for id in ids {
        let statuses = manager.firmware_status(&id);
        // A core with a 3.0 table is judged by it (design v3.0 §3.5.5); its `.info` list is informative.
        let required = required_checks(dirs, &prefs, &id);
        if statuses.is_empty() && required.is_empty() {
            println!("{id} : aucun BIOS déclaré.");
            continue;
        }
        println!("{id} :");
        for line in &required {
            println!("{}", describe_required(line));
        }
        // Built-in ROMs of the systems without table (picodrive: 32X BIOS) are shown.
        let known = KnownFirmware::builtin();
        for (system, cores) in prefs.systems() {
            if cores.contains(&id)
                && prefs.required_firmware(&id, system).is_none()
                && let Some(Ok(check)) =
                    check_core_firmware(&dirs.system, &prefs, &id, system, None, &known)
                && matches!(check.state, FirmwareState::Builtin { .. })
            {
                println!(
                    "{}",
                    describe_required(&RequiredLine {
                        region: None,
                        check
                    })
                );
            }
        }
        // A core that also plays a system without table (genesis_plus_gx, picodrive: Mega Drive / 32X) is not
        // in error for a missing Mega CD BIOS: the exit code only reflects cores that cannot play without it.
        let system_name = |database: &str| {
            emu_ui::library_db::systems()
                .iter()
                .find(|s| s.database == database)
                .map_or_else(|| database.to_owned(), |s| s.name.clone())
        };
        let (with_table, without_table): (Vec<_>, Vec<_>) = prefs
            .systems()
            .filter(|(_, cores)| cores.contains(&id))
            .map(|(system, _)| system)
            .partition(|system| prefs.required_firmware(&id, system).is_some());
        if required_is_problem(&required) && !without_table.is_empty() {
            let names = |list: &[&str]| {
                list.iter()
                    .map(|s| system_name(s))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            println!(
                "  (BIOS requis pour les seuls jeux {} ; {} jouables sans)",
                names(&with_table),
                names(&without_table)
            );
        } else {
            required_problem |= required_is_problem(&required);
        }
        for status in &statuses {
            println!("{}", describe(status));
            required_problem |= required.is_empty()
                && !status.spec.optional
                && !matches!(
                    status.state,
                    FirmwareState::Ok | FirmwareState::Unverified | FirmwareState::Free { .. }
                );
        }
    }
    Ok(!required_problem)
}

fn list_bios(dirs: &DataDirs) -> Result<(), String> {
    let manager = emu_ui::open_free_bios_manager(dirs)?;
    println!("Dossier des BIOS : {}", dirs.system.display());
    for bios in builtin_free_bios() {
        let state = match manager.state(&bios) {
            FreeBiosState::Missing => "non installé".to_owned(),
            FreeBiosState::Installed { variant } => format!("installé ({variant})"),
            FreeBiosState::UserFile { .. } => {
                format!("{} fourni par l'utilisateur (conservé)", bios.install_as)
            }
            FreeBiosState::Modified => format!(
                "{} modifié ou remplacé par l'utilisateur (conservé)",
                bios.install_as
            ),
            FreeBiosState::Unreadable(e) => format!("illisible : {e}"),
        };
        let kind = match bios.kind {
            emu_ui::assets::bios::FreeBiosKind::SoundFont => "SoundFont",
            _ => "BIOS",
        };
        println!(
            "{} : {kind} {} {} ({}) pour {}, fichier {} : {state}",
            bios.id, bios.name, bios.version, bios.license, bios.system, bios.install_as
        );
        for v in &bios.variants {
            let mark = if v.id == bios.default_variant {
                " (défaut)"
            } else {
                ""
            };
            println!("  {}{mark} : {}", v.id, v.label);
        }
    }
    list_required(dirs);
    Ok(())
}

/// Files expected per model by the `required_firmware` tables (Kickstarts of puae / puae2021): never
/// downloaded, imported with `--import-bios`.
fn list_required(dirs: &DataDirs) {
    let prefs = CorePreferences::builtin();
    let mut seen = BTreeSet::new();
    for (system, cores) in prefs.systems() {
        for core in cores {
            let Some(req) = prefs.required_firmware(core, system) else {
                continue;
            };
            let mut by_file: std::collections::BTreeMap<&str, Vec<&str>> =
                std::collections::BTreeMap::new();
            for (variant, file) in &req.files {
                by_file
                    .entry(file.as_str())
                    .or_default()
                    .push(variant.as_str());
            }
            if !seen.insert((system.to_owned(), by_file.clone())) {
                continue;
            }
            let cores_list: Vec<&str> = cores
                .iter()
                .filter(|c| prefs.required_firmware(c, system).is_some())
                .map(CoreId::as_str)
                .collect();
            let per = if req.variant == FirmwareVariant::Region {
                "région"
            } else {
                "modèle"
            };
            println!(
                "{system} ({}) : fichiers attendus par {per}",
                cores_list.join(", ")
            );
            for (file, models) in by_file {
                let present = if dirs.system.join(file).is_file() {
                    "présent"
                } else {
                    "absent"
                };
                println!("  {file} : {} — {present}", models.join(", "));
            }
            if let Some(b) = prefs
                .builtin_firmware(core)
                .filter(|_| req.fallback.is_some())
            {
                println!("  sans fichier : {} (compatibilité limitée)", b.label);
            }
        }
    }
}

fn hex(md5: &[u8; 16]) -> String {
    md5.iter().map(|b| format!("{b:02x}")).collect()
}

/// `--import-bios` (design v3.0 §4): known file ⇒ every expected name; unknown ⇒ `--bios-name` + `--unverified`;
/// a different user file is only replaced (renamed first) with `--replace-user-bios`.
fn import_bios_file(
    dirs: &DataDirs,
    config: &Config,
    file: &std::path::Path,
    name: Option<&str>,
    unverified: bool,
    replace: bool,
) -> Result<bool, String> {
    // The `.info` of the installed cores add expected names; the catalogue works without them.
    let manager = open_manager(dirs, config).ok();
    let catalogue = emu_ui::firmware_catalogue(manager.as_deref());
    let identified = emu_ui::identify_bios(&dirs.system, file, &catalogue)?;
    println!("Dossier des BIOS : {}", dirs.system.display());
    let policy = if replace {
        ImportPolicy::BackupAndReplace
    } else {
        ImportPolicy::Refuse
    };
    let (items, md5) = match &identified {
        BiosIdentified::Known { md5, targets } => {
            let targets: Vec<_> = match name {
                Some(n) => {
                    let t: Vec<_> = targets
                        .iter()
                        .filter(|(t, _)| t.name == n)
                        .cloned()
                        .collect();
                    if t.is_empty() {
                        return Err(format!(
                            "fichier reconnu, mais pas sous le nom {n} ; noms attendus : {}",
                            targets
                                .iter()
                                .map(|(t, _)| t.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    t
                }
                None => targets.clone(),
            };
            println!("Fichier reconnu (md5 {}).", hex(md5));
            let mut kept = false;
            for (t, state) in &targets {
                match state {
                    Ok(TargetState::Same) => println!("{} : déjà installé", t.name),
                    Ok(TargetState::Other { .. }) if !replace => {
                        kept = true;
                        println!(
                            "{} : un autre fichier existe déjà sous ce nom : conservé \
                             (--replace-user-bios pour le remplacer ; il sera renommé, jamais effacé)",
                            t.name
                        );
                    }
                    Err(e) => println!("{} : {e}", t.name),
                    _ => {}
                }
            }
            let items = emu_ui::known_import_items(&targets, replace);
            if items.is_empty() {
                return Ok(!kept);
            }
            (items, Some(*md5))
        }
        BiosIdentified::Encrypted { suggested, .. } => {
            if !unverified {
                return Err(
                    "ROM chiffrée d'Amiga Forever : relancer avec --unverified [--bios-name <nom>] \
                     (et importer aussi rom.key)"
                        .into(),
                );
            }
            let chosen = name
                .map(str::to_owned)
                .or_else(|| suggested.clone())
                .ok_or(
                    "ROM chiffrée : --bios-name <nom> obligatoire (noms attendus : --list-bios)",
                )?;
            let mut item = emu_ui::unverified_item(&identified, &chosen)
                .ok_or_else(|| format!("nom non attendu par les cores : {chosen}"))?;
            item.policy = policy;
            println!("ROM chiffrée d'Amiga Forever : importez aussi le fichier rom.key.");
            (vec![item], None)
        }
        BiosIdentified::Unknown { md5, rom_key, .. } => {
            let chosen = name.or(rom_key.then_some(ROM_KEY_NAME));
            let (true, Some(chosen)) = (unverified, chosen) else {
                return Err(format!(
                    "fichier inconnu (md5 {}) : il ne correspond à aucun BIOS connu et n'est pas importé.\n\
                     Pour l'importer quand même : --bios-name <nom attendu> --unverified (noms : --list-bios, \
                     --check-bios)",
                    hex(md5)
                ));
            };
            let mut item = emu_ui::unverified_item(&identified, chosen)
                .ok_or_else(|| format!("nom non attendu par les cores : {chosen}"))?;
            item.policy = policy;
            (vec![item], None)
        }
    };
    let report = emu_ui::import_bios(&dirs.system, file, &items, md5);
    for line in &report.lines {
        println!("{line}");
    }
    Ok(report.failed == 0)
}

/// Variant: the one asked, else `[computer] emutos_variant`, else the system language (fallback: catalogue).
fn install_bios(
    dirs: &DataDirs,
    config: &Config,
    id: &str,
    variant: Option<&str>,
    replace: bool,
) -> Result<(), String> {
    let bios = find_free_bios(id).map_err(|e| e.to_string())?;
    let configured = config
        .computer
        .emutos_variant
        .as_deref()
        .filter(|_| id == "emutos");
    let variant = match variant.or(configured) {
        Some(v) => bios
            .variant(v)
            .ok_or_else(|| format!("variante inconnue : {v} (voir --list-bios)"))?,
        // Same logic as the UI; a system variant absent from the catalogue falls back to the default (192fr).
        None => emu_ui::emutos_variant_or_system()
            .filter(|_| id == "emutos")
            .and_then(|v| bios.variant(&v))
            .or_else(|| bios.preferred_variant(None))
            .ok_or("catalogue de BIOS vide")?,
    };
    let manager = emu_ui::open_free_bios_manager(dirs)?;
    let policy = if replace {
        ReplacePolicy::BackupAndReplace
    } else {
        ReplacePolicy::Refuse
    };
    println!("{} ({}) : {}", bios.name, bios.license, variant.label);
    let mut progress = progress_printer(format!("Téléchargement de {}", bios.name));
    let result = manager.install(
        &bios,
        &variant.id,
        policy,
        &mut progress,
        &CancelToken::new(),
    );
    end_progress();
    match result {
        Ok(path) => {
            println!("Installé : {}", path.display());
            emu_ui::invalidate_core_caches(&dirs.system);
            Ok(())
        }
        Err(BiosError::UserFileExists(path)) => Err(format!(
            "{} existe déjà et n'a pas été installé par emu-rust, ou a été modifié (TOS d'origine ?) : conservé.\n\
             Pour le remplacer (il sera renommé, jamais effacé) : --replace-user-bios",
            path.display()
        )),
        Err(e) => Err(e.to_string()),
    }
}

/// `Ok(true)` = success, `Ok(false)` = ran but found a problem (exit 1), `Err` = failure.
pub fn run(action: &CoresAction, dirs: &DataDirs, config: &Config) -> Result<bool, String> {
    match action {
        CoresAction::ListBios => return list_bios(dirs).map(|()| true),
        CoresAction::ClearDosCache => {
            let freed = emu_ui::clear_dos_cache(dirs, config)?;
            println!(
                "Cache DOS vidé : {} Kio libérés (sauvegardes conservées).",
                freed / 1024
            );
            return Ok(true);
        }
        CoresAction::InstallBios {
            id,
            variant,
            replace,
        } => return install_bios(dirs, config, id, variant.as_deref(), *replace).map(|()| true),
        CoresAction::ImportBios {
            file,
            name,
            unverified,
            replace,
        } => {
            return import_bios_file(dirs, config, file, name.as_deref(), *unverified, *replace);
        }
        _ => {}
    }
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
        CoresAction::ListBios
        | CoresAction::ClearDosCache
        | CoresAction::InstallBios { .. }
        | CoresAction::ImportBios { .. } => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> (tempfile::TempDir, DataDirs) {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::under(tmp.path());
        std::fs::create_dir_all(&dirs.system).unwrap();
        (tmp, dirs)
    }

    #[test]
    fn mega_cd_bios_are_checked_per_region() {
        let (_tmp, dirs) = dirs();
        let prefs = CorePreferences::builtin();
        let gpgx = CoreId::new("genesis_plus_gx").unwrap();
        let lines = required_checks(&dirs, &prefs, &gpgx);
        let wanted: Vec<_> = lines
            .iter()
            .map(|l| (l.region.as_deref(), l.check.wanted.as_deref()))
            .collect();
        assert_eq!(
            wanted,
            vec![
                (Some("europe"), Some("bios_CD_E.bin")),
                (Some("usa"), Some("bios_CD_U.bin")),
                (Some("japan"), Some("bios_CD_J.bin")),
            ]
        );
        assert_eq!(
            describe_required(&lines[0]),
            "  région Europe : bios_CD_E.bin : absent"
        );
        assert!(required_is_problem(&lines));
        // One region present is enough (a game of an unknown region launches with it).
        std::fs::write(dirs.system.join("bios_CD_J.bin"), b"x").unwrap();
        let lines = required_checks(&dirs, &prefs, &gpgx);
        let japan = lines.iter().find(|l| l.region.as_deref() == Some("japan"));
        // A fake file has an unexpected md5: still no usable BIOS.
        assert!(japan.is_some_and(|l| !usable(&l.check)));
        assert!(required_is_problem(&lines));
    }

    #[test]
    fn model_tables_keep_the_default_line() {
        let (_tmp, dirs) = dirs();
        let prefs = CorePreferences::builtin();
        let lines = required_checks(&dirs, &prefs, &CoreId::new("puae").unwrap());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].region.is_none());
        assert!(describe_required(&lines[0]).starts_with("  modèle par défaut : kick"));
        // AROS fallback: not a problem.
        assert!(!required_is_problem(&lines));
        assert!(required_checks(&dirs, &prefs, &CoreId::new("gambatte").unwrap()).is_empty());
    }
}
