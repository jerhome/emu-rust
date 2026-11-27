//! Custom systems, UI side (design 8.0 §2.5, ADR 0025). Contract stubs (PX0): bodies belong to lots PU1 to PU4.

mod addability;

pub use self::addability::{AddWarning, Addability, addability, addability_with};
