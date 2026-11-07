//! The brake: which risk classes can ever be released (ADR 0054).
//!
//! Pattern: Specification: pure predicates over a risk class, one built from the other
//! (`offers_allow_class` asks `can_be_released`), with no state and no side effect. Everything else asks
//! these, and a record in the store has no say: `unknown`, `destroys` and `actsForPerson` ask every time whatever is stored.

use fs_messenger_core::ToolCallRisk;

/// Whether a person may release this class at all. Only `readsOnly` and `changes`.
pub fn can_be_released(risk: ToolCallRisk) -> bool {
    matches!(risk, ToolCallRisk::ReadsOnly | ToolCallRisk::Changes)
}

/// Whether a question of this class offers `allowClass`. A call without input never does: there is
/// nothing yet that the person could judge.
pub fn offers_allow_class(risk: ToolCallRisk, has_input: bool) -> bool {
    has_input && can_be_released(risk)
}
