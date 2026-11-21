//! Host platform facts: container detection and the "open the ROM folder" decision (design v3.0 §2.3, §4).
//! The decision is pure ([`plan_open_folder`]); [`open_folder`] adds the `opener::open` call. No shell is involved.

use std::path::{Path, PathBuf};

use emu_assets::cores::Platform;

/// Set to `1` by the Docker entrypoint: the app runs in a container, where a host file manager cannot be opened.
pub(crate) const CONTAINER_ENV: &str = "EMU_RUST_CONTAINER";
/// Optional host path of the ROM folder, shown as a hint when the app runs in a container.
pub(crate) const HOST_ROMS_ENV: &str = "EMU_HOST_ROMS_DIR";

pub(crate) fn running_in_container() -> bool {
    is_container(std::env::var(CONTAINER_ENV).ok().as_deref())
}

fn is_container(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Host path hint from `EMU_HOST_ROMS_DIR` (empty = none).
pub(crate) fn host_roms_hint() -> Option<String> {
    clean_hint(std::env::var(HOST_ROMS_ENV).ok())
}

fn clean_hint(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderOutcome {
    Opened,
    /// Cannot open a file manager: show the path (and where it is on the host, if known).
    ShowPath {
        path: PathBuf,
        host_hint: Option<String>,
    },
}

/// What to do to "open the folder": try the system opener, or only show the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FolderPlan {
    TryOpen,
    ShowPath {
        path: PathBuf,
        host_hint: Option<String>,
    },
}

/// Container first (before any opener call, which would fail or open something unexpected): show the path.
pub(crate) fn plan_open_folder(
    path: &Path,
    in_container: bool,
    host_hint: Option<String>,
) -> FolderPlan {
    if in_container {
        FolderPlan::ShowPath {
            path: path.to_owned(),
            host_hint,
        }
    } else {
        FolderPlan::TryOpen
    }
}

/// Opens `path` in the system file manager, or says where it is when that is impossible (container, no opener,
/// failure). May wait for the opener process: call it off the UI thread ([`spawn_open_folder`]).
pub fn open_folder(path: &Path) -> FolderOutcome {
    open_folder_with(path, running_in_container(), host_roms_hint(), |p| {
        opener::open(p).map_err(|e| e.to_string())
    })
}

/// [`open_folder`] with its inputs injected (tests): the opener is never called in a container.
fn open_folder_with(
    path: &Path,
    in_container: bool,
    host_hint: Option<String>,
    open: impl FnOnce(&Path) -> Result<(), String>,
) -> FolderOutcome {
    match plan_open_folder(path, in_container, host_hint.clone()) {
        FolderPlan::ShowPath { path, host_hint } => FolderOutcome::ShowPath { path, host_hint },
        FolderPlan::TryOpen => match open(path) {
            Ok(()) => FolderOutcome::Opened,
            Err(e) => {
                tracing::warn!("ouverture du dossier impossible : {e}");
                FolderOutcome::ShowPath {
                    path: path.to_owned(),
                    host_hint,
                }
            }
        },
    }
}

/// Runs [`open_folder`] on a worker thread; the receiver yields exactly one outcome (a failure to spawn the
/// thread yields the `ShowPath` fallback).
pub(crate) fn spawn_open_folder(path: PathBuf) -> std::sync::mpsc::Receiver<FolderOutcome> {
    let (tx, rx) = std::sync::mpsc::channel();
    let fallback = FolderOutcome::ShowPath {
        path: path.clone(),
        host_hint: host_roms_hint(),
    };
    let tx2 = tx.clone();
    let spawned = std::thread::Builder::new()
        .name("open-folder".into())
        .spawn(move || {
            let _ = tx.send(open_folder(&path));
        });
    if spawned.is_err() {
        let _ = tx2.send(fallback);
    }
    rx
}

/// Current OS as a core-catalog platform; `None` on unsupported targets.
pub(crate) fn current_platform() -> Option<Platform> {
    Platform::current()
}

/// `recommended_on` of a core contains the running platform (« recommandé sur Mac » shows on every OS; this is
/// for highlighting only, never for choosing the default core).
pub(crate) fn recommended_here(recommended_on: &[Platform]) -> bool {
    current_platform().is_some_and(|p| recommended_on.contains(&p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_only_for_exactly_one() {
        assert!(is_container(Some("1")));
        for v in [None, Some(""), Some("0"), Some("true"), Some("11")] {
            assert!(!is_container(v));
        }
    }

    #[test]
    fn hint_is_trimmed_and_empty_means_none() {
        assert_eq!(clean_hint(None), None);
        assert_eq!(clean_hint(Some("  ".into())), None);
        assert_eq!(
            clean_hint(Some(" C:\\Roms ".into())),
            Some("C:\\Roms".to_owned())
        );
    }

    #[test]
    fn container_never_tries_the_opener() {
        let p = Path::new("/data/roms");
        assert_eq!(
            plan_open_folder(p, true, Some("D:\\roms".into())),
            FolderPlan::ShowPath {
                path: p.to_owned(),
                host_hint: Some("D:\\roms".into())
            }
        );
        assert_eq!(plan_open_folder(p, false, None), FolderPlan::TryOpen);
    }

    #[test]
    fn open_folder_outcomes() {
        let p = Path::new("/r");
        let ok = open_folder_with(p, false, None, |_| Ok(()));
        assert_eq!(ok, FolderOutcome::Opened);
        let failed = open_folder_with(p, false, Some("h".into()), |_| Err("no".into()));
        assert_eq!(
            failed,
            FolderOutcome::ShowPath {
                path: p.to_owned(),
                host_hint: Some("h".into())
            }
        );
        let container = open_folder_with(p, true, None, |_| panic!("opener called in a container"));
        assert!(matches!(container, FolderOutcome::ShowPath { .. }));
    }

    #[test]
    fn recommended_here_matches_current_platform() {
        let Some(cur) = current_platform() else {
            return;
        };
        assert!(recommended_here(&[cur]));
        assert!(!recommended_here(&[]));
    }
}
