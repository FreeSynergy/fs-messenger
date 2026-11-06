//! Reading a fetched mail into the core's `RawMessage`.
//!
//! Pattern: Template method: classify, open, verify, learn, map. The state is `openpgp` only if
//! decryption really happened in this flow. Everything else is `none`, stored and marked. `transport-only`
//! is never claimed: we measure TLS on our own leg to the server, not the way the mail travelled. A mail
//! that cannot be opened keeps its original and a visible mark. Cryptography and parsing run on a blocking
//! thread, where even a panic in a parser only leaves that mail undecrypted.

use fs_messenger_core::{Encryption, ExternalId, RawAttachment, RawMessage};
use fs_store::Blob;

use super::facade::MailProvider;
use crate::address::normalise_address;
use crate::autocrypt::{Offer, offered_for};
use crate::error::MailError;
use crate::fetch::{FetchedMail, MailboxRole};
use crate::mime::{MailFields, Shape, classify, thread_root};
use crate::openpgp::{PublicCert, SecretCert, SignatureState, decrypt, verify_detached};

/// The body of a mail that is encrypted and could not be opened here.
pub const UNDECRYPTED_MARK: &str =
    "[This message is encrypted and could not be decrypted here. Its original is kept unchanged.]";

pub(super) const SCHEME_MESSAGE_ID: &str = "message-id";
pub(super) const SCHEME_THREAD: &str = "mail-thread";
/// For a mail without `Message-ID`: account, mailbox and the server's own id.
const SCHEME_LOCAL: &str = "mail-local";
const MEDIA_RFC822: &str = "message/rfc822";

/// What came out of reading one mail.
pub(super) struct ReadMail {
    pub(super) fields: MailFields,
    pub(super) outer_message_id: Option<String>,
    pub(super) encryption: Encryption,
    pub(super) undecrypted: bool,
    pub(super) signature: SignatureState,
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(work).await.ok()
}

fn offer_of(values: &[Vec<u8>], sender: &str) -> Option<PublicCert> {
    let header = offered_for(values.iter().map(Vec::as_slice), sender)?;
    PublicCert::from_binary(&header.keydata).ok()
}

impl MailProvider {
    /// Whom a signature of `sender` must verify with: the known key, and only if none is known, what
    /// the mail itself offers.
    async fn signers(
        &self,
        sender: &str,
        offers: &[Option<PublicCert>],
    ) -> Result<(Vec<PublicCert>, bool), MailError> {
        let sender = normalise_address(sender);
        if sender == self.identity.address {
            let own = self.keys.own_key(&sender).await?;
            return Ok((own.iter().map(SecretCert::public).collect(), true));
        }
        if let Some(peer) = self.keys.peer(&sender).await? {
            return Ok((vec![peer.certificate()?], true));
        }
        Ok((offers.iter().flatten().cloned().collect(), false))
    }

    async fn learn(&self, sender: &str, offer: Option<PublicCert>, proven: bool) {
        if normalise_address(sender) == self.identity.address {
            return;
        }
        let Some(cert) = offer else { return };
        let offer = Offer { cert, proven_by_known_key: proven };
        if let Err(e) = self.learner().learn(sender, &offer).await {
            tracing::warn!(error = %e, "could not learn a key from an Autocrypt header");
        }
    }

    /// Classify, open, verify, learn. Fails only if the key store fails, so the cursor stays and
    /// nothing is lost.
    pub(super) async fn read_mail(&self, raw: Vec<u8>) -> Result<ReadMail, MailError> {
        let bytes = raw.clone();
        let parsed = blocking(move || (classify(&bytes), MailFields::read(&bytes))).await;
        let (shape, outer) = parsed.unwrap_or((Shape::Unparsable, None));
        let outer = outer.unwrap_or_default();
        let outer_message_id = outer.message_id.clone();
        let sender = outer.from.clone().unwrap_or_default();
        let outer_offer = offer_of(&outer.autocrypt, &sender);
        let plain = |fields, signature| ReadMail {
            fields,
            outer_message_id: outer_message_id.clone(),
            encryption: Encryption::None,
            undecrypted: false,
            signature,
        };
        match shape {
            Shape::Plain | Shape::Unparsable => {
                self.learn(&sender, outer_offer, false).await;
                Ok(plain(outer, SignatureState::NotSigned))
            }
            Shape::Signed { content, signature } => {
                let (signers, _) = self.signers(&sender, &[outer_offer.clone()]).await?;
                let state = blocking(move || verify_detached(&signature, &content, &signers))
                    .await
                    .unwrap_or(SignatureState::Unverified);
                self.learn(&sender, outer_offer, false).await;
                Ok(plain(outer, state))
            }
            Shape::MalformedEncrypted => {
                self.learn(&sender, outer_offer, false).await;
                Ok(undecrypted(outer, outer_message_id))
            }
            Shape::Encrypted { armored } => {
                let own: Vec<SecretCert> = self
                    .keys
                    .own_key(&self.identity.address)
                    .await?
                    .into_iter()
                    .collect();
                let (text, keys) = (armored.clone(), own.clone());
                let first = blocking(move || decrypt(&text, &keys, &[]).ok()).await.flatten();
                let Some((payload, _)) = first else {
                    self.learn(&sender, outer_offer, false).await;
                    return Ok(undecrypted(outer, outer_message_id));
                };
                let bytes = payload.clone();
                let inner = blocking(move || MailFields::read(&bytes)).await.flatten();
                let inner = inner.unwrap_or_default();
                Ok(self
                    .read_opened(outer, inner, &payload, armored, own, outer_offer)
                    .await?)
            }
        }
    }

    /// The payload is open: verify with the right keys, and learn with the rule of `autocrypt::rule`.
    async fn read_opened(
        &self,
        outer: MailFields,
        inner: MailFields,
        payload: &[u8],
        armored: Vec<u8>,
        own: Vec<SecretCert>,
        outer_offer: Option<PublicCert>,
    ) -> Result<ReadMail, MailError> {
        let outer_message_id = outer.message_id.clone();
        let protected = inner.header_protected;
        let sender = if protected { inner.from.clone() } else { outer.from.clone() };
        let sender = sender.unwrap_or_default();
        let inner_offer = offer_of(&inner.autocrypt, &sender);
        let offers = [inner_offer.clone(), outer_offer.clone()];
        let (signers, known) = self.signers(&sender, &offers).await?;
        let mut state = blocking(move || decrypt(&armored, &own, &signers).ok())
            .await
            .flatten()
            .map_or(SignatureState::Unverified, |(_, state)| state);
        if state == SignatureState::NotSigned
            && let Shape::Signed { content, signature } = classify(payload)
        {
            let (signers, _) = self.signers(&sender, &offers).await?;
            state = verify_detached(&signature, &content, &signers);
        }
        let verified = matches!(state, SignatureState::Valid { .. });
        match inner_offer {
            Some(cert) => self.learn(&sender, Some(cert), known && verified).await,
            None => self.learn(&sender, outer_offer, false).await,
        }
        Ok(ReadMail {
            fields: merged(outer, inner),
            outer_message_id,
            encryption: Encryption::Openpgp,
            undecrypted: false,
            signature: state,
        })
    }

    /// One fetched mail as the core wants it.
    pub(super) async fn receive(&self, mail: FetchedMail) -> Result<RawMessage, MailError> {
        let read = self.read_mail(mail.bytes.clone()).await?;
        let body = if read.undecrypted {
            UNDECRYPTED_MARK.to_string()
        } else {
            read.fields.text_body.clone()
        };
        let fields = read.fields;
        let local = format!("{}/{}", self.identity.address, mail.stable_id);
        let alias = read.outer_message_id.as_ref().map_or_else(
            || ExternalId::new(SCHEME_LOCAL, local.clone()),
            |id| ExternalId::new(SCHEME_MESSAGE_ID, id.clone()),
        );
        let root = thread_root(&fields).unwrap_or(&local);
        let conversation = ExternalId::new(SCHEME_THREAD, root);
        let sender = fields.from.clone().unwrap_or_default();
        let mut raw = RawMessage::new(alias, conversation, sender.clone(), body);
        raw.encryption = read.encryption;
        raw.to = fields.to;
        raw.cc = fields.cc;
        raw.conversation_name.clone_from(&fields.subject);
        raw.subject = fields.subject;
        raw.published = fields.sent_at;
        raw.in_reply_to = fields
            .in_reply_to
            .first()
            .map(|id| ExternalId::new(SCHEME_MESSAGE_ID, id.clone()));
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
        raw.outgoing =
            mail.role == MailboxRole::Sent && normalise_address(&sender) == self.identity.address;
        Ok(raw)
    }
}

fn undecrypted(outer: MailFields, outer_message_id: Option<String>) -> ReadMail {
    ReadMail {
        fields: MailFields {
            text_body: String::new(),
            text_media_type: "text/plain".to_string(),
            attachments: vec![],
            ..outer
        },
        outer_message_id,
        encryption: Encryption::None,
        undecrypted: true,
        signature: SignatureState::NotSigned,
    }
}

/// Body and files from inside. With Header Protection the headers too; without it they stay the outer
/// ones, and an unprotected inner `Subject` is not believed over them.
fn merged(outer: MailFields, inner: MailFields) -> MailFields {
    if inner.header_protected {
        return inner;
    }
    MailFields {
        text_body: inner.text_body,
        text_media_type: inner.text_media_type,
        attachments: inner.attachments,
        ..outer
    }
}
