//! Automatic recovery of the audio output after a stream failure (device removed, session locked, default
//! device switched on WASAPI). Device-independent: `CpalSink` plugs its cpal opener in, tests plug a `RingSink`
//! factory. Everything here runs on the emulation thread; the device callback is untouched.

use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use crate::audio::AudioSink;
use crate::audio_out::AudioStatus;
use crate::audio_ring::{AudioConfig, AudioError, AudioStats, RingSink};
use crate::pacing::AudioLevel;

/// Delay between two reopen attempts while the device is lost.
pub(crate) const RETRY_PERIOD: Duration = Duration::from_secs(2);
/// Upper bound of the retry delay when a reopened stream keeps failing right away.
pub(crate) const MAX_RETRY_PERIOD: Duration = Duration::from_secs(32);
/// A stream that lived this long is considered healthy: the next loss retries at once, then every `RETRY_PERIOD`.
pub(crate) const STABLE_AFTER: Duration = Duration::from_secs(10);

/// One open device stream (`CpalSink`'s cpal stream + ring, or a bare `RingSink` in tests).
pub(crate) trait OutputStream: AudioSink {
    fn stats(&self) -> AudioStats;
    fn level(&self) -> Option<AudioLevel>;
    fn set_paused(&mut self, paused: bool);
    fn set_input_rate(&mut self, input_rate: f64);
    fn device_rate(&self) -> u32;
}

impl OutputStream for RingSink {
    fn stats(&self) -> AudioStats {
        RingSink::stats(self)
    }
    fn level(&self) -> Option<AudioLevel> {
        RingSink::level(self)
    }
    fn set_paused(&mut self, paused: bool) {
        RingSink::set_paused(self, paused);
    }
    fn set_input_rate(&mut self, input_rate: f64) {
        RingSink::set_input_rate(self, input_rate);
    }
    fn device_rate(&self) -> u32 {
        RingSink::device_rate(self)
    }
}

/// Wraps the current stream; after `stream_failed` drops it and calls `open` again (same input rate and config)
/// at most every `retry` until it succeeds. Behaves like `NullSink` meanwhile. Counters survive reopenings.
pub(crate) struct Reopening<S, F> {
    open: F,
    current: Option<S>,
    input_rate: f64,
    config: AudioConfig,
    paused: bool,
    device_rate: u32,
    /// Underruns / dropped samples of the streams already dropped.
    past: AudioStats,
    recoveries: u64,
    opened_at: Instant,
    lost_at: Instant,
    next_attempt: Instant,
    retry: Duration,
}

impl<S, F> Reopening<S, F>
where
    S: OutputStream,
    F: FnMut(f64, &AudioConfig) -> Result<S, AudioError>,
{
    pub(crate) fn new(
        open: F,
        first: S,
        input_rate: f64,
        config: AudioConfig,
        now: Instant,
    ) -> Self {
        Self {
            open,
            device_rate: first.device_rate(),
            current: Some(first),
            input_rate,
            config,
            paused: false,
            past: AudioStats::default(),
            recoveries: 0,
            opened_at: now,
            lost_at: now,
            next_attempt: now,
            retry: RETRY_PERIOD,
        }
    }

    /// Detects a failed stream and, when due, tries to reopen. Cheap when healthy (one relaxed atomic load).
    pub(crate) fn maintain(&mut self, now: Instant) {
        if let Some(stream) = &self.current {
            if !stream.stats().stream_failed {
                return;
            }
            self.on_lost(now);
        }
        if now < self.next_attempt {
            return;
        }
        match (self.open)(self.input_rate, &self.config) {
            Ok(mut stream) => {
                stream.set_paused(self.paused);
                self.device_rate = stream.device_rate();
                self.current = Some(stream);
                self.recoveries += 1;
                self.opened_at = now;
                info!(
                    "audio output restored after {:.1} s (recovery #{})",
                    now.saturating_duration_since(self.lost_at).as_secs_f64(),
                    self.recoveries
                );
            }
            Err(err) => {
                debug!("audio output reopen failed: {err}");
                self.next_attempt = now + self.retry;
            }
        }
    }

    fn on_lost(&mut self, now: Instant) {
        if let Some(stream) = self.current.take() {
            let s = stream.stats();
            self.past.underruns += s.underruns;
            self.past.dropped_samples += s.dropped_samples;
            // Dropped here (not at the next attempt) so the device handle is released before reopening.
            drop(stream);
        }
        let stable = now.saturating_duration_since(self.opened_at) >= STABLE_AFTER;
        if stable {
            self.retry = RETRY_PERIOD;
            self.next_attempt = now;
        } else {
            // Reopened stream failing again right away: back off so we neither spin nor flood the log.
            self.retry = (self.retry * 2).min(MAX_RETRY_PERIOD);
            self.next_attempt = now + self.retry;
        }
        self.lost_at = now;
        warn!(
            "audio stream failed; reopening the default output device (retry every {} s)",
            self.retry.as_secs()
        );
    }

    pub(crate) fn status(&self) -> AudioStatus {
        match &self.current {
            Some(s) if !s.stats().stream_failed => AudioStatus::Playing,
            _ => AudioStatus::Recovering,
        }
    }

    pub(crate) fn stats(&self) -> AudioStats {
        let current = self.current.as_ref().map(|s| s.stats());
        AudioStats {
            underruns: self.past.underruns + current.map_or(0, |s| s.underruns),
            dropped_samples: self.past.dropped_samples + current.map_or(0, |s| s.dropped_samples),
            stream_failed: current.is_none_or(|s| s.stream_failed),
            recoveries: self.recoveries,
        }
    }

    pub(crate) fn level(&self) -> Option<AudioLevel> {
        self.current.as_ref().and_then(|s| s.level())
    }

    /// Rate of the current device, or of the last one while recovering.
    pub(crate) fn device_rate(&self) -> u32 {
        self.current
            .as_ref()
            .map_or(self.device_rate, |s| s.device_rate())
    }

    pub(crate) fn set_input_rate(&mut self, input_rate: f64) {
        self.input_rate = input_rate;
        if let Some(s) = &mut self.current {
            s.set_input_rate(input_rate);
        }
    }

    pub(crate) fn set_paused(&mut self, paused: bool, now: Instant) {
        self.paused = paused;
        self.maintain(now);
        if let Some(s) = &mut self.current {
            s.set_paused(paused);
        }
    }

    pub(crate) fn push_samples(&mut self, interleaved_stereo: &[i16], now: Instant) {
        self.maintain(now);
        if let Some(s) = &mut self.current {
            s.push_samples(interleaved_stereo);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::audio_ring::{RingReader, audio_ring};

    /// Test "host": whether opening succeeds, which device rate it reports, and the readers of opened rings.
    #[derive(Default)]
    struct Host {
        available: bool,
        device_rate: u32,
        attempts: u32,
        opened_with: Vec<f64>,
        readers: Vec<RingReader>,
    }

    type Opener = Box<dyn FnMut(f64, &AudioConfig) -> Result<RingSink, AudioError>>;

    fn config() -> AudioConfig {
        AudioConfig {
            latency: Duration::from_millis(10),
            max_rate_deviation: 0.005,
        }
    }

    fn setup(now: Instant) -> (Reopening<RingSink, Opener>, Rc<RefCell<Host>>) {
        let host = Rc::new(RefCell::new(Host {
            available: true,
            device_rate: 1_000,
            ..Host::default()
        }));
        let h = Rc::clone(&host);
        let mut open: Opener = Box::new(move |rate, cfg| {
            let mut h = h.borrow_mut();
            h.attempts += 1;
            if !h.available {
                return Err(AudioError::NoDevice);
            }
            h.opened_with.push(rate);
            let (sink, reader) = audio_ring(rate, h.device_rate, cfg);
            h.readers.push(reader);
            Ok(sink)
        });
        let first = open(1_000.0, &config()).unwrap();
        host.borrow_mut().attempts = 0;
        (Reopening::new(open, first, 1_000.0, config(), now), host)
    }

    fn fail_current(host: &Rc<RefCell<Host>>) {
        let h = host.borrow();
        h.readers.last().unwrap().failure_notifier()();
    }

    fn frames(n: usize) -> Vec<i16> {
        vec![100; n * 2]
    }

    #[test]
    fn healthy_stream_forwards_and_never_reopens() {
        let t0 = Instant::now();
        let (mut out, host) = setup(t0);
        out.push_samples(&frames(5), t0);
        assert_eq!(out.status(), AudioStatus::Playing);
        assert!(out.level().unwrap().buffered > Duration::ZERO);
        assert_eq!(host.borrow().attempts, 0);
        assert_eq!(out.stats().recoveries, 0);
        assert!(!out.stats().stream_failed);
    }

    #[test]
    fn lost_device_is_silent_and_retried_periodically() {
        let t0 = Instant::now();
        let (mut out, host) = setup(t0);
        host.borrow_mut().available = false;
        fail_current(&host);

        // Stable stream lost: one immediate attempt, then nothing before RETRY_PERIOD.
        let t1 = t0 + STABLE_AFTER;
        out.push_samples(&frames(5), t1);
        assert_eq!(host.borrow().attempts, 1);
        assert_eq!(out.status(), AudioStatus::Recovering);
        assert!(out.stats().stream_failed);
        assert_eq!(out.level(), None);
        for ms in [1, 500, 1_999] {
            out.push_samples(&frames(5), t1 + Duration::from_millis(ms));
        }
        assert_eq!(host.borrow().attempts, 1);
        out.push_samples(&frames(5), t1 + RETRY_PERIOD);
        assert_eq!(host.borrow().attempts, 2);
        out.push_samples(&frames(5), t1 + RETRY_PERIOD * 2);
        assert_eq!(host.borrow().attempts, 3);
        assert_eq!(out.stats().recoveries, 0);
    }

    #[test]
    fn reopen_restores_rate_pause_and_keeps_counters() {
        let t0 = Instant::now();
        let (mut out, host) = setup(t0);
        // One underrun on the first stream.
        out.push_samples(&frames(10), t0);
        let mut buf = [0.0f32; 64];
        host.borrow_mut().readers[0].fill(&mut buf, 2);
        assert_eq!(out.stats().underruns, 1);

        out.set_input_rate(2_000.0);
        out.set_paused(true, t0);
        {
            let mut h = host.borrow_mut();
            h.available = false;
            h.device_rate = 2_000;
        }
        fail_current(&host);
        let t1 = t0 + STABLE_AFTER;
        out.set_paused(true, t1);
        assert_eq!(out.status(), AudioStatus::Recovering);
        assert_eq!(out.device_rate(), 1_000);

        host.borrow_mut().available = true;
        out.set_paused(true, t1 + RETRY_PERIOD);
        assert_eq!(out.status(), AudioStatus::Playing);
        let stats = out.stats();
        assert_eq!(stats.recoveries, 1);
        assert_eq!(stats.underruns, 1);
        assert!(!stats.stream_failed);
        assert_eq!(host.borrow().opened_with.last(), Some(&2_000.0));
        assert_eq!(out.device_rate(), 2_000);
        // Pause re-applied to the new ring: no level while paused.
        assert_eq!(out.level(), None);
        out.set_paused(false, t1 + RETRY_PERIOD);
        assert_eq!(out.level().unwrap().buffered, Duration::ZERO);
        out.push_samples(&frames(5), t1 + RETRY_PERIOD);
        assert!(out.level().unwrap().buffered > Duration::ZERO);
    }

    #[test]
    fn flapping_stream_backs_off() {
        let t0 = Instant::now();
        let (mut out, host) = setup(t0);
        fail_current(&host);
        let mut now = t0 + STABLE_AFTER;
        out.push_samples(&[], now); // immediate reopen succeeds
        assert_eq!(out.stats().recoveries, 1);
        assert_eq!(host.borrow().attempts, 1);

        // The reopened stream fails at once: retries wait 4 s, then 8 s…
        let mut expected = RETRY_PERIOD;
        for round in 2..=6 {
            fail_current(&host);
            expected = (expected * 2).min(MAX_RETRY_PERIOD);
            out.push_samples(&[], now);
            assert_eq!(out.status(), AudioStatus::Recovering);
            out.push_samples(&[], now + expected - Duration::from_millis(1));
            assert_eq!(out.status(), AudioStatus::Recovering, "round {round}");
            now += expected;
            out.push_samples(&[], now);
            assert_eq!(out.stats().recoveries, round, "round {round}");
        }
        assert_eq!(expected, MAX_RETRY_PERIOD);

        // After a stable period, the next loss retries immediately again.
        now += STABLE_AFTER;
        fail_current(&host);
        out.push_samples(&[], now);
        assert_eq!(out.status(), AudioStatus::Playing);
    }

    #[test]
    fn failure_seen_before_push_reports_recovering() {
        let t0 = Instant::now();
        let (out, host) = setup(t0);
        fail_current(&host);
        assert_eq!(out.status(), AudioStatus::Recovering);
        assert!(out.stats().stream_failed);
        assert_eq!(out.level(), None);
    }
}
