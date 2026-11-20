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
/// Peak heap of one decode (ADR 0014: 64 MiB), downscale included, computed from the headers before any pixel
/// buffer: see `checked`. Not counted: copies of the input itself (≤ [`MAX_COVER_BYTES`] each: `image`'s JPEG
/// decoder copies it, metadata segments), and allocator overhead.
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;

/// Largest `decode_cover` side covered by the download-time check (the library shows covers at 320).
const VALIDATED_DISPLAY_SIDE: u32 = 512;

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
/// (0 is taken as 1). Truncated, oversized or non PNG/JPEG data is `AssetError::InvalidImage`, never a panic, as
/// is an image whose decoding and downscale would need more than 64 MiB: in practice 3-component JPEGs above
/// about 2700² and RGBA8 PNGs above about 3100² (download check, see [`validate_cover`]).
/// CPU-bound (up to a few hundred ms for a large JPEG): worker threads only.
pub fn decode_cover(bytes: &[u8], max_side: u32) -> Result<CoverImage, AssetError> {
    let max_side = max_side.max(1);
    let (_, decoded) = decode_checked(bytes, max_side)?;
    let image = if decoded.width() > max_side || decoded.height() > max_side {
        let resized = decoded.resize(max_side, max_side, FilterType::Triangle);
        // Freed before the RGBA conversion below (see `post_decode_bytes`).
        drop(decoded);
        resized
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

/// Same checks and full decode as [`decode_cover`], without the downscale and RGBA conversion (download-time
/// validation: the bytes are decoded again when displayed). The memory budget counts a downscale to
/// [`VALIDATED_DISPLAY_SIDE`]: what passes here also passes `decode_cover` with that `max_side` or less.
pub(crate) fn validate_cover(bytes: &[u8]) -> Result<ImageFormat, AssetError> {
    decode_checked(bytes, VALIDATED_DISPLAY_SIDE).map(|(format, _)| format)
}

/// `max_side`: the downscale [`decode_cover`] applies afterwards, counted in the memory budget.
fn decode_checked(bytes: &[u8], max_side: u32) -> Result<(ImageFormat, DynamicImage), AssetError> {
    if bytes.len() as u64 > MAX_COVER_BYTES {
        return Err(invalid("fichier trop volumineux"));
    }
    let format = sniff_format(bytes).ok_or_else(|| invalid("format non pris en charge"))?;
    // Third-party decoders on hostile input: a panic there must not take the worker thread down. Nothing shared
    // is touched inside, so unwind safety is not a concern.
    let decoded = catch_unwind(AssertUnwindSafe(|| decode(bytes, format, max_side)))
        .map_err(|_| invalid("décodeur en échec"))??;
    Ok((format, decoded))
}
fn decode(bytes: &[u8], format: ImageFormat, max_side: u32) -> Result<DynamicImage, AssetError> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_COVER_SIDE);
    limits.max_image_height = Some(MAX_COVER_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    let cursor = Cursor::new(bytes);
    match format {
        // Header (IHDR) read and dimensions checked inside `with_limits`, before any pixel buffer.
        ImageFormat::Png => checked(
            PngDecoder::with_limits(cursor, limits.clone()),
            limits,
            0,
            max_side,
        ),
        // `new` parses the headers only; `set_limits` then checks the declared dimensions.
        ImageFormat::Jpeg => {
            let working = jpeg_working_bytes(bytes)?;
            checked(JpegDecoder::new(cursor), limits, working, max_side)
        }
    }
}

/// Worst case of what zune-jpeg 0.5.15 allocates on top of the output buffer, from its own header pass only
/// (`image` 0.25 does not pass `max_alloc` to it). No second parser: whatever zune reads, it budgets for.
///
/// - Whole-image `i16` coefficients (mcu_prog.rs `block`, mcu.rs `progressive_mcus`): needed for progressive
///   images and for baseline ones whose first scan lacks a component, i.e. per component
///   `mcus_x × mcus_y × h × v × 64` with `h ≤ h_max`, `v ≤ v_max`. Always counted, every component at
///   `h_max × v_max`: whether they are needed depends on scans zune reads later, so it is not trusted.
///   Neither are the per-component factors (not exposed): their maxima are.
/// - Per MCU row (`raw_coeff`, upsampling rows and scratch: mcu.rs, mcu_prog.rs, components.rs, worker.rs): per
///   component at most 35 `i16` per padded column (`mcus_x × 8 × h_max`) and per `v_max`; counted as 40.
///
/// Header values cannot change afterwards: zune refuses a second SOF. Consequence: 3-component images above
/// about 2700 × 2700 (6 bytes of coefficients + 3 of RGB output per pixel) are refused, whatever their sampling
/// and coding (baseline included).
fn jpeg_working_bytes(bytes: &[u8]) -> Result<u64, AssetError> {
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::options::DecoderOptions;
    use zune_jpeg::{JpegDecoder, SampleRatios};

    // Same options as `image` 0.25's JPEG decoder (non-strict), our dimension limits on top.
    let side = MAX_COVER_SIDE as usize;
    let options = DecoderOptions::default()
        .set_strict_mode(false)
        .set_max_width(side)
        .set_max_height(side);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder
        .decode_headers()
        .map_err(|e| AssetError::InvalidImage(e.to_string()))?;
    let info = decoder
        .info()
        .ok_or_else(|| invalid("en-tête JPEG illisible"))?;
    let (h_max, v_max) = match info.sample_ratio {
        SampleRatios::None => (1, 1),
        SampleRatios::H => (2, 1),
        SampleRatios::V => (1, 2),
        SampleRatios::HV => (2, 2),
        SampleRatios::Generic(h, v) => (h as u64, v as u64),
    };
    Ok(jpeg_budget(
        u64::from(info.width),
        u64::from(info.height),
        u64::from(info.components),
        h_max,
        v_max,
    ))
}

/// See [`jpeg_working_bytes`].
fn jpeg_budget(width: u64, height: u64, components: u64, h_max: u64, v_max: u64) -> u64 {
    let (h_max, v_max) = (h_max.max(1), v_max.max(1));
    let mcus_x = width.div_ceil(8 * h_max);
    let mcus_y = height.div_ceil(8 * v_max);
    let coefficients = components * mcus_x * mcus_y * h_max * v_max * 64 * 2;
    let rows = components * mcus_x * 8 * h_max * v_max * 40 * 2;
    coefficients.saturating_add(rows)
}

/// Peak bytes after decoding, while the decoded image (`decoded` bytes) is still alive: the downscale of
/// [`decode_cover`] to at most `max_side` (`imageops::resize`: an `Rgba32F` buffer of source width × target
/// height, 16 bytes per element, then the resized image), then the RGBA conversion. Also an upper bound for any
/// smaller `max_side`, and for no downscale at all (RGBA copy of at most 4 bytes per pixel).
fn post_decode_bytes(
    width: u32,
    height: u32,
    bytes_per_pixel: u64,
    decoded: u64,
    max_side: u32,
) -> u64 {
    let (w, h, side) = (u64::from(width), u64::from(height), u64::from(max_side));
    // The aspect is kept: no target side exceeds `max_side` nor the source side.
    let (nw, nh) = (w.min(side), h.min(side));
    decoded + w * nh * 16 + nw * nh * (bytes_per_pixel + 4)
}

/// `working`: what the decoder allocates besides its output buffer, freed before it returns.
fn checked<D: ImageDecoder>(
    decoder: image::ImageResult<D>,
    limits: Limits,
    working: u64,
    max_side: u32,
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
    let output = decoder.total_bytes();
    let bytes_per_pixel = u64::from(decoder.color_type().bytes_per_pixel());
    let peak = working.saturating_add(output).max(post_decode_bytes(
        width,
        height,
        bytes_per_pixel,
        output,
        max_side,
    ));
    if peak > MAX_DECODE_ALLOC {
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
