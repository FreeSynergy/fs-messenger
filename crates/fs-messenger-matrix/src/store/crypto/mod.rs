//! [`FsCryptoStore`]: the SDK's `CryptoStore`: Olm account, sessions, Megolm keys, device lists.
//!
//! Pattern: Decorator over the SDK's `MemoryStore`: `persist.rs` turns a change into journal entries,
//! `restore.rs` rebuilds the memory store from them, `decorate.rs` is the trait.

mod decorate;
mod kind;
mod persist;
mod restore;
mod store;

pub use store::FsCryptoStore;
