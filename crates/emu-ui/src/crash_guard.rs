//! Launch witness (design 8.0 §2.5, ADR 0025 §5 « témoin »): a marker file created before the game is loaded,
//! that survives a crash and is reported at the next start. All cores, official ones included.
//!
//! `launch-<pid>.tmp` is created and locked (`File::try_lock`), filled, then renamed to `launch-<pid>.marker`
//! (the lock follows the open file): another instance never sees an unlocked marker being created. The marker is
//! removed when the game ends cleanly (`Drop`). At start-up a `.marker` whose lock is free belongs to a process
//! that died during a launch: it is reported once and removed. A failure never blocks a launch.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Largest marker read.
const MAX_MARKER_BYTES: u64 = 16 * 1024;
const PREFIX: &str = "launch-";
const MARKER_EXT: &str = "marker";
const TMP_EXT: &str = "tmp";
/// A `.tmp` younger than this is never removed by `orphan_markers`.
const TMP_MIN_AGE: Duration = Duration::from_secs(600);

/// What was being launched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LaunchWhat {
    pub core: String,
    pub system: String,
    pub game: String,
    /// A core of a custom system that has not been tried (the notice then offers another core).
    pub untested: bool,
}

/// An orphan marker found at start-up: the previous run stopped during this launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CrashNotice {
    pub what: LaunchWhat,
    pub started_unix: u64,
}

#[derive(Serialize, Deserialize)]
struct MarkerFile {
    #[serde(flatten)]
    what: LaunchWhat,
    started_unix: u64,
}

/// `.tmp` locked then renamed to `.marker`; removed on `Drop`.
#[derive(Debug)]
pub(crate) struct LaunchMarker {
    path: PathBuf,
    /// Holds the lock until the end.
    _file: File,
}

fn file_name(pid: u32, ext: &str) -> String {
    format!("{PREFIX}{pid}.{ext}")
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Creates the marker of the launch in `cache`. `None`: warned, never blocking.
pub(crate) fn begin_launch(cache: &Path, what: &LaunchWhat) -> Option<LaunchMarker> {
    match create_marker(cache, what, std::process::id(), now_unix()) {
        Ok(marker) => Some(marker),
        Err(e) => {
            tracing::warn!("témoin de lancement non créé : {e}");
            None
        }
    }
}

fn create_marker(
    cache: &Path,
    what: &LaunchWhat,
    pid: u32,
    started_unix: u64,
) -> Result<LaunchMarker, String> {
    fs::create_dir_all(cache).map_err(|e| format!("{} : {e}", cache.display()))?;
    let text = toml::to_string(&MarkerFile {
        what: what.clone(),
        started_unix,
    })
    .map_err(|e| e.to_string())?;
    let tmp = cache.join(file_name(pid, TMP_EXT));
    let marker = cache.join(file_name(pid, MARKER_EXT));
    let mut file = File::create(&tmp).map_err(|e| format!("{} : {e}", tmp.display()))?;
    let made = file
        .try_lock()
        .map_err(|e| e.to_string())
        .and_then(|()| file.write_all(text.as_bytes()).map_err(|e| e.to_string()))
        .and_then(|()| file.flush().map_err(|e| e.to_string()))
        .and_then(|()| fs::rename(&tmp, &marker).map_err(|e| e.to_string()));
    match made {
        Ok(()) => Ok(LaunchMarker {
            path: marker,
            _file: file,
        }),
        Err(e) => {
            // A `.tmp` is never left behind.
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

impl Drop for LaunchMarker {
    fn drop(&mut self) {
        // Removed while still locked (the handle closes after this): no instance sees a free lock on it.
        if let Err(e) = fs::remove_file(&self.path) {
            tracing::warn!("témoin de lancement non retiré : {e}");
        }
    }
}

/// `launch-<digits>.<ext>`.
fn is_witness_name(name: &str, ext: &str) -> bool {
    name.strip_prefix(PREFIX)
        .and_then(|rest| rest.strip_suffix(ext))
        .and_then(|rest| rest.strip_suffix('.'))
        .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
}

/// The markers of dead processes, oldest first; each one is removed once read. A marker whose lock is held belongs
/// to a live instance (this one included) and is left alone. An unreadable marker is removed without a notice;
/// an old `.tmp` whose lock is free (crash while creating it) is removed.
pub(crate) fn orphan_markers(cache: &Path) -> Vec<CrashNotice> {
    let Ok(entries) = fs::read_dir(cache) else {
        return Vec::new();
    };
    let mut notices = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let is_marker = is_witness_name(name, MARKER_EXT);
        if !(is_marker || is_witness_name(name, TMP_EXT))
            || !entry.file_type().is_ok_and(|t| t.is_file())
        {
            continue;
        }
        let path = entry.path();
        let Ok(mut file) = File::open(&path) else {
            continue;
        };
        if file.try_lock().is_err() {
            continue;
        }
        // A `.tmp` may be one `create_marker` has just created and not yet locked: only an old one is removed.
        if !is_marker
            && file
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_none_or(|age| age < TMP_MIN_AGE)
        {
            continue;
        }
        if is_marker {
            let mut text = String::new();
            let parsed = (&mut file)
                .take(MAX_MARKER_BYTES)
                .read_to_string(&mut text)
                .ok()
                .and_then(|_| toml::from_str::<MarkerFile>(&text).ok());
            if let Some(marker) = parsed {
                // The file is the user's to edit: what is shown is cleaned (bidi and control characters).
                let clean = |s: &str| crate::ui::text::sanitize_core_line(s);
                notices.push(CrashNotice {
                    what: LaunchWhat {
                        core: clean(&marker.what.core),
                        system: clean(&marker.what.system),
                        game: clean(&marker.what.game),
                        untested: marker.what.untested,
                    },
                    started_unix: marker.started_unix,
                });
            }
        }
        if let Err(e) = fs::remove_file(&path) {
            tracing::warn!("témoin {} non retiré : {e}", path.display());
        }
    }
    notices.sort_by_key(|n| n.started_unix);
    notices
}

/// Notices of the previous run, read once at start-up and taken by the library when it opens.
static PENDING: std::sync::Mutex<Vec<CrashNotice>> = std::sync::Mutex::new(Vec::new());

/// Start-up step of the witness (design 8.0 §2.5): the dry-run folders of dead instances are removed, then the
/// orphan markers are read (and removed). Must run before any [`begin_launch`] of this process: in a container
/// the pid can be the one of the dead instance. The notices wait for [`take_crash_notices`].
pub fn check_previous_launches(dirs: &crate::DataDirs) {
    let notices = read_previous_launches(&dirs.cache_dir());
    if let Ok(mut pending) = PENDING.lock() {
        *pending = notices;
    }
}

/// [`check_previous_launches`] without the global: the work of one start-up in `cache`.
fn read_previous_launches(cache: &Path) -> Vec<CrashNotice> {
    crate::dry_run::clean_stale_dry_runs(cache);
    let notices = orphan_markers(cache);
    for n in &notices {
        tracing::warn!(
            "la session précédente s'est arrêtée pendant {} avec {} (système {})",
            n.what.game,
            n.what.core,
            n.what.system
        );
    }
    notices
}

/// The notices found by [`check_previous_launches`]; empty after the first call.
pub(crate) fn take_crash_notices() -> Vec<CrashNotice> {
    PENDING
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn what(game: &str) -> LaunchWhat {
        LaunchWhat {
            core: "stella".into(),
            system: "atari2600".into(),
            game: game.into(),
            untested: true,
        }
    }

    fn names(cache: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(cache)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    /// A marker left by a process that died: written by hand, nobody holds its lock.
    fn dead_marker(cache: &Path, pid: u32, what: &LaunchWhat, started: u64) -> PathBuf {
        let text = toml::to_string(&MarkerFile {
            what: what.clone(),
            started_unix: started,
        })
        .unwrap();
        let path = cache.join(file_name(pid, MARKER_EXT));
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn begin_launch_leaves_a_marker_and_never_a_tmp() {
        let cache = tempfile::tempdir().unwrap();
        let marker = begin_launch(cache.path(), &what("Jeu.bin")).expect("marker");
        assert_eq!(
            names(cache.path()),
            vec![file_name(std::process::id(), MARKER_EXT)]
        );
        drop(marker);
        assert!(names(cache.path()).is_empty());
    }

    #[test]
    fn begin_launch_creates_the_cache_folder() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let marker = begin_launch(&cache, &what("g")).expect("marker");
        assert!(cache.is_dir());
        drop(marker);
    }

    #[test]
    fn a_live_marker_is_not_an_orphan() {
        let cache = tempfile::tempdir().unwrap();
        let marker = begin_launch(cache.path(), &what("g")).unwrap();
        assert!(orphan_markers(cache.path()).is_empty());
        assert_eq!(names(cache.path()).len(), 1, "left alone");
        drop(marker);
    }

    #[test]
    fn a_marker_of_a_dead_process_is_reported_once_with_its_content_and_removed() {
        let cache = tempfile::tempdir().unwrap();
        let w = what("Jeu \"étrange\".bin");
        dead_marker(cache.path(), 4242, &w, 1_700_000_000);
        let notices = orphan_markers(cache.path());
        assert_eq!(
            notices,
            vec![CrashNotice {
                what: w,
                started_unix: 1_700_000_000
            }]
        );
        assert!(names(cache.path()).is_empty());
        assert!(orphan_markers(cache.path()).is_empty());
    }

    #[test]
    fn what_a_notice_shows_is_cleaned_of_control_and_bidi_characters() {
        let cache = tempfile::tempdir().unwrap();
        let mut w = what("ligne\none\u{202E}gnp.bin");
        w.core = "co\u{200B}re".into();
        dead_marker(cache.path(), 4244, &w, 5);
        let notice = orphan_markers(cache.path()).remove(0);
        assert_eq!(notice.what.game, "ligne one gnp.bin");
        assert!(!notice.what.core.contains('\u{200B}'));
        assert!(!notice.what.game.contains('\u{202E}'));
    }

    #[test]
    fn the_start_up_reports_a_dead_marker_before_any_launch_of_this_process() {
        let cache = tempfile::tempdir().unwrap();
        // A container: the dead instance had this very pid.
        dead_marker(cache.path(), std::process::id(), &what("old.bin"), 7);
        let notices = read_previous_launches(cache.path());
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].what.game, "old.bin");
        let marker = begin_launch(cache.path(), &what("new.bin")).expect("marker");
        assert!(
            read_previous_launches(cache.path()).is_empty(),
            "ours is alive"
        );
        drop(marker);
        assert!(names(cache.path()).is_empty());
    }

    #[test]
    fn a_locked_foreign_marker_is_left_alone() {
        let cache = tempfile::tempdir().unwrap();
        let path = dead_marker(cache.path(), 4243, &what("g"), 5);
        let holder = File::open(&path).unwrap();
        holder.try_lock().unwrap();
        assert!(orphan_markers(cache.path()).is_empty());
        assert!(path.exists());
        drop(holder);
        assert_eq!(orphan_markers(cache.path()).len(), 1);
    }

    #[test]
    fn orphans_come_oldest_first() {
        let cache = tempfile::tempdir().unwrap();
        dead_marker(cache.path(), 1, &what("recent"), 20);
        dead_marker(cache.path(), 2, &what("old"), 10);
        let games: Vec<String> = orphan_markers(cache.path())
            .into_iter()
            .map(|n| n.what.game)
            .collect();
        assert_eq!(games, ["old", "recent"]);
    }

    #[test]
    fn unreadable_markers_and_stale_tmp_files_are_removed_without_a_notice() {
        let cache = tempfile::tempdir().unwrap();
        fs::write(cache.path().join(file_name(7, MARKER_EXT)), "garbage = [").unwrap();
        fs::write(
            cache.path().join(file_name(8, MARKER_EXT)),
            vec![b'a'; usize::try_from(MAX_MARKER_BYTES).unwrap() * 2],
        )
        .unwrap();
        let tmp = File::create(cache.path().join(file_name(9, TMP_EXT))).unwrap();
        tmp.set_modified(SystemTime::now() - 2 * TMP_MIN_AGE)
            .unwrap();
        drop(tmp);
        // A recent `.tmp` with a free lock may be being created: kept.
        fs::write(cache.path().join(file_name(10, TMP_EXT)), "half").unwrap();
        fs::write(cache.path().join("launch-x.marker"), "other").unwrap();
        fs::write(cache.path().join("launch-1.marker.bak"), "other").unwrap();
        assert!(orphan_markers(cache.path()).is_empty());
        assert_eq!(
            names(cache.path()),
            vec!["launch-1.marker.bak", "launch-10.tmp", "launch-x.marker"]
        );
    }

    #[test]
    fn a_live_tmp_is_never_removed() {
        let cache = tempfile::tempdir().unwrap();
        let path = cache.path().join(file_name(11, TMP_EXT));
        let creating = File::create(&path).unwrap();
        creating.try_lock().unwrap();
        assert!(orphan_markers(cache.path()).is_empty());
        assert!(path.exists());
        drop(creating);
    }

    #[test]
    fn a_failure_never_blocks_and_leaves_no_tmp() {
        let root = tempfile::tempdir().unwrap();
        // The cache path is a file: nothing can be created in it.
        let blocked = root.path().join("cache");
        fs::write(&blocked, b"x").unwrap();
        assert!(begin_launch(&blocked, &what("g")).is_none());
        // The `.tmp` name is taken by a folder: the creation fails and the launch goes on.
        let cache = root.path().join("ok");
        fs::create_dir_all(cache.join(file_name(std::process::id(), TMP_EXT))).unwrap();
        assert!(begin_launch(&cache, &what("g")).is_none());
        assert_eq!(
            names(&cache),
            vec![file_name(std::process::id(), TMP_EXT)],
            "only the folder in the way"
        );
    }

    #[test]
    fn orphans_of_a_missing_cache_are_none() {
        let root = tempfile::tempdir().unwrap();
        assert!(orphan_markers(&root.path().join("absent")).is_empty());
    }
}
