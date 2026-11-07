//! Matrix events to what the core takes.
//!
//! Pattern: none (this file only declares modules and re-exports; no logic, section 3 of the coding
//! rules). `alias.rs` is the alias (a Value object), `ids.rs` the one encoding of server ids,
//! `event.rs` reads an event's JSON, `adapter.rs` builds the `RawMessage`, `cursor.rs` is what the
//! core keeps for this provider (a Memento) and `gaps.rs` plans and merges its open gaps. All pure,
//! no network.

mod adapter;
mod alias;
#[allow(dead_code)] // a shell until the implementation
mod apply;
mod cursor;
mod event;
#[allow(dead_code)] // a shell until the implementation
mod evidence;
mod gaps;
mod given_up;
mod ids;
#[allow(dead_code)] // a shell until the implementation
mod membership;
mod pending;

pub(crate) use adapter::{RoomContext, message, placeholder};
pub use adapter::{SCHEME_ROOM, UNDECRYPTED_MARK};
pub use alias::{EventAlias, SCHEME_EVENT};
pub(crate) use cursor::{Gap, MatrixCursor};
pub(crate) use event::parse;
pub(crate) use ids::{conversation_of, room_of};
#[allow(unused_imports)] // a shell until the implementation
pub(crate) use membership::{Known, Section, Seen, Step};
pub(crate) use pending::Pending;
