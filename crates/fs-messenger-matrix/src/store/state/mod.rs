//! [`FsStateStore`]: the SDK's `StateStore`: rooms, members, room state, the sync token.
//!
//! Pattern: none (this file only declares modules and re-exports; no logic, section 3 of the coding
//! rules). The type is a Decorator over the SDK's memory store (`store.rs`, `decorate.rs`,
//! `restore.rs`); `persist.rs` turns a change into journal entries. What is not kept across a
//! restart is listed in `DESIGN.md`: caches the SDK fills again, and features this crate does not use.

mod decorate;
mod kind;
mod persist;
mod restore;
mod store;

pub use store::FsStateStore;
