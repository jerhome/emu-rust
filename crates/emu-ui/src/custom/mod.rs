//! Custom systems, UI side (design 8.0 §2.5, ADR 0025): the filter of the « add » view, the machines a core offers,
//! the definition written from a `.info` and a probe, and the probe itself.

mod addability;
mod draft;
mod probe;

pub use self::addability::{AddWarning, Addability, addability, addability_with};
pub(crate) use self::draft::{AddRow, Machine, MachineState, add_rows, drafts_for};
pub(crate) use self::probe::{ProbeFailure, probe_core, probe_outcome};
