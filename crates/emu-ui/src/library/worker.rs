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
    /// The task holds the catalog of systems until it ends (see [`Task::spawn_pinning_catalog`]).
    #[cfg(test)]
    pinned: bool,
}

impl<P: Send + 'static, R: Send + 'static> Task<P, R> {
    pub(crate) fn spawn<F>(name: &str, notify: Notify, work: F) -> Self
    where
        F: FnOnce(&AtomicBool, &mut dyn FnMut(P)) -> R + Send + 'static,
    {
        Self::spawn_inner(name, notify, None, work)
    }

    /// [`Task::spawn`] for a task that reads the catalog of systems while it runs (scan, tidying): the catalog
    /// cannot be published (`crate::try_publish_catalog`) until the task ends, whatever the way it ends.
    pub(crate) fn spawn_pinning_catalog<F>(name: &str, notify: Notify, work: F) -> Self
    where
        F: FnOnce(&AtomicBool, &mut dyn FnMut(P)) -> R + Send + 'static,
    {
        Self::spawn_inner(name, notify, Some(crate::CatalogPin::new()), work)
    }

    fn spawn_inner<F>(name: &str, notify: Notify, pin: Option<crate::CatalogPin>, work: F) -> Self
    where
        F: FnOnce(&AtomicBool, &mut dyn FnMut(P)) -> R + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        #[cfg(test)]
        let pinned = pin.is_some();
        let spawned = thread::Builder::new()
            .name(format!("emu-library-{name}"))
            .spawn(move || {
                // Held until the work is over (or the thread dies): the pin is released by its `Drop`.
                let _pin = pin;
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
        Self {
            rx,
            cancel,
            #[cfg(test)]
            pinned,
        }
    }

    /// Whether the catalog is held for this task.
    #[cfg(test)]
    pub(crate) fn pins_catalog(&self) -> bool {
        self.pinned
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Blocks until the worker ends or `timeout` passes; `true` when it ended. Only at exit (window closed).
    pub(crate) fn wait_end(&mut self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(Event::Progress(_)) => {}
                Ok(Event::Done(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => return true,
                Err(mpsc::RecvTimeoutError::Timeout) => return false,
            }
        }
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
    Task::spawn_pinning_catalog("scan", notify, move |cancel, progress| {
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
    // The task reads the catalog of systems all along: it pins it like a scan does.
    Task::spawn_pinning_catalog("dat-download", notify, move |cancel, progress| {
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

/// The sources `search_dats` asks, in this order: at most one request each (ADR 0025 §9).
const SEARCH_SOURCES: [DatSource; 3] = [DatSource::NoIntro, DatSource::Tosec, DatSource::Libretro];

/// A DAT found by [`search_dats`]: its source and the ETag the server gave.
pub(crate) type FoundDat = (DatSource, Option<String>);

/// « Chercher la base d'identification » of a custom system: the DAT named after its libretro `database`, in each
/// of [`SEARCH_SOURCES`] under `base_url`; only a 404 means « not there ». The files found are saved where the DAT
/// update puts them and returned with their ETag. Any other failure (refusal, quota, size limit, disk) stops the
/// search: nothing is concluded.
pub(crate) fn search_dats(
    client: &HttpClient,
    base_url: &str,
    dest_dir: &Path,
    database: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<FoundDat>, String> {
    let mut found = Vec::new();
    for source in SEARCH_SOURCES {
        if cancelled() {
            break;
        }
        let dest = dat_file(dest_dir, source, &emu_library::dat_file_stem(database));
        let url = dat_url(&source_base(base_url, source), database);
        match download_dat(client, &url, &dest, None) {
            Ok(DatDownload::Updated { etag, .. }) => found.push((source, etag)),
            Ok(DatDownload::NotModified) => {}
            Err(emu_assets::AssetError::Http { status: 404 }) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(found)
}

/// Imports the files [`search_dats`] saved for the system `id` (named after `database`), and only those: no
/// request. The catalog must already list the sources (published definition).
pub(crate) fn import_found_dats(
    library: &mut Library,
    dest_dir: &Path,
    id: &str,
    database: &str,
    found: &[FoundDat],
) -> DatSummary {
    let mut summary = DatSummary::default();
    let label = system(id).map_or_else(|| id.to_owned(), |s| s.name.clone());
    for (source, etag) in found {
        let dest = dat_file(dest_dir, *source, &emu_library::dat_file_stem(database));
        let imported = load_dat_file(&dest)
            .and_then(|dat| library.import_dat_source(id, *source, &dat, etag.as_deref()));
        match imported {
            Ok(n) => summary.updated.push((label.clone(), n)),
            Err(e) => summary.failed.push((label.clone(), e.to_string())),
        }
    }
    summary
}

/// The local import of [`import_found_dats`] as a task (pins the catalog like the DAT update).
pub(crate) fn spawn_found_dats_import(
    db: PathBuf,
    dest_dir: PathBuf,
    id: String,
    database: String,
    found: Vec<FoundDat>,
    notify: Notify,
) -> DatDownloadTask {
    Task::spawn_pinning_catalog("dat-import", notify, move |_, _| {
        let mut library = Library::open_existing(&db).map_err(|e| e.to_string())?;
        Ok(import_found_dats(
            &mut library,
            &dest_dir,
            &id,
            &database,
            &found,
        ))
    })
}

/// The stored ETag is sent only when the file is still there: otherwise a 304 would leave it missing for good.
/// Every `(system, source)` is fetched, the Mega CD from `metadat/redump/` (`DatSource::Redump`, design v6.0 §3.4).
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
        .filter(|s| s.libretro_name().is_some())
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
        // No libretro name: no request and no file (filtered above).
        let name = s.libretro_name().unwrap_or_default();
        let dest = dat_file(dest_dir, *source, &emu_library::dat_file_stem(name));
        let etag = library
            .dat_info_for(&s.id, *source)
            .ok()
            .flatten()
            .and_then(|info| info.etag)
            .filter(|_| dest.is_file());
        let base = source_base(base_url, *source);
        match download_dat(client, &dat_url(&base, name), &dest, etag.as_deref()) {
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
