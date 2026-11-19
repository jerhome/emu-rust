//! Phase 2 frame scheduling: audio-driven sync with a wall-clock fallback, pause and fast-forward
//! (docs/design/phase2-frontend.md §3, ADR 0010). Emulation stays on the UI thread.

use std::time::{Duration, Instant};

/// Most `Run` decisions in a row before `poll` forces a `Wait` so the event loop gets a turn.
pub const MAX_BURST: u32 = 3;

/// Default fast-forward speed (× core fps).
pub const DEFAULT_FAST_FORWARD_RATIO: f64 = 4.0;

/// How normal-speed frames are scheduled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SyncMode {
    /// Run a frame whenever the audio queue is below its target (falls back to `Clock` while `poll` gets no
    /// `AudioLevel`: no device, stream failed, audio paused).
    #[default]
    Audio,
    /// Wall clock at `timing.fps` (phase 1 behaviour, `FramePacer`).
    Clock,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Speed {
    Normal,
    /// Wall clock at `fps × ratio`, audio ignored. `ratio <= 1` or non-finite = unlimited (bounded by
    /// `MAX_BURST` per event-loop turn).
    FastForward {
        ratio: f64,
    },
    Paused,
}

/// Audio queue as seen by the producer (`AudioOutput::level`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioLevel {
    /// Audio queued for the device, at device rate.
    pub buffered: Duration,
    /// Set point of the DRC (`AudioConfig::latency`).
    pub target: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaceDecision {
    /// Run one emulated frame now, then call `poll` again.
    Run,
    /// Nothing to do before this instant (`ControlFlow::WaitUntil`).
    WaitUntil(Instant),
    /// Paused: no deadline, wait for window events (`ControlFlow::Wait`).
    Idle,
}

/// Decides when to run emulated frames.
///
/// Rules:
/// - `Paused` → `Idle`.
/// - `FastForward` → wall clock at `fps × ratio`.
/// - `Normal` + `Audio` + `Some(level)` → `Run` while `level.buffered < level.target`, else
///   `WaitUntil(now + (buffered − target))` clamped to `[1 ms, one frame period]`.
/// - `Normal` otherwise → wall clock at `fps` (no backlog after a stall, as `FramePacer`).
/// - Never more than `MAX_BURST` consecutive `Run`; switching speed or mode never produces a burst
///   (the clock restarts from `now`).
#[derive(Debug, Clone)]
pub struct Pacer {
    _todo: (),
}

impl Pacer {
    pub fn new(fps: f64, mode: SyncMode, now: Instant) -> Self {
        todo!("{fps} {mode:?} {now:?}")
    }

    /// Invalid rates fall back to 60 fps with one `warn!` (as `FramePacer`).
    pub fn set_fps(&mut self, fps: f64) {
        todo!("{fps}")
    }

    pub fn set_mode(&mut self, mode: SyncMode, now: Instant) {
        todo!("{mode:?} {now:?}")
    }

    pub fn set_speed(&mut self, speed: Speed, now: Instant) {
        todo!("{speed:?} {now:?}")
    }

    pub fn speed(&self) -> Speed {
        todo!()
    }

    /// `audio`: `AudioOutput::level()`; `None` when there is no usable device.
    pub fn poll(&mut self, now: Instant, audio: Option<AudioLevel>) -> PaceDecision {
        todo!("{now:?} {audio:?}")
    }
}
