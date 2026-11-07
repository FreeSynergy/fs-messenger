//! Autocrypt, written by ourselves (ADR 0018, no Delta Chat core).

mod header;
mod learner;
mod rule;

pub use header::{AutocryptHeader, format_header, offered_for};
pub use learner::{KeyLearner, Offer};
pub use rule::{Decision, decide};
