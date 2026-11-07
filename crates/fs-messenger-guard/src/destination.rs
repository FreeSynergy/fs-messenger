//! Where the text of a call goes, as the engine knows it (ADR 0054, "Lesen ist nicht harmlos").
//!
//! Pattern: Strategy: trait [`DestinationLookup`]. The engine asks it when it shows a question, for the
//! participant and the program of the call, both from its own record. The answer never comes from an
//! AI: not from a call's arguments, not from a name, not from what the AI says of itself. What the
//! engine does not know reads [`Destination::Unknown`], and the question then says so (`words.rs`).
//! [`Destination`] is a Value object: a small type, compared by value.

use async_trait::async_trait;

/// What the engine knows about where the text goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Known to the engine: in words a person can read ("only this device", "to Anthropic").
    Known(String),
    /// Not known. A question shows "unknown", never an empty place.
    Unknown,
}

/// Where the engine learns where the text of a participant's calls in a program goes: from what it
/// knows about the participants and the operator.
#[async_trait]
pub trait DestinationLookup: Send + Sync {
    /// The destination for this participant and this program. A source that cannot tell answers
    /// `Unknown`.
    async fn destination(&self, participant: &str, program: &str) -> Destination;
}
