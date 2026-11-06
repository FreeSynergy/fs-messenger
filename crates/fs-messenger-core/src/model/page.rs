//! Cursor paging: [`Cursor`] and [`Page`].
//!
//! Pattern: Iterator (by hand): each page names where the next one starts. The cursor is the last
//! sort key, not an offset, so new messages cannot shift a page that is being read.

use fs_store::ObjectId;
use serde::{Deserialize, Serialize};

use super::{ConversationSummary, MessageSummary};

/// Where a page starts. Opaque: pass back what a [`Page`] returned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(String);

impl Cursor {
    pub(crate) fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// Rebuild a cursor from its text, for example after it crossed the wire.
    pub fn from_opaque(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The text of the cursor.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One page of a longer list.
#[derive(Clone, Debug, PartialEq)]
pub struct Page<T> {
    /// The items of this page.
    pub items: Vec<T>,
    /// Where the next page starts; `None` at the end.
    pub next: Option<Cursor>,
    /// Stored items on this page that could not be read. They do not fail the page.
    pub skipped: Vec<ObjectId>,
}

/// A page of messages.
pub type MessagePage = Page<MessageSummary>;
/// A page of conversations.
pub type ConversationPage = Page<ConversationSummary>;
