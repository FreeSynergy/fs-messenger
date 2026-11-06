//! Planning and sending.
//!
//! Pattern: Template method: check again, build, encrypt, submit, keep the copy. The plan is asked again
//! inside `send`, because a key can be revoked between the core's check and now; and nothing reaches a
//! strategy but the finished ciphertext. The core names the plan; this file is where it is kept.

use chrono::Utc;
use fs_messenger_core::{
    Encryption, EncryptionPlan, Error as CoreError, ExternalId, Outgoing, Result as CoreResult,
    SendReceipt,
};

use super::facade::MailProvider;
use super::receive::{SCHEME_MESSAGE_ID, SCHEME_THREAD};
use crate::address::normalise_address;
use crate::autocrypt::format_header;
use crate::error::MailError;
use crate::mime::{PayloadInput, build_outer, build_payload, subject_from_text};
use crate::openpgp::{
    EncryptionTargets, PublicCert, SecretCert, sign_and_encrypt, unix_now, usable_encryption_key,
};
use crate::submit::{MailEnvelope, SentCopy};

impl MailProvider {
    /// The certificate to use for `address`, if there is a usable one.
    async fn usable_cert(&self, address: &str, now: u64) -> Result<Option<PublicCert>, MailError> {
        let address = normalise_address(address);
        let cert = if address == self.identity.address {
            self.keys.own_key(&address).await?.map(|k| k.public())
        } else {
            match self.keys.peer(&address).await? {
                Some(peer) => Some(peer.certificate()?),
                None => None,
            }
        };
        Ok(cert.filter(|c| usable_encryption_key(c, now).is_ok()))
    }

    /// `openpgp` only if everyone has a usable key, hidden recipients too, and we have our own: every
    /// mail is encrypted to the sender as well. Otherwise it names exactly who lacks one.
    pub(super) async fn plan(&self, recipients: &[String]) -> Result<EncryptionPlan, MailError> {
        let now = unix_now();
        let own = self.identity.address.clone();
        let mut missing: Vec<String> = Vec::new();
        for who in std::iter::once(&own).chain(recipients) {
            if self.usable_cert(who, now).await?.is_none() && !missing.contains(who) {
                missing.push(who.clone());
            }
        }
        Ok(EncryptionPlan {
            encryption: if missing.is_empty() {
                Encryption::Openpgp
            } else {
                Encryption::None
            },
            missing_keys: missing,
        })
    }

    /// Certificates for `who`; a gap here is the refusal of ADR 0018.
    async fn certs_of(&self, who: &[String], now: u64) -> CoreResult<Vec<PublicCert>> {
        let mut certs = Vec::with_capacity(who.len());
        let mut missing = Vec::new();
        for address in who {
            match self.usable_cert(address, now).await? {
                Some(cert) => certs.push(cert),
                None => missing.push(address.clone()),
            }
        }
        if missing.is_empty() {
            Ok(certs)
        } else {
            Err(CoreError::EncryptionRefused {
                encryption: Encryption::None,
                missing_keys: missing,
            })
        }
    }

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
        let visible = [message.to(), message.cc()].concat();
        let certs = self.certs_of(&visible, now).await?;
        let hidden_certs = self.certs_of(message.bcc(), now).await?;

        let domain = from.rsplit_once('@').map_or("localhost", |(_, d)| d);
        let message_id = format!("{}@{domain}", message.id());
        let parent = alias_of(message.in_reply_to(), SCHEME_MESSAGE_ID);
        let root = alias_of(message.conversation(), SCHEME_THREAD)
            .or(parent)
            .unwrap_or(message_id.as_str());
        let references: Vec<String> = match parent {
            Some(p) if p == root => vec![root.to_string()],
            Some(p) => vec![root.to_string(), p.to_string()],
            None => vec![],
        };
        let own_public = own.public();
        let autocrypt = format_header(&from, &own_public)?;
        let subject = subject_from_text(message.text());
        let input = PayloadInput {
            from: &from,
            to: message.to(),
            cc: message.cc(),
            subject: &subject,
            message_id: &message_id,
            in_reply_to: parent,
            references: &references,
            date: Utc::now().timestamp(),
            text: message.text(),
            media_type: message.media_type(),
            attachments: message.attachments(),
            autocrypt: &autocrypt,
        };
        let payload = build_payload(&input)?;

        let own_sub = usable_encryption_key(&own_public, now).map_err(refused_now(&from))?;
        let mut targets = EncryptionTargets {
            visible: vec![own_sub],
            hidden: Vec::with_capacity(hidden_certs.len()),
        };
        for cert in &certs {
            targets
                .visible
                .push(usable_encryption_key(cert, now).map_err(refused_now(&from))?);
        }
        for cert in &hidden_certs {
            targets
                .hidden
                .push(usable_encryption_key(cert, now).map_err(refused_now(&from))?);
        }
        let armored = sign_and_encrypt(&payload, &own, &targets)?;
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
            conversation: Some(ExternalId::new(SCHEME_THREAD, root)),
            encryption: Encryption::Openpgp,
        })
    }
}

fn alias_of<'a>(aliases: &'a [ExternalId], scheme: &str) -> Option<&'a str> {
    aliases
        .iter()
        .find(|a| a.scheme == scheme)
        .map(|a| a.id.as_str())
}

/// A key checked a moment ago failed now. Refuse rather than send to fewer people.
fn refused_now(from: &str) -> impl Fn(crate::openpgp::KeyRefusal) -> CoreError {
    let from = from.to_string();
    move |_| CoreError::EncryptionRefused {
        encryption: Encryption::None,
        missing_keys: vec![from.clone()],
    }
}
