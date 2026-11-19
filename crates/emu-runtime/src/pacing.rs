//! Phase 2 frame scheduling: audio-driven sync with a wall-clock fallback, pause and fast-forward
//! (docs/design/phase2-frontend.md §3, ADR 0010). Emulation stays on the UI thread.

use std::time::{Duration, Instant};

use crate::timing::{FramePacer, valid_fps};

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
///   Runaway guard: a core that pushes no audio would keep the queue empty forever, so audio-driven frames
///   are also capped at `fps × AUDIO_MAX_SPEEDUP` on average, with at most `target + one period` of credit
///   (enough for the pre-roll). Normal audio-driven pacing never reaches this cap.
/// - `Normal` otherwise → wall clock at `fps` (no backlog after a stall, as `FramePacer`).
/// - Never more than `MAX_BURST` consecutive `Run`; switching speed or mode never produces a burst
///   (the clock restarts from `now`).
#[derive(Debug, Clone)]
pub struct Pacer {
    fps: f64,
    mode: SyncMode,
    speed: Speed,
    /// Wall clock in use; `None` until the next poll that needs it (it then starts at that poll's `now`).
    clock: Option<FramePacer>,
    /// Earliest instant of the next audio-driven frame, minus the allowed credit (runaway guard).
    audio_next: Instant,
    /// Consecutive `Run` decisions.
    burst: u32,
}

/// Audio-driven pacing may run this much faster than `fps` before the runaway guard kicks in (core
/// rate vs real device rate mismatch is far below this).
const AUDIO_MAX_SPEEDUP: f64 = 1.05;
const MIN_AUDIO_WAIT: Duration = Duration::from_millis(1);

fn secs(s: f64) -> Duration {
    Duration::try_from_secs_f64(s).unwrap_or(Duration::from_secs(1))
}

impl Pacer {
    pub fn new(fps: f64, mode: SyncMode, now: Instant) -> Self {
        Self {
            fps: valid_fps(fps),
            mode,
            speed: Speed::Normal,
            clock: None,
            audio_next: now,
            burst: 0,
        }
    }

    /// Invalid rates fall back to 60 fps with one `warn!` (as `FramePacer`).
    pub fn set_fps(&mut self, fps: f64) {
        self.fps = valid_fps(fps);
        // Keeps the pending deadline, as `FramePacer::set_fps`.
        let rate = self.clock_rate();
        if let (Some(clock), Some(rate)) = (self.clock.as_mut(), rate) {
            clock.set_fps(rate);
        }
    }

    pub fn set_mode(&mut self, mode: SyncMode, now: Instant) {
        if mode != self.mode {
            self.mode = mode;
            self.restart(now);
        }
    }

    pub fn set_speed(&mut self, speed: Speed, now: Instant) {
        if speed != self.speed {
            self.speed = speed;
            self.restart(now);
        }
    }

    pub fn speed(&self) -> Speed {
        self.speed
    }

    /// `audio`: `AudioOutput::level()`; `None` when there is no usable device.
    pub fn poll(&mut self, now: Instant, audio: Option<AudioLevel>) -> PaceDecision {
        let decision = self.decide(now, audio);
        if decision == PaceDecision::Run {
            self.burst += 1;
        } else {
            self.burst = 0;
        }
        decision
    }

    fn restart(&mut self, now: Instant) {
        self.clock = None;
        self.audio_next = now;
        self.burst = 0;
    }

    fn period(&self) -> Duration {
        secs(1.0 / self.fps)
    }

    /// Wall-clock rate for the current speed; `None` = unlimited fast-forward.
    fn clock_rate(&self) -> Option<f64> {
        match self.speed {
            Speed::FastForward { ratio } if !(ratio.is_finite() && ratio > 1.0) => None,
            Speed::FastForward { ratio } => Some(self.fps * ratio).filter(|r| r.is_finite()),
            Speed::Normal | Speed::Paused => Some(self.fps),
        }
    }

    fn decide(&mut self, now: Instant, audio: Option<AudioLevel>) -> PaceDecision {
        if self.speed == Speed::Paused {
            self.clock = None;
            return PaceDecision::Idle;
        }
        if self.burst >= MAX_BURST {
            // Already due: the event loop runs once, then `poll` is called again.
            return PaceDecision::WaitUntil(now);
        }
        match (self.speed, self.mode, audio) {
            (Speed::Normal, SyncMode::Audio, Some(level)) => {
                self.clock = None;
                self.audio_paced(now, level)
            }
            _ => {
                let Some(rate) = self.clock_rate() else {
                    self.clock = None;
                    return PaceDecision::Run;
                };
                let clock = self.clock.get_or_insert_with(|| FramePacer::new(rate, now));
                if clock.frame_due(now) {
                    PaceDecision::Run
                } else {
                    PaceDecision::WaitUntil(clock.next_deadline())
                }
            }
        }
    }

    fn audio_paced(&mut self, now: Instant, level: AudioLevel) -> PaceDecision {
        let period = self.period();
        if level.buffered >= level.target {
            let excess =
                (level.buffered - level.target).clamp(MIN_AUDIO_WAIT, period.max(MIN_AUDIO_WAIT));
            return PaceDecision::WaitUntil(now + excess);
        }
        let credit = level.target.saturating_add(period);
        // `audio_next - credit > now`, written without subtracting from an `Instant`.
        if self.audio_next > now + credit {
            return PaceDecision::WaitUntil(self.audio_next.checked_sub(credit).unwrap_or(now));
        }
        // No credit accumulates while the queue is full: the guard only counts from `now`.
        self.audio_next = self.audio_next.max(now) + secs(1.0 / (self.fps * AUDIO_MAX_SPEEDUP));
        PaceDecision::Run
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    fn level(buffered_ms: u32, target_ms: u32) -> Option<AudioLevel> {
        Some(AudioLevel {
            buffered: buffered_ms * MS,
            target: target_ms * MS,
        })
    }

    /// Runs `poll` from `t0` until `t0 + span`, jumping to each `WaitUntil`. Returns the number of frames.
    fn simulate(p: &mut Pacer, t0: Instant, span: Duration) -> u32 {
        let mut now = t0;
        let mut frames = 0;
        while now < t0 + span {
            match p.poll(now, None) {
                PaceDecision::Run => frames += 1,
                PaceDecision::WaitUntil(t) => {
                    assert!(t >= now);
                    now = t;
                }
                PaceDecision::Idle => break,
            }
        }
        frames
    }

    #[test]
    fn below_target_runs_above_waits() {
        let t0 = Instant::now();
        let mut p = Pacer::new(50.0, SyncMode::Audio, t0);
        assert_eq!(p.poll(t0, level(10, 64)), PaceDecision::Run);
        // 6 ms over the target: wait for the device to consume them.
        assert_eq!(
            p.poll(t0, level(70, 64)),
            PaceDecision::WaitUntil(t0 + 6 * MS)
        );
        // Exactly at target: minimum wait.
        assert_eq!(p.poll(t0, level(64, 64)), PaceDecision::WaitUntil(t0 + MS));
        // Far above: bounded by one frame period (20 ms).
        assert_eq!(
            p.poll(t0, level(500, 64)),
            PaceDecision::WaitUntil(t0 + 20 * MS)
        );
    }

    #[test]
    fn burst_is_bounded() {
        let t0 = Instant::now();
        let mut p = Pacer::new(60.0, SyncMode::Audio, t0);
        for _ in 0..MAX_BURST {
            assert_eq!(p.poll(t0, level(0, 64)), PaceDecision::Run);
        }
        assert_eq!(p.poll(t0, level(0, 64)), PaceDecision::WaitUntil(t0));
        assert_eq!(p.poll(t0, level(0, 64)), PaceDecision::Run);

        // Unlimited fast-forward is bounded the same way.
        let mut p = Pacer::new(60.0, SyncMode::Audio, t0);
        p.set_speed(
            Speed::FastForward {
                ratio: f64::INFINITY,
            },
            t0,
        );
        let runs = (0..4)
            .take_while(|_| p.poll(t0, None) == PaceDecision::Run)
            .count();
        assert_eq!(runs, MAX_BURST as usize);
    }

    #[test]
    fn silent_core_does_not_run_away() {
        // Queue stays empty (core pushes no audio): average rate capped near fps.
        let t0 = Instant::now();
        let mut p = Pacer::new(60.0, SyncMode::Audio, t0);
        let mut now = t0;
        let mut frames = 0;
        while now < t0 + Duration::from_secs(1) {
            match p.poll(now, level(0, 64)) {
                PaceDecision::Run => frames += 1,
                PaceDecision::WaitUntil(t) => now = t.max(now + Duration::from_micros(100)),
                PaceDecision::Idle => unreachable!(),
            }
        }
        assert!((60..=72).contains(&frames), "{frames}");
    }

    #[test]
    fn clock_fallback_without_audio() {
        let t0 = Instant::now();
        let mut p = Pacer::new(50.0, SyncMode::Audio, t0);
        assert_eq!(p.poll(t0, None), PaceDecision::Run);
        assert_eq!(p.poll(t0, None), PaceDecision::WaitUntil(t0 + 20 * MS));
        assert_eq!(simulate(&mut p, t0, Duration::from_secs(1)), 49);

        let mut p = Pacer::new(50.0, SyncMode::Clock, t0);
        // Clock mode ignores the audio level.
        assert_eq!(p.poll(t0, level(500, 64)), PaceDecision::Run);
        assert_eq!(
            p.poll(t0, level(0, 64)),
            PaceDecision::WaitUntil(t0 + 20 * MS)
        );
    }

    #[test]
    fn stall_does_not_burst() {
        let t0 = Instant::now();
        let mut p = Pacer::new(50.0, SyncMode::Clock, t0);
        assert_eq!(p.poll(t0, None), PaceDecision::Run);
        let late = t0 + Duration::from_secs(2);
        assert_eq!(p.poll(late, None), PaceDecision::Run);
        assert_eq!(p.poll(late, None), PaceDecision::WaitUntil(late + 20 * MS));
    }

    #[test]
    fn paused_is_idle() {
        let t0 = Instant::now();
        let mut p = Pacer::new(60.0, SyncMode::Audio, t0);
        p.set_speed(Speed::Paused, t0);
        assert_eq!(p.speed(), Speed::Paused);
        assert_eq!(p.poll(t0, level(0, 64)), PaceDecision::Idle);
        assert_eq!(p.poll(t0, None), PaceDecision::Idle);
    }

    #[test]
    fn fast_forward_rate() {
        let t0 = Instant::now();
        let mut p = Pacer::new(60.0, SyncMode::Audio, t0);
        p.set_speed(
            Speed::FastForward {
                ratio: DEFAULT_FAST_FORWARD_RATIO,
            },
            t0,
        );
        let frames = simulate(&mut p, t0, Duration::from_secs(1));
        assert!((238..=241).contains(&frames), "{frames}");
        // Audio is ignored while fast-forwarding.
        let t1 = t0 + Duration::from_secs(1);
        let mut p = Pacer::new(60.0, SyncMode::Audio, t1);
        p.set_speed(Speed::FastForward { ratio: 4.0 }, t1);
        assert_eq!(p.poll(t1, level(500, 64)), PaceDecision::Run);
        assert!(matches!(
            p.poll(t1, level(0, 64)),
            PaceDecision::WaitUntil(t) if t > t1
        ));
    }

    #[test]
    fn speed_change_restarts_clock_without_burst() {
        let t0 = Instant::now();
        let mut p = Pacer::new(50.0, SyncMode::Clock, t0);
        assert_eq!(p.poll(t0, None), PaceDecision::Run);
        // Long fast-forward then back to normal 3 s later: exactly one frame due, no catch-up.
        let t1 = t0 + 10 * MS;
        p.set_speed(Speed::FastForward { ratio: 4.0 }, t1);
        assert_eq!(p.poll(t1, None), PaceDecision::Run);
        assert_eq!(p.poll(t1, None), PaceDecision::WaitUntil(t1 + 5 * MS));
        let t2 = t1 + Duration::from_secs(3);
        p.set_speed(Speed::Normal, t2);
        assert_eq!(p.poll(t2, None), PaceDecision::Run);
        assert_eq!(p.poll(t2, None), PaceDecision::WaitUntil(t2 + 20 * MS));
        // Same speed again: no restart.
        p.set_speed(Speed::Normal, t2);
        assert_eq!(p.poll(t2, None), PaceDecision::WaitUntil(t2 + 20 * MS));
        // Mode switch restarts as well.
        p.set_mode(SyncMode::Audio, t2 + MS);
        assert_eq!(p.poll(t2 + MS, None), PaceDecision::Run);
        assert_eq!(p.poll(t2 + MS, None), PaceDecision::WaitUntil(t2 + 21 * MS));
    }

    #[test]
    fn audio_to_clock_fallback_restarts_clock() {
        let t0 = Instant::now();
        let mut p = Pacer::new(50.0, SyncMode::Audio, t0);
        assert_eq!(p.poll(t0, None), PaceDecision::Run);
        let t1 = t0 + 5 * MS;
        assert_eq!(
            p.poll(t1, level(70, 64)),
            PaceDecision::WaitUntil(t1 + 6 * MS)
        );
        // Audio goes away (stream failed, paused): the clock restarts from now.
        let t2 = t0 + Duration::from_secs(1);
        assert_eq!(p.poll(t2, None), PaceDecision::Run);
        assert_eq!(p.poll(t2, None), PaceDecision::WaitUntil(t2 + 20 * MS));
    }

    #[test]
    fn invalid_fps_falls_back() {
        let t0 = Instant::now();
        for fps in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e-19, 1e9, 0.5] {
            let mut p = Pacer::new(fps, SyncMode::Clock, t0);
            assert_eq!(p.poll(t0, None), PaceDecision::Run);
            let PaceDecision::WaitUntil(t) = p.poll(t0, None) else {
                panic!("expected a wait");
            };
            assert!(t - t0 > 16 * MS && t - t0 < 17 * MS);
        }
        let mut p = Pacer::new(50.0, SyncMode::Clock, t0);
        p.set_fps(f64::NAN);
        p.set_fps(100.0);
        assert_eq!(p.poll(t0, None), PaceDecision::Run);
        assert_eq!(p.poll(t0, None), PaceDecision::WaitUntil(t0 + 10 * MS));
    }
}
