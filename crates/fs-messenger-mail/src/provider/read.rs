//! Reading one mail: classify, open, verify. Writes nothing.
//!
//! Pattern: Template method. The state is `openpgp` only if decryption really happened in this flow;
//! everything else is `none`. `transport-only` is never claimed: we measure TLS on our own leg to the
//! server, not the way the mail travelled. Cryptography and parsing run on a blocking thread, where even
//! a panic in a parser only leaves that mail undecrypted.

use fs_messenger_core::Encryption;

use super::facade::MailProvider;
use crate::address::normalise_address;
use crate::autocrypt::{Offer, offered_for};
use crate::error::MailError;
use crate::mime::{MailFields, Shape, classify};
use crate::openpgp::{PublicCert, SecretCert, SignatureState, decrypt, verify_detached};

/// What came out of reading one mail.
pub(super) struct ReadMail {
    pub(super) fields: MailFields,
    pub(super) outer_message_id: Option<String>,
    pub(super) encryption: Encryption,
    pub(super) undecrypted: bool,
    pub(super) signature: SignatureState,
    /// The key this mail offers. Reading never writes; only a real delivery applies it.
    pub(super) offer: Option<(String, Offer)>,
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(work).await.ok()
}

fn offer_of(values: &[Vec<u8>], sender: &str) -> Option<PublicCert> {
    let header = offered_for(values.iter().map(Vec::as_slice), sender)?;
    PublicCert::from_binary(&header.keydata).ok()
}

fn offering(sender: &str, cert: Option<PublicCert>, proven: bool) -> Option<(String, Offer)> {
    cert.map(|cert| {
        let offer = Offer {
            cert,
            proven_by_known_key: proven,
        };
        (sender.to_string(), offer)
    })
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

    /// Classify, open, verify. Writes nothing. Fails only if the key store fails, so the cursor stays
    /// and nothing is lost.
    pub(super) async fn read_mail(&self, raw: Vec<u8>) -> Result<ReadMail, MailError> {
        let bytes = raw.clone();
        let parsed = blocking(move || (classify(&bytes), MailFields::read(&bytes))).await;
        let (shape, outer) = parsed.unwrap_or((Shape::Unparsable, None));
        let outer = outer.unwrap_or_default();
        let outer_message_id = outer.message_id.clone();
        let sender = outer.from.clone().unwrap_or_default();
        let outer_offer = offer_of(&outer.autocrypt, &sender);
        let unproven = offering(&sender, outer_offer.clone(), false);
        let plain = |fields, signature| ReadMail {
            fields,
            outer_message_id: outer_message_id.clone(),
            encryption: Encryption::None,
            undecrypted: false,
            signature,
            offer: unproven.clone(),
        };
        match shape {
            Shape::Plain | Shape::Unparsable => Ok(plain(outer, SignatureState::NotSigned)),
            Shape::Signed { content, signature } => {
                let (signers, _) = self
                    .signers(&sender, std::slice::from_ref(&outer_offer))
                    .await?;
                let state = blocking(move || verify_detached(&signature, &content, &signers))
                    .await
                    .unwrap_or(SignatureState::Unverified);
                Ok(plain(outer, state))
            }
            Shape::MalformedEncrypted => Ok(undecrypted(outer, outer_message_id, unproven)),
            Shape::Encrypted { armored } => {
                let own: Vec<SecretCert> = self
                    .keys
                    .own_key(&self.identity.address)
                    .await?
                    .into_iter()
                    .collect();
                let (text, keys) = (armored.clone(), own.clone());
                let first = blocking(move || decrypt(&text, &keys, &[]).ok())
                    .await
                    .flatten();
                let Some((payload, _)) = first else {
                    return Ok(undecrypted(outer, outer_message_id, unproven));
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
        let sender = if protected {
            inner.from.clone()
        } else {
            outer.from.clone()
        };
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
        let offer = match inner_offer {
            Some(cert) => offering(&sender, Some(cert), known && verified),
            None => offering(&sender, outer_offer, false),
        };
        Ok(ReadMail {
            fields: merged(outer, inner),
            outer_message_id,
            encryption: Encryption::Openpgp,
            undecrypted: false,
            signature: state,
            offer,
        })
    }
}

fn undecrypted(
    outer: MailFields,
    outer_message_id: Option<String>,
    offer: Option<(String, Offer)>,
) -> ReadMail {
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
        offer,
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
