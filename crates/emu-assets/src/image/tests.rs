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
