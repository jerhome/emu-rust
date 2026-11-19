//! Background work of the library (ADR 0014 §7): scan and DAT import run on `std` threads with their own SQLite
//! connection; progress and result come back through a channel drained once per frame. `notify` wakes the
//! event loop (egui repaint callback → winit `EventLoopProxy`), so the UI thread never polls nor blocks.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use emu_library::{Library, ScanOptions, ScanProgress, ScanReport, load_dat_file, system, systems};

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
        let (mut library, _) = Library::open(&db).map_err(|e| e.to_string())?;
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
        let (mut library, _) = Library::open(&db).map_err(|e| e.to_string())?;
        let n = library
            .import_dat(&id, &dat, None)
            .map_err(|e| e.to_string())?;
        Ok((id, n))
    })
}
