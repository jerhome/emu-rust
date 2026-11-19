//! Real audio output. The cpal stream is behind the `audio-cpal` feature (default on); built without it,
//! `AudioOutput::open_or_null` always returns the silent output.

use crate::audio::{AudioSink, NullSink};
use crate::audio_ring::{AudioConfig, AudioStats};
#[cfg(feature = "audio-cpal")]
use crate::audio_ring::{AudioError, RingSink};

/// Plays the core's audio on the default output device: `RingSink` + cpal stream whose callback owns the
/// `RingReader`. `!Send` (owns the cpal stream): create and use it on the emulation thread.
#[cfg(feature = "audio-cpal")]
pub struct CpalSink {
    // Placeholder until runtime-av implements it.
    _stream: std::marker::PhantomData<(cpal::Stream, RingSink)>,
}

#[cfg(feature = "audio-cpal")]
impl CpalSink {
    /// Default host → default output device → default output config (F32/I16/U16/I32, else
    /// `UnsupportedFormat`), `BufferSize::Default`, stream started. Never panics.
    pub fn open(input_rate: f64, config: &AudioConfig) -> Result<Self, AudioError> {
        let _ = (input_rate, config);
        todo!()
    }

    pub fn device_rate(&self) -> u32 {
        todo!()
    }

    pub fn set_input_rate(&mut self, input_rate: f64) {
        let _ = input_rate;
        todo!()
    }

    pub fn stats(&self) -> AudioStats {
        todo!()
    }
}

#[cfg(feature = "audio-cpal")]
impl AudioSink for CpalSink {
    fn push_samples(&mut self, interleaved_stereo: &[i16]) {
        let _ = interleaved_stereo;
        todo!()
    }
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
        let _ = (input_rate, config);
        todo!()
    }

    /// Silent output (`--no-audio`, tests).
    pub fn null() -> Self {
        Self::Null(NullSink)
    }

    pub fn is_device(&self) -> bool {
        todo!()
    }

    /// No-op for `Null`.
    pub fn set_input_rate(&mut self, input_rate: f64) {
        let _ = input_rate;
        todo!()
    }

    /// `AudioStats::default()` for `Null`.
    pub fn stats(&self) -> AudioStats {
        todo!()
    }
}

impl AudioSink for AudioOutput {
    fn push_samples(&mut self, interleaved_stereo: &[i16]) {
        let _ = interleaved_stereo;
        todo!()
    }
}
