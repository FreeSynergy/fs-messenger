//! The Backend of the data type `Message`.
//!
//! Pattern: Adapter + Command, as `accounts.rs`. The core addresses a message by conversation and id, the description by
//! id only: this Backend learns the conversation of each message it lists (`context` of the envelope) and keeps it in a
//! map, so a command on a record it has listed finds its conversation. A message it has not listed is `NotFound`.
//! The list is `message.search` (empty text = every message, newest first), or the messages of one conversation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::DateTime;
use fs_admin_frontend::{Backend, BackendError, Outcome, Record, Values};
use fs_messenger_core::{CoolDown, MessageAction, MessageSummary, SearchFilter};
use fs_objects::ObjectId;

use crate::convert;
use crate::shared::{
    Shared, iri, is_yes, many, media_type, need, need_id, now_secs, one, parse_id, put,
};

const TYPE: &str = "Message";
/// Pages read for one list (50 messages each by default).
const MAX_PAGES: usize = 20;

/// Messages, over the core.
pub struct MessageBackend {
    shared: Arc<Shared>,
    conversation_of: Mutex<HashMap<ObjectId, ObjectId>>,
}

impl MessageBackend {
    /// A Backend over `shared`.
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            conversation_of: Mutex::new(HashMap::new()),
        }
    }

    fn remember(&self, message: ObjectId, conversation: ObjectId) {
        self.conversation_of
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(message, conversation);
    }

    fn conversation_for(&self, message: ObjectId) -> Result<ObjectId, BackendError> {
        self.conversation_of
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&message)
            .copied()
            .ok_or(BackendError::NotFound)
    }

    fn summaries(&self, query: &Values) -> Result<Vec<MessageSummary>, BackendError> {
        let m = &self.shared.messenger;
        let conversation = one(query, "conversation").map(parse_id).transpose()?;
        let text = one(query, "text").unwrap_or("");
        let filter = SearchFilter {
            from: one(query, "from").map(str::to_owned),
            unread: is_yes(query, "unread"),
            has_attachment: is_yes(query, "hasAttachment"),
            since: one(query, "since")
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.to_utc()),
        };
        // A conversation without a search reads that conversation; with one, the search is over everything and the
        // conversation narrows its result.
        let plain = text.is_empty() && filter == SearchFilter::default();
        let mut out = Vec::new();
        let mut cursor = None;
        for _ in 0..MAX_PAGES {
            let page = match conversation {
                Some(c) if plain => self.shared.block(m.messages(c, cursor.as_ref()))?,
                _ => self
                    .shared
                    .block(m.search(text, &filter, cursor.as_ref()))?,
            };
            out.extend(page.items);
            match page.next {
                Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
                _ => break,
            }
        }
        if let Some(c) = conversation {
            let c = iri(c);
            out.retain(|s| {
                s.envelope
                    .context
                    .as_deref()
                    .is_none_or(|x| convert_context(x) == c)
            });
        }
        Ok(out)
    }

    fn act(&self, input: &Values, action: MessageAction) -> Result<Outcome, BackendError> {
        let id = need_id(input, "message")?;
        let conversation = self.conversation_for(id)?;
        self.shared
            .block(self.shared.messenger.act(conversation, id, action))?;
        Ok(Outcome {
            id: Some(iri(id)),
            values: Values::new(),
        })
    }
}

fn convert_context(text: &str) -> String {
    text.parse::<ObjectId>()
        .map_or_else(|_| text.to_owned(), iri)
}

impl Backend for MessageBackend {
    /// What the capability table says: the core carries out `command` and this Backend runs it.
    fn supported(&self, command: &str) -> bool {
        self.shared.gate(TYPE, command).is_ok()
    }

    fn list(&self, query: &Values) -> Result<Vec<Record>, BackendError> {
        let m = &self.shared.messenger;
        let mut out = Vec::new();
        for s in self.summaries(query)? {
            let id = s.envelope.id;
            let conversation = s
                .envelope
                .context
                .as_deref()
                .and_then(|c| c.parse::<ObjectId>().ok());
            let Some(conversation) = conversation else {
                out.push(convert::message_summary(&s));
                continue;
            };
            self.remember(id, conversation);
            // A message whose text cannot be read still lists, by its summary.
            match self.shared.block(m.message(conversation, id)) {
                Ok(full) => out.push(convert::message(&full)),
                Err(_) => out.push(convert::message_summary(&s)),
            }
        }
        Ok(out)
    }

    fn run(&self, name: &str, input: &Values) -> Result<Outcome, BackendError> {
        self.shared.gate(TYPE, name)?;
        let m = &self.shared.messenger;
        let now = now_secs();
        match name {
            "create" => {
                let mut draft = self.shared.block(m.compose(None))?;
                self.shared.block(async {
                    if let Some(a) = one(input, "account").and_then(|a| a.parse().ok()) {
                        draft.set_account(a).await?;
                    }
                    draft
                        .set_recipients(many(input, "to"), many(input, "cc"), many(input, "bcc"))
                        .await?;
                    if let Some(subject) = one(input, "subject") {
                        draft.set_subject(subject.to_owned()).await?;
                    }
                    draft
                        .set_text(
                            need(input, "text").unwrap_or_default().to_owned(),
                            media_type(input).to_owned(),
                        )
                        .await?;
                    let id = draft.send(None).await?;
                    Ok(Outcome {
                        id: Some(iri(id)),
                        values: Values::new(),
                    })
                })
            }
            "reply" => {
                let parent = need_id(input, "message")?;
                let mut draft = self.shared.block(m.compose(Some(parent)))?;
                self.shared.block(async {
                    draft
                        .set_text(
                            need(input, "text").unwrap_or_default().to_owned(),
                            media_type(input).to_owned(),
                        )
                        .await?;
                    let id = draft.send(None).await?;
                    Ok(Outcome {
                        id: Some(iri(id)),
                        values: Values::new(),
                    })
                })
            }
            "forward" => self.act(input, MessageAction::Forward(many(input, "to"))),
            "delete" => self.act(input, MessageAction::Delete),
            "flag" => self.act(input, MessageAction::Flag(is_yes(input, "flagged"))),
            "read" => self.act(input, MessageAction::Unread(is_yes(input, "unread"))),
            "nextUnread" => {
                let after = one(input, "after").map(parse_id).transpose()?;
                let next = self.shared.block(m.next_unread(after))?;
                let mut out = Outcome::default();
                if let Some(s) = next {
                    put(&mut out.values, "next", iri(s.envelope.id));
                }
                Ok(out)
            }
            "heldStage" => {
                let mut out = Outcome::default();
                match self.shared.block(m.held_stage(need_id(input, "held")?))? {
                    CoolDown::Held { release_at } => {
                        put(&mut out.values, "stage", "held");
                        put(&mut out.values, "releaseAt", release_at.to_string());
                    }
                    CoolDown::SentUnread => put(&mut out.values, "stage", "sent-unread"),
                    CoolDown::SentRead => put(&mut out.values, "stage", "sent-read"),
                    CoolDown::Recalled => put(&mut out.values, "stage", "recalled"),
                }
                Ok(out)
            }
            "sendHeldNow" => {
                let sent = self
                    .shared
                    .block(m.send_held_now(need_id(input, "held")?, now))?;
                let mut out = Outcome::default();
                put(&mut out.values, "sent", iri(sent));
                Ok(out)
            }
            "recallHeld" => {
                self.shared.block(m.recall_held(need_id(input, "held")?))?;
                Ok(Outcome::default())
            }
            "releaseDue" => {
                let mut out = Outcome::default();
                for id in self.shared.block(m.release_due(now))? {
                    put(&mut out.values, "released", iri(id));
                }
                Ok(out)
            }
            "heldUnsure" => {
                let mut out = Outcome::default();
                for id in self.shared.block(m.held_unsure())? {
                    put(&mut out.values, "unsure", iri(id));
                }
                Ok(out)
            }
            _ => Err(BackendError::NotFound),
        }
    }
}
