//! [`KeyLearner`]: applying the rule to the key store.
//!
//! Pattern: Template method (read, decide, write) under one lock, so two mails from the same peer
//! cannot interleave. A failure here never costs a message: it is logged without any key material and
//! the message goes on.

use std::sync::Arc;

use tokio::sync::Mutex;

use super::header::AutocryptHeader;
use super::rule::{Decision, decide};
use crate::address::normalise_address;
use crate::error::MailError;
use crate::keys::{CandidateKey, KeySource, KeyStore, LegacyKey, PeerKey};
use crate::openpgp::{PublicCert, unix_now, usable_encryption_key};

/// A key somebody offers for `address`.
#[derive(Clone, Debug)]
pub struct Offer {
    /// The offered certificate: the v6 key if the header carries one, else what `keydata` holds.
    pub cert: PublicCert,
    /// The v4 certificate the same header carries beside a v6 `cert`.
    pub companion: Option<PublicCert>,
    /// It came inside an encrypted payload whose signature verified with the known key of `address`.
    pub proven_by_known_key: bool,
}

impl Offer {
    /// One key, no companion.
    pub fn single(cert: PublicCert, proven_by_known_key: bool) -> Self {
        Self {
            cert,
            companion: None,
            proven_by_known_key,
        }
    }

    /// What a parsed header offers. A readable v6 key in `_fs-v6` is the key; `keydata` beside it must
    /// be a v4 key to count as its companion.
    pub fn from_header(header: &AutocryptHeader) -> Option<Self> {
        let main = PublicCert::from_binary(&header.keydata).ok();
        let v6 = header
            .v6keydata
            .as_deref()
            .and_then(|bytes| PublicCert::from_binary(bytes).ok())
            .filter(PublicCert::is_v6);
        match (v6, main) {
            (Some(v6), Some(v4)) if v4.is_v4() => Some(Self {
                cert: v6,
                companion: Some(v4),
                proven_by_known_key: false,
            }),
            (Some(v6), _) => Some(Self::single(v6, false)),
            (None, Some(main)) => Some(Self::single(main, false)),
            (None, None) => None,
        }
    }

    /// Every certificate in the offer.
    pub fn certs(&self) -> impl Iterator<Item = &PublicCert> {
        std::iter::once(&self.cert).chain(&self.companion)
    }
}

/// The companion of a first sight or a proven rotation, as it is held.
fn first_legacy(offer: &Offer, now: u64) -> Result<Option<LegacyKey>, MailError> {
    offer
        .companion
        .as_ref()
        .map_or(Ok(None), |c| held_legacy(c, now))
}

/// Learns peers' keys.
pub struct KeyLearner {
    keys: Arc<dyn KeyStore>,
    writes: Mutex<()>,
}

impl KeyLearner {
    /// Over `keys`.
    pub fn new(keys: Arc<dyn KeyStore>) -> Self {
        Self {
            keys,
            writes: Mutex::new(()),
        }
    }

    /// Apply the rule to one offer and answer the decision.
    pub async fn learn(&self, address: &str, offer: &Offer) -> Result<Decision, MailError> {
        self.learn_at(address, offer, unix_now()).await
    }

    /// As [`KeyLearner::learn`], at a given time.
    pub async fn learn_at(
        &self,
        address: &str,
        offer: &Offer,
        now: u64,
    ) -> Result<Decision, MailError> {
        let address = normalise_address(address);
        let _writes = self.writes.lock().await;
        let known = self.keys.peer(&address).await?;
        let decision = decide(known.as_ref(), offer, now);
        // What is kept is what the signatures cover: every later write logs this record again.
        let Some(kept) = offer.cert.compact() else {
            return Ok(Decision::Ignore);
        };
        match (&decision, known) {
            (Decision::FirstSeen, _) => {
                let peer = PeerKey::new(&address, &kept, KeySource::AutocryptFirstSeen, now)?;
                // No companion at first sight: nothing binds the v4 key to the v6 key, so a forger could
                // pair a real v6 key with his own v4 key. It is learned from a later mail that the held
                // key signed (`beside` with proof).
                self.keys.put_peer(&peer).await?;
            }
            (Decision::Rotate, Some(old)) => {
                let mut peer = PeerKey::new(&address, &kept, KeySource::AutocryptRotated, now)?;
                peer.first_seen = old.first_seen;
                peer.legacy = first_legacy(offer, now)?;
                self.keys.put_peer(&peer).await?;
            }
            (Decision::SameKey { take_offered }, Some(mut peer)) => {
                if *take_offered && let Some(merged) = peer.certificate()?.merged_with(&offer.cert)
                {
                    peer.cert = PeerKey::new(&address, &merged, peer.source, now)?.cert;
                }
                let proven = offer.proven_by_known_key;
                peer.legacy = beside(peer.legacy.take(), offer.companion.as_ref(), proven, now)?;
                peer.last_seen = now;
                self.keys.put_peer(&peer).await?;
            }
            (Decision::Legacy, Some(mut peer)) => {
                let proven = offer.proven_by_known_key;
                peer.legacy = beside(peer.legacy.take(), Some(&offer.cert), proven, now)?;
                peer.last_seen = now;
                self.keys.put_peer(&peer).await?;
            }
            (Decision::Conflict, Some(mut peer)) => {
                let fresh = PeerKey::new(&address, &kept, peer.source, now)?;
                peer.candidate = Some(CandidateKey {
                    fingerprint: fresh.fingerprint,
                    cert: fresh.cert,
                    last_offered: now,
                });
                self.keys.put_peer(&peer).await?;
            }
            _ => {}
        }
        Ok(decision)
    }

    /// A person compared `fingerprint` with its owner and says it is right. The only way a key replaces
    /// another without proof. Answers whether a key was replaced. Fails if the candidate held now is
    /// another one: somebody offered a new key in the meantime, and nobody checked that one.
    pub async fn accept_candidate(
        &self,
        address: &str,
        fingerprint: &str,
    ) -> Result<bool, MailError> {
        let _writes = self.writes.lock().await;
        let Some(mut peer) = self.keys.peer(address).await? else {
            return Ok(false);
        };
        let Some(candidate) = peer.candidate.take() else {
            return Ok(false);
        };
        let cert = crate::keys::decode_cert(&candidate.cert)?;
        if !cert.fingerprint().eq_ignore_ascii_case(fingerprint.trim()) {
            return Err(MailError::Keys(format!(
                "the candidate of {address} is {}, not the key that was checked",
                cert.fingerprint()
            )));
        }
        peer.fingerprint = candidate.fingerprint;
        peer.cert = candidate.cert;
        peer.source = KeySource::Person;
        peer.last_seen = candidate.last_offered;
        self.keys.put_peer(&peer).await?;
        Ok(true)
    }
}

/// `cert` as the v4 key held beside a v6 one: compacted, and only if it is v4 and usable.
fn held_legacy(cert: &PublicCert, now: u64) -> Result<Option<LegacyKey>, MailError> {
    if !cert.is_v4() || usable_encryption_key(cert, now).is_err() {
        return Ok(None);
    }
    cert.compact().map(|c| LegacyKey::new(&c)).transpose()
}

/// The v4 key held beside the key in use after an offer of `cert`, given what was held. Without a held
/// one it comes in only with proof; the held one is refreshed by merging (nothing is taken away) and
/// replaced only with proof. Anything else leaves it as it is.
fn beside(
    held: Option<LegacyKey>,
    cert: Option<&PublicCert>,
    proven: bool,
    now: u64,
) -> Result<Option<LegacyKey>, MailError> {
    let Some(cert) = cert else {
        return Ok(held);
    };
    let Some(held) = held else {
        return if proven {
            held_legacy(cert, now)
        } else {
            Ok(None)
        };
    };
    if held.fingerprint == cert.fingerprint() {
        return match held.certificate()?.merged_with(cert) {
            Some(merged) => Ok(Some(LegacyKey::new(&merged)?)),
            None => Ok(Some(held)),
        };
    }
    if proven {
        return Ok(held_legacy(cert, now)?.or(Some(held)));
    }
    Ok(Some(held))
}
