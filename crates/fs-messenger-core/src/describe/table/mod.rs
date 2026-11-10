//! THE capability table: every capability id of `requirements.md` and of the core, with the command (or field) that
//! carries it, its kind, input, whether the core can do it, and why not.
//!
//! Pattern: Facade — declares the parts; the table is `all::table`.

mod account;
mod all;
mod common;
mod conversation;
mod draft;
mod message;

pub use all::table;
