//! PC-DOS launch logic (design v4.0 §2, §4): from a library game or a CLI path to the `ContentSpec` of dosbox_pure,
//! the folder snapshot to build first, the programs at the root of a game folder, the first-launch hint and the
//! DOS cache. Pure (file system reads only), tested; the UI runs the snapshot job and the cache size off the UI
//! thread.

use std::io;
use std::path::{Path, PathBuf};

use emu_assets::cores::{CoreId, CorePreferences};
use emu_library::Game;
use emu_runtime::{ContentSpec, SnapshotStatus, snapshot_path, snapshot_status};

use crate::content::{GameContent, content_spec};
use crate::ui::text;

/// Option of dosbox_pure listing the MIDI devices found in `system/` (SoundFont, MT-32).
pub(crate) const MIDI_OPTION: &str = "dosbox_pure_midi";
/// Free SoundFont of the catalogue (`free_bios.toml`), installed on request only.
pub(crate) const SOUNDFONT_FILE: &str = "GeneralUser-GS.sf2";

/// What remains to do before `Emulator::load_content_as` (design v4.0 §2.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DosPlan {
    /// Load as is (zip, image, or a folder whose snapshot is current).
    Ready(ContentSpec),
    /// Build `snapshot` from `dir` first (job « Préparation du jeu… », progress, cancel), then load `spec`.
    NeedsSnapshot {
        spec: ContentSpec,
        dir: PathBuf,
        snapshot: PathBuf,
    },
}

/// Content of a library game: a folder game (`Game::is_dir`) is a `Folder`; anything else keeps the 2.0 rule (the
/// ordered disks of a computer game, else its file: a DOS zip is `path`, never `entry`).
pub(crate) fn game_content(game: &Game, computer: bool, start: Option<&str>) -> GameContent {
    if game.is_dir {
        return GameContent::Folder {
            dir: game.path.clone(),
            start: start.map(str::to_owned),
        };
    }
    if computer && !game.media.is_empty() {
        GameContent::Files(game.media.clone())
    } else {
        GameContent::Files(vec![game.path.clone()])
    }
}

/// `ContentSpec` of a library game for `core`. A folder needs a core declaring `folders` (`FolderPolicy`) and a
/// valid `start`; the rest goes through [`content_spec`] (zips and CD images direct, writable images copied).
/// The library and `App::build` reach it through [`game_content`] + [`spec_for_content`].
#[cfg(test)]
pub(crate) fn plan_dos_launch(
    game: &Game,
    prefs: &CorePreferences,
    core: &CoreId,
    start: Option<&str>,
) -> Result<ContentSpec, String> {
    let computer = emu_library::system(&game.system)
        .is_some_and(|s| s.kind == emu_library::SystemKind::Computer);
    spec_for_content(&game_content(game, computer, start), prefs, core)
}

/// [`plan_dos_launch`] on a `GameContent` (CLI, desktop, library).
pub(crate) fn spec_for_content(
    content: &GameContent,
    prefs: &CorePreferences,
    core: &CoreId,
) -> Result<ContentSpec, String> {
    match content {
        GameContent::Folder { dir, start } => {
            if prefs.folder_policy(core).is_none() {
                return Err(text::dos_folder_refused(core.as_str()));
            }
            if let Some(start) = start
                && !root_executables(dir)
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(start))
            {
                return Err(text::dos_start_invalid(start));
            }
            Ok(ContentSpec::Folder {
                dir: dir.clone(),
                name: folder_name(dir),
                start: start.clone(),
            })
        }
        GameContent::Files(files)
            if prefs.folder_policy(core).is_some()
                && files.iter().any(|f| has_ext(f, &["conf"])) =>
        {
            Err(text::DOS_CONF_REFUSED.to_owned())
        }
        other => Ok(content_spec(core.as_str(), other, prefs)),
    }
}

/// Second step, once the game identity is resolved: a `Folder` whose snapshot (named after
/// `GameIdentity::save_name`, as the runtime does) is not `Current` must be built first.
pub(crate) fn snapshot_plan(
    spec: ContentSpec,
    save_name: &str,
    cache_dir: &Path,
    core: &CoreId,
) -> Result<DosPlan, String> {
    let ContentSpec::Folder { dir, .. } = &spec else {
        return Ok(DosPlan::Ready(spec));
    };
    let snapshot = snapshot_path(&dos_cache_dir(cache_dir, core), save_name);
    match snapshot_status(dir, &snapshot) {
        Ok(SnapshotStatus::Current) => Ok(DosPlan::Ready(spec)),
        Ok(SnapshotStatus::Stale | SnapshotStatus::Missing) => Ok(DosPlan::NeedsSnapshot {
            dir: dir.clone(),
            snapshot,
            spec,
        }),
        Err(e) => Err(text::dos_snapshot_failed(&e)),
    }
}

/// Content of a command-line path for `core` (design v4.0 §4, CLI): a folder is a folder game for a core that reads
/// folders; `JEU/START.EXE` is the folder `JEU` started on `START.EXE` (same game, same identity); `.conf` is
/// refused (the core would mount and write its folder). Other paths: `None` (the caller keeps its 1.0 rule).
pub(crate) fn cli_content(
    path: &Path,
    prefs: &CorePreferences,
    core: Option<&CoreId>,
) -> Option<Result<GameContent, String>> {
    if has_ext(path, &["conf"]) {
        return Some(Err(text::DOS_CONF_REFUSED.to_owned()));
    }
    let reads_folders = core.is_some_and(|c| prefs.folder_policy(c).is_some());
    if path.is_dir() {
        return Some(if reads_folders {
            Ok(GameContent::Folder {
                dir: path.to_path_buf(),
                start: None,
            })
        } else {
            Err(text::dos_folder_refused(core.map_or("?", CoreId::as_str)))
        });
    }
    if reads_folders && has_ext(path, &["exe", "com", "bat"]) {
        let dir = path.parent().filter(|p| !p.as_os_str().is_empty())?;
        let name = path.file_name()?.to_str()?;
        if !is_valid_start(name) {
            return Some(Err(text::dos_start_invalid(name)));
        }
        return Some(Ok(GameContent::Folder {
            dir: dir.to_path_buf(),
            start: Some(name.to_ascii_uppercase()),
        }));
    }
    None
}

/// Programs a folder game can start on (« Lancer un autre programme… »): `exe` / `com` / `bat` files at the root
/// only (the core's `#START` fragment cannot hold a path), valid 8.3 names, upper case, sorted. Unreadable folder:
/// empty.
pub(crate) fn root_executables(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|n| is_valid_start(n))
        .map(|n| n.to_ascii_uppercase())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Same rule as the runtime's `start` check: 8.3, printable ASCII without `/ \ # : * ? < > | "` nor space,
/// extension `exe` / `com` / `bat`.
pub(crate) fn is_valid_start(name: &str) -> bool {
    let printable = name.bytes().all(|b| {
        b.is_ascii_graphic()
            && !matches!(
                b,
                b'/' | b'\\' | b'#' | b':' | b'*' | b'?' | b'<' | b'>' | b'|' | b'"'
            )
    });
    let Some((base, ext)) = name.split_once('.') else {
        return false;
    };
    printable
        && name.len() <= 12
        && !base.is_empty()
        && base.len() <= 8
        && ["exe", "com", "bat"]
            .iter()
            .any(|e| ext.eq_ignore_ascii_case(e))
}

/// No `<content_name>.pure.zip` yet in the game's save folder (`GameIdentity::core_save_dir`): first launch, the
/// boot menu hint is shown.
pub(crate) fn first_launch(core_save_dir: &Path, content_name: &str) -> bool {
    !core_save_dir
        .join(format!("{content_name}.pure.zip"))
        .exists()
}

/// `<cache>/<core_id>`: snapshots of one core (« Vider le cache DOS » deletes it).
pub(crate) fn dos_cache_dir(cache_dir: &Path, core: &CoreId) -> PathBuf {
    cache_dir.join(core.as_str())
}

/// Snapshot folders « Vider le cache DOS » may delete: the DOS cores that take game folders. `for_system` includes
/// the user's `[cores.preferred]`, so a core named `core-copies` (private core copies, ADR 0022 section 5) is
/// explicitly excluded.
pub(crate) fn clearable_cache_dirs(
    prefs: &emu_assets::cores::CorePreferences,
    cache_dir: &Path,
) -> Vec<PathBuf> {
    prefs
        .for_system("DOS")
        .iter()
        .filter(|c| c.as_str() != "core-copies" && prefs.folder_policy(c).is_some())
        .map(|c| dos_cache_dir(cache_dir, c))
        .collect()
}

/// Bytes under `dir` (links not followed); missing folder: 0. Off the UI thread for a large cache.
pub(crate) fn cache_size(dir: &Path) -> u64 {
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    read.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => cache_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// Deletes the snapshots folder; the progress lives in the `.pure.zip` files, so nothing is lost. The game must be
/// stopped (Windows cannot delete a file the core holds open). Missing folder: `Ok`.
pub(crate) fn clear_cache(dir: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "dos".to_owned())
}

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emu_library::{GameId, Identification};
    use emu_runtime::{MediaPolicy, SnapshotLimits, snapshot_folder};

    fn dosbox() -> CoreId {
        CoreId::new("dosbox_pure").unwrap()
    }

    fn game(path: &Path, is_dir: bool, media: Vec<PathBuf>) -> Game {
        Game {
            id: GameId(1),
            system: "dos".into(),
            path: path.to_path_buf(),
            entry: None,
            title: "Jeu".into(),
            identification: Identification::Unknown,
            size: 0,
            crc32: None,
            sha1: None,
            favorite: false,
            last_played_unix: None,
            play_count: 0,
            media,
            is_dir,
            region: None,
            needs_32x: false,
        }
    }

    fn folder_game(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("Monkey");
        std::fs::create_dir(&game).unwrap();
        for f in files {
            std::fs::write(game.join(f), b"x").unwrap();
        }
        dir
    }

    #[test]
    fn folder_game_becomes_a_folder_spec() {
        let root = folder_game(&["MONKEY.EXE", "setup.exe"]);
        let dir = root.path().join("Monkey");
        let prefs = CorePreferences::builtin();
        let g = game(&dir, true, Vec::new());
        assert_eq!(
            plan_dos_launch(&g, &prefs, &dosbox(), None).unwrap(),
            ContentSpec::Folder {
                dir: dir.clone(),
                name: "Monkey".into(),
                start: None
            }
        );
        // Case ignored: the runtime upper-cases what it hands to the core.
        assert_eq!(
            plan_dos_launch(&g, &prefs, &dosbox(), Some("SETUP.EXE")).unwrap(),
            ContentSpec::Folder {
                dir: dir.clone(),
                name: "Monkey".into(),
                start: Some("SETUP.EXE".into())
            }
        );
        assert!(plan_dos_launch(&g, &prefs, &dosbox(), Some("NOPE.EXE")).is_err());
        // A core that does not read folders refuses them.
        let hatari = CoreId::new("hatari").unwrap();
        assert!(plan_dos_launch(&g, &prefs, &hatari, None).is_err());
    }

    /// A DOS zip has `entry = None` and `path` = the zip: handed over as is (`media_direct`), never extracted.
    #[test]
    fn zips_and_cd_images_are_direct_images_are_copied() {
        let prefs = CorePreferences::builtin();
        for name in [
            "/g/Doom.zip",
            "/g/Doom.dosz",
            "/g/Rebel.iso",
            "/g/Rebel.cue",
            "/g/R.chd",
        ] {
            let g = game(Path::new(name), false, Vec::new());
            match plan_dos_launch(&g, &prefs, &dosbox(), None).unwrap() {
                ContentSpec::File(p) => assert_eq!(p, Path::new(name)),
                ContentSpec::Media { set, rules } => {
                    assert_eq!(set.images, vec![PathBuf::from(name)], "{name}");
                    assert!(
                        rules
                            .media_direct
                            .iter()
                            .any(|e| name.ends_with(e.as_str()))
                    );
                }
                other => panic!("{name}: {other:?}"),
            }
        }
        for name in ["/g/Disk.img", "/g/Disk.ima", "/g/HD.vhd"] {
            let g = game(Path::new(name), false, Vec::new());
            match plan_dos_launch(&g, &prefs, &dosbox(), None).unwrap() {
                ContentSpec::Media { rules, .. } => {
                    assert_eq!(rules.policy, MediaPolicy::WorkingCopy, "{name}")
                }
                other => panic!("{name}: {other:?}"),
            }
        }
        let disks = vec![
            PathBuf::from("/g/X (Disk 1 of 2).img"),
            PathBuf::from("/g/X (Disk 2 of 2).img"),
        ];
        let g = game(&disks[0], false, disks.clone());
        match plan_dos_launch(&g, &prefs, &dosbox(), None).unwrap() {
            ContentSpec::Media { set, .. } => assert_eq!(set.images, disks),
            other => panic!("{other:?}"),
        }
        match plan_dos_launch(
            &game(Path::new("/g/Jeu.m3u8"), false, Vec::new()),
            &prefs,
            &dosbox(),
            None,
        )
        .unwrap()
        {
            ContentSpec::Media { set, .. } => {
                assert_eq!(set.playlist, Some(PathBuf::from("/g/Jeu.m3u8")))
            }
            other => panic!("{other:?}"),
        }
        let conf = game(Path::new("/g/dosbox.conf"), false, Vec::new());
        assert!(plan_dos_launch(&conf, &prefs, &dosbox(), None).is_err());
    }

    #[test]
    fn snapshot_is_planned_until_current() {
        let root = folder_game(&["GAME.EXE"]);
        let dir = root.path().join("Monkey");
        let cache = root.path().join("cache");
        let spec = ContentSpec::Folder {
            dir: dir.clone(),
            name: "Monkey".into(),
            start: None,
        };
        let plan = snapshot_plan(spec.clone(), "Monkey (2)", &cache, &dosbox()).unwrap();
        let DosPlan::NeedsSnapshot { snapshot, .. } = &plan else {
            panic!("{plan:?}");
        };
        assert_eq!(snapshot, &cache.join("dosbox_pure").join("Monkey (2).zip"));
        let cancel = std::sync::atomic::AtomicBool::new(false);
        snapshot_folder(
            &dir,
            snapshot,
            &SnapshotLimits::DEFAULT,
            &mut |_, _| {},
            &cancel,
        )
        .unwrap();
        assert_eq!(
            snapshot_plan(spec.clone(), "Monkey (2)", &cache, &dosbox()).unwrap(),
            DosPlan::Ready(spec)
        );
        let file = ContentSpec::File(PathBuf::from("/g/a.zip"));
        assert_eq!(
            snapshot_plan(file.clone(), "a", &cache, &dosbox()).unwrap(),
            DosPlan::Ready(file)
        );
        assert!(cache_size(&cache) > 0);
        clear_cache(&dos_cache_dir(&cache, &dosbox())).unwrap();
        assert_eq!(cache_size(&cache), 0);
        clear_cache(&dos_cache_dir(&cache, &dosbox())).unwrap();
    }

    #[test]
    fn root_executables_lists_valid_programs_at_the_root_only() {
        let root = folder_game(&[
            "monkey.exe",
            "SETUP.EXE",
            "INSTALL.BAT",
            "go.com",
            "README.TXT",
            "LONGNAME1.EXE",
            "a b.exe",
            "noext",
        ]);
        let dir = root.path().join("Monkey");
        std::fs::create_dir(dir.join("SUB")).unwrap();
        std::fs::write(dir.join("SUB/INNER.EXE"), b"x").unwrap();
        std::fs::create_dir(dir.join("DIR.EXE")).unwrap();
        assert_eq!(
            root_executables(&dir),
            ["GO.COM", "INSTALL.BAT", "MONKEY.EXE", "SETUP.EXE"]
        );
        assert!(root_executables(&root.path().join("absent")).is_empty());
        assert!(is_valid_start("A.EXE"));
        for bad in ["", ".EXE", "A#B.EXE", "A.TXT", "NINECHARS.EXE", "A:B.COM"] {
            assert!(!is_valid_start(bad), "{bad}");
        }
    }

    #[test]
    fn first_launch_until_the_core_writes_its_pure_zip() {
        let dir = tempfile::tempdir().unwrap();
        assert!(first_launch(dir.path(), "Monkey"));
        std::fs::write(dir.path().join("Monkey.pure.zip"), b"PK").unwrap();
        assert!(!first_launch(dir.path(), "Monkey"));
        assert!(first_launch(dir.path(), "Other"));
    }

    #[test]
    fn cli_paths() {
        let root = folder_game(&["START.EXE"]);
        let dir = root.path().join("Monkey");
        let prefs = CorePreferences::builtin();
        let core = dosbox();
        assert_eq!(
            cli_content(&dir, &prefs, Some(&core)).unwrap().unwrap(),
            GameContent::Folder {
                dir: dir.clone(),
                start: None
            }
        );
        assert_eq!(
            cli_content(&dir.join("start.exe"), &prefs, Some(&core))
                .unwrap()
                .unwrap(),
            GameContent::Folder {
                dir: dir.clone(),
                start: Some("START.EXE".into())
            }
        );
        assert!(
            cli_content(Path::new("/g/dosbox.conf"), &prefs, Some(&core))
                .unwrap()
                .is_err()
        );
        let hatari = CoreId::new("hatari").unwrap();
        assert!(cli_content(&dir, &prefs, Some(&hatari)).unwrap().is_err());
        // Files of other cores keep the 1.0 path.
        assert!(cli_content(Path::new("/g/a.zip"), &prefs, Some(&core)).is_none());
        assert!(cli_content(&dir.join("START.EXE"), &prefs, Some(&hatari)).is_none());
    }
}
