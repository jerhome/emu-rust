//! The only file that touches gilrs (ADR 0016 §2: replaceable by an SDL3 source without other changes).

use super::source::{GamepadError, GamepadSource, PadEvent};

/// Real pads through gilrs 0.11. Pads with `MappingSource::None` are ignored with a `warn!`.
pub struct GilrsSource {
    // gilrs::Gilrs and a GamepadId -> PadId table, filled by T2.
}

impl GilrsSource {
    /// Fails (never panics) when the platform backend cannot start, e.g. no `/dev/input` in a container.
    pub fn new() -> Result<Self, GamepadError> {
        todo!("T2")
    }
}

impl GamepadSource for GilrsSource {
    fn poll_events(&mut self, out: &mut Vec<PadEvent>) {
        let _ = out;
        todo!("T2")
    }
}
