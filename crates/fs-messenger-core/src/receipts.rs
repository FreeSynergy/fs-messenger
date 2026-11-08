//! Read receipts only both ways (accepted idea 4, `requirements.md`): who wants to see them must show them.
//!
//! Pattern: Specification: a pure predicate with no state. Providers send a receipt only if it says yes, and
//! a frontend shows one only if it says yes.

/// What one person chose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReceiptChoice {
    /// Not asked: the safe reading, no receipts.
    #[default]
    Hidden,
    /// Shows that they have read, and may see it of others.
    Shown,
}

/// Whether a receipt between me and the other person is sent and shown: only when **both** chose `Shown`.
/// One side hiding hides it for both, so nobody sees what they do not show themselves.
#[must_use]
pub fn receipts_both_ways(mine: ReceiptChoice, theirs: ReceiptChoice) -> bool {
    mine == ReceiptChoice::Shown && theirs == ReceiptChoice::Shown
}

#[cfg(test)]
mod tests {
    use super::ReceiptChoice::{Hidden, Shown};
    use super::*;

    #[test]
    fn only_both_shown_counts() {
        assert!(receipts_both_ways(Shown, Shown));
        assert!(!receipts_both_ways(Shown, Hidden));
        assert!(!receipts_both_ways(Hidden, Shown));
        assert!(!receipts_both_ways(Hidden, Hidden));
    }

    #[test]
    fn the_default_is_hidden() {
        assert!(!receipts_both_ways(
            ReceiptChoice::default(),
            ReceiptChoice::default()
        ));
        assert!(!receipts_both_ways(Shown, ReceiptChoice::default()));
    }
}
