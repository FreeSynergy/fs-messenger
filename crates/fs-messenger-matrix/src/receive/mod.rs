//! Matrix events to what the core takes.
//!
//! No pattern from the coding rules fits the whole (see DESIGN.md, open points). `alias.rs` is the
//! alias newtype, `event.rs` reads an event's JSON, `adapter.rs` builds the `RawMessage`, `cursor.rs`
//! is what the core keeps for this provider. All pure, no network.

mod adapter;
mod alias;
mod cursor;
mod event;

pub(crate) use adapter::{RoomContext, message, placeholder};
pub use adapter::{SCHEME_ROOM, UNDECRYPTED_MARK};
pub use alias::{EventAlias, SCHEME_EVENT};
pub(crate) use cursor::{Gap, MatrixCursor, Pending};
pub(crate) use event::parse;
