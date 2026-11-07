//! The [`KeyStore`] trait.
//!
//! Pattern: Repository (interface): the provider depends on this, never on `fs-objects`.

use async_trait::async_trait;

use super::PeerKey;
use crate::error::MailError;
use crate::openpgp::SecretCert;

/// Where keys live.
#[async_trait]
pub trait KeyStore: Send + Sync {
    /// The person's own secret key for `address`.
    async fn own_key(&self, address: &str) -> Result<Option<SecretCert>, MailError>;
    /// Keep the person's own secret key for `address`, replacing an earlier one.
    async fn put_own_key(&self, address: &str, key: &SecretCert) -> Result<(), MailError>;
    /// What is known about a peer.
    async fn peer(&self, address: &str) -> Result<Option<PeerKey>, MailError>;
    /// Keep what is known about a peer, replacing the earlier record.
    async fn put_peer(&self, peer: &PeerKey) -> Result<(), MailError>;
}
