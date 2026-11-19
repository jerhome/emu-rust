use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// Destination for the core's interleaved stereo `i16` samples, at the core's `timing.sample_rate`.
/// Real output: `CpalSink` / `AudioOutput` (audio_out.rs).
pub trait AudioSink {
    fn push_samples(&mut self, interleaved_stereo: &[i16]);
}

/// Discards samples (no audio device: dev container, noVNC).
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl AudioSink for NullSink {
    fn push_samples(&mut self, _interleaved_stereo: &[i16]) {}
}

/// Accumulates every sample (tests, WAV dumps).
#[derive(Debug, Default, Clone)]
pub struct CollectSink {
    pub samples: Vec<i16>,
}

impl AudioSink for CollectSink {
    fn push_samples(&mut self, interleaved_stereo: &[i16]) {
        self.samples.extend_from_slice(interleaved_stereo);
    }
}

/// Writes a 16-bit PCM stereo WAV. `sample_rate` is the core rate rounded to the nearest integer.
/// A trailing odd sample (incomplete stereo frame) is dropped so the file stays well-formed.
pub fn write_wav(path: &Path, sample_rate: u32, interleaved_stereo: &[i16]) -> io::Result<()> {
    const CHANNELS: u16 = 2;
    const BITS: u16 = 16;
    const BLOCK_ALIGN: u16 = CHANNELS * BITS / 8;

    let samples = &interleaved_stereo[..interleaved_stereo.len() & !1];
    let too_big = || io::Error::new(io::ErrorKind::InvalidInput, "audio too long for a WAV file");
    let data_len = u32::try_from(samples.len() * 2).map_err(|_| too_big())?;
    let riff_len = data_len.checked_add(36).ok_or_else(too_big)?;
    let byte_rate = sample_rate
        .checked_mul(u32::from(BLOCK_ALIGN))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "sample rate too high"))?;

    let mut out = BufWriter::new(File::create(path)?);
    out.write_all(b"RIFF")?;
    out.write_all(&riff_len.to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?; // PCM
    out.write_all(&CHANNELS.to_le_bytes())?;
    out.write_all(&sample_rate.to_le_bytes())?;
    out.write_all(&byte_rate.to_le_bytes())?;
    out.write_all(&BLOCK_ALIGN.to_le_bytes())?;
    out.write_all(&BITS.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data_len.to_le_bytes())?;
    for sample in samples {
        out.write_all(&sample.to_le_bytes())?;
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_sink_appends() {
        let mut sink = CollectSink::default();
        sink.push_samples(&[1, 2]);
        sink.push_samples(&[3, -4]);
        assert_eq!(sink.samples, [1, 2, 3, -4]);
        NullSink.push_samples(&[1, 2]);
    }

    #[test]
    fn wav_layout() {
        let dir = std::env::temp_dir().join(format!("emu-runtime-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.wav");
        write_wav(&path, 32_040, &[1, -1, 0x1234, 7, 99]).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(bytes.len(), 44 + 8);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 36 + 8);
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1);
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2);
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            32_040
        );
        assert_eq!(
            u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            32_040 * 4
        );
        assert_eq!(u16::from_le_bytes([bytes[32], bytes[33]]), 4);
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8);
        assert_eq!(&bytes[44..], &[1, 0, 0xff, 0xff, 0x34, 0x12, 7, 0]);
    }
}
