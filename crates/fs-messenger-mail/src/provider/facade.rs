//! [`MailProvider`]: one account's mail behind the core's `Provider` trait.
//!
//! Pattern: Facade over the strategies [`MailFetch`] and [`MailSubmit`] and the [`KeyStore`], and a
//! Strategy itself to the core. What it cannot do answers `Unsupported`, through the trait's defaults
//! and in `capabilities`.

use std::sync::Arc;

use async_trait::async_trait;
use fs_interfaces::provider::Capabilities;
use fs_messenger_core::{
    EncryptionPlan, FetchBatch, MessengerConfig, Outgoing, Provider, ProviderCursor,
    Result as CoreResult, SendReceipt,
};

use tokio::runtime::Handle;

use super::contained::contained;
use crate::address::normalise_address;
use crate::autocrypt::{Decision, KeyLearner, Offer};
use crate::error::MailError;
use crate::fetch::{FetchLimits, MailCursor, MailFetch};
use crate::keys::{KeyStore, PeerKey};
use crate::openpgp::{SecretCert, generate_key};
use crate::submit::MailSubmit;

/// Mails per mailbox in one fetch.
const FETCH_MAILS: usize = 50;
/// Bytes in one fetch, by the sizes the server reports. Twice the core's default limit on a mail.
const FETCH_BATCH_BYTES: u64 = 50 * 1024 * 1024;

/// Whose mail this is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailIdentity {
    /// The address, normalised.
    pub address: String,
}

impl MailIdentity {
    /// The identity of `address`.
    pub fn new(address: &str) -> Self {
        Self {
            address: normalise_address(address),
        }
    }
}

/// The mail provider of one account.
pub struct MailProvider {
    pub(super) identity: MailIdentity,
    pub(super) fetcher: Arc<dyn MailFetch>,
    pub(super) submitter: Arc<dyn MailSubmit>,
    pub(super) keys: Arc<dyn KeyStore>,
    pub(super) limits: FetchLimits,
    learner: Arc<KeyLearner>,
}

impl MailProvider {
    /// A provider from its strategies.
    pub fn new(
        identity: MailIdentity,
        fetcher: Arc<dyn MailFetch>,
        submitter: Arc<dyn MailSubmit>,
        keys: Arc<dyn KeyStore>,
    ) -> Self {
        Self {
            identity,
            fetcher,
            submitter,
            learner: Arc::new(KeyLearner::new(keys.clone())),
            keys,
            // The core's own default, so that both refuse at the same point.
            limits: FetchLimits {
                mails: FETCH_MAILS,
                bytes: MessengerConfig::new("").max_message_bytes,
                batch_bytes: FETCH_BATCH_BYTES,
            },
        }
    }

    /// The largest mail, and the largest plaintext after decrypting.
    #[must_use]
    pub fn with_max_message_bytes(mut self, bytes: usize) -> Self {
        self.limits.bytes = bytes;
        self
    }

    /// Apply the rule to an offer where a panic in rPGP costs the offer, not the mail. A failure only
    /// costs the offer too: nothing is learned from this mail.
    pub(super) async fn learn_contained(&self, sender: String, offer: Offer) {
        let (learner, runtime) = (self.learner.clone(), Handle::current());
        let work = move || runtime.block_on(learner.learn(&sender, &offer));
        let outcome = contained(work, || Ok(Decision::Ignore)).await;
        if let Err(e) = outcome {
            tracing::warn!(error = %e, "could not learn a key from an Autocrypt header");
        }
    }

    /// Our own key: the stored one, or a new one, stored at once.
    pub async fn ensure_own_key(&self) -> Result<SecretCert, MailError> {
        if let Some(key) = self.keys.own_key(&self.identity.address).await? {
            return Ok(key);
        }
        let key = generate_key(&self.identity.address)?;
        self.keys.put_own_key(&self.identity.address, &key).await?;
        Ok(key)
    }

    /// What is known about `address`; `candidate` set means a key was offered and not taken, which a
    /// person should see. The core has no field for it (`DESIGN.md`).
    pub async fn peer(&self, address: &str) -> Result<Option<PeerKey>, MailError> {
        self.keys.peer(address).await
    }

    /// A person brought the key of `address` herself, out of band. Replaces what was held. The first
    /// way to reach someone at all: nothing leaves unencrypted, so no key arrives by asking.
    pub async fn import_peer_key(
        &self,
        address: &str,
        cert: &crate::openpgp::PublicCert,
    ) -> Result<(), MailError> {
        let now = crate::openpgp::unix_now();
        let peer = PeerKey::new(address, cert, crate::keys::KeySource::Person, now)?;
        self.keys.put_peer(&peer).await
    }

    /// A person compared `fingerprint` with its owner and accepts that key for `address`. Answers
    /// whether a key was replaced; fails if the candidate held now is another one.
    pub async fn accept_candidate(
        &self,
        address: &str,
        fingerprint: &str,
    ) -> Result<bool, MailError> {
        self.learner.accept_candidate(address, fingerprint).await
    }
}

#[async_trait]
impl Provider for MailProvider {
    fn kind(&self) -> CoreResult<String> {
        Ok("mail".to_string())
    }

    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::new("fs-messenger-mail");
        for id in [
            "provider.kind",
            "message.fetch",
            "message.send",
            "encryption.plan",
        ] {
            caps.supported(id);
        }
        caps.unsupported("message.push", "no IMAP IDLE or JMAP push yet; call fetch");
        caps.unsupported(
            "key.discovery",
            "keys come from Autocrypt only; no WKD, no key server",
        );
        caps.unsupported("encryption.smime", "S/MIME is not built");
        caps.unsupported(
            "transport.starttls",
            "implicit TLS only; the test server has no such port",
        );
        caps
    }

    async fn fetch(&self, since: Option<&ProviderCursor>) -> CoreResult<FetchBatch> {
        let cursor = MailCursor::read(since);
        let batch = self.fetcher.fetch(&cursor, &self.limits).await?;
        let mut messages = Vec::with_capacity(batch.mails.len());
        for mail in batch.mails {
            messages.push(self.receive(mail).await?);
        }
        Ok(FetchBatch {
            messages,
            next: Some(batch.cursor.write()?),
            more: batch.more,
        })
    }

    async fn send(&self, message: &Outgoing) -> CoreResult<SendReceipt> {
        self.send_outgoing(message).await
    }

    async fn encryption_plan(&self, recipients: &[String]) -> CoreResult<EncryptionPlan> {
        Ok(self.plan(recipients).await?)
    }
}
