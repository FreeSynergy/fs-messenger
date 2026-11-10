//! The whole capability table, as one iterator over the parts.
//!
//! Pattern: Registry — rows with one (type, command) describe one command and must agree on kind and support; the first
//! row of a name defines the command, later rows only add capabilities. It is the only place that says what is supported.
//! When `fs-types` has the field `supported` on a command, `support::apply_support` is the one function that sets it;
//! the table does not change. A test fails when an id of `requirements.md` is missing.

use super::super::entry::Entry;
use super::{account, conversation, draft, message};

/// Every row, in the order Account, Conversation, Message, Draft.
pub fn table() -> impl Iterator<Item = &'static Entry> + Clone {
    account::ROWS
        .iter()
        .chain(conversation::ROWS)
        .chain(message::ROWS)
        .chain(draft::ROWS)
}
