//! The words of the program: the English key table (ADR 0057, coding.md 2a).
//!
//! Pattern: Facade — declares the tables and `say.rs` (the `Say` strategy over them).

mod choices;
mod commands;
mod frontend;
mod labels;
mod say;

pub use say::{Words, frontend_keys};
