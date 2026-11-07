//! Who can read what: [`Reach`], [`method_for`], and the provider's way to find the keys of a person.
//!
//! Pattern: Specification (`method_for`, `Reach`) plus the lookups the Template method in `send` uses. A
//! person has the key in use and perhaps a v4 key beside it; the message is `Modern` only if every
//! reader takes it (RFC 9580 section 13.7), and `Older` for all otherwise (sections 5.1.1 and 5.1.2).

use fs_messenger_core::{Encryption, EncryptionPlan, Error as CoreError, Result as CoreResult};

use super::facade::MailProvider;
use crate::address::{clean_address, normalise_address};
use crate::error::MailError;
use crate::openpgp::{
    Method, PublicCert, SecretCert, generate_legacy_key, supports_seipd_v2, unix_now,
    usable_encryption_key,
};

/// The keys that reach one person: the key in use, and a v4 key beside it if she has one.
pub(super) struct Reach {
    pub(super) key: PublicCert,
    pub(super) legacy: Option<PublicCert>,
}

impl Reach {
    /// Whether she reads `SEIPDv2`: the key in use says so (RFC 9580 section 13.7).
    pub(super) fn takes_modern(&self) -> bool {
        supports_seipd_v2(&self.key)
    }

    /// The key an older message goes to: her v4 key if there is one, else the key in use, which a
    /// reader of v6 also reads in a v1 message (a v3 PKESK precedes a v1 SEIPD whatever the key).
    pub(super) fn for_older(&self) -> &PublicCert {
        self.legacy.as_ref().unwrap_or(&self.key)
    }

    /// The v4 key if there is one, else the key in use: what goes into `keydata`.
    pub(super) fn legacy_or_key(&self) -> PublicCert {
        self.for_older().clone()
    }

    pub(super) fn for_method(&self, method: Method) -> &PublicCert {
        match method {
            Method::Modern => &self.key,
            Method::Older => self.for_older(),
        }
    }
}

/// `Modern` only if every reader takes it, ours included (RFC 9580 section 13.7). One reader who does
/// not turns the whole message `Older`: a message never mixes the two (sections 5.1.1 and 5.1.2).
pub(super) fn method_for<'a>(readers: impl IntoIterator<Item = &'a Reach>) -> Method {
    if readers.into_iter().all(Reach::takes_modern) {
        Method::Modern
    } else {
        Method::Older
    }
}

impl MailProvider {
    /// The keys to use for `address`, if there is a usable one. Her legacy key counts only if it is
    /// usable itself; the key in use must be.
    pub(super) async fn usable_cert(
        &self,
        address: &str,
        now: u64,
    ) -> Result<Option<Reach>, MailError> {
        if !clean_address(address) {
            return Ok(None);
        }
        let address = normalise_address(address);
        let reach = if address == self.identity.address {
            match self.keys.own_key(&address).await? {
                Some(key) => Some(Reach {
                    key: key.public(),
                    legacy: self
                        .keys
                        .own_legacy_key(&address)
                        .await?
                        .map(|k| k.public()),
                }),
                None => None,
            }
        } else {
            match self.keys.peer(&address).await? {
                Some(peer) => Some(Reach {
                    key: peer.certificate()?,
                    legacy: peer
                        .legacy
                        .as_ref()
                        .map(crate::keys::LegacyKey::certificate)
                        .transpose()?,
                }),
                None => None,
            }
        };
        Ok(reach
            .filter(|r| usable_encryption_key(&r.key, now).is_ok())
            .map(|mut r| {
                r.legacy = r
                    .legacy
                    .take()
                    .filter(|l| usable_encryption_key(l, now).is_ok());
                r
            }))
    }

    /// Our own v4 key; made and kept if the account has none yet.
    pub(super) async fn own_legacy(&self, address: &str) -> Result<Option<SecretCert>, MailError> {
        if let Some(key) = self.keys.own_legacy_key(address).await? {
            return Ok(Some(key));
        }
        if self.keys.own_key(address).await?.is_none() {
            return Ok(None);
        }
        let key = generate_legacy_key(address)?;
        self.keys.put_own_legacy_key(address, &key).await?;
        Ok(Some(key))
    }

    /// `openpgp` only if everyone has a usable key, hidden recipients too, and we have our own: every
    /// mail is encrypted to the sender as well. Otherwise it names exactly who lacks one.
    pub(super) async fn plan(&self, recipients: &[String]) -> Result<EncryptionPlan, MailError> {
        let (missing, _) = self.reach_of(recipients, unix_now()).await?;
        Ok(EncryptionPlan {
            encryption: if missing.is_empty() {
                Encryption::Openpgp
            } else {
                Encryption::None
            },
            missing_keys: missing,
        })
    }

    /// How a message to `recipients` would be encrypted: `None` if some of them has no usable key. A
    /// frontend shows `Method::Older` as "older method" (`DESIGN.md`); the core has no field for it.
    pub async fn encryption_method(
        &self,
        recipients: &[String],
    ) -> Result<Option<Method>, MailError> {
        let (missing, reach) = self.reach_of(recipients, unix_now()).await?;
        Ok(missing.is_empty().then(|| method_for(&reach)))
    }

    /// Who lacks a usable key, and the keys of everybody else; ourselves first.
    async fn reach_of(
        &self,
        recipients: &[String],
        now: u64,
    ) -> Result<(Vec<String>, Vec<Reach>), MailError> {
        let own = self.identity.address.clone();
        let mut missing: Vec<String> = Vec::new();
        let mut reach = Vec::new();
        for who in std::iter::once(&own).chain(recipients) {
            match self.usable_cert(who, now).await? {
                Some(r) => reach.push(r),
                None if !missing.contains(who) => missing.push(who.clone()),
                None => {}
            }
        }
        Ok((missing, reach))
    }

    /// Keys for `who`; a gap here is the refusal of ADR 0018.
    pub(super) async fn certs_of(&self, who: &[String], now: u64) -> CoreResult<Vec<Reach>> {
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
}
