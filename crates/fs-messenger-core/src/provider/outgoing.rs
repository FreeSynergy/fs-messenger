//! What a provider is handed to send: [`Outgoing`], and what it answers: [`SendReceipt`].
//!
//! Pattern: Newtype as proof: [`Outgoing`] has no public constructor and its only constructor runs
//! the check of ADR 0018, so no code outside this crate can make a provider send something that did
//! not pass. A provider can still send on its own account; that limit is in `DESIGN.md`.

use fs_objects::ObjectId;

use crate::error::Result;
use crate::gate;
use crate::model::{Encryption, EncryptionPlan, ExternalId};

/// A file to send.
#[derive(Clone, Debug)]
pub struct OutgoingAttachment {
    /// File name.
    pub name: String,
    /// Media type.
    pub media_type: String,
    /// The bytes.
    pub data: Vec<u8>,
}

/// What goes out, before it is approved.
#[derive(Clone, Debug, Default)]
pub(crate) struct OutgoingBody {
    pub(crate) subject: String,
    pub(crate) references: Vec<ExternalId>,
    pub(crate) to: Vec<String>,
    pub(crate) cc: Vec<String>,
    pub(crate) bcc: Vec<String>,
    pub(crate) text: String,
    pub(crate) media_type: String,
    pub(crate) attachments: Vec<OutgoingAttachment>,
    pub(crate) conversation: Vec<ExternalId>,
    pub(crate) in_reply_to: Vec<ExternalId>,
}

/// A message that passed the encryption check. Only the core can make one.
#[derive(Clone, Debug)]
pub struct Outgoing {
    id: ObjectId,
    plan: EncryptionPlan,
    body: OutgoingBody,
}

impl Outgoing {
    /// The one way to build this type.
    pub(crate) fn approve(id: ObjectId, plan: EncryptionPlan, body: OutgoingBody) -> Result<Self> {
        let everyone = [body.to.as_slice(), &body.cc, &body.bcc].concat();
        gate::check(&plan, &everyone)?;
        Ok(Self { id, plan, body })
    }

    /// Our id of the message.
    pub fn id(&self) -> ObjectId {
        self.id
    }
    /// The encryption to use.
    pub fn encryption(&self) -> Encryption {
        self.plan.encryption
    }
    /// Primary recipients.
    pub fn to(&self) -> &[String] {
        &self.body.to
    }
    /// Secondary recipients.
    pub fn cc(&self) -> &[String] {
        &self.body.cc
    }
    /// Hidden recipients.
    pub fn bcc(&self) -> &[String] {
        &self.body.bcc
    }
    /// The subject; empty if the person gave none, and a provider without subjects ignores it.
    pub fn subject(&self) -> &str {
        &self.body.subject
    }
    /// The chain of ancestors of the parent, oldest first, without the parent itself, which is in
    /// [`Outgoing::in_reply_to`]. Empty if this is no reply.
    pub fn references(&self) -> &[ExternalId] {
        &self.body.references
    }
    /// The text.
    pub fn text(&self) -> &str {
        &self.body.text
    }
    /// Media type of the text.
    pub fn media_type(&self) -> &str {
        &self.body.media_type
    }
    /// Files.
    pub fn attachments(&self) -> &[OutgoingAttachment] {
        &self.body.attachments
    }
    /// What the provider calls the conversation; empty for a new one.
    pub fn conversation(&self) -> &[ExternalId] {
        &self.body.conversation
    }
    /// All the parent is known as, `Message-ID` included; empty if this is no reply.
    pub fn in_reply_to(&self) -> &[ExternalId] {
        &self.body.in_reply_to
    }
}

/// What a provider answers after sending.
#[derive(Clone, Debug, Default)]
pub struct SendReceipt {
    /// What the message is known as now, `Message-ID` included. A later copy is recognised by these
    /// only if its sender is the account's own address.
    pub aliases: Vec<ExternalId>,
    /// What the provider calls the conversation.
    pub conversation: Option<ExternalId>,
    /// The encryption really used. Unset is `unknown`, which reads as a downgrade.
    pub encryption: Encryption,
}
