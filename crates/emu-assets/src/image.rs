//! Untrusted cover images (ADR 0014 §5): signature check, then `image` decoding with `Limits`.

use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};

use image::codecs::jpeg::JpegDecoder;
use image::codecs::png::PngDecoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, Limits};

use crate::AssetError;

pub const MAX_COVER_BYTES: u64 = 8 * 1024 * 1024;
/// Larger declared dimensions are refused before any pixel allocation.
pub const MAX_COVER_SIDE: u32 = 4096;
/// Decoder allocations (ADR 0014: 64 MiB): a 4096 × 4096 RGBA8 image fits, a 16-bit one does not.
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const JPEG_SIGNATURE: &[u8] = &[0xFF, 0xD8, 0xFF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverImage {
    pub width: u32,
    pub height: u32,
    /// RGBA8, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// From the magic bytes only (never the server's Content-Type or the URL extension).
pub fn sniff_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(PNG_SIGNATURE) {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(JPEG_SIGNATURE) {
        Some(ImageFormat::Jpeg)
    } else {
        None
    }
}

/// Decodes and downscales (aspect kept, Triangle filter) so that the longest side is at most `max_side`
/// (0 is taken as 1). Truncated, oversized or non PNG/JPEG data is `AssetError::InvalidImage`, never a panic.
/// CPU-bound (up to a few hundred ms for a 4096² JPEG): worker threads only.
pub fn decode_cover(bytes: &[u8], max_side: u32) -> Result<CoverImage, AssetError> {
    if bytes.len() as u64 > MAX_COVER_BYTES {
        return Err(invalid("fichier trop volumineux"));
    }
    let format = sniff_format(bytes).ok_or_else(|| invalid("format non pris en charge"))?;
    // Third-party decoders on hostile input: a panic there must not take the worker thread down. Nothing shared
    // is touched inside, so unwind safety is not a concern.
    let decoded = catch_unwind(AssertUnwindSafe(|| decode(bytes, format)))
        .map_err(|_| invalid("décodeur en échec"))??;
    let max_side = max_side.max(1);
    let image = if decoded.width() > max_side || decoded.height() > max_side {
        decoded.resize(max_side, max_side, FilterType::Triangle)
    } else {
        decoded
    };
    let rgba = image.into_rgba8();
    Ok(CoverImage {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

fn decode(bytes: &[u8], format: ImageFormat) -> Result<DynamicImage, AssetError> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_COVER_SIDE);
    limits.max_image_height = Some(MAX_COVER_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    let cursor = Cursor::new(bytes);
    match format {
        // Header (IHDR) read and dimensions checked inside `with_limits`, before any pixel buffer.
        ImageFormat::Png => checked(PngDecoder::with_limits(cursor, limits.clone()), limits),
        // `new` parses the headers only; `set_limits` then checks the declared dimensions.
        ImageFormat::Jpeg => checked(JpegDecoder::new(cursor), limits),
    }
}

fn checked<D: ImageDecoder>(
    decoder: image::ImageResult<D>,
    limits: Limits,
) -> Result<DynamicImage, AssetError> {
    let mut decoder = decoder.map_err(image_error)?;
    decoder.set_limits(limits).map_err(image_error)?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || width > MAX_COVER_SIDE || height > MAX_COVER_SIDE {
        return Err(invalid(&format!(
            "dimensions refusées ({width} × {height})"
        )));
    }
    // The output buffer is allocated by `from_decoder`, which does not enforce `max_alloc` for every codec.
    if decoder.total_bytes() > MAX_DECODE_ALLOC {
        return Err(invalid("image trop grande en mémoire"));
    }
    DynamicImage::from_decoder(decoder).map_err(image_error)
}

/// The decoder's message only describes the data (never a path or a URL).
fn image_error(error: image::ImageError) -> AssetError {
    match error {
        image::ImageError::Limits(_) => invalid("limites dépassées"),
        other => AssetError::InvalidImage(other.to_string()),
    }
}

fn invalid(message: &str) -> AssetError {
    AssetError::InvalidImage(message.to_owned())
}

#[cfg(test)]
mod tests;
