//! Matrix events to what the core takes.
//!
//! Pattern: none (no pattern of the coding rules, section 2, fits; see DESIGN.md, open points).
//! `alias.rs` is the alias (two public fields, no newtype), `event.rs` reads an event's JSON,
//! `adapter.rs` builds the `RawMessage`, `cursor.rs` is what the core keeps for this provider and
//! `gaps.rs` plans and merges its open gaps. All pure, no network.

mod adapter;
mod alias;
mod cursor;
mod event;
mod gaps;

pub(crate) use adapter::{RoomContext, message, placeholder};
pub use adapter::{SCHEME_ROOM, UNDECRYPTED_MARK};
pub use alias::{EventAlias, SCHEME_EVENT};
pub(crate) use cursor::{Gap, MatrixCursor, Pending};
pub(crate) use event::parse;
