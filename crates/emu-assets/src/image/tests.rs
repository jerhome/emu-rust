use super::*;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/data/covers/images/"
);

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIXTURES}{name}")).unwrap()
}

fn encoded(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let img = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
    });
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img)
        .write_to(&mut out, format)
        .unwrap();
    out.into_inner()
}

#[test]
fn sniff() {
    assert_eq!(sniff_format(&fixture("valid.png")), Some(ImageFormat::Png));
    assert_eq!(
        sniff_format(&[0xFF, 0xD8, 0xFF, 0xE0]),
        Some(ImageFormat::Jpeg)
    );
    assert_eq!(sniff_format(&fixture("bad-signature.png")), None);
    assert_eq!(sniff_format(b"GIF89a"), None);
    assert_eq!(sniff_format(b"RIFF\0\0\0\0WEBPVP8 "), None);
    assert_eq!(sniff_format(&[]), None);
}

#[test]
fn valid_png_fixture_and_generated_images() {
    let img = decode_cover(&fixture("valid.png"), 512).unwrap();
    assert_eq!(img.rgba.len() as u32, img.width * img.height * 4);

    let png = decode_cover(&encoded(300, 200, image::ImageFormat::Png), 512).unwrap();
    assert_eq!((png.width, png.height), (300, 200));
    let jpg = decode_cover(&encoded(64, 32, image::ImageFormat::Jpeg), 512).unwrap();
    assert_eq!((jpg.width, jpg.height), (64, 32));
}

#[test]
fn downscaled_keeping_aspect() {
    let img = decode_cover(&encoded(1000, 500, image::ImageFormat::Png), 100).unwrap();
    assert_eq!((img.width, img.height), (100, 50));
    assert_eq!(img.rgba.len(), 100 * 50 * 4);
    let tiny = decode_cover(&encoded(10, 10, image::ImageFormat::Png), 0).unwrap();
    assert_eq!((tiny.width, tiny.height), (1, 1));
}

#[test]
fn hostile_inputs_are_refused() {
    for name in ["truncated.png", "huge-header.png", "bad-signature.png"] {
        assert!(
            matches!(
                decode_cover(&fixture(name), 512),
                Err(AssetError::InvalidImage(_))
            ),
            "{name}"
        );
    }
    let mut jpeg = encoded(64, 64, image::ImageFormat::Jpeg);
    let half = jpeg.len() / 2;
    jpeg[20..half].fill(0x55);
    assert!(decode_cover(&jpeg, 512).is_err());
    jpeg.truncate(10);
    assert!(decode_cover(&jpeg, 512).is_err());
    assert!(decode_cover(&[], 512).is_err());
    assert!(decode_cover(b"GIF89a\x01\0\x01\0", 512).is_err());
    // Larger than the 4096 limit, declared in a valid JPEG.
    let wide = encoded(4097, 1, image::ImageFormat::Jpeg);
    assert!(decode_cover(&wide, 512).is_err());
    let big = vec![0x89u8; MAX_COVER_BYTES as usize + 1];
    assert!(decode_cover(&big, 512).is_err());
}

#[test]
fn random_bytes_never_panic() {
    // xorshift: deterministic, no extra dependency.
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let valid_png = fixture("valid.png");
    for seed in 0..1000u32 {
        let len = (next() % 512) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        // Half of the cases keep a real signature so the decoders are exercised.
        let prefix: &[u8] = match seed % 3 {
            0 => PNG_SIGNATURE,
            1 => JPEG_SIGNATURE,
            _ => &[],
        };
        bytes.splice(0..0, prefix.iter().copied());
        let _ = decode_cover(&bytes, 64);
        // Mutated valid PNG.
        let mut mutated = valid_png.clone();
        let at = (next() as usize) % mutated.len();
        mutated[at] = next() as u8;
        let _ = decode_cover(&mutated, 64);
    }
}

/// Uniform grey JPEG: DQT, one DC table (and one AC table for baseline) whose single 1-bit code is category 0 /
/// EOB, one interleaved scan holding every component (DC only when progressive). `sampling`: one `0xHV` byte per
/// component.
fn uniform_jpeg(progressive: bool, width: u16, height: u16, sampling: &[u8]) -> Vec<u8> {
    let components = sampling.len() as u8;
    let mut out = vec![0xFF, 0xD8];
    // DQT: table 0, all ones.
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 67, 0x00]);
    out.extend_from_slice(&[1; 64]);
    let len = 8 + 3 * u16::from(components);
    out.extend_from_slice(&[0xFF, if progressive { 0xC2 } else { 0xC0 }]);
    out.extend_from_slice(&len.to_be_bytes());
    out.push(8);
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.push(components);
    for (id, hv) in (1..).zip(sampling) {
        out.extend_from_slice(&[id, *hv, 0x00]);
    }
    let tables: &[u8] = if progressive { &[0x00] } else { &[0x00, 0x10] };
    out.extend_from_slice(&[0xFF, 0xC4, 0x00, 2 + 18 * tables.len() as u8]);
    for &class in tables {
        out.extend_from_slice(&[class, 1]);
        out.extend_from_slice(&[0; 15]);
        out.push(0x00);
    }
    let len = 6 + 2 * u16::from(components);
    out.extend_from_slice(&[0xFF, 0xDA]);
    out.extend_from_slice(&len.to_be_bytes());
    out.push(components);
    for id in 1..=components {
        out.extend_from_slice(&[id, 0x00]);
    }
    let spectral_end = if progressive { 0 } else { 63 };
    out.extend_from_slice(&[0, spectral_end, 0]);
    let h_max = sampling.iter().map(|s| s >> 4).max().unwrap();
    let v_max = sampling.iter().map(|s| s & 0x0F).max().unwrap();
    let mcus = usize::from(width).div_ceil(8 * usize::from(h_max))
        * usize::from(height).div_ceil(8 * usize::from(v_max));
    let per_mcu: usize = sampling
        .iter()
        .map(|s| usize::from(s >> 4) * usize::from(s & 0x0F))
        .sum();
    // One `0` bit per code (DC, plus EOB for baseline), then padding with ones.
    let bits = mcus * per_mcu * if progressive { 1 } else { 2 };
    out.extend(std::iter::repeat_n(0x00, bits / 8));
    if !bits.is_multiple_of(8) {
        out.push(0xFF >> (bits % 8));
    }
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

fn progressive_jpeg(width: u16, height: u16, components: u8) -> Vec<u8> {
    uniform_jpeg(true, width, height, &vec![0x11; usize::from(components)])
}

const YUV420: &[u8] = &[0x22, 0x11, 0x11];
const YUV444: &[u8] = &[0x11, 0x11, 0x11];

fn too_large(result: Result<CoverImage, AssetError>) -> bool {
    matches!(result, Err(AssetError::InvalidImage(m)) if m.contains("mémoire"))
}

#[test]
fn jpeg_budget_counts_every_component_at_max_sampling() {
    // 2048² 4:2:0: 128 × 128 MCUs × 4 blocks × 3 components × 128 bytes, plus 3 × 2048 × 2 × 80 per row.
    assert_eq!(
        jpeg_budget(2048, 2048, 3, 2, 2),
        3 * 128 * 128 * 4 * 128 + 3 * 2048 * 2 * 80
    );
    // Subsampling saves nothing: chroma is counted at the luma's resolution.
    assert_eq!(
        jpeg_budget(2048, 2048, 3, 2, 2) - 3 * 2048 * 2 * 80,
        jpeg_budget(2048, 2048, 3, 1, 1) - 3 * 2048 * 80
    );
    // Partial MCUs are padded.
    assert_eq!(jpeg_budget(17, 9, 1, 2, 2), 2 * 4 * 128 + 2 * 16 * 2 * 80);
    // Factors up to 15 (zune does not bound the vertical one): still about 2 bytes per pixel per component.
    assert!(jpeg_budget(4096, 4096, 4, 8, 15) < 4 * 4096 * 4096 * 2 + 80 * 1024 * 1024);
}

#[test]
fn usual_jpegs_are_decoded() {
    for (progressive, side, sampling) in [
        (false, 64, YUV420),
        (true, 48, YUV420),
        (false, 2048, YUV420),
        (true, 2048, YUV420),
        (true, 2048, YUV444),
        (false, 2048, &[0x11][..]),
    ] {
        let bytes = uniform_jpeg(progressive, side, side, sampling);
        let img = decode_cover(&bytes, 320)
            .unwrap_or_else(|e| panic!("{progressive} {side} {sampling:02X?}: {e}"));
        assert_eq!(
            (img.width, img.height),
            (side.min(320) as u32, side.min(320) as u32)
        );
        assert_eq!(validate_cover(&bytes).unwrap(), ImageFormat::Jpeg);
    }
    // Real encoder output (baseline).
    for (w, h) in [(64, 32), (333, 17)] {
        assert!(decode_cover(&encoded(w, h, image::ImageFormat::Jpeg), 512).is_ok());
    }
}

#[test]
fn large_jpegs_are_refused_before_decoding() {
    // 3 components above about 2700²: refused whatever the sampling or the coding (6 bytes of coefficients and
    // 3 of output per pixel), baseline included.
    for progressive in [false, true] {
        for sampling in [YUV420, YUV444] {
            for side in [2800, 4096] {
                let bytes = uniform_jpeg(progressive, side, side, sampling);
                assert!(too_large(decode_cover(&bytes, 320)), "{progressive} {side}");
                assert!(validate_cover(&bytes).is_err());
            }
        }
    }
    // A single component fits up to 4096².
    assert!(jpeg_budget(4096, 4096, 1, 1, 1) + 4096 * 4096 <= MAX_DECODE_ALLOC);
}

/// The reviewer's forgery: an APP0 of declared length 6, of which zune-jpeg 0.5.15 consumes 5 bytes before
/// skipping `length - 5 - 2` (saturating), so it swallows the `FF` of the next marker and reads the frame hidden
/// in the following APP1 payload, while a plain marker walk sees an opaque APP1 and the frame after it.
fn app0_forgery(
    baseline: bool,
    side: u16,
    visible: &[u8],
    visible_scan: u8,
    hidden: &[u8],
    hidden_scan: u8,
) -> Vec<u8> {
    let frame = |sampling: &[u8], scan: u8| {
        let mut jpeg = uniform_jpeg(!baseline, side, side, sampling);
        // Drop SOI and the entropy-coded data, keep DQT, SOF, DHT and SOS.
        let sos = jpeg.windows(2).position(|w| w == [0xFF, 0xDA]).unwrap();
        jpeg.truncate(sos);
        jpeg.drain(..2);
        let len = 6 + 2 * u16::from(scan);
        jpeg.extend_from_slice(&[0xFF, 0xDA]);
        jpeg.extend_from_slice(&len.to_be_bytes());
        jpeg.push(scan);
        for id in 1..=scan {
            jpeg.extend_from_slice(&[id, 0x00]);
        }
        jpeg.extend_from_slice(&[0, if baseline { 63 } else { 0 }, 0]);
        jpeg
    };
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x06, b'J', b'F', b'I', b'F'];
    let payload = frame(hidden, hidden_scan);
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&(payload.len() as u16 + 2).to_be_bytes());
    out.extend(payload);
    out.extend(frame(visible, visible_scan));
    out.extend([0x12; 4096]);
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

#[test]
fn app0_forgeries_are_refused() {
    let variants = [
        app0_forgery(true, 4096, &[0x44, 0x11, 0x11, 0x11], 4, &[0x44; 4], 1),
        app0_forgery(false, 3000, &[0x22, 0x11, 0x11], 3, &[0x22; 3], 3),
        app0_forgery(false, 3100, &[0x44, 0x11, 0x11, 0x11], 4, &[0x44; 4], 4),
    ];
    for (i, bytes) in variants.iter().enumerate() {
        assert!(too_large(decode_cover(bytes, 320)), "variant {i}");
        assert!(validate_cover(bytes).is_err(), "variant {i}");
    }
}

/// Hides a fake 8×8 SOF0 + SOS inside the payload zune-jpeg skips after `FF D0` (it reads RSTn with a length:
/// `FF C0` = 0xFFC0 bytes), then puts a real 4096² progressive frame after it.
fn smuggled_progressive_jpeg() -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xD0];
    let skipped_from = out.len() + 2;
    out.extend_from_slice(&[0xFF, 0xC0, 0x00, 11, 8, 0, 8, 0, 8, 1, 1, 0x11, 0]);
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 8, 1, 1, 0, 0, 63, 0]);
    out.resize(skipped_from + 0xFFC0 - 2, 0);
    out.extend_from_slice(&progressive_jpeg(4096, 4096, 3)[2..]);
    out
}

#[test]
fn smuggled_frames_are_refused() {
    let bytes = smuggled_progressive_jpeg();
    assert!(decode_cover(&bytes, 512).is_err());
    assert!(validate_cover(&bytes).is_err());
}

#[test]
fn downscale_is_in_the_budget() {
    // 3000² RGBA8: 34 MiB decoded, plus 3000 × 512 × 16 for the Rgba32F pass, plus the small results.
    assert_eq!(
        post_decode_bytes(3000, 3000, 4, 36_000_000, 512),
        36_000_000 + 3000 * 512 * 16 + 512 * 512 * 8
    );
    // No downscale: bounded by the same formula (the RGBA copy is at most 4 bytes per pixel).
    assert!(post_decode_bytes(100, 50, 3, 15_000, 512) >= 15_000 + 100 * 50 * 4);
    // A smaller `max_side` never costs more: what `validate_cover` accepted, `decode_cover(.., 320)` decodes.
    for side in [1, 64, 320, 512] {
        assert!(
            post_decode_bytes(4000, 3000, 4, 48_000_000, side)
                <= post_decode_bytes(4000, 3000, 4, 48_000_000, VALIDATED_DISPLAY_SIDE)
        );
    }
    // 4096² RGBA8 fits the decoder (exactly 64 MiB) but not with its downscale.
    assert!(
        post_decode_bytes(4096, 4096, 4, MAX_DECODE_ALLOC, VALIDATED_DISPLAY_SIDE)
            > MAX_DECODE_ALLOC
    );
}
