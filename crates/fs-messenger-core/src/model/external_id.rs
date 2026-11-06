//! [`ExternalId`]: a message's id in somebody else's system.
//!
//! Pattern: Value object: the row of the alias table in `data/messages/technical.md`.

use serde::{Deserialize, Serialize};

const MAX_ID_BYTES: usize = 2048;

/// The scheme of the mail header `Message-ID`, which whoever sends the mail writes.
pub const MESSAGE_ID: &str = "message-id";

/// What another system calls a message or a conversation: `Message-ID`, Matrix `event_id`, ...
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExternalId {
    /// Whose id this is, for example `imap-uid`, `jmap-email`, `matrix-event`, or [`MESSAGE_ID`] for
    /// what a sender wrote. Only [`MESSAGE_ID`] is recognised as sender-chosen, so a connector must
    /// not put a sender's id under another label in `RawMessage::aliases`: it would dedupe.
    pub scheme: String,
    /// The id in that scheme.
    pub id: String,
}

impl ExternalId {
    /// Whether the sender chose this id, so that it can only link and never dedupes.
    pub fn is_sender_chosen(&self) -> bool {
        self.scheme.eq_ignore_ascii_case(MESSAGE_ID)
    }

    /// Whether it can be kept and handed on: both parts present, of a sane length, without `|`, which
    /// the store's keys are built with, or control characters: a CRLF must never reach a mail header.
    pub fn is_well_formed(&self) -> bool {
        let good = |s: &str| {
            !s.is_empty()
                && s.len() <= MAX_ID_BYTES
                && !s.contains('|')
                && !s.contains(char::is_control)
        };
        good(&self.scheme) && good(&self.id)
    }

    /// An id in `scheme`.
    pub fn new(scheme: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            scheme: scheme.into(),
            id: id.into(),
        }
    }
}
