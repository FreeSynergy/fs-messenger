//! The brake: which risk classes can ever be released (ADR 0054).
//!
//! Pattern: Specification: a pure predicate over a risk class, with no state and no side effect.
//! Everything else asks it, and a record in the store has no say: `unknown`, `destroys` and
//! `actsForPerson` ask every time whatever is stored. There is no "allow this class" in any question
//! (strategy 4d(1), 2026-11-07): a release is only ever given by a person through `Guard::release`.

use fs_messenger_core::ToolCallRisk;

/// Whether a person may release this class at all. Only `readsOnly` and `changes`.
pub fn can_be_released(risk: ToolCallRisk) -> bool {
    matches!(risk, ToolCallRisk::ReadsOnly | ToolCallRisk::Changes)
}
