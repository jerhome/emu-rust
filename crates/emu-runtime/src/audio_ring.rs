//! Device-independent audio pipeline (phase 1b, docs/design/phase1b-docker.md): resampling with dynamic rate
//! control on the emulation thread, lock-free SPSC ring (`rtrb`), and the reader used by the device callback.
//! Compiles and is tested without `cpal` or any sound device; `audio_out.rs` only plugs a cpal stream on top.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};
use tracing::{debug, warn};

use crate::audio::AudioSink;
use crate::resample::{MAX_RATE, MIN_RATE, StereoResampler, rate_adjust};

/// Output tuning. `Default` is what the UI uses in phase 1b.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioConfig {
    /// Target ring-buffer fill (DRC set point). Ring capacity = 2 × `latency`. Default 64 ms.
    pub latency: Duration,
    /// Maximum DRC deviation of the resampling ratio. Default 0.005 (±0.5 %, ADR 0003).
    pub max_rate_deviation: f64,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            latency: Duration::from_millis(64),
            max_rate_deviation: 0.005,
        }
    }
}

/// Counters since creation (relaxed atomics; for logs and tests only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AudioStats {
    /// Reader calls that had to pad with silence after the initial pre-roll.
    pub underruns: u64,
    /// Interleaved samples discarded because the ring was full.
    pub dropped_samples: u64,
    /// `true` once the failure notifier fired (device lost); the sink then discards samples.
    pub stream_failed: bool,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AudioError {
    #[error("no audio output device")]
    NoDevice,
    #[error("audio device configuration failed: {0}")]
    Config(String),
    #[error("unsupported audio sample format: {0}")]
    UnsupportedFormat(String),
    #[error("audio stream failed: {0}")]
    Stream(String),
    /// Binary built without the `audio-cpal` feature.
    #[error("audio output not compiled in (feature `audio-cpal`)")]
    NotCompiled,
}

/// Device sample type the reader can write. Implemented for `f32`, `i16`, `u16`, `i32` (cpal's common formats).
pub trait OutputSample: Copy + Send + 'static {
    const SILENCE: Self;
    /// `v` in `[-1, 1]` (clamped by the implementation).
    fn from_f32(v: f32) -> Self;
}

/// `[-1, 1]`, NaN → 0.
fn clamp_unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(-1.0, 1.0) }
}

impl OutputSample for f32 {
    const SILENCE: Self = 0.0;
    fn from_f32(v: f32) -> Self {
        clamp_unit(v)
    }
}

impl OutputSample for i16 {
    const SILENCE: Self = 0;
    fn from_f32(v: f32) -> Self {
        // Symmetric scale: -1 → -32767, the extra negative code is never produced.
        (clamp_unit(v) * f32::from(i16::MAX)).round() as i16
    }
}

impl OutputSample for u16 {
    const SILENCE: Self = 0x8000;
    fn from_f32(v: f32) -> Self {
        (i32::from(i16::from_f32(v)) + 0x8000) as u16
    }
}

impl OutputSample for i32 {
    const SILENCE: Self = 0;
    fn from_f32(v: f32) -> Self {
        // f64: f32 cannot represent i32::MAX exactly; `as` saturates anyway.
        (f64::from(clamp_unit(v)) * f64::from(i32::MAX)).round() as i32
    }
}

const FALLBACK_DEVICE_RATE: u32 = 48_000;
/// Producer-side underrun log period (never logged from the device callback).
const UNDERRUN_LOG_PERIOD: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
struct Shared {
    underruns: AtomicU64,
    dropped_samples: AtomicU64,
    stream_failed: AtomicBool,
}

impl Shared {
    fn stats(&self) -> AudioStats {
        AudioStats {
            underruns: self.underruns.load(Ordering::Relaxed),
            dropped_samples: self.dropped_samples.load(Ordering::Relaxed),
            stream_failed: self.stream_failed.load(Ordering::Relaxed),
        }
    }
}

/// Creates a connected pair: `RingSink` for the emulation thread, `RingReader` for the device thread.
/// `input_rate` = core `timing.sample_rate`; `device_rate` in Hz. Invalid rates are sanitised, never panic.
pub fn audio_ring(
    input_rate: f64,
    device_rate: u32,
    config: &AudioConfig,
) -> (RingSink, RingReader) {
    let device_rate = if !(MIN_RATE..=MAX_RATE).contains(&f64::from(device_rate)) {
        warn!("invalid audio device rate {device_rate}, using {FALLBACK_DEVICE_RATE} Hz");
        FALLBACK_DEVICE_RATE
    } else {
        device_rate
    };
    let max_rate_deviation = if config.max_rate_deviation.is_finite() {
        config.max_rate_deviation.abs()
    } else {
        0.0
    };
    // Target fill = `latency` worth of stereo frames; capacity = twice that. Both even (whole stereo pairs),
    // at least one frame; capped at 10 s so an absurd config cannot allocate gigabytes.
    let latency = config.latency.min(Duration::from_secs(10));
    let target_frames = ((latency.as_secs_f64() * f64::from(device_rate)).round() as usize).max(1);
    let target = target_frames * 2;
    let capacity = target * 2;

    let (producer, consumer) = RingBuffer::new(capacity);
    let shared = Arc::new(Shared::default());
    let sink = RingSink {
        producer,
        resampler: StereoResampler::new(input_rate, f64::from(device_rate)),
        // Room for a typical frame batch (a few ms) without reallocation; grows once if a core pushes more.
        scratch: Vec::with_capacity(capacity),
        shared: Arc::clone(&shared),
        device_rate,
        capacity,
        max_rate_deviation,
        failure_reported: false,
        last_log: Instant::now(),
        logged_underruns: 0,
    };
    let reader = RingReader {
        consumer,
        shared,
        target,
        started: false,
    };
    (sink, reader)
}

/// Producer side: resamples with the DRC ratio for the current fill level, pushes whole stereo frames, drops
/// the overflow (counted). Never blocks.
pub struct RingSink {
    producer: Producer<f32>,
    resampler: StereoResampler,
    scratch: Vec<f32>,
    shared: Arc<Shared>,
    device_rate: u32,
    capacity: usize,
    max_rate_deviation: f64,
    failure_reported: bool,
    last_log: Instant,
    logged_underruns: u64,
}

impl RingSink {
    pub fn device_rate(&self) -> u32 {
        self.device_rate
    }

    /// Core rate changed (`FrameStatus::av_info_changed`).
    pub fn set_input_rate(&mut self, input_rate: f64) {
        self.resampler.set_input_rate(input_rate);
    }

    /// Ring fill level in `[0, 1]` as seen by the producer.
    pub fn fill(&self) -> f64 {
        let used = self.capacity.saturating_sub(self.producer.slots());
        used as f64 / self.capacity as f64
    }

    pub fn stats(&self) -> AudioStats {
        self.shared.stats()
    }

    fn log_underruns(&mut self) {
        if self.last_log.elapsed() < UNDERRUN_LOG_PERIOD {
            return;
        }
        self.last_log = Instant::now();
        let underruns = self.shared.underruns.load(Ordering::Relaxed);
        if underruns != self.logged_underruns {
            debug!(
                "audio underruns: {underruns} (+{})",
                underruns - self.logged_underruns
            );
            self.logged_underruns = underruns;
        }
    }
}

impl AudioSink for RingSink {
    /// After `stream_failed`: one `warn!`, then samples are discarded.
    fn push_samples(&mut self, interleaved_stereo: &[i16]) {
        if self.shared.stream_failed.load(Ordering::Relaxed) {
            if !self.failure_reported {
                self.failure_reported = true;
                warn!("audio stream failed, audio disabled until restart");
            }
            return;
        }
        self.log_underruns();

        self.resampler
            .set_rate_adjust(rate_adjust(self.fill(), self.max_rate_deviation));
        self.scratch.clear();
        self.resampler
            .process(interleaved_stereo, &mut self.scratch);

        // Only whole stereo pairs: capacity and every write are even, so the ring never holds half a frame.
        let room = self.producer.slots() & !1;
        let n = room.min(self.scratch.len());
        let (written, _) = self.producer.push_partial_slice(&self.scratch[..n]);
        let dropped = self.scratch.len() - written.len();
        if dropped > 0 {
            self.shared
                .dropped_samples
                .fetch_add(dropped as u64, Ordering::Relaxed);
        }
    }
}

/// Consumer side, moved into the device callback (`Send`). Real-time safe: no allocation, lock or log.
pub struct RingReader {
    consumer: Consumer<f32>,
    shared: Arc<Shared>,
    /// Pre-roll threshold in interleaved samples (= DRC set point).
    target: usize,
    started: bool,
}

impl RingReader {
    /// Fills `out` (interleaved, `channels` per frame): 1 → (L+R)/2, ≥ 2 → L, R then silence. Silence before the
    /// ring first reaches the target fill (pre-roll, not counted) and on underrun (counted). `channels == 0` → silence.
    pub fn fill<T: OutputSample>(&mut self, out: &mut [T], channels: usize) {
        if channels == 0 {
            out.fill(T::SILENCE);
            return;
        }
        if !self.started {
            if self.consumer.slots() < self.target {
                out.fill(T::SILENCE);
                return;
            }
            self.started = true;
        }

        let frames = out.len() / channels;
        let wanted = frames * 2;
        let take = (self.consumer.slots() & !1).min(wanted);
        let mut written_frames = 0;
        if let Ok(chunk) = self.consumer.read_chunk(take) {
            let (a, b) = chunk.as_slices();
            // Both slices have even lengths (capacity and all writes/reads are even), so pairs never straddle.
            for (pair, frame) in a
                .as_chunks::<2>()
                .0
                .iter()
                .chain(b.as_chunks::<2>().0.iter())
                .zip(out.chunks_exact_mut(channels))
            {
                write_frame(frame, pair[0], pair[1]);
                written_frames += 1;
            }
            chunk.commit_all();
        }
        out[written_frames * channels..].fill(T::SILENCE);
        if take < wanted {
            self.shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Closure for the device error callback: marks the stream failed (seen by `RingSink::stats`).
    pub fn failure_notifier(&self) -> impl Fn() + Send + Sync + 'static {
        let shared = Arc::clone(&self.shared);
        move || shared.stream_failed.store(true, Ordering::Relaxed)
    }
}

#[inline]
fn write_frame<T: OutputSample>(frame: &mut [T], left: f32, right: f32) {
    match frame {
        [mono] => *mono = T::from_f32((left + right) * 0.5),
        [l, r, rest @ ..] => {
            *l = T::from_f32(left);
            *r = T::from_f32(right);
            rest.fill(T::SILENCE);
        }
        [] => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 kHz device rate, 10 ms latency → target 20 samples (10 frames), capacity 40 samples.
    fn small_ring(input_rate: f64) -> (RingSink, RingReader) {
        let config = AudioConfig {
            latency: Duration::from_millis(10),
            max_rate_deviation: 0.005,
        };
        audio_ring(input_rate, 1_000, &config)
    }

    fn frames(n: usize, l: i16, r: i16) -> Vec<i16> {
        (0..n).flat_map(|_| [l, r]).collect()
    }

    #[test]
    fn output_sample_bounds_and_clamp() {
        assert_eq!(f32::from_f32(0.25), 0.25);
        assert_eq!(f32::from_f32(3.0), 1.0);
        assert_eq!(f32::from_f32(-3.0), -1.0);
        assert_eq!(f32::from_f32(f32::NAN), 0.0);

        assert_eq!(i16::from_f32(1.0), i16::MAX);
        assert_eq!(i16::from_f32(-1.0), -i16::MAX);
        assert_eq!(i16::from_f32(2.0), i16::MAX);
        assert_eq!(i16::from_f32(-2.0), -i16::MAX);
        assert_eq!(i16::from_f32(0.0), 0);

        assert_eq!(u16::from_f32(0.0), 0x8000);
        assert_eq!(u16::from_f32(1.0), u16::MAX);
        assert_eq!(u16::from_f32(-5.0), 1);
        assert_eq!(u16::SILENCE, 0x8000);

        assert_eq!(i32::from_f32(1.0), i32::MAX);
        assert_eq!(i32::from_f32(-1.0), -i32::MAX);
        assert_eq!(i32::from_f32(9.0), i32::MAX);
        assert_eq!(i32::from_f32(0.0), 0);
    }

    #[test]
    fn geometry_and_fill() {
        let (mut sink, _reader) = small_ring(1_000.0);
        assert_eq!(sink.device_rate(), 1_000);
        assert_eq!(sink.fill(), 0.0);
        sink.push_samples(&frames(10, 0, 0));
        // 10 frames × DRC boost (empty ring) → 10 or 11 frames out of 20.
        assert!((0.5..=0.55).contains(&sink.fill()), "{}", sink.fill());
    }

    #[test]
    fn invalid_rates_do_not_panic() {
        let (mut sink, mut reader) = audio_ring(f64::NAN, 0, &AudioConfig::default());
        assert_eq!(sink.device_rate(), FALLBACK_DEVICE_RATE);
        sink.push_samples(&frames(100, 1, 1));
        let mut out = [0.0f32; 64];
        reader.fill(&mut out, 2);
    }

    #[test]
    fn overflow_keeps_whole_pairs_and_counts() {
        let (mut sink, mut reader) = small_ring(1_000.0);
        // 25 frames = 50 samples into a 40-sample ring.
        sink.push_samples(&frames(25, 1000, -1000));
        assert_eq!(sink.fill(), 1.0);
        let stats = sink.stats();
        assert!(stats.dropped_samples > 0);
        assert_eq!(stats.dropped_samples % 2, 0);

        // Odd-sized ring room never splits a frame: L/R order stays intact after the overflow.
        let mut out = [0.0f32; 40];
        reader.fill(&mut out, 2);
        for frame in out.as_chunks::<2>().0.iter() {
            assert!(frame[0] > 0.0 && frame[1] < 0.0, "{frame:?}");
        }
    }

    #[test]
    fn drc_produces_more_when_empty() {
        // Empty ring → ratio above nominal; > half full → below nominal.
        let (mut sink, _reader) = small_ring(1_000.0);
        sink.push_samples(&[]);
        assert!(sink.resampler.ratio() > 1.0);
        sink.push_samples(&frames(16, 0, 0));
        assert!(sink.fill() > 0.5);
        sink.push_samples(&[]);
        assert!(sink.resampler.ratio() < 1.0);

        // Over many frames the empty-ring ratio yields measurably more output than nominal.
        let config = AudioConfig {
            latency: Duration::from_secs(2),
            max_rate_deviation: 0.005,
        };
        let (mut sink, _reader) = audio_ring(1_000.0, 1_000, &config);
        sink.push_samples(&frames(1_000, 0, 0));
        let produced = sink.fill() * 8_000.0 / 2.0;
        assert!(produced >= 1_004.0, "{produced}");
    }

    #[test]
    fn preroll_is_silent_and_not_counted() {
        let (mut sink, mut reader) = small_ring(1_000.0);
        let mut out = [1.0f32; 8];
        reader.fill(&mut out, 2);
        assert!(out.iter().all(|&v| v == 0.0));

        sink.push_samples(&frames(5, 8192, 8192)); // below target (10 frames)
        let mut out = [1.0f32; 8];
        reader.fill(&mut out, 2);
        assert!(out.iter().all(|&v| v == 0.0));
        assert_eq!(sink.stats().underruns, 0);

        sink.push_samples(&frames(5, 8192, 8192)); // reaches target
        let mut out = [0.0f32; 8];
        reader.fill(&mut out, 2);
        assert!(out.iter().all(|&v| (v - 0.25).abs() < 1e-6), "{out:?}");
        assert_eq!(sink.stats().underruns, 0);
    }

    #[test]
    fn underrun_pads_with_silence_and_counts() {
        let (mut sink, mut reader) = small_ring(1_000.0);
        sink.push_samples(&frames(10, 16384, 16384));
        let filled = 40 - sink.producer.slots();
        let mut out = vec![1.0f32; filled + 6];
        reader.fill(&mut out, 2);
        assert!(out[..filled].iter().all(|&v| (v - 0.5).abs() < 1e-6));
        assert!(out[filled..].iter().all(|&v| v == 0.0));
        assert_eq!(sink.stats().underruns, 1);

        // Ring empty: full silence, counted again.
        let mut out = [1i16; 4];
        reader.fill(&mut out, 2);
        assert_eq!(out, [0; 4]);
        assert_eq!(sink.stats().underruns, 2);
    }

    #[test]
    fn mono_and_surround_mapping() {
        let (mut sink, mut reader) = small_ring(1_000.0);
        sink.push_samples(&frames(20, 16384, 0));

        let mut mono = [0.0f32; 2];
        reader.fill(&mut mono, 1);
        assert!(mono.iter().all(|&v| (v - 0.25).abs() < 1e-6), "{mono:?}");

        let mut six = [9u16; 12];
        reader.fill(&mut six, 6);
        for frame in six.as_chunks::<6>().0.iter() {
            assert_eq!(frame[0], u16::from_f32(0.5));
            assert_eq!(frame[1], 0x8000);
            assert!(frame[2..].iter().all(|&v| v == 0x8000));
        }

        // Partial trailing frame (len not a multiple of channels) is silenced.
        let mut odd = [7i32; 5];
        reader.fill(&mut odd, 2);
        assert_eq!(odd[4], 0);

        let mut none = [3i16; 4];
        reader.fill(&mut none, 0);
        assert_eq!(none, [0; 4]);
    }

    #[test]
    fn failure_notifier_marks_and_discards() {
        let (mut sink, reader) = small_ring(1_000.0);
        let notify = reader.failure_notifier();
        assert!(!sink.stats().stream_failed);
        notify();
        assert!(sink.stats().stream_failed);
        sink.push_samples(&frames(5, 1, 1));
        sink.push_samples(&frames(5, 1, 1));
        assert_eq!(sink.fill(), 0.0);
        assert_eq!(sink.stats().dropped_samples, 0);
    }

    #[test]
    fn reader_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<RingReader>();
        assert_send::<RingSink>();
    }
}
