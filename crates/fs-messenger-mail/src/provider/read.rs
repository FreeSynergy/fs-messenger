//! Reading one mail: classify, open, verify. Writes nothing.
//!
//! Pattern: Template method. The state is `openpgp` only if decryption really happened in this flow;
//! everything else is `none`. `transport-only` is never claimed: we measure TLS on our own leg to the
//! server, not the way the mail travelled. Parsing, decrypting and verifying run on blocking threads; a
//! panic in there leaves that mail stored and undecrypted, with its original and its outer headers.
//!
//! What a signature says, PROVISIONAL: it vouches only for what it covers. In a `multipart/signed` that
//! is the signed part, never the headers around it, so headers and key offers come from inside that part.
//! `Valid` needs the covered part to name the sender in its own `From`, in a clean header block: a
//! signature over a bare body says nothing about who sent it, and prose with a `From:` line is prose.

use std::sync::Arc;

use fs_messenger_core::{Encryption, SignatureCheck};
use tokio::runtime::Handle;

use super::contained::contained;
use super::facade::MailProvider;
use super::opened::{check_of, opened_mail, plain, undecrypted};
use crate::address::normalise_address;
use crate::autocrypt::{Offer, offered_for};
use crate::error::{MailError, Result};
use crate::keys::KeyStore;
use crate::mime::{MailFields, Shape, classify};
use crate::openpgp::{
    PublicCert, SecretCert, SignatureState, decrypt_selecting, may_vouch, verify_detached,
};

/// What came out of reading one mail.
#[derive(Debug)]
pub(super) struct ReadMail {
    pub(super) fields: MailFields,
    /// The `Message-ID` to link by: from inside if inside is believed, because the outer one is not
    /// covered by anything and a replay can carry any.
    pub(super) message_id: Option<String>,
    pub(super) encryption: Encryption,
    pub(super) undecrypted: bool,
    pub(super) signature: SignatureCheck,
    /// The key this mail offers. Reading never writes; only a real delivery applies it.
    pub(super) offer: Option<(String, Offer)>,
}

impl ReadMail {
    /// Only the start of a mail over the limit was read. Its headers are all that can be said; it is
    /// not opened and offers no key. The core keeps a stub of it and not these bytes.
    pub(super) fn cut_off(start: &[u8]) -> Self {
        let mut read = plain(
            MailFields::read(start).unwrap_or_default(),
            SignatureCheck::NotVerified,
        );
        read.offer = None;
        read.fields.text_body = String::new();
        read.fields.attachments = vec![];
        read
    }

    /// Nothing could be read, not even the headers.
    fn unreadable() -> Self {
        undecrypted(MailFields::default(), None)
    }
}

pub(super) fn offer_of(values: &[Vec<u8>], sender: &str) -> Option<PublicCert> {
    let header = offered_for(values.iter().map(Vec::as_slice), sender)?;
    PublicCert::from_binary(&header.keydata).ok()
}

pub(super) fn offering(
    sender: &str,
    cert: Option<PublicCert>,
    proven: bool,
) -> Option<(String, Offer)> {
    cert.map(|cert| {
        let offer = Offer {
            cert,
            proven_by_known_key: proven,
        };
        (sender.to_string(), offer)
    })
}

/// The part a signature can cover: the signed part of a `multipart/signed`, else the whole payload.
struct Covered {
    fields: MailFields,
    detached: Option<(Vec<u8>, Vec<u8>)>,
}

impl Covered {
    fn of(payload: &[u8]) -> Self {
        if let Shape::Signed { content, signature } = classify(payload) {
            return Self {
                fields: MailFields::read_covered(&content).unwrap_or_default(),
                detached: Some((content, signature)),
            };
        }
        Self {
            fields: MailFields::read_covered(payload).unwrap_or_default(),
            detached: None,
        }
    }
}

/// Whom a signature of a sender must verify with.
struct Signers {
    certs: Vec<PublicCert>,
    /// They were known before this mail. Otherwise it offered them itself, which proves nothing.
    known: bool,
}

/// What the blocking part needs. Key lookups run on the runtime it was started from.
struct Reader {
    keys: Arc<dyn KeyStore>,
    address: String,
    limit: usize,
    runtime: Handle,
}

impl Reader {
    /// A held key that may not vouch any more (revoked, broken) counts as known and verifies nothing.
    fn signers(&self, sender: &str, offers: &[Option<PublicCert>]) -> Result<Signers> {
        let sender = normalise_address(sender);
        if sender == self.address {
            let own = self.runtime.block_on(self.keys.own_key(&sender))?;
            let certs = own.iter().map(SecretCert::public).collect();
            return Ok(Signers { certs, known: true });
        }
        if let Some(peer) = self.runtime.block_on(self.keys.peer(&sender))? {
            let cert = peer
                .certificate()
                .map_err(|e| MailError::Keys(e.to_string()))?;
            let certs = Some(cert).filter(may_vouch).into_iter().collect();
            return Ok(Signers { certs, known: true });
        }
        Ok(Signers {
            certs: offers.iter().flatten().cloned().collect(),
            known: false,
        })
    }

    fn read(&self, raw: &[u8], outer: MailFields) -> Result<ReadMail> {
        match classify(raw) {
            Shape::Plain | Shape::Unparsable => Ok(plain(outer, SignatureCheck::NotSigned)),
            Shape::MalformedEncrypted => Ok(undecrypted(outer.clone(), outer.message_id)),
            Shape::Signed { content, signature } => self.read_signed(outer, &content, &signature),
            Shape::Encrypted { armored } => self.read_encrypted(outer, &armored),
        }
    }

    /// `multipart/signed` at the root, not encrypted. It can show who wrote it; a key offer needs more.
    fn read_signed(&self, outer: MailFields, content: &[u8], signature: &[u8]) -> Result<ReadMail> {
        let covered = MailFields::read_covered(content).unwrap_or_default();
        let claimed = covered.from.clone().or_else(|| outer.from.clone());
        let claimed = claimed.unwrap_or_default();
        let offers = [offer_of(&outer.autocrypt, &claimed)];
        let signers = self.signers(&claimed, &offers)?;
        let state = verify_detached(signature, content, &signers.certs);
        let check = check_of(&state, signers.known, covered.from.is_some());
        if check == SignatureCheck::Valid {
            let mut read = plain(covered.clone(), check);
            read.message_id = covered.message_id.or(outer.message_id);
            read.offer = offering(&claimed, offers.into_iter().flatten().next(), false);
            return Ok(read);
        }
        Ok(plain(outer, check))
    }

    fn read_encrypted(&self, outer: MailFields, armored: &[u8]) -> Result<ReadMail> {
        let own: Vec<SecretCert> = self
            .runtime
            .block_on(self.keys.own_key(&self.address))?
            .into_iter()
            .collect();
        let mut seen = None;
        let opened = decrypt_selecting(armored, &own, self.limit, |payload| {
            let covered = Covered::of(payload);
            let claimed = covered.fields.from.clone().or_else(|| outer.from.clone());
            let claimed = claimed.unwrap_or_default();
            let offers = [
                offer_of(&covered.fields.autocrypt, &claimed),
                offer_of(&outer.autocrypt, &claimed),
            ];
            let signers = self.signers(&claimed, &offers)?;
            let certs = signers.certs.clone();
            seen = Some((covered, signers));
            Ok(certs)
        });
        let (combined, covered, signers) = match (opened, seen) {
            (Ok((_, combined)), Some((covered, signers))) => (combined, covered, signers),
            (Err(MailError::Pgp(e)), _) => {
                tracing::debug!(error = %e, "a mail stays undecrypted");
                return Ok(undecrypted(outer.clone(), outer.message_id));
            }
            (Err(e), _) => return Err(e),
            (Ok(_), None) => return Err(MailError::Pgp("decrypted without a payload".into())),
        };
        // A signature on the whole payload covers the signed part too; otherwise only the detached one.
        let state = match (&combined, &covered.detached) {
            (SignatureState::Valid { .. }, _) | (_, None) => combined,
            (_, Some((content, signature))) => verify_detached(signature, content, &signers.certs),
        };
        Ok(opened_mail(outer, covered.fields, &state, signers.known))
    }
}

impl MailProvider {
    /// One mail. Fails only if the key store fails, so the cursor stays and nothing is lost. The outer
    /// headers are read first and alone: a panic while opening costs the content, not who sent it.
    pub(super) async fn read_mail(&self, raw: Vec<u8>) -> Result<ReadMail> {
        let reader = Reader {
            keys: self.keys.clone(),
            address: self.identity.address.clone(),
            limit: self.limits.bytes,
            runtime: Handle::current(),
        };
        let bytes = raw.clone();
        let outer = contained(
            move || MailFields::read(&bytes).unwrap_or_default(),
            MailFields::default,
        )
        .await;
        let for_panic = outer.clone();
        contained(
            move || reader.read(&raw, outer),
            move || Ok(undecrypted(for_panic.clone(), for_panic.message_id)),
        )
        .await
    }

    /// Only the start of a mail over the limit was read: who and what, nothing else.
    pub(super) async fn read_cut_off(&self, start: Vec<u8>) -> ReadMail {
        contained(move || ReadMail::cut_off(&start), ReadMail::unreadable).await
    }
}
