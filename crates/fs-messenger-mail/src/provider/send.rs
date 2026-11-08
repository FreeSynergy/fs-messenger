//! Planning and sending.
//!
//! Pattern: Template method: check again, build, encrypt, submit, keep the copy. The plan is asked again
//! inside `send`, because a key can be revoked between the core's check and now; and nothing reaches a
//! strategy but the finished ciphertext. The core names the plan; this file is where it is kept.

use chrono::Utc;
use fs_messenger_core::{
    Encryption, Error as CoreError, ExternalId, Outgoing, Result as CoreResult, SendReceipt,
};

use super::facade::MailProvider;
use super::ids::{encode_id, header_id};
use super::reach::{Reach, method_for};
use super::receive::{SCHEME_MESSAGE_ID, SCHEME_THREAD};
use crate::autocrypt::format_header_with;
use crate::mime::{PayloadInput, build_outer, build_payload};
use crate::openpgp::{
    EncryptionTargets, Method, SecretCert, sign_and_encrypt, unix_now, usable_encryption_key,
};
use crate::submit::{MailEnvelope, SentCopy};

impl MailProvider {
    /// The whole way of a message.
    pub(super) async fn send_outgoing(&self, message: &Outgoing) -> CoreResult<SendReceipt> {
        if message.encryption() != Encryption::Openpgp {
            return Err(CoreError::EncryptionRefused {
                encryption: message.encryption(),
                missing_keys: vec![],
            });
        }
        let now = unix_now();
        let from = self.identity.address.clone();
        let own: SecretCert =
            self.keys
                .own_key(&from)
                .await?
                .ok_or_else(|| CoreError::EncryptionRefused {
                    encryption: Encryption::None,
                    missing_keys: vec![from.clone()],
                })?;
        let own_legacy = self
            .own_legacy(&from)
            .await?
            .ok_or_else(|| refused_now(&from)(crate::openpgp::KeyRefusal::CannotEncrypt))?;
        let visible = [message.to(), message.cc()].concat();
        let readers = self.certs_of(&visible, now).await?;
        let hidden_readers = self.certs_of(message.bcc(), now).await?;
        let ours = Reach {
            key: own.public(),
            legacy: Some(own_legacy.public()),
        };
        // Checked again here: a key checked a moment ago may be revoked now.
        if self.usable_cert(&from, now).await?.is_none() {
            return Err(refused_now(&from)(crate::openpgp::KeyRefusal::Invalid));
        }
        let method = method_for(
            std::iter::once(&ours)
                .chain(&readers)
                .chain(&hidden_readers),
        );

        let domain = from.rsplit_once('@').map_or("localhost", |(_, d)| d);
        let message_id = format!("{}@{domain}", message.id());
        let parent = id_of(message.in_reply_to(), SCHEME_MESSAGE_ID);
        let root = id_of(message.conversation(), SCHEME_THREAD)
            .or_else(|| parent.clone())
            .unwrap_or_else(|| message_id.clone());
        let references = references_of(message, parent.as_deref(), &root);
        // The v4 key is the one in `keydata`, as Autocrypt 1.1 asks; the v6 key rides beside it.
        let autocrypt = format_header_with(&from, &ours.legacy_or_key(), Some(&ours.key))?;
        let input = PayloadInput {
            from: &from,
            to: message.to(),
            cc: message.cc(),
            subject: message.subject(),
            message_id: &message_id,
            in_reply_to: parent.as_deref(),
            references: &references,
            date: Utc::now().timestamp(),
            text: message.text(),
            media_type: message.media_type(),
            attachments: message.attachments(),
            autocrypt: &autocrypt,
        };
        let payload = build_payload(&input)?;

        let sub = |reach| subkey_for(reach, method, now, &from);
        let mut targets = EncryptionTargets {
            visible: vec![sub(&ours)?],
            hidden: Vec::with_capacity(hidden_readers.len()),
        };
        for reach in &readers {
            targets.visible.push(sub(reach)?);
        }
        for reach in &hidden_readers {
            targets.hidden.push(sub(reach)?);
        }
        // The signature is made by the key of the method: a v4 key for a message the older programs read.
        let signer = match method {
            Method::Modern => &own,
            Method::Older => &own_legacy,
        };
        let armored = sign_and_encrypt(&payload, signer, &targets, method)?;
        let raw = build_outer(&input, &armored)?;

        let envelope = MailEnvelope {
            from: from.clone(),
            recipients: [visible.as_slice(), message.bcc()].concat(),
        };
        if self.submitter.submit(&envelope, &raw).await? == SentCopy::NotStored
            && let Err(e) = self.fetcher.store_sent(&raw).await
        {
            // It left. Failing now would make a retry send it twice; the core holds the message.
            tracing::warn!(error = %e, "the mail left, but no sent copy reached the server");
        }
        Ok(SendReceipt {
            aliases: vec![ExternalId::new(SCHEME_MESSAGE_ID, message_id.clone())],
            conversation: Some(ExternalId::new(SCHEME_THREAD, encode_id(&root))),
            encryption: Encryption::Openpgp,
        })
    }
}

/// The id under `scheme`, as it was written in the mail.
fn id_of(aliases: &[ExternalId], scheme: &str) -> Option<String> {
    aliases
        .iter()
        .find(|a| a.scheme == scheme)
        .and_then(|a| header_id(&a.id))
}

/// RFC 5322: the parent's chain, then the parent. The first entry stays the thread's root.
fn references_of(message: &Outgoing, parent: Option<&str>, root: &str) -> Vec<String> {
    let Some(parent) = parent else {
        return vec![];
    };
    let mut chain: Vec<String> = message
        .references()
        .iter()
        .filter(|r| r.is_sender_chosen())
        .filter_map(|r| header_id(&r.id))
        .collect();
    if root != parent && chain.first().map(String::as_str) != Some(root) {
        chain.insert(0, root.to_string());
    }
    chain.push(parent.to_string());
    chain
}

/// The subkey of `reach` that `method` encrypts to.
fn subkey_for<'a>(
    reach: &'a Reach,
    method: Method,
    now: u64,
    from: &str,
) -> CoreResult<&'a pgp::composed::SignedPublicSubKey> {
    usable_encryption_key(reach.for_method(method), now).map_err(refused_now(from))
}

/// A key checked a moment ago failed now. Refuse rather than send to fewer people.
fn refused_now(from: &str) -> impl Fn(crate::openpgp::KeyRefusal) -> CoreError {
    let from = from.to_string();
    move |_| CoreError::EncryptionRefused {
        encryption: Encryption::None,
        missing_keys: vec![from.clone()],
    }
}
