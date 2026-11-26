//! Hardware rendering requests and input descriptors (design 8.0 §2.3, ADR 0025 §8; contract 9.0 builds on it).
//! Contract stubs (PX0): the bodies belong to lot PF1.

use crate::core::Core;

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
    /// Raw value not known to this frontend.
    Unknown(u32),
}

/// Upper bound of kept `SET_INPUT_DESCRIPTORS` entries.
pub const MAX_INPUT_DESCRIPTORS: usize = 256;

/// One `retro_input_descriptor` (description ≤ 64 chars, control and bidi characters removed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDescriptor {
    pub port: u32,
    pub device: u32,
    pub index: u32,
    pub id: u32,
    pub description: String,
}

/// What the last `run` produced as video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameKind {
    Software,
    /// `RETRO_HW_FRAME_BUFFER_VALID`.
    HwSentinel,
    /// No software frame during the last `run`.
    Missing,
}

#[allow(unused_variables)]
impl Core {
    /// Request recorded from `SET_HW_RENDER` (the call is still refused to the core), if any.
    pub fn hw_render_request(&self) -> Option<&HwRenderRequest> {
        todo!("lot PF1")
    }

    /// Kind of the frame of the last `run`; resets the flag.
    pub fn take_frame_kind(&mut self) -> FrameKind {
        todo!("lot PF1")
    }

    /// Last `SET_INPUT_DESCRIPTORS` (≤ [`MAX_INPUT_DESCRIPTORS`]).
    pub fn input_descriptors(&self) -> &[InputDescriptor] {
        todo!("lot PF1")
    }
}
