use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use emu_libretro::{PixelFormat, VideoFrame};

use crate::error::RuntimeError;

/// Tightly packed RGBA8 image (row-major, top row first, alpha always 255).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameRgba {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl FrameRgba {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// `width * height * 4` bytes.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Stable fingerprint used by headless tests: FNV-1a 64 over `width` (u32 LE), `height` (u32 LE), then
    /// `pixels`. Must never change once reference fingerprints are recorded.
    pub fn fingerprint(&self) -> u64 {
        const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let header = self
            .width
            .to_le_bytes()
            .into_iter()
            .chain(self.height.to_le_bytes());
        header
            .chain(self.pixels.iter().copied())
            .fold(OFFSET_BASIS, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(PRIME)
            })
    }

    pub fn write_png(&self, path: &Path) -> Result<(), RuntimeError> {
        if self.width == 0 || self.height == 0 {
            return Err(RuntimeError::Png("empty frame".to_owned()));
        }
        let file = File::create(path).map_err(|source| RuntimeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let png_error = |e: png::EncodingError| match e {
            png::EncodingError::IoError(source) => RuntimeError::Io {
                path: path.to_path_buf(),
                source,
            },
            other => RuntimeError::Png(other.to_string()),
        };
        let mut writer = encoder.write_header().map_err(png_error)?;
        writer.write_image_data(&self.pixels).map_err(png_error)?;
        writer.finish().map_err(png_error)
    }
}

/// Converts a core frame into `dst`, reusing its allocation when the size is unchanged.
///
/// Channel expansion is fixed so fingerprints are reproducible: 5-bit `v` → `(v << 3) | (v >> 2)`,
/// 6-bit `v` → `(v << 2) | (v >> 4)`; XRGB8888 ignores the X byte.
///
/// A frame whose `data` is too short for its declared geometry (core bug) is rejected with a warning and leaves
/// `dst` unchanged.
pub fn convert_frame(src: &VideoFrame<'_>, dst: &mut FrameRgba) {
    let width = src.width as usize;
    let height = src.height as usize;
    let bpp = src.format.bytes_per_pixel();
    // Geometry comes from the core: overflow means a corrupt frame, not something to wrap or panic on.
    let (Some(row_bytes), Some(dst_len)) = (
        width.checked_mul(bpp),
        width.checked_mul(height).and_then(|n| n.checked_mul(4)),
    ) else {
        tracing::warn!(width, height, "ignoring video frame with overflowing size");
        return;
    };
    let required = match height {
        0 => 0,
        h => src.pitch.saturating_mul(h - 1).saturating_add(row_bytes),
    };
    if src.pitch < row_bytes || src.data.len() < required {
        tracing::warn!(
            width,
            height,
            pitch = src.pitch,
            len = src.data.len(),
            "ignoring malformed video frame"
        );
        return;
    }

    dst.width = src.width;
    dst.height = src.height;
    // `resize` keeps the allocation when the size is unchanged: no per-frame allocation.
    dst.pixels.resize(dst_len, 0);
    if row_bytes == 0 {
        return;
    }

    let src_rows = src.data.chunks(src.pitch.max(1)).take(height);
    let dst_rows = dst.pixels.chunks_exact_mut(width * 4);
    for (src_row, dst_row) in src_rows.zip(dst_rows) {
        let src_row = &src_row[..row_bytes];
        match src.format {
            PixelFormat::Xrgb8888 => convert_row_8888(src_row, dst_row),
            PixelFormat::Rgb565 => convert_row_16(src_row, dst_row, rgb565),
            PixelFormat::Xrgb1555 => convert_row_16(src_row, dst_row, xrgb1555),
        }
    }
}

fn convert_row_8888(src: &[u8], dst: &mut [u8]) {
    for (s, d) in src
        .as_chunks::<4>()
        .0
        .iter()
        .zip(dst.as_chunks_mut::<4>().0)
    {
        let v = u32::from_ne_bytes(*s);
        *d = [(v >> 16) as u8, (v >> 8) as u8, v as u8, 0xff];
    }
}

fn convert_row_16(src: &[u8], dst: &mut [u8], expand: fn(u16) -> [u8; 4]) {
    for (s, d) in src
        .as_chunks::<2>()
        .0
        .iter()
        .zip(dst.as_chunks_mut::<4>().0)
    {
        *d = expand(u16::from_ne_bytes(*s));
    }
}

#[inline]
fn expand5(v: u16) -> u8 {
    let v = (v & 0x1f) as u8;
    (v << 3) | (v >> 2)
}

#[inline]
fn expand6(v: u16) -> u8 {
    let v = (v & 0x3f) as u8;
    (v << 2) | (v >> 4)
}

fn rgb565(v: u16) -> [u8; 4] {
    [expand5(v >> 11), expand6(v >> 5), expand5(v), 0xff]
}

fn xrgb1555(v: u16) -> [u8; 4] {
    [expand5(v >> 10), expand5(v >> 5), expand5(v), 0xff]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_of(
        data: &[u8],
        width: u32,
        height: u32,
        pitch: usize,
        format: PixelFormat,
    ) -> FrameRgba {
        let src = VideoFrame {
            data,
            width,
            height,
            pitch,
            format,
        };
        let mut dst = FrameRgba::default();
        convert_frame(&src, &mut dst);
        dst
    }

    #[test]
    fn xrgb8888_ignores_x_byte_and_honours_pitch() {
        let mut data = Vec::new();
        // Row 0: two pixels + 4 bytes of padding; row 1 (last) unpadded.
        data.extend_from_slice(&0xAA11_2233u32.to_ne_bytes());
        data.extend_from_slice(&0x0044_5566u32.to_ne_bytes());
        data.extend_from_slice(&[9, 9, 9, 9]);
        data.extend_from_slice(&0xFF77_8899u32.to_ne_bytes());
        data.extend_from_slice(&0x0000_0000u32.to_ne_bytes());
        let f = frame_of(&data, 2, 2, 12, PixelFormat::Xrgb8888);
        assert_eq!((f.width(), f.height()), (2, 2));
        assert_eq!(
            f.pixels(),
            &[
                0x11, 0x22, 0x33, 255, 0x44, 0x55, 0x66, 255, //
                0x77, 0x88, 0x99, 255, 0, 0, 0, 255,
            ]
        );
    }

    #[test]
    fn rgb565_expansion() {
        let px: [u16; 4] = [0xFFFF, 0x0000, 0xF800, 0x07E0 | 0x0001];
        let data: Vec<u8> = px.iter().flat_map(|p| p.to_ne_bytes()).collect();
        let f = frame_of(&data, 4, 1, 8, PixelFormat::Rgb565);
        assert_eq!(
            f.pixels(),
            &[
                255, 255, 255, 255, 0, 0, 0, 255, 255, 0, 0, 255, 0, 255, 8, 255,
            ]
        );
        // 6-bit mid value: 0b100000 -> (32 << 2) | (32 >> 4) = 130.
        let data = (0b10_0000u16 << 5).to_ne_bytes();
        assert_eq!(
            frame_of(&data, 1, 1, 2, PixelFormat::Rgb565).pixels(),
            &[0, 130, 0, 255]
        );
    }

    #[test]
    fn xrgb1555_expansion_ignores_top_bit() {
        let px: [u16; 3] = [0x7FFF, 0x8000 | 0x7C00, 0x0010];
        let data: Vec<u8> = px.iter().flat_map(|p| p.to_ne_bytes()).collect();
        let f = frame_of(&data, 3, 1, 6, PixelFormat::Xrgb1555);
        // 0b10000 -> (16 << 3) | (16 >> 2) = 132.
        assert_eq!(
            f.pixels(),
            &[255, 255, 255, 255, 255, 0, 0, 255, 0, 0, 132, 255]
        );
    }

    #[test]
    fn padded_pitch_16bit() {
        // 1x2 RGB565 with pitch 6 (4 bytes of padding on row 0).
        let mut data = 0xF800u16.to_ne_bytes().to_vec();
        data.extend_from_slice(&[1, 2, 3, 4]);
        data.extend_from_slice(&0x001Fu16.to_ne_bytes());
        let f = frame_of(&data, 1, 2, 6, PixelFormat::Rgb565);
        assert_eq!(f.pixels(), &[255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn malformed_frame_is_ignored() {
        let data = [0u8; 7];
        let src = VideoFrame {
            data: &data,
            width: 2,
            height: 2,
            pitch: 4,
            format: PixelFormat::Rgb565,
        };
        let mut dst = frame_of(&[1, 2, 3, 4], 1, 1, 4, PixelFormat::Xrgb8888);
        let before = dst.clone();
        convert_frame(&src, &mut dst);
        assert_eq!(dst, before);
    }

    #[test]
    fn overflowing_geometry_is_ignored() {
        let data = [0u8; 4];
        let src = VideoFrame {
            data: &data,
            width: u32::MAX,
            height: u32::MAX,
            pitch: 0,
            format: PixelFormat::Xrgb8888,
        };
        let mut dst = frame_of(&[1, 2, 3, 4], 1, 1, 4, PixelFormat::Xrgb8888);
        let before = dst.clone();
        convert_frame(&src, &mut dst);
        assert_eq!(dst, before);
    }

    #[test]
    fn reuses_allocation_for_same_size() {
        let data = [0u8; 16];
        let mut dst = frame_of(&data, 2, 2, 8, PixelFormat::Xrgb8888);
        let ptr = dst.pixels().as_ptr();
        let src = VideoFrame {
            data: &data,
            width: 2,
            height: 2,
            pitch: 8,
            format: PixelFormat::Xrgb8888,
        };
        convert_frame(&src, &mut dst);
        assert_eq!(dst.pixels().as_ptr(), ptr);
    }

    #[test]
    fn fingerprint_is_fnv1a64_of_header_and_pixels() {
        // FNV-1a 64 of the empty input is the offset basis; the header is always hashed.
        let empty = FrameRgba::default();
        let mut expected: u64 = 0xcbf2_9ce4_8422_2325;
        for b in [0u8; 8] {
            expected = (expected ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
        assert_eq!(empty.fingerprint(), expected);

        let a = frame_of(
            &0x0011_2233u32.to_ne_bytes(),
            1,
            1,
            4,
            PixelFormat::Xrgb8888,
        );
        let b = frame_of(
            &0x0011_2234u32.to_ne_bytes(),
            1,
            1,
            4,
            PixelFormat::Xrgb8888,
        );
        assert_ne!(a.fingerprint(), b.fingerprint());
        // Same pixels, different shape: header makes them differ.
        let wide = frame_of(&[0u8; 8], 2, 1, 8, PixelFormat::Xrgb8888);
        let tall = frame_of(&[0u8; 8], 1, 2, 4, PixelFormat::Xrgb8888);
        assert_ne!(wide.fingerprint(), tall.fingerprint());
        // Frozen reference value: guards against accidental changes of the algorithm.
        assert_eq!(a.fingerprint(), FROZEN_1X1);
    }

    // Independently computed FNV-1a 64 of [1,0,0,0, 1,0,0,0, 0x11,0x22,0x33,0xff].
    const FROZEN_1X1: u64 = 0xdb3d_a0b4_80f9_226c;

    #[test]
    fn png_round_trip() {
        let dir = std::env::temp_dir().join(format!("emu-runtime-png-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("frame.png");
        let f = frame_of(
            &0x0011_2233u32.to_ne_bytes(),
            1,
            1,
            4,
            PixelFormat::Xrgb8888,
        );
        f.write_png(&path).unwrap();
        let decoder =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (1, 1));
        assert_eq!(&buf[..info.buffer_size()], &[0x11, 0x22, 0x33, 255]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(FrameRgba::default().write_png(&path).is_err());
    }
}
