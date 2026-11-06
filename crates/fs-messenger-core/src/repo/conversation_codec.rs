//! Conversation <-> stored object.
//!
//! Pattern: Adapter. Unread count, newest message and encryption are kept on the object so a list
//! needs no scan; `Inner::refresh` always recomputes them from the messages, so they cannot drift.

use chrono::{DateTime, Utc};
use fs_store::{Envelope, ObjectId, StoredObject};
use serde::{Deserialize, Serialize};

use super::consts::{
    CONTENT_TYPE, IDX_ACCOUNT, IDX_CONV_ALIAS, IDX_FOLDER, IDX_SORT, IDX_UNREAD, SENSITIVE, YES,
};
use super::keys::{alias_key, conversation_key};
use crate::error::Result;
use crate::model::{Encryption, ExternalId, Kind, Usage};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ConversationContent {
    pub(super) account: Option<ObjectId>,
    pub(super) kind: Kind,
    #[serde(rename = "freesynergy.net:encryption")]
    pub(super) encryption: Encryption,
    pub(super) pinned: bool,
    pub(super) folders: Vec<String>,
    pub(super) unread_count: u32,
    pub(super) last_message: Option<ObjectId>,
    pub(super) last_at: Option<DateTime<Utc>>,
    pub(super) usage: Usage,
    pub(super) aliases: Vec<ExternalId>,
}

/// A stored conversation.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ConversationRecord {
    pub(super) envelope: Envelope,
    pub(super) content: ConversationContent,
}

impl ConversationRecord {
    pub(super) fn from_object(o: StoredObject) -> Result<Self> {
        Ok(Self {
            envelope: o.envelope,
            content: serde_json::from_value(o.content)?,
        })
    }

    /// Always marked sensitive (ADR 0029): names and members say who talks to whom.
    pub(super) fn to_object(&self) -> Result<StoredObject> {
        let c = &self.content;
        let mut env = self.envelope.clone();
        env.sensitive = vec![SENSITIVE.to_string()];
        let at = c.last_at.unwrap_or(env.published);
        let id = env.id;
        let mut o = StoredObject::new(env, CONTENT_TYPE, serde_json::to_value(c)?)
            .with_index(IDX_SORT, conversation_key(c.pinned, at, id));
        if c.unread_count > 0 {
            o = o.with_index(IDX_UNREAD, YES);
        }
        for f in &c.folders {
            o = o.with_index(IDX_FOLDER, f.clone());
        }
        if let Some(account) = c.account {
            o = o.with_index(IDX_ACCOUNT, account.to_string());
            for a in &c.aliases {
                o = o.with_index(IDX_CONV_ALIAS, alias_key(account, a));
            }
        }
        Ok(o)
    }
}
