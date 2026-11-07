//! Names that hand out a secret: a rule, not a list (strategy decision 4d, 2026-11-07).
//!
//! Pattern: Specification: a pure predicate over a name, with no state and no side effect. SHELL: it
//! answers `false` until the rule is built.

/// Whether a tool name is one of a secret.
pub fn is_secret_name(_name: &str) -> bool {
    false
}
