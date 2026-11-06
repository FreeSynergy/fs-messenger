//! Persistent stores of the SDK over `fs-store`, in pure Rust and encrypted at rest.
//!
//! Pattern: Decorator: [`FsCryptoStore`] and [`FsStateStore`] wrap the SDK's own in-memory stores, so
//! all its query logic is reused, and write every change to a [`Journal`] before it reaches them. On
//! opening, the journal rebuilds them. See `DESIGN.md`, "Stores".

mod crypto;
mod forward;
mod fs_journal;
mod journal;
mod keys;
mod state;

pub use crypto::FsCryptoStore;
pub use fs_journal::FsJournal;
pub use journal::{Journal, Op};
pub use state::FsStateStore;
