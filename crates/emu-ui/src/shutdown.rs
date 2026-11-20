//! Shutdown requested from outside the event loop (SIGINT / SIGTERM / SIGHUP, console close on Windows).
//!
//! The signal handler (installed by `emu-app`, on its own thread) only raises a flag and wakes the loop; the loop
//! then quits through the same path as closing the window, so the game is unloaded (`retro_unload_game`: the
//! core writes its own files, e.g. the Mega CD `.brm`) and the SRAM and configuration are written.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

type Waker = Box<dyn Fn() + Send>;

pub(crate) struct ShutdownFlag {
    requested: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl ShutdownFlag {
    pub(crate) const fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            waker: Mutex::new(None),
        }
    }

    /// Raises the flag and wakes the loop. Returns `true` if a shutdown was already requested.
    pub(crate) fn request(&self) -> bool {
        let already = self.requested.swap(true, Ordering::SeqCst);
        if let Ok(waker) = self.waker.lock()
            && let Some(wake) = waker.as_ref()
        {
            wake();
        }
        already
    }

    pub(crate) fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    /// Wakes a loop sleeping in `ControlFlow::Wait`; replaced by each new event loop, cleared after it.
    pub(crate) fn set_waker(&self, waker: Option<Waker>) {
        if let Ok(mut slot) = self.waker.lock() {
            *slot = waker;
        }
    }
}

static SHUTDOWN: ShutdownFlag = ShutdownFlag::new();

/// Asks the running window (game or library) to quit cleanly: saves written, game unloaded. Callable from any
/// thread (a signal handler thread such as `ctrlc`'s). Returns `true` if a shutdown was already requested, so
/// a second Ctrl+C can force the exit.
pub fn request_shutdown() -> bool {
    SHUTDOWN.request()
}

pub(crate) fn requested() -> bool {
    SHUTDOWN.is_requested()
}

pub(crate) fn set_waker(waker: Option<Waker>) {
    SHUTDOWN.set_waker(waker);
}

#[cfg(test)]
mod tests {
    use super::ShutdownFlag;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn request_raises_the_flag_and_wakes_the_loop() {
        let flag = ShutdownFlag::new();
        let woken = Arc::new(AtomicU32::new(0));
        let w = Arc::clone(&woken);
        flag.set_waker(Some(Box::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        })));
        assert!(!flag.is_requested());
        assert!(!flag.request());
        assert!(flag.is_requested());
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        // Second request: reported as repeated (the binary may then force the exit).
        assert!(flag.request());
        assert_eq!(woken.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn request_without_loop_only_raises_the_flag() {
        let flag = ShutdownFlag::new();
        assert!(!flag.request());
        assert!(flag.is_requested());
    }

    #[test]
    fn request_from_another_thread() {
        let flag = Arc::new(ShutdownFlag::new());
        let f = Arc::clone(&flag);
        std::thread::spawn(move || f.request()).join().unwrap();
        assert!(flag.is_requested());
    }
}
