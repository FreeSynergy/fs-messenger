//! Persistent stores of the SDK over `fs-objects`, in pure Rust and encrypted at rest.
//!
//! Pattern: none (this file only declares modules and re-exports; no logic, section 3 of the coding
//! rules). [`FsCryptoStore`] and [`FsStateStore`] are Decorators over the SDK's own in-memory stores:
//! all its query logic is reused, and every change goes to a [`Journal`] before it reaches them. On
//! opening, the journal rebuilds them. See `DESIGN.md`, "Stores".

mod crypto;
mod forward;
mod fs_journal;
mod hex;
mod journal;
mod keys;
mod read_back;
mod state;

pub use crypto::FsCryptoStore;
pub use fs_journal::FsJournal;
pub use journal::{Journal, Op};
pub use state::FsStateStore;
