//! [`FsCryptoStore`]: the SDK's `CryptoStore`: Olm account, sessions, Megolm keys, device lists.
//!
//! Pattern: none (this file only declares modules and re-exports; no logic, section 3 of the coding
//! rules). The type is a Decorator over the SDK's `MemoryStore` (`store.rs`, `decorate.rs`,
//! `restore.rs`); `persist.rs` turns a change into journal entries.

mod decorate;
mod kind;
mod persist;
mod restore;
mod store;

pub use store::FsCryptoStore;
