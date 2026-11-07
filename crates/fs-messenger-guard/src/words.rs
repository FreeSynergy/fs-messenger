//! Words a person reads, apart from the code that decides (ADR 0057, coding.md section 2a).
//!
//! Pattern: none, allowed by section 2 (constants). Code never decides by comparing these.

/// Where the text goes, when the engine does not know.
pub(crate) const DESTINATION_UNKNOWN: &str = "unbekannt";
