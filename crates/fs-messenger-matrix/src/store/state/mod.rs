//! [`FsStateStore`]: the SDK's `StateStore`: rooms, members, room state, the sync token.
//!
//! Pattern: Decorator over the SDK's memory store: `persist.rs` turns a change into journal entries,
//! `restore.rs` rebuilds the memory store, `decorate.rs` is the trait. What is not kept across a
//! restart is listed in `DESIGN.md`: caches the SDK fills again, and features this crate does not use.

mod decorate;
mod kind;
mod persist;
mod restore;
mod store;

pub use store::FsStateStore;
