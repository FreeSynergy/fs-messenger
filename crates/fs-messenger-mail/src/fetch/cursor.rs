//! [`MailCursor`]: where each mailbox stands.
//!
//! Pattern: Value object, opaque to the core (`ProviderCursor`). An unreadable cursor means "from the
//! start": that only costs a replay, which the core dedupes by alias, and never a lost mail.

use std::collections::BTreeMap;

use fs_messenger_core::ProviderCursor;
use serde::{Deserialize, Serialize};

use crate::error::MailError;

/// Where one mailbox stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MailboxCursor {
    /// IMAP: a change of `uid_validity` voids `last_uid`, so it starts over.
    #[serde(rename_all = "camelCase")]
    Imap {
        /// `UIDVALIDITY` of the mailbox.
        uid_validity: u32,
        /// The last UID delivered.
        last_uid: u32,
    },
    /// JMAP: `state` is the `Email` state from before the listing began.
    #[serde(rename_all = "camelCase")]
    Jmap {
        /// Everything from this state on comes through `Email/changes`.
        state: String,
        /// Still walking the first listing.
        walking: bool,
        /// The last id of that walk: an id stays right when mail is deleted, a position does not.
        anchor: Option<String>,
        /// Ids found and not read yet, because the byte budget of the last call ran out. The state has
        /// moved past them, so they are kept here until they are read.
        #[serde(default)]
        pending: Vec<String>,
    },
}

/// One cursor per mailbox.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailCursor {
    /// By the mailbox's name (IMAP) or id (JMAP).
    pub mailboxes: BTreeMap<String, MailboxCursor>,
}

impl MailCursor {
    /// Read what the core holds.
    pub fn read(cursor: Option<&ProviderCursor>) -> Self {
        cursor
            .and_then(|c| serde_json::from_str(&c.0).ok())
            .unwrap_or_default()
    }

    /// The form the core holds.
    pub fn write(&self) -> Result<ProviderCursor, MailError> {
        Ok(ProviderCursor(serde_json::to_string(self)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_a_bad_cursor_starts_over() {
        let mut c = MailCursor::default();
        c.mailboxes.insert(
            "INBOX".into(),
            MailboxCursor::Imap {
                uid_validity: 7,
                last_uid: 9,
            },
        );
        c.mailboxes.insert(
            "a".into(),
            MailboxCursor::Jmap {
                state: "s1".into(),
                walking: false,
                anchor: None,
                pending: vec![],
            },
        );
        let written = c.write().unwrap();
        assert_eq!(MailCursor::read(Some(&written)), c);
        let bad = ProviderCursor("not json".into());
        assert_eq!(MailCursor::read(Some(&bad)), MailCursor::default());
        assert_eq!(MailCursor::read(None), MailCursor::default());
    }
}
