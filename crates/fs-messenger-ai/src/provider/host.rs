//! [`Host`] and [`ProviderLink`]: the way back to the messenger.
//!
//! Pattern: Strategy (interface segregation): a provider sees the two traits it needs, and a guard's
//! [`Gate`], never a `Guard`. The link holds the messenger weakly, as a provider that holds a repo would
//! keep the claim on its store for as long as it is registered.

use std::sync::{Arc, Weak};

use fs_messenger_core::{Ingest, Messenger};
use fs_messenger_guard::Gate;
use fs_store::ObjectId;

use crate::error::AiError;

/// How deep to look for a conversation by its newest message, which a send has just written.
const PAGES: usize = 8;

/// What this provider asks of the messenger.
pub trait Host: Messenger + Ingest {}

impl<T: Messenger + Ingest> Host for T {}

/// What a provider is attached to.
#[derive(Clone)]
pub struct ProviderLink {
    /// The account: its kind must be `agent`, so that the core asks the guard about every answer.
    pub account: ObjectId,
    /// The messenger.
    pub host: Weak<dyn Host>,
    /// The guard's face for those who want to act. Asked before anything goes to a model.
    pub gate: Arc<dyn Gate>,
}

/// The account must exist, and be of kind `agent`.
pub(super) async fn agent_account(link: &ProviderLink) -> Result<(), AiError> {
    let host = link
        .host
        .upgrade()
        .ok_or_else(|| AiError::Host("the messenger has closed".into()))?;
    let accounts = host
        .accounts()
        .await
        .map_err(|e| AiError::Host(e.to_string()))?;
    match accounts.iter().find(|a| a.id == link.account) {
        Some(account) if account.kind == "agent" => Ok(()),
        Some(_) => Err(AiError::Config(
            "a model's account must be of kind `agent`".into(),
        )),
        None => Err(AiError::Config("no such account".into())),
    }
}

/// The conversation whose newest message this is. The core does not tell a provider which conversation
/// it is sending in (`DESIGN.md`, "Wanted").
pub(super) async fn conversation_of(
    host: &dyn Host,
    message: ObjectId,
) -> Result<ObjectId, AiError> {
    let mut cursor = None;
    for _ in 0..PAGES {
        let page = host
            .conversations(None, false, cursor.as_ref())
            .await
            .map_err(|e| AiError::Host(e.to_string()))?;
        let found = page.items.iter().find(|c| {
            c.last_message
                .as_ref()
                .is_some_and(|m| m.envelope.id == message)
        });
        if let Some(conversation) = found {
            return Ok(conversation.id);
        }
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Err(AiError::Host(
        "the conversation of this message was not found".into(),
    ))
}
