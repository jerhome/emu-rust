//! Background jobs for the UI thread: a closure runs on its own thread and reports progress and its result
//! through a channel polled once per egui frame (no blocking call on the UI thread, ADR 0010).
//! Generic so that cover downloads can reuse it.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::http::{CancelToken, Progress};

/// Called from the worker thread after each event (e.g. `egui::Context::request_repaint`).
pub type Notify = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug)]
pub enum JobEvent<T> {
    Progress(Progress),
    /// Last event; the job is over.
    Done(T),
}

/// Progress events are forwarded at most this often (the final `Done` always is).
const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);

/// Dropping a `Job` cancels it; the thread ends at its next cancellation check.
pub struct Job<T> {
    rx: Receiver<JobEvent<T>>,
    cancel: CancelToken,
    finished: bool,
}

impl<T: Send + 'static> Job<T> {
    /// `name` names the thread (`emu-job-<name>`).
    pub fn spawn<F>(name: &str, notify: Notify, work: F) -> Self
    where
        F: FnOnce(&mut dyn FnMut(Progress), &CancelToken) -> T + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let cancel = CancelToken::new();
        let token = cancel.clone();
        let spawned = thread::Builder::new()
            .name(format!("emu-job-{name}"))
            .spawn(move || {
                let mut last: Option<Instant> = None;
                let mut progress = |p: Progress| {
                    let now = Instant::now();
                    if last.is_some_and(|t| now.duration_since(t) < PROGRESS_INTERVAL) {
                        return;
                    }
                    last = Some(now);
                    // The receiver is gone only when the job was dropped: nothing left to report to.
                    if tx.send(JobEvent::Progress(p)).is_ok() {
                        notify();
                    }
                };
                let result = work(&mut progress, &token);
                if tx.send(JobEvent::Done(result)).is_ok() {
                    notify();
                }
            });
        let finished = match spawned {
            Ok(_) => false,
            Err(e) => {
                tracing::error!("impossible de lancer la tâche {name} : {e}");
                true
            }
        };
        Self {
            rx,
            cancel,
            finished,
        }
    }

    /// Events received since the last call, without blocking.
    pub fn poll(&mut self) -> Vec<JobEvent<T>> {
        let mut events = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(event) => {
                    if matches!(event, JobEvent::Done(_)) {
                        self.finished = true;
                    }
                    events.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The worker ended without `Done`: it panicked (or could not start).
                    self.finished = true;
                    break;
                }
            }
        }
        events
    }

    /// `true` once [`Job::poll`] returned `Done`, or when the worker ended without a result (panic, thread
    /// creation failure): the caller can then forget the job instead of waiting forever.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    fn wait_done<T: Send + 'static>(job: &mut Job<T>) -> Vec<JobEvent<T>> {
        let start = Instant::now();
        let mut all = Vec::new();
        while !job.is_finished() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "job never finished"
            );
            all.extend(job.poll());
            thread::sleep(Duration::from_millis(5));
        }
        all
    }

    #[test]
    fn reports_progress_then_result_and_notifies() {
        let notified = Arc::new(AtomicUsize::new(0));
        let n = notified.clone();
        let mut job = Job::spawn(
            "test",
            Arc::new(move || {
                n.fetch_add(1, Ordering::SeqCst);
            }),
            |progress, _| {
                progress(Progress {
                    done: 1,
                    total: Some(2),
                });
                42
            },
        );
        let events = wait_done(&mut job);
        assert!(matches!(
            events.first(),
            Some(JobEvent::Progress(Progress { done: 1, .. }))
        ));
        assert!(matches!(events.last(), Some(JobEvent::Done(42))));
        assert_eq!(notified.load(Ordering::SeqCst), 2);
        assert!(job.poll().is_empty());
    }

    #[test]
    fn progress_is_throttled() {
        let mut job = Job::spawn("burst", Arc::new(|| {}), |progress, _| {
            for done in 0..10_000 {
                progress(Progress { done, total: None });
            }
        });
        let events = wait_done(&mut job);
        assert!(events.len() < 100, "{} events", events.len());
    }

    #[test]
    fn cancel_reaches_the_worker() {
        let mut job = Job::spawn("cancel", Arc::new(|| {}), |_, cancel| {
            let start = Instant::now();
            while !cancel.is_cancelled() {
                assert!(start.elapsed() < Duration::from_secs(10));
                thread::sleep(Duration::from_millis(1));
            }
            "stopped"
        });
        job.cancel();
        assert!(job.is_cancelled());
        let events = wait_done(&mut job);
        assert!(matches!(events.last(), Some(JobEvent::Done("stopped"))));
    }

    #[test]
    fn drop_cancels() {
        let (tx, rx) = mpsc::channel();
        let job = Job::spawn("drop", Arc::new(|| {}), move |_, cancel| {
            let token = cancel.clone();
            tx.send(token).unwrap();
        });
        let token = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        drop(job);
        assert!(token.is_cancelled());
    }

    #[test]
    fn panicking_worker_finishes_without_result() {
        let mut job: Job<u32> = Job::spawn("panic", Arc::new(|| {}), |_, _| panic!("boom"));
        let events = wait_done(&mut job);
        assert!(events.is_empty());
    }
}
