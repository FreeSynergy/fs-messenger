//! Autocrypt, written by ourselves (ADR 0018, no Delta Chat core).

mod header;
mod learner;
mod rule;

pub use header::{AutocryptHeader, V6_ATTRIBUTE, format_header, format_header_with, offered_for};
pub use learner::{KeyLearner, Offer};
pub use rule::{Decision, decide};
