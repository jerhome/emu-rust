//! Hardware rendering requests, frame kinds and input descriptors (design 8.0 §2.3, ADR 0025 §8; contract 9.0
//! builds on it).
//!
//! 8.0 never grants `SET_HW_RENDER`: the request is copied (context and version only) so the runtime can refuse the
//! game with "planned for 9.0", and the call is answered `false` (the core may fall back to software rendering).
//! No function pointer of the core's `retro_hw_render_callback` is ever read or called (`context_reset` included).

use std::ffi::{c_char, c_uint, c_void};

/// What a core asked through `SET_HW_RENDER`. `#[non_exhaustive]` (contract 9.0: fields may be added); built
/// outside the crate with [`HwRenderRequest::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HwRenderRequest {
    pub context: HwContextKind,
    pub version_major: u32,
    pub version_minor: u32,
}

impl HwRenderRequest {
    pub fn new(context: HwContextKind, version_major: u32, version_minor: u32) -> Self {
        Self {
            context,
            version_major,
            version_minor,
        }
    }
}

/// `retro_hw_context_type`. Conversion from the raw value is done by `match`, never by transmutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HwContextKind {
    OpenGl,
    OpenGlEs2,
    OpenGlCore,
    OpenGlEs3,
    OpenGlEsVersion,
    Vulkan,
    Direct3D11,
    /// Raw value not known to this frontend (`RETRO_HW_CONTEXT_NONE`, D3D10 / 12 / 9, future values).
    Unknown(u32),
}

impl HwContextKind {
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            RETRO_HW_CONTEXT_OPENGL => Self::OpenGl,
            RETRO_HW_CONTEXT_OPENGLES2 => Self::OpenGlEs2,
            RETRO_HW_CONTEXT_OPENGL_CORE => Self::OpenGlCore,
            RETRO_HW_CONTEXT_OPENGLES3 => Self::OpenGlEs3,
            RETRO_HW_CONTEXT_OPENGLES_VERSION => Self::OpenGlEsVersion,
            RETRO_HW_CONTEXT_VULKAN => Self::Vulkan,
            RETRO_HW_CONTEXT_D3D11 => Self::Direct3D11,
            other => Self::Unknown(other),
        }
    }
}

// `enum retro_hw_context_type` (libretro.h).
const RETRO_HW_CONTEXT_OPENGL: u32 = 1;
const RETRO_HW_CONTEXT_OPENGLES2: u32 = 2;
const RETRO_HW_CONTEXT_OPENGL_CORE: u32 = 3;
const RETRO_HW_CONTEXT_OPENGLES3: u32 = 4;
const RETRO_HW_CONTEXT_OPENGLES_VERSION: u32 = 5;
const RETRO_HW_CONTEXT_VULKAN: u32 = 6;
const RETRO_HW_CONTEXT_D3D11: u32 = 7;

/// `struct retro_hw_render_callback` (libretro.h). Only `context_type` and the version are read, field by field:
/// the function pointers may be null and the C `bool` fields are kept as `u8` so no read can build an invalid
/// Rust value.
#[repr(C)]
#[allow(non_camel_case_types, dead_code)]
pub(crate) struct retro_hw_render_callback {
    pub(crate) context_type: c_uint,
    pub(crate) context_reset: *const c_void,
    pub(crate) get_current_framebuffer: *const c_void,
    pub(crate) get_proc_address: *const c_void,
    pub(crate) depth: u8,
    pub(crate) stencil: u8,
    pub(crate) bottom_left_origin: u8,
    pub(crate) version_major: c_uint,
    pub(crate) version_minor: c_uint,
    pub(crate) cache_context: u8,
    pub(crate) context_destroy: *const c_void,
    pub(crate) debug_context: u8,
}

// Layout checks against libretro.h on 64-bit targets (C `bool` = 1 byte, pointers = 8 bytes).
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::offset_of!(retro_hw_render_callback, version_major) == 36);
    assert!(std::mem::size_of::<retro_hw_render_callback>() == 64);
    assert!(std::mem::size_of::<retro_input_descriptor>() == 24);
};

/// Copies the request; `None` for null.
///
/// # Safety
/// `data` is null or points to a `retro_hw_render_callback`, aligned and readable for the call.
pub(crate) unsafe fn read_hw_request(
    data: *const retro_hw_render_callback,
) -> Option<HwRenderRequest> {
    if data.is_null() {
        return None;
    }
    // SAFETY: non-null, aligned and readable per the contract; only `c_uint` fields are read (any bit pattern is
    // valid), through raw place projections, so no reference to the core's struct is created.
    let (context, major, minor) = unsafe {
        (
            (&raw const (*data).context_type).read(),
            (&raw const (*data).version_major).read(),
            (&raw const (*data).version_minor).read(),
        )
    };
    Some(HwRenderRequest::new(
        HwContextKind::from_raw(context),
        major,
        minor,
    ))
}

/// Upper bound of kept `SET_INPUT_DESCRIPTORS` entries.
pub const MAX_INPUT_DESCRIPTORS: usize = 256;
/// Entries read from the core's array at most (empty descriptions are skipped, so more than kept); bounds the scan
/// of an array the core forgot to terminate.
const MAX_DESCRIPTOR_SCAN: usize = MAX_INPUT_DESCRIPTORS * 4;
/// Characters kept from a description (after cleaning).
const MAX_DESCRIPTION_CHARS: usize = 64;
/// Bytes scanned for a description: `MAX_DESCRIPTION_CHARS` 4-byte characters, so an unterminated string is never
/// scanned unboundedly.
const MAX_DESCRIPTION_BYTES: usize = MAX_DESCRIPTION_CHARS * 4;

/// One `retro_input_descriptor` (description ≤ 64 chars, control and bidi characters removed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDescriptor {
    pub port: u32,
    pub device: u32,
    pub index: u32,
    pub id: u32,
    pub description: String,
}

/// `struct retro_input_descriptor` (libretro.h); the array ends with an entry whose `description` is null.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(non_camel_case_types)]
pub(crate) struct retro_input_descriptor {
    pub(crate) port: c_uint,
    pub(crate) device: c_uint,
    pub(crate) index: c_uint,
    pub(crate) id: c_uint,
    pub(crate) description: *const c_char,
}

/// Copies the array: stops at the terminator (null `description`), after [`MAX_INPUT_DESCRIPTORS`] kept entries or
/// `MAX_DESCRIPTOR_SCAN` read entries. Entries whose cleaned description is empty are skipped. Null `data`: empty.
///
/// # Safety
/// `data` is null or points to an aligned `retro_input_descriptor` array, valid for the call, terminated by an
/// entry with a null `description` or holding at least `MAX_DESCRIPTOR_SCAN` entries; each non-null
/// `description` is NUL-terminated or readable for `MAX_DESCRIPTION_BYTES` bytes.
pub(crate) unsafe fn copy_input_descriptors(
    data: *const retro_input_descriptor,
) -> Vec<InputDescriptor> {
    let mut out = Vec::new();
    if data.is_null() {
        return out;
    }
    for i in 0..MAX_DESCRIPTOR_SCAN {
        if out.len() >= MAX_INPUT_DESCRIPTORS {
            break;
        }
        // SAFETY: `i` < MAX_DESCRIPTOR_SCAN and no earlier entry was the terminator: inside the array per the
        // contract, aligned like any C array element; plain data (`Copy`, any bit pattern valid).
        let raw = unsafe { data.add(i).read() };
        if raw.description.is_null() {
            break;
        }
        // SAFETY: non-null, NUL-terminated or `MAX_DESCRIPTION_BYTES` long per the contract.
        let description = unsafe { copy_description(raw.description) };
        if description.is_empty() {
            continue;
        }
        out.push(InputDescriptor {
            port: raw.port,
            device: raw.device,
            index: raw.index,
            id: raw.id,
            description,
        });
    }
    out
}

/// Reads at most `MAX_DESCRIPTION_BYTES` bytes up to the NUL, decodes lossily and cleans with [`clean_text`].
///
/// # Safety
/// `text` is non-null and readable up to its NUL or for `MAX_DESCRIPTION_BYTES` bytes, whichever comes first.
unsafe fn copy_description(text: *const c_char) -> String {
    let mut bytes = Vec::new();
    for offset in 0..MAX_DESCRIPTION_BYTES {
        // SAFETY: no NUL among the `offset` previous bytes, so `offset` is inside the string or the readable
        // `MAX_DESCRIPTION_BYTES` per the contract.
        let byte = unsafe { text.add(offset).cast::<u8>().read() };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    clean_text(&String::from_utf8_lossy(&bytes))
}

/// Removes control, invisible and bidirectional formatting characters, trims, keeps `MAX_DESCRIPTION_CHARS` characters.
///
/// Twin of `clean_text` in `crates/emu-assets/src/cores/info.rs` (the crates may not depend on each other): keep the
/// invisible character lists of [`is_invisible_format`] and its `is_invisible` aligned. U+2028 / U+2029 are listed
/// here only because the other one already drops them as whitespace.
pub(crate) fn clean_text(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter(|c| !c.is_control() && !is_invisible_format(*c))
        .collect();
    kept.trim().chars().take(MAX_DESCRIPTION_CHARS).collect()
}

/// Unicode format characters that are invisible or reorder the text shown around them: soft hyphen, ALM, zero
/// width (ZWSP, ZWNJ, ZWJ, word joiner, invisible operators, BOM), LRM / RLM, line / paragraph separators,
/// LRE…RLO, LRI…PDI.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// What the last `run` produced as video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameKind {
    /// At least one software frame (or a dupe of the previous one) and no sentinel.
    Software,
    /// `RETRO_HW_FRAME_BUFFER_VALID` (wins over software frames of the same run).
    HwSentinel,
    /// No software frame during the last `run` (or no `run` since the previous call).
    Missing,
}

/// Bits of the per-run video record kept by the callbacks.
pub(crate) const SEEN_SOFTWARE: u8 = 1;
pub(crate) const SEEN_SENTINEL: u8 = 2;

impl FrameKind {
    pub(crate) fn from_seen(seen: u8) -> Self {
        if seen & SEEN_SENTINEL != 0 {
            Self::HwSentinel
        } else if seen & SEEN_SOFTWARE != 0 {
            Self::Software
        } else {
            Self::Missing
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_kinds_by_match() {
        let kinds: Vec<_> = (0..=8).map(HwContextKind::from_raw).collect();
        assert_eq!(
            kinds,
            [
                HwContextKind::Unknown(0),
                HwContextKind::OpenGl,
                HwContextKind::OpenGlEs2,
                HwContextKind::OpenGlCore,
                HwContextKind::OpenGlEs3,
                HwContextKind::OpenGlEsVersion,
                HwContextKind::Vulkan,
                HwContextKind::Direct3D11,
                HwContextKind::Unknown(8),
            ]
        );
        assert_eq!(
            HwContextKind::from_raw(u32::MAX),
            HwContextKind::Unknown(u32::MAX)
        );
    }

    #[test]
    fn hw_request_reads_context_and_version_only() {
        let cb = retro_hw_render_callback {
            context_type: 3,
            context_reset: std::ptr::null(),
            get_current_framebuffer: std::ptr::null(),
            get_proc_address: std::ptr::null(),
            depth: 7, // not a valid Rust bool: never read
            stencil: 1,
            bottom_left_origin: 0,
            version_major: 4,
            version_minor: 1,
            cache_context: 0,
            context_destroy: std::ptr::null(),
            debug_context: 0,
        };
        // SAFETY: `cb` is a valid, aligned struct; null is allowed.
        let got = unsafe { read_hw_request(&raw const cb) };
        assert_eq!(
            got,
            Some(HwRenderRequest::new(HwContextKind::OpenGlCore, 4, 1))
        );
        // SAFETY: null is allowed by the contract.
        assert_eq!(unsafe { read_hw_request(std::ptr::null()) }, None);
    }

    fn entry(port: u32, id: u32, text: *const c_char) -> retro_input_descriptor {
        retro_input_descriptor {
            port,
            device: 1,
            index: 0,
            id,
            description: text,
        }
    }

    #[test]
    fn descriptors_cleaned_and_terminated() {
        let long = [b'A'; 300].iter().copied().chain([0]).collect::<Vec<u8>>();
        let array = [
            entry(0, 0, c"Fire".as_ptr()),
            entry(0, 1, c"Jump\x07\x1b[31m\u{202E}end".as_ptr()),
            entry(0, 2, long.as_ptr().cast()),
            entry(0, 3, c"".as_ptr()),
            entry(1, 4, c"  \u{2066}Start\u{2069} ".as_ptr()),
            entry(0, 0, std::ptr::null()),
            entry(0, 9, c"after the end".as_ptr()),
        ];
        // SAFETY: terminated array of valid entries.
        let got = unsafe { copy_input_descriptors(array.as_ptr()) };
        let texts: Vec<&str> = got.iter().map(|d| d.description.as_str()).collect();
        assert_eq!(texts, ["Fire", "Jump[31mend", &"A".repeat(64), "Start"]);
        assert_eq!((got[3].port, got[3].device, got[3].id), (1, 1, 4));
        // SAFETY: null is allowed by the contract.
        assert!(unsafe { copy_input_descriptors(std::ptr::null()) }.is_empty());
    }

    #[test]
    fn invisible_and_bidi_characters_removed() {
        let text = "a\u{2028}b\u{2029}c\u{200B}d\u{200D}e\u{2060}f\u{2064}g\u{FEFF}h\u{00AD}i\u{061C}j\u{202E}k\u{2067}l";
        assert_eq!(clean_text(text), "abcdefghijkl");
    }

    #[test]
    fn descriptors_bounded() {
        let mut array: Vec<_> = (0..300).map(|i| entry(0, i, c"Button".as_ptr())).collect();
        array.push(entry(0, 0, std::ptr::null()));
        // SAFETY: terminated array of valid entries.
        let got = unsafe { copy_input_descriptors(array.as_ptr()) };
        assert_eq!(got.len(), MAX_INPUT_DESCRIPTORS);
        assert_eq!(got.last().map(|d| d.id), Some(255));
    }

    #[test]
    fn frame_kind_priority() {
        assert_eq!(FrameKind::from_seen(0), FrameKind::Missing);
        assert_eq!(FrameKind::from_seen(SEEN_SOFTWARE), FrameKind::Software);
        assert_eq!(FrameKind::from_seen(SEEN_SENTINEL), FrameKind::HwSentinel);
        assert_eq!(
            FrameKind::from_seen(SEEN_SOFTWARE | SEEN_SENTINEL),
            FrameKind::HwSentinel
        );
    }
}
