//! A fetched mail as the core's `RawMessage`.
//!
//! Pattern: Adapter. Applies the key offer of the mail, and decides what may dedupe and what only links:
//! the alias is what the provider issued, `Message-ID` is what the sender chose. A mail that cannot be
//! opened keeps its original and says so in `undecrypted`, with an empty body. Every id passes
//! `encode_id`, because the core refuses `|` and a legal mail must not be lost over it.

use fs_messenger_core::{ExternalId, RawAttachment, RawMessage, SignatureCheck};
use fs_store::Blob;

use super::facade::MailProvider;
use super::ids::encode_id;
use crate::address::normalise_address;
use crate::error::MailError;
use crate::fetch::{FetchedMail, MailboxRole};
use crate::mime::thread_root;

pub(super) const SCHEME_MESSAGE_ID: &str = fs_messenger_core::MESSAGE_ID;
pub(super) const SCHEME_THREAD: &str = "mail-thread";
/// Issued by the provider, not the sender: account, mailbox and the server's own id.
const SCHEME_LOCAL: &str = "mail-local";
const MEDIA_RFC822: &str = "message/rfc822";

impl MailProvider {
    /// One fetched mail as the core wants it.
    pub(super) async fn receive(&self, mail: FetchedMail) -> Result<RawMessage, MailError> {
        let read = self.read_mail(mail.bytes.clone()).await?;
        if let Some((sender, offer)) = &read.offer
            && normalise_address(sender) != self.identity.address
            && let Err(e) = self.learner().learn(sender, offer).await
        {
            tracing::warn!(error = %e, "could not learn a key from an Autocrypt header");
        }
        let fields = read.fields;
        let local = encode_id(&format!("{}/{}", self.identity.address, mail.stable_id));
        let alias = ExternalId::new(SCHEME_LOCAL, local.clone());
        let root = thread_root(&fields).map_or_else(|| local.clone(), encode_id);
        let conversation = ExternalId::new(SCHEME_THREAD, root);
        let sender = fields.from.clone().unwrap_or_default();
        let mut raw = RawMessage::new(alias, conversation, sender.clone(), fields.text_body);
        let from_me = normalise_address(&sender) == self.identity.address;
        // The core matches a link against our own receipts if the sender is us, and a `From` proves
        // nothing. So a mail in the inbox claiming to be from us only links if our own key signed it;
        // otherwise a stranger could hide his mail, or claim one of ours, with a known `Message-ID`.
        let may_link =
            !from_me || mail.role == MailboxRole::Sent || read.signature == SignatureCheck::Valid;
        if may_link {
            raw.links.extend(
                read.message_id
                    .map(|id| ExternalId::new(SCHEME_MESSAGE_ID, encode_id(&id))),
            );
        }
        raw.undecrypted = read.undecrypted;
        raw.signature = read.signature;
        raw.encryption = read.encryption;
        raw.to = fields.to;
        raw.cc = fields.cc;
        raw.conversation_name.clone_from(&fields.subject);
        raw.subject = fields.subject;
        raw.published = fields.sent_at;
        let linked = |id: &String| ExternalId::new(SCHEME_MESSAGE_ID, encode_id(id));
        raw.in_reply_to = fields.in_reply_to.first().map(linked);
        raw.references = fields.references.iter().map(linked).collect();
        raw.body_media_type = fields.text_media_type;
        raw.attachments = fields
            .attachments
            .into_iter()
            .map(|a| RawAttachment {
                name: a.name,
                media_type: a.media_type,
                data: a.data,
            })
            .collect();
        raw.original = Some(Blob {
            media_type: MEDIA_RFC822.to_string(),
            bytes: mail.bytes,
        });
        raw.outgoing = mail.role == MailboxRole::Sent && from_me;
        Ok(raw)
    }
}
