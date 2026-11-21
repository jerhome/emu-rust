//! Background work of the library (ADR 0014 §7): scan and DAT import run on `std` threads with their own SQLite
//! connection; progress and result come back through a channel drained once per frame. `notify` wakes the
//! event loop (egui repaint callback → winit `EventLoopProxy`), so the UI thread never polls nor blocks.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use emu_assets::http::{HttpClient, HttpConfig};
use emu_assets::{DatDownload, dat_url, download_dat};
use emu_library::{
    DatSource, Library, ScanOptions, ScanProgress, ScanReport, SystemInfo, load_dat_file, system,
    systems,
};

use super::grid::detect_dat_system;
use super::text;

pub(crate) type Notify = Arc<dyn Fn() + Send + Sync>;

pub(crate) enum Event<P, R> {
    Progress(P),
    Done(R),
}

/// Dropping a task cancels it (the thread ends at its next check).
pub(crate) struct Task<P, R> {
    rx: Receiver<Event<P, R>>,
    cancel: Arc<AtomicBool>,
}

impl<P: Send + 'static, R: Send + 'static> Task<P, R> {
    pub(crate) fn spawn<F>(name: &str, notify: Notify, work: F) -> Self
    where
        F: FnOnce(&AtomicBool, &mut dyn FnMut(P)) -> R + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let spawned = thread::Builder::new()
            .name(format!("emu-library-{name}"))
            .spawn(move || {
                let mut progress = |p: P| {
                    if tx.send(Event::Progress(p)).is_ok() {
                        notify();
                    }
                };
                let result = work(&flag, &mut progress);
                if tx.send(Event::Done(result)).is_ok() {
                    notify();
                }
            });
        if let Err(e) = spawned {
            // The sender was dropped with the closure: `poll` reports the task as interrupted.
            tracing::error!("impossible de lancer la tâche {name} : {e}");
        }
        Self { rx, cancel }
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Events since the last call. `Some(None)` as the second value: the worker ended without a result (panic).
    pub(crate) fn poll(&mut self) -> (Vec<P>, Option<Option<R>>) {
        let mut progress = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(Event::Progress(p)) => progress.push(p),
                Ok(Event::Done(r)) => return (progress, Some(Some(r))),
                Err(TryRecvError::Empty) => return (progress, None),
                Err(TryRecvError::Disconnected) => return (progress, Some(None)),
            }
        }
    }
}

impl<P, R> Drop for Task<P, R> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub(crate) type ScanTask = Task<ScanProgress, Result<ScanReport, String>>;
/// Result: `(system id, entries)`.
pub(crate) type DatTask = Task<(), Result<(String, u64), String>>;

pub(crate) fn spawn_scan(db: PathBuf, roots: Vec<PathBuf>, notify: Notify) -> ScanTask {
    Task::spawn("scan", notify, move |cancel, progress| {
        let mut library = Library::open_existing(&db).map_err(|e| e.to_string())?;
        let options = ScanOptions {
            roots,
            ..ScanOptions::default()
        };
        library
            .scan(&options, cancel, &mut |p: &ScanProgress| {
                progress(p.clone())
            })
            .map_err(|e| e.to_string())
    })
}

/// `system`: library system id chosen by the user, or `None` to detect it from the DAT name / file name.
pub(crate) fn spawn_dat_import(
    db: PathBuf,
    path: PathBuf,
    system_id: Option<String>,
    notify: Notify,
) -> DatTask {
    Task::spawn("dat", notify, move |_cancel, _progress| {
        let dat = load_dat_file(&path).map_err(|e| e.to_string())?;
        let id = match system_id {
            Some(id) => id,
            None => {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                detect_dat_system(&dat.name, &stem, systems())
                    .map(|s| s.id.clone())
                    .ok_or_else(|| text::dat_unknown_system(&dat.name))?
            }
        };
        if system(&id).is_none() {
            return Err(text::dat_unknown_system(&id));
        }
        let mut library = Library::open_existing(&db).map_err(|e| e.to_string())?;
        let n = library
            .import_dat(&id, &dat, None)
            .map_err(|e| e.to_string())?;
        Ok((id, n))
    })
}

/// Progress of a DAT update: `done` systems out of `total`, `system` being downloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatProgress {
    pub done: usize,
    pub total: usize,
    pub system: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DatSummary {
    /// `(system name, entries)`.
    pub updated: Vec<(String, u64)>,
    pub unchanged: usize,
    /// `(system name, message)`; messages never hold a URL (`AssetError`).
    pub failed: Vec<(String, String)>,
    /// Stopped after consecutive network errors.
    pub offline: bool,
}

pub(crate) type DatDownloadTask = Task<DatProgress, Result<DatSummary, String>>;

/// Network errors in a row after which the remaining systems are skipped (each would wait for its timeout).
const DAT_MAX_NETWORK_ERRORS: usize = 2;

/// Root of libretro-database: the DAT of a source is under `<root><DatSource::repo_dir>/<database>.dat`
/// (`emu_assets::DAT_*_BASE_URL`, checked by a test).
pub(crate) const DAT_REPO_ROOT: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master/";

/// Download folder of one source: TOSEC and WHDLoad (`dat/`) give the Amiga DAT the same file name.
fn dat_file(dest_dir: &Path, source: DatSource, database: &str) -> PathBuf {
    dest_dir.join(source.id()).join(format!("{database}.dat"))
}

/// Base URL of a source under `repo_root` (ends with `/`).
fn source_base(repo_root: &str, source: DatSource) -> String {
    format!("{repo_root}{}/", source.repo_dir())
}

/// Downloads (conditional GET) and imports the DAT of every `(system, source)` of the catalog; `base_url` is the
/// libretro-database root ([`DAT_REPO_ROOT`]; tests: a local server).
pub(crate) fn spawn_dat_download(
    db: PathBuf,
    dest_dir: PathBuf,
    base_url: String,
    http: HttpConfig,
    notify: Notify,
) -> DatDownloadTask {
    Task::spawn("dat-download", notify, move |cancel, progress| {
        let client = HttpClient::new(http).map_err(|e| e.to_string())?;
        let mut library = Library::open_existing(&db).map_err(|e| e.to_string())?;
        Ok(download_dats(
            &mut library,
            &client,
            &base_url,
            &dest_dir,
            systems(),
            cancel,
            progress,
        ))
    })
}

/// The stored ETag is sent only when the file is still there: otherwise a 304 would leave it missing for good.
/// Systems skipped by the automatic DAT download: their DAT is not under `metadat/no-intro/` (redump: 6.0).
const NO_AUTO_DAT: &[&str] = &["megacd"];

pub(crate) fn download_dats(
    library: &mut Library,
    client: &HttpClient,
    base_url: &str,
    dest_dir: &Path,
    catalog: &[SystemInfo],
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(DatProgress),
) -> DatSummary {
    let mut summary = DatSummary::default();
    let mut network_errors = 0;
    let jobs: Vec<(&SystemInfo, DatSource)> = catalog
        .iter()
        .filter(|s| !NO_AUTO_DAT.contains(&s.id.as_str()))
        .flat_map(|s| s.dats.iter().map(move |d| (s, *d)))
        .collect();
    for (i, (s, source)) in jobs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // Two sources of one system are told apart in messages (« Amiga (tosec) »).
        let label = if s.dats.len() > 1 {
            format!("{} ({})", s.name, source.id())
        } else {
            s.name.clone()
        };
        progress(DatProgress {
            done: i,
            total: jobs.len(),
            system: label.clone(),
        });
        let dest = dat_file(dest_dir, *source, &s.database);
        let etag = library
            .dat_info_for(&s.id, *source)
            .ok()
            .flatten()
            .and_then(|info| info.etag)
            .filter(|_| dest.is_file());
        let base = source_base(base_url, *source);
        match download_dat(client, &dat_url(&base, &s.database), &dest, etag.as_deref()) {
            Ok(DatDownload::NotModified) => {
                network_errors = 0;
                summary.unchanged += 1;
            }
            Ok(DatDownload::Updated { etag, .. }) => {
                network_errors = 0;
                let imported = load_dat_file(&dest).and_then(|dat| {
                    library.import_dat_source(&s.id, *source, &dat, etag.as_deref())
                });
                match imported {
                    Ok(n) => summary.updated.push((label, n)),
                    Err(e) => summary.failed.push((label, e.to_string())),
                }
            }
            Err(e) => {
                let transient = e.is_transient();
                summary.failed.push((label, e.to_string()));
                if transient {
                    network_errors += 1;
                    if network_errors >= DAT_MAX_NETWORK_ERRORS {
                        summary.offline = true;
                        break;
                    }
                } else {
                    network_errors = 0;
                }
            }
        }
    }
    summary
}
