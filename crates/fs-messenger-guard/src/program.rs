//! The program a tool belongs to, from its name.
//!
//! Pattern: none, allowed by section 2 (a single pure function). A release holds for one program
//! (ADR 0054, "Ohne Befugnis nichts"), and a tool's program is the part of its name before the first
//! dot: `message.open` belongs to `message`. A name without a dot is its own program. It is compared
//! in the one spelling the guard stores (`id_key`), so a variant cannot reach another program's release.

use crate::identity::id_key;

/// The program of a capability id, as the guard compares it.
pub fn program_of(capability: &str) -> String {
    let plain = id_key(capability);
    plain
        .split_once('.')
        .map_or(plain.as_str(), |(program, _)| program)
        .to_string()
}
