//! [`KeyLearner`]: applying the rule to the key store.
//!
//! Pattern: Template method (read, decide, write) under one lock, so two mails from the same peer
//! cannot interleave. A failure here never costs a message: it is logged without any key material and
//! the message goes on.

use std::sync::Arc;

use tokio::sync::Mutex;

use super::rule::{Decision, decide};
use crate::address::normalise_address;
use crate::error::MailError;
use crate::keys::{CandidateKey, KeySource, KeyStore, PeerKey};
use crate::openpgp::{PublicCert, unix_now};

/// A key somebody offers for `address`.
#[derive(Clone, Debug)]
pub struct Offer {
    /// The offered certificate.
    pub cert: PublicCert,
    /// It came inside an encrypted payload whose signature verified with the known key of `address`.
    pub proven_by_known_key: bool,
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
        match (&decision, known) {
            (Decision::FirstSeen, _) => {
                let peer = PeerKey::new(&address, &offer.cert, KeySource::AutocryptFirstSeen, now)?;
                self.keys.put_peer(&peer).await?;
            }
            (Decision::Rotate, Some(old)) => {
                let mut peer =
                    PeerKey::new(&address, &offer.cert, KeySource::AutocryptRotated, now)?;
                peer.first_seen = old.first_seen;
                self.keys.put_peer(&peer).await?;
            }
            (Decision::SameKey { take_offered }, Some(mut peer)) => {
                if *take_offered {
                    let mut merged = peer.certificate()?;
                    merged.merge(&offer.cert);
                    peer.cert = PeerKey::new(&address, &merged, peer.source, now)?.cert;
                }
                peer.last_seen = now;
                self.keys.put_peer(&peer).await?;
            }
            (Decision::Conflict, Some(mut peer)) => {
                let fresh = PeerKey::new(&address, &offer.cert, peer.source, now)?;
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
