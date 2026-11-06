//! Matrix events to what the core takes.
//!
//! Pattern: Adapter. `event.rs` reads an event's JSON, `adapter.rs` builds the `RawMessage`, `cursor.rs`
//! is what the core keeps for this provider. All pure, no network.

mod adapter;
mod cursor;
mod event;

pub(crate) use adapter::{RoomContext, late_message, message, placeholder};
pub use adapter::{SCHEME_EVENT, SCHEME_LATE, SCHEME_ROOM, UNDECRYPTED_MARK};
pub(crate) use cursor::{MatrixCursor, Pending};
pub(crate) use event::parse;
