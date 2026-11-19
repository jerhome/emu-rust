use std::time::{Duration, Instant};

/// Used when the core reports a nonsensical rate (0, negative, NaN).
const FALLBACK_FPS: f64 = 60.0;

/// Paces emulated frames on the core's `timing.fps` (phase 1: wall clock only; audio-driven sync in phase 2).
/// Never accumulates more than one frame of lag: after a stall it resynchronises instead of bursting.
#[derive(Debug, Clone)]
pub struct FramePacer {
    period: Duration,
    next: Instant,
}

impl FramePacer {
    /// The first frame is due at `now`.
    pub fn new(fps: f64, now: Instant) -> Self {
        Self {
            period: period_of(fps),
            next: now,
        }
    }

    /// Takes effect from the next frame; the pending deadline is kept.
    pub fn set_fps(&mut self, fps: f64) {
        self.period = period_of(fps);
    }

    /// `true` if a frame is due at `now`; advances the deadline when it returns `true`.
    pub fn frame_due(&mut self, now: Instant) -> bool {
        if now < self.next {
            return false;
        }
        self.next += self.period;
        // Late by more than one period: drop the backlog rather than running frames back-to-back.
        if self.next <= now {
            self.next = now + self.period;
        }
        true
    }

    /// Deadline of the next frame, for `ControlFlow::WaitUntil`.
    pub fn next_deadline(&self) -> Instant {
        self.next
    }
}

/// Frame rates outside this range are treated as invalid (absurd values would overflow `Instant` arithmetic).
pub(crate) const FPS_RANGE: std::ops::RangeInclusive<f64> = 1.0..=1000.0;

/// `fps` if finite and within `FPS_RANGE`, else 60 with a warning.
pub(crate) fn valid_fps(fps: f64) -> f64 {
    if fps.is_finite() && FPS_RANGE.contains(&fps) {
        fps
    } else {
        tracing::warn!(fps, "invalid core frame rate, pacing at {FALLBACK_FPS} fps");
        FALLBACK_FPS
    }
}

fn period_of(fps: f64) -> Duration {
    let fps = valid_fps(fps);
    Duration::try_from_secs_f64(1.0 / fps).unwrap_or(Duration::from_secs(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn steady_cadence() {
        let t0 = Instant::now();
        let mut p = FramePacer::new(50.0, t0);
        assert!(p.frame_due(t0));
        assert!(!p.frame_due(t0));
        assert_eq!(p.next_deadline(), t0 + 20 * MS);
        assert!(!p.frame_due(t0 + 19 * MS));
        // Slightly late: cadence is kept (deadline advances by one period from the previous one).
        assert!(p.frame_due(t0 + 25 * MS));
        assert_eq!(p.next_deadline(), t0 + 40 * MS);
    }

    #[test]
    fn stall_resynchronises_without_burst() {
        let t0 = Instant::now();
        let mut p = FramePacer::new(50.0, t0);
        assert!(p.frame_due(t0));
        let late = t0 + 1000 * MS;
        assert!(p.frame_due(late));
        assert!(!p.frame_due(late));
        assert_eq!(p.next_deadline(), late + 20 * MS);
    }

    #[test]
    fn absurd_fps_does_not_panic() {
        let t0 = Instant::now();
        for fps in [1e-19, 1e9, f64::MAX, f64::MIN_POSITIVE] {
            let mut p = FramePacer::new(fps, t0);
            assert!(p.frame_due(t0));
            let period = p.next_deadline() - t0;
            assert!(period > 16 * MS && period < 17 * MS);
        }
        assert_eq!(valid_fps(1.0), 1.0);
        assert_eq!(valid_fps(1000.0), 1000.0);
    }

    #[test]
    fn invalid_fps_falls_back() {
        let t0 = Instant::now();
        let mut p = FramePacer::new(0.0, t0);
        assert!(p.frame_due(t0));
        let period = p.next_deadline() - t0;
        assert!(period > 16 * MS && period < 17 * MS);
        p.set_fps(f64::NAN);
        p.set_fps(100.0);
        assert!(p.frame_due(p.next_deadline()));
        assert_eq!(p.next_deadline() - t0, period + 10 * MS);
    }
}
