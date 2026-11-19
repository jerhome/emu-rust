//! Device-independent audio pipeline (phase 1b, docs/design/phase1b-docker.md): resampling with dynamic rate
//! control on the emulation thread, lock-free SPSC ring (`rtrb`), and the reader used by the device callback.
//! Compiles and is tested without `cpal` or any sound device; `audio_out.rs` only plugs a cpal stream on top.

use std::time::Duration;

use crate::audio::AudioSink;

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

impl OutputSample for f32 {
    const SILENCE: Self = 0.0;
    fn from_f32(v: f32) -> Self {
        let _ = v;
        todo!()
    }
}

impl OutputSample for i16 {
    const SILENCE: Self = 0;
    fn from_f32(v: f32) -> Self {
        let _ = v;
        todo!()
    }
}

impl OutputSample for u16 {
    const SILENCE: Self = 0x8000;
    fn from_f32(v: f32) -> Self {
        let _ = v;
        todo!()
    }
}

impl OutputSample for i32 {
    const SILENCE: Self = 0;
    fn from_f32(v: f32) -> Self {
        let _ = v;
        todo!()
    }
}

/// Creates a connected pair: `RingSink` for the emulation thread, `RingReader` for the device thread.
/// `input_rate` = core `timing.sample_rate`; `device_rate` in Hz. Invalid rates are sanitised, never panic.
pub fn audio_ring(
    input_rate: f64,
    device_rate: u32,
    config: &AudioConfig,
) -> (RingSink, RingReader) {
    let _ = (input_rate, device_rate, config);
    todo!()
}

/// Producer side: resamples with the DRC ratio for the current fill level, pushes whole stereo frames, drops
/// the overflow (counted). Never blocks.
pub struct RingSink {
    // Placeholder until runtime-av implements it (+ StereoResampler, scratch, Arc<shared>).
    _producer: std::marker::PhantomData<rtrb::Producer<f32>>,
}

impl RingSink {
    pub fn device_rate(&self) -> u32 {
        todo!()
    }

    /// Core rate changed (`FrameStatus::av_info_changed`).
    pub fn set_input_rate(&mut self, input_rate: f64) {
        let _ = input_rate;
        todo!()
    }

    /// Ring fill level in `[0, 1]` as seen by the producer.
    pub fn fill(&self) -> f64 {
        todo!()
    }

    pub fn stats(&self) -> AudioStats {
        todo!()
    }
}

impl AudioSink for RingSink {
    /// After `stream_failed`: one `warn!`, then samples are discarded.
    fn push_samples(&mut self, interleaved_stereo: &[i16]) {
        let _ = interleaved_stereo;
        todo!()
    }
}

/// Consumer side, moved into the device callback (`Send`). Real-time safe: no allocation, lock or log.
pub struct RingReader {
    // Placeholder until runtime-av implements it (+ channel mapping, Arc<shared>).
    _consumer: std::marker::PhantomData<rtrb::Consumer<f32>>,
}

impl RingReader {
    /// Fills `out` (interleaved, `channels` per frame): 1 → (L+R)/2, ≥ 2 → L, R then silence. Silence before the
    /// ring first reaches the target fill (pre-roll, not counted) and on underrun (counted). `channels == 0` → silence.
    pub fn fill<T: OutputSample>(&mut self, out: &mut [T], channels: usize) {
        let _ = (out, channels);
        todo!()
    }

    /// Closure for the device error callback: marks the stream failed (seen by `RingSink::stats`).
    pub fn failure_notifier(&self) -> impl Fn() + Send + Sync + 'static {
        || todo!()
    }
}
