use winit::window::Icon;

static ICON_256: &[u8] = include_bytes!("../../../assets/icon-256.png");

/// Window icon (`assets/icon-256.png`); `None` when it cannot be decoded. macOS ignores it (bundle icon).
pub fn window_icon() -> Option<Icon> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(ICON_256))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()]
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return None,
    };
    Icon::from_rgba(rgba, info.width, info.height).ok()
}
