//! The Backend of the data type `Conversation`.
//!
//! Pattern: Adapter + Command, as `accounts.rs`. The list is `conversation.list`: pinned first, then newest first; the
//! pages are read to their end (at most `MAX_PAGES`), because the generated list has no paging yet.

use std::sync::Arc;

use fs_admin_frontend::{Backend, BackendError, Outcome, Record, Values};
use fs_messenger_core::{Remedy, WhyNot};

use crate::convert;
use crate::shared::{Shared, iri, is_yes, need_id, one, put};

const TYPE: &str = "Conversation";
/// Pages read for one list (50 conversations each by default).
const MAX_PAGES: usize = 20;

/// Conversations, over the core.
pub struct ConversationBackend {
    shared: Arc<Shared>,
}

impl ConversationBackend {
    /// A Backend over `shared`.
    pub fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

fn why(w: WhyNot) -> &'static str {
    match w {
        WhyNot::NoKeyOfTheOtherSide => "no-key-of-the-other-side",
        WhyNot::AccountCannot => "account-cannot",
        WhyNot::OnTheWireOnly => "on-the-wire-only",
        WhyNot::NotKnown => "not-known",
    }
}

fn remedy(r: Remedy) -> &'static str {
    match r {
        Remedy::ExchangeKeys => "exchange-keys",
        Remedy::ChooseOtherChannel => "choose-other-channel",
        Remedy::TurnOn => "turn-on",
        Remedy::Wait => "wait",
    }
}

impl Backend for ConversationBackend {
    /// What the capability table says: the core carries out `command` and this Backend runs it.
    fn supported(&self, command: &str) -> bool {
        self.shared.gate(TYPE, command).is_ok()
    }

    fn list(&self, query: &Values) -> Result<Vec<Record>, BackendError> {
        let m = &self.shared.messenger;
        let folder = one(query, "folder");
        let only_unread = is_yes(query, "onlyUnread");
        let mut out = Vec::new();
        let mut cursor = None;
        for _ in 0..MAX_PAGES {
            let page = self
                .shared
                .block(m.conversations(folder, only_unread, cursor.as_ref()))?;
            out.extend(page.items.iter().map(convert::conversation));
            match page.next {
                Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
                _ => break,
            }
        }
        Ok(out)
    }

    fn run(&self, name: &str, input: &Values) -> Result<Outcome, BackendError> {
        self.shared.gate(TYPE, name)?;
        let m = &self.shared.messenger;
        let mut out = Outcome::default();
        let conversation = need_id(input, "conversation")?;
        match name {
            "markRead" => {
                self.shared
                    .block(m.mark_read(conversation, need_id(input, "upTo")?))?;
                out.id = Some(iri(conversation));
            }
            "pin" => {
                self.shared
                    .block(m.pin(conversation, is_yes(input, "pinned")))?;
                out.id = Some(iri(conversation));
            }
            "move" => {
                self.shared
                    .block(m.move_to(conversation, one(input, "folder").unwrap_or("")))?;
                out.id = Some(iri(conversation));
            }
            "usage" => {
                let u = self.shared.block(m.usage(conversation))?;
                put(&mut out.values, "inputTokens", u.input_tokens.to_string());
                put(&mut out.values, "outputTokens", u.output_tokens.to_string());
                put(&mut out.values, "contextUsed", u.context_used.to_string());
                put(&mut out.values, "contextSize", u.context_size.to_string());
                put(&mut out.values, "costMicros", u.cost_micros.to_string());
                put(&mut out.values, "currency", u.currency);
            }
            "export" => put(
                &mut out.values,
                "export",
                self.shared.block(m.export_conversation(conversation))?,
            ),
            "whyPlain" => {
                // None = the conversation is end-to-end encrypted: nothing to explain, so no result values.
                if let Some((w, r)) = self.shared.block(m.why_not_encrypted(conversation))? {
                    put(&mut out.values, "reason", why(w));
                    put(&mut out.values, "remedy", remedy(r));
                }
            }
            _ => return Err(BackendError::NotFound),
        }
        Ok(out)
    }
}
