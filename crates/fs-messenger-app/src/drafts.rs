//! The Backend of the data type `Draft`.
//!
//! Pattern: Adapter + Command, as `accounts.rs`. A draft is picked up again by its id (`resume_draft`), changed and saved;
//! `change` leaves what it does not name as it is. Recipients of a saved draft cannot be changed: the core's `Draft` sets
//! to, cc and bcc together and cannot read cc and bcc back (DESIGN.md, Unsupported).

use std::sync::Arc;

use fs_admin_frontend::{Backend, BackendError, Outcome, Record, Values};

use crate::convert;
use crate::shared::{Shared, iri, many, media_type, need_id, now_secs, one, parse_id, put};

const TYPE: &str = "Draft";

/// Drafts, over the core.
pub struct DraftBackend {
    shared: Arc<Shared>,
}

impl DraftBackend {
    /// A Backend over `shared`.
    pub fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

impl Backend for DraftBackend {
    /// What the capability table says: the core carries out `command` and this Backend runs it.
    fn supported(&self, command: &str) -> bool {
        self.shared.gate(TYPE, command).is_ok()
    }

    fn list(&self, _query: &Values) -> Result<Vec<Record>, BackendError> {
        let m = &self.shared.messenger;
        Ok(self
            .shared
            .block(m.drafts())?
            .iter()
            .map(convert::draft)
            .collect())
    }

    fn run(&self, name: &str, input: &Values) -> Result<Outcome, BackendError> {
        self.shared.gate(TYPE, name)?;
        let m = &self.shared.messenger;
        match name {
            "create" => {
                let reply_to = one(input, "replyTo").map(parse_id).transpose()?;
                let mut draft = self.shared.block(m.compose(reply_to))?;
                self.shared.block(async {
                    if let Some(a) = one(input, "account").and_then(|a| a.parse().ok()) {
                        draft.set_account(a).await?;
                    }
                    if input.contains_key("to")
                        || input.contains_key("cc")
                        || input.contains_key("bcc")
                    {
                        draft
                            .set_recipients(
                                many(input, "to"),
                                many(input, "cc"),
                                many(input, "bcc"),
                            )
                            .await?;
                    }
                    if let Some(subject) = one(input, "subject") {
                        draft.set_subject(subject.to_owned()).await?;
                    }
                    if let Some(text) = one(input, "text") {
                        draft
                            .set_text(text.to_owned(), media_type(input).to_owned())
                            .await?;
                    }
                    let id = draft.save().await?;
                    Ok::<_, fs_messenger_core::Error>(Outcome {
                        id: Some(iri(id)),
                        values: Values::new(),
                    })
                })
            }
            "change" => {
                let id = need_id(input, "draft")?;
                let held = self
                    .shared
                    .block(m.drafts())?
                    .into_iter()
                    .find(|d| d.id == id);
                let mut draft = self.shared.block(m.resume_draft(id))?;
                self.shared.block(async {
                    if let Some(a) = one(input, "account").and_then(|a| a.parse().ok()) {
                        draft.set_account(a).await?;
                    }
                    if let Some(subject) = input.get("subject") {
                        draft
                            .set_subject(subject.first().cloned().unwrap_or_default())
                            .await?;
                    }
                    // The media type alone keeps the text the draft holds.
                    if input.contains_key("text") || input.contains_key("mediaType") {
                        let text = match input.get("text") {
                            Some(t) => t.first().cloned().unwrap_or_default(),
                            None => held.map(|d| d.text).unwrap_or_default(),
                        };
                        draft.set_text(text, media_type(input).to_owned()).await?;
                    }
                    let id = draft.save().await?;
                    Ok::<_, fs_messenger_core::Error>(Outcome {
                        id: Some(iri(id)),
                        values: Values::new(),
                    })
                })
            }
            "delete" => {
                let mut draft = self
                    .shared
                    .block(m.resume_draft(need_id(input, "draft")?))?;
                self.shared.block(async { draft.discard().await })?;
                Ok(Outcome::default())
            }
            "send" => {
                let mut draft = self
                    .shared
                    .block(m.resume_draft(need_id(input, "draft")?))?;
                let sent = self.shared.block(async { draft.send(None).await })?;
                let mut out = Outcome::default();
                put(&mut out.values, "sent", iri(sent));
                Ok(out)
            }
            "hold" => {
                let wait = one(input, "waitSeconds").and_then(|w| w.parse().ok());
                let held = self
                    .shared
                    .block(m.hold(need_id(input, "draft")?, now_secs(), wait))?;
                let mut out = Outcome::default();
                put(&mut out.values, "held", iri(held));
                Ok(out)
            }
            _ => Err(BackendError::NotFound),
        }
    }
}
