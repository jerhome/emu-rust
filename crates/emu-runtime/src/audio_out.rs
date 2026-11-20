//! Real audio output. The cpal stream is behind the `audio-cpal` feature (default on); built without it,
//! `AudioOutput::open_or_null` always returns the silent output.

use tracing::warn;

use crate::audio::{AudioSink, NullSink};
#[cfg(feature = "audio-cpal")]
use crate::audio_reopen::{OutputStream, Reopening};
use crate::audio_ring::{AudioConfig, AudioStats};
#[cfg(feature = "audio-cpal")]
use crate::audio_ring::{AudioError, OutputSample, RingReader, RingSink, audio_ring};
use crate::pacing::AudioLevel;
#[cfg(feature = "audio-cpal")]
use std::time::Instant;

/// Plays the core's audio on the default output device: `RingSink` + cpal stream whose callback owns the
/// `RingReader`. `!Send` (owns the cpal stream): create and use it on the emulation thread.
///
/// When the stream fails (device removed, session locked, default device switched), the stream is dropped and
/// the default device is reopened with the same input rate and config: at once if the stream had been running
/// for a while, then every 2 s (backing off up to 32 s if reopened streams keep failing). Attempts happen in
/// `push_samples` / `set_paused` on the emulation thread; samples are discarded meanwhile (`status()`).
#[cfg(feature = "audio-cpal")]
pub struct CpalSink {
    // Boxed so `AudioOutput::Device` stays small next to `Null`.
    inner: Box<Reopening<CpalStream, StreamOpener>>,
    /// Keeps the type `!Send` whatever cpal's `Stream` is on this platform (documented contract).
    _not_send: std::marker::PhantomData<*const ()>,
}

#[cfg(feature = "audio-cpal")]
type StreamOpener = fn(f64, &AudioConfig) -> Result<CpalStream, AudioError>;

#[cfg(feature = "audio-cpal")]
struct CpalStream {
    // Field order: the stream (and its callback holding the reader) is dropped before the sink.
    _stream: cpal::Stream,
    sink: RingSink,
}

#[cfg(feature = "audio-cpal")]
impl AudioSink for CpalStream {
    fn push_samples(&mut self, interleaved_stereo: &[i16]) {
        self.sink.push_samples(interleaved_stereo);
    }
}

#[cfg(feature = "audio-cpal")]
impl OutputStream for CpalStream {
    fn stats(&self) -> AudioStats {
        self.sink.stats()
    }
    fn level(&self) -> Option<AudioLevel> {
        self.sink.level()
    }
    fn set_paused(&mut self, paused: bool) {
        self.sink.set_paused(paused);
    }
    fn set_input_rate(&mut self, input_rate: f64) {
        self.sink.set_input_rate(input_rate);
    }
    fn device_rate(&self) -> u32 {
        self.sink.device_rate()
    }
}

#[cfg(feature = "audio-cpal")]
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut reader: RingReader,
) -> Result<cpal::Stream, AudioError>
where
    T: cpal::SizedSample + OutputSample,
{
    use cpal::traits::DeviceTrait;

    let channels = usize::from(config.channels);
    let notify = reader.failure_notifier();
    // Real-time callback: only ring reads and sample conversion (no allocation, lock or log).
    let data_callback =
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| reader.fill(data, channels);
    let error_callback = move |err: cpal::Error| {
        use cpal::ErrorKind;
        // Recoverable conditions: cpal keeps the stream running (ALSA recovers xruns via prepare()).
        if !matches!(
            err.kind(),
            ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied
        ) {
            notify();
        }
    };
    device
        .build_output_stream::<T, _, _>(*config, data_callback, error_callback, None)
        .map_err(|e| AudioError::Stream(e.to_string()))
}

/// Default host → default output device → default output config (F32/I16/U16/I32, else `UnsupportedFormat`),
/// `BufferSize::Default`, stream started. Never panics.
#[cfg(feature = "audio-cpal")]
fn open_stream(input_rate: f64, config: &AudioConfig) -> Result<CpalStream, AudioError> {
    use cpal::SampleFormat;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(AudioError::NoDevice)?;
    let supported = device
        .default_output_config()
        .map_err(|e| AudioError::Config(e.to_string()))?;
    let format = supported.sample_format();
    let stream_config = supported.config(); // BufferSize::Default
    if stream_config.channels == 0 {
        return Err(AudioError::Config("device reports 0 channels".into()));
    }

    let (sink, reader) = audio_ring(input_rate, stream_config.sample_rate, config);
    let stream = match format {
        SampleFormat::F32 => build_stream::<f32>(&device, &stream_config, reader),
        SampleFormat::I16 => build_stream::<i16>(&device, &stream_config, reader),
        SampleFormat::U16 => build_stream::<u16>(&device, &stream_config, reader),
        SampleFormat::I32 => build_stream::<i32>(&device, &stream_config, reader),
        other => return Err(AudioError::UnsupportedFormat(other.to_string())),
    }?;
    stream
        .play()
        .map_err(|e| AudioError::Stream(e.to_string()))?;

    tracing::info!(
        "audio output: {} Hz, {} channel(s), {format}, latency target {} ms",
        stream_config.sample_rate,
        stream_config.channels,
        config.latency.as_millis()
    );
    Ok(CpalStream {
        _stream: stream,
        sink,
    })
}

#[cfg(feature = "audio-cpal")]
impl CpalSink {
    /// Opens the default output device (see `CpalSink`). Fails if there is none: recovery only applies to a
    /// stream that was open once. Never panics.
    pub fn open(input_rate: f64, config: &AudioConfig) -> Result<Self, AudioError> {
        let first = open_stream(input_rate, config)?;
        let opener: StreamOpener = open_stream;
        Ok(Self {
            inner: Box::new(Reopening::new(
                opener,
                first,
                input_rate,
                *config,
                Instant::now(),
            )),
            _not_send: std::marker::PhantomData,
        })
    }

    /// Rate of the current device (of the last one while recovering).
    pub fn device_rate(&self) -> u32 {
        self.inner.device_rate()
    }

    /// Also used for later reopenings.
    pub fn set_input_rate(&mut self, input_rate: f64) {
        self.inner.set_input_rate(input_rate);
    }

    /// Cumulated over reopenings; `stream_failed` while the device is lost; `recoveries` = successful reopenings.
    pub fn stats(&self) -> AudioStats {
        self.inner.stats()
    }

    /// `RingSink::set_paused`: the stream keeps running and plays silence. Kept across reopenings.
    pub fn set_paused(&mut self, paused: bool) {
        self.inner.set_paused(paused, Instant::now());
    }

    /// `RingSink::level`; `None` while recovering.
    pub fn level(&self) -> Option<AudioLevel> {
        self.inner.level()
    }

    /// `Playing` or `Recovering`.
    pub fn status(&self) -> AudioStatus {
        self.inner.status()
    }
}

#[cfg(feature = "audio-cpal")]
impl AudioSink for CpalSink {
    /// May reopen the device (only after a failure, at most every 2 s): no allocation while healthy.
    fn push_samples(&mut self, interleaved_stereo: &[i16]) {
        self.inner.push_samples(interleaved_stereo, Instant::now());
    }
}

/// Health of the audio output, for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AudioStatus {
    /// A device stream is open (it may be paused and playing silence).
    Playing,
    /// The stream failed; samples are discarded and the default device is reopened periodically.
    Recovering,
    /// No device output (`--no-audio`, no device at startup, built without `audio-cpal`).
    Disabled,
}

/// What the UI holds: a real device, or a silent fallback. The UI never names `CpalSink`, so it compiles the
/// same with or without `audio-cpal`.
#[non_exhaustive]
pub enum AudioOutput {
    #[cfg(feature = "audio-cpal")]
    Device(CpalSink),
    Null(NullSink),
}

impl AudioOutput {
    /// With `audio-cpal`: tries `CpalSink::open`, on error one `warn!` with the reason and `Null`.
    /// Without: one `warn!` (`AudioError::NotCompiled`) and `Null`. Never panics.
    pub fn open_or_null(input_rate: f64, config: &AudioConfig) -> Self {
        #[cfg(feature = "audio-cpal")]
        let result = CpalSink::open(input_rate, config).map(Self::Device);
        #[cfg(not(feature = "audio-cpal"))]
        let result: Result<Self, crate::audio_ring::AudioError> = {
            let _ = (input_rate, config);
            Err(crate::audio_ring::AudioError::NotCompiled)
        };
        result.unwrap_or_else(|err| {
            warn!("audio disabled: {err}");
            Self::null()
        })
    }

    /// Silent output (`--no-audio`, tests).
    pub fn null() -> Self {
        Self::Null(NullSink)
    }

    /// `true` for a device output, even while it is recovering from a stream failure (see `status`).
    pub fn is_device(&self) -> bool {
        match self {
            #[cfg(feature = "audio-cpal")]
            Self::Device(_) => true,
            Self::Null(_) => false,
        }
    }

    /// No-op for `Null`.
    pub fn set_input_rate(&mut self, input_rate: f64) {
        match self {
            #[cfg(feature = "audio-cpal")]
            Self::Device(sink) => sink.set_input_rate(input_rate),
            Self::Null(_) => {
                let _ = input_rate;
            }
        }
    }

    /// `RingSink::set_paused`; no-op for `Null`.
    pub fn set_paused(&mut self, paused: bool) {
        match self {
            #[cfg(feature = "audio-cpal")]
            Self::Device(sink) => sink.set_paused(paused),
            Self::Null(_) => {
                let _ = paused;
            }
        }
    }

    /// `RingSink::level`; `None` for `Null` (the pacer then falls back to the wall clock).
    pub fn level(&self) -> Option<AudioLevel> {
        match self {
            #[cfg(feature = "audio-cpal")]
            Self::Device(sink) => sink.level(),
            Self::Null(_) => None,
        }
    }

    /// `Disabled` for `Null`; for a device, `Playing` or `Recovering` (stream failed, reopening periodically).
    pub fn status(&self) -> AudioStatus {
        match self {
            #[cfg(feature = "audio-cpal")]
            Self::Device(sink) => sink.status(),
            Self::Null(_) => AudioStatus::Disabled,
        }
    }

    /// `AudioStats::default()` for `Null`.
    pub fn stats(&self) -> AudioStats {
        match self {
            #[cfg(feature = "audio-cpal")]
            Self::Device(sink) => sink.stats(),
            Self::Null(_) => AudioStats::default(),
        }
    }
}

impl AudioSink for AudioOutput {
    fn push_samples(&mut self, interleaved_stereo: &[i16]) {
        match self {
            #[cfg(feature = "audio-cpal")]
            Self::Device(sink) => sink.push_samples(interleaved_stereo),
            Self::Null(sink) => sink.push_samples(interleaved_stereo),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_output_accepts_samples() {
        let mut out = AudioOutput::null();
        assert!(!out.is_device());
        out.push_samples(&[1, 2, 3, 4]);
        out.set_input_rate(32_040.0);
        out.set_paused(true);
        out.set_paused(false);
        assert_eq!(out.level(), None);
        assert_eq!(out.stats(), AudioStats::default());
        assert_eq!(out.status(), AudioStatus::Disabled);
    }

    /// Opens the real default device: falls back to `Null` with a warning when there is none (this container).
    /// `cargo test -p emu-runtime -- --ignored open_or_null --nocapture`
    #[test]
    #[ignore = "touches the host audio system"]
    fn open_or_null_never_panics() {
        let out = AudioOutput::open_or_null(32_040.0, &AudioConfig::default());
        eprintln!(
            "audio output is_device = {}, status = {:?}",
            out.is_device(),
            out.status()
        );
        #[cfg(feature = "audio-cpal")]
        if let Err(err) = CpalSink::open(32_040.0, &AudioConfig::default()) {
            eprintln!("CpalSink::open: {err}");
        }
        assert_eq!(out.stats().underruns, 0);
    }
}
