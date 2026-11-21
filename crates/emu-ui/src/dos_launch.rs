//! PC-DOS entry points for the command line (design v4.0 §4, CLI) and the system caches invalidated after an
//! install or import in `system/` (§2.4). The library uses the same functions of `computer::dos`.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use emu_assets::cores::{CoreId, CorePreferences};
use emu_runtime::{SnapshotLimits, core_id_from_path, snapshot_folder};

use crate::computer::dos::{self, DosPlan};
use crate::{Config, DataDirs, GameContent};

/// Content of `--rom <path>` for the core at `core`: a folder game (`JEU/` or `JEU/START.EXE`) for a core that
/// reads folders, or the refusal of a `.conf`. `None`: the 1.0 rule applies (`GameContent::Files`).
pub fn dos_cli_content(
    path: &Path,
    core: &Path,
    config: &Config,
) -> Option<Result<GameContent, String>> {
    let prefs = crate::cores_ui::preferences(&config.cores);
    let id = CoreId::new(&core_id_from_path(core));
    dos::cli_content(path, &prefs, id.as_ref())
}

/// Builds the snapshot of a folder game before `run` (same identity and name as the launch), reporting
/// `progress(done, total)` in bytes. `Ok(false)`: nothing to build (not a folder, or the snapshot is current).
pub fn prepare_dos_snapshot(
    core: &Path,
    content: &GameContent,
    dirs: &DataDirs,
    config: &Config,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<bool, String> {
    if !matches!(content, GameContent::Folder { .. }) {
        return Ok(false);
    }
    let core_id = core_id_from_path(core);
    let Some(id) = CoreId::new(&core_id) else {
        return Ok(false);
    };
    let prefs = crate::cores_ui::preferences(&config.cores);
    let spec = dos::spec_for_content(content, &prefs, &id)?;
    let resolved = crate::game_identity::resolve_game(&core_id, spec, dirs);
    match dos::snapshot_plan(
        resolved.spec,
        &resolved.identity.save_name,
        &dirs.cache_dir(),
        &id,
    )? {
        DosPlan::Ready(_) => Ok(false),
        DosPlan::NeedsSnapshot { dir, snapshot, .. } => {
            let cancel = AtomicBool::new(false);
            snapshot_folder(&dir, &snapshot, &SnapshotLimits::DEFAULT, progress, &cancel)
                .map_err(|e| crate::ui::text::dos_snapshot_failed(&e))?;
            Ok(true)
        }
    }
}

/// « Vider le cache DOS » (`--clear-dos-cache`): deletes the snapshots of the DOS cores; bytes freed. The game must
/// not be running (Windows cannot delete an open snapshot).
pub fn clear_dos_cache(dirs: &DataDirs, config: &Config) -> Result<u64, String> {
    let prefs = crate::cores_ui::preferences(&config.cores);
    let mut freed = 0;
    for dir in dos::clearable_cache_dirs(&prefs, &dirs.cache_dir()) {
        let size = dos::cache_size(&dir);
        dos::clear_cache(&dir).map_err(|e| format!("{} : {e}", dir.display()))?;
        freed += size;
    }
    Ok(freed)
}

/// Union of the `system_caches` of the cores of `cores.toml` (`DOSBoxPureMidiCache.txt`): files a core writes in
/// `system/` to remember what it found there.
pub fn system_cache_names() -> Vec<String> {
    let prefs = CorePreferences::builtin();
    let mut names: Vec<String> = prefs
        .systems()
        .flat_map(|(_, cores)| cores.iter())
        .flat_map(|c| prefs.system_caches(c))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// After a file was installed or imported in `system/`: drops the cores' caches so the new file is seen (the
/// MIDI list of dosbox_pure). Failures are logged, never fatal.
pub fn invalidate_core_caches(system_dir: &Path) {
    let names = system_cache_names();
    if names.is_empty() {
        return;
    }
    match emu_assets::bios::invalidate_system_caches(system_dir, &names) {
        Ok(removed) => {
            for p in removed {
                tracing::info!("cache du core supprimé : {}", p.display());
            }
        }
        Err(e) => tracing::warn!("caches du dossier système non supprimés : {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caches_are_invalidated_after_an_install() {
        assert!(system_cache_names().contains(&"DOSBoxPureMidiCache.txt".to_owned()));
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("DOSBoxPureMidiCache.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("GeneralUser-GS.sf2"), b"x").unwrap();
        invalidate_core_caches(dir.path());
        assert!(!dir.path().join("DOSBoxPureMidiCache.txt").exists());
        assert!(dir.path().join("GeneralUser-GS.sf2").exists());
        invalidate_core_caches(&dir.path().join("absent"));
    }

    #[test]
    fn cli_folder_snapshot_and_cache() {
        let root = tempfile::tempdir().unwrap();
        let dirs = DataDirs::under(&root.path().join("data"));
        let game = root.path().join("Jeu");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("START.EXE"), b"MZ").unwrap();
        let config = Config::default();
        let core = Path::new("/cores/dosbox_pure_libretro.so");
        let content = dos_cli_content(&game.join("START.EXE"), core, &config)
            .unwrap()
            .unwrap();
        assert_eq!(
            content,
            GameContent::Folder {
                dir: game.clone(),
                start: Some("START.EXE".into())
            }
        );
        let mut calls = 0;
        assert!(
            prepare_dos_snapshot(core, &content, &dirs, &config, &mut |_, _| calls += 1).unwrap()
        );
        assert!(calls > 0);
        assert!(!prepare_dos_snapshot(core, &content, &dirs, &config, &mut |_, _| {}).unwrap());
        let copies = dirs.cache_dir().join("core-copies");
        std::fs::create_dir_all(&copies).unwrap();
        std::fs::write(copies.join("x.so"), b"core").unwrap();
        assert!(clear_dos_cache(&dirs, &config).unwrap() > 0);
        assert!(
            copies.join("x.so").is_file(),
            "core copies are not the DOS cache"
        );
        assert_eq!(clear_dos_cache(&dirs, &config).unwrap(), 0);
        // A user preference naming `core-copies` never reaches the private copies.
        let mut hostile = Config::default();
        hostile
            .cores
            .preferred
            .insert("DOS".into(), "core-copies".into());
        assert_eq!(clear_dos_cache(&dirs, &hostile).unwrap(), 0);
        assert!(copies.join("x.so").is_file());
        // The source folder is never written.
        assert_eq!(std::fs::read_dir(&game).unwrap().count(), 1);
        assert!(dos_cli_content(Path::new("/x/a.zip"), core, &config).is_none());
    }
}
