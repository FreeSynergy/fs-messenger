//! Merging two forms of one certificate, and keeping only what is covered.
//!
//! Pattern: Specification over rPGP's packets. Anyone can write in the unhashed area of a copied signature
//! without any key, so nothing from there counts as new and nothing from there is kept. The one exception:
//! rPGP takes a signing subkey's back-signature from there too, so on a sign-flagged subkey binding that
//! one stays, if it verifies as that subkey's back-signature, stripped in turn. Revocations are merged
//! first and do not count against the size cap: a padded form must never keep a key usable that its owner
//! revoked. A revocation needs the owner's key, so what bounds them is [`MAX_REVOCATIONS`].

use pgp::composed::SignedPublicSubKey;
use pgp::packet::{PublicKey, Signature, SignatureType, Subpacket, SubpacketData};
use pgp::types::KeyDetails;

use super::PublicCert;
use super::cert::MAX_CERT_BYTES;

/// Revocations kept for the key, and for each subkey. One is enough; the bound is against its owner.
const MAX_REVOCATIONS: usize = 16;

impl PublicCert {
    /// This certificate and everything `other` adds to the same key. `None` if it adds nothing or does
    /// not verify. Nothing is ever removed, so a form with a subkey stripped cannot make a peer unusable,
    /// and a form without a revocation cannot undo one. What is not a revocation is left out if it would
    /// take the certificate past the size limit; revocations go in regardless.
    pub(crate) fn merged_with(&self, other: &Self) -> Option<Self> {
        self.merged_within(other, MAX_CERT_BYTES)
    }

    pub(super) fn merged_within(&self, other: &Self, cap: usize) -> Option<Self> {
        let mut revoked = self.clone();
        let revocations = revoked.add_revocations(other);
        let mut whole = revoked.clone();
        let rest = whole.add_rest(other);
        let fits = whole.to_binary().is_ok_and(|b| b.len() <= cap);
        if rest && fits && whole.0.verify_bindings().is_ok() {
            return Some(whole);
        }
        (revocations && revoked.0.verify_bindings().is_ok()).then_some(revoked)
    }

    /// The same certificate without what its signatures do not cover, for keeping. `None` if that no
    /// longer verifies: such a certificate is not kept at all.
    pub(crate) fn compact(&self) -> Option<Self> {
        let mut compact = self.clone();
        let primary = compact.0.primary_key.clone();
        let details = &mut compact.0.details;
        strip_all(&mut details.revocation_signatures);
        strip_all(&mut details.direct_signatures);
        details
            .users
            .iter_mut()
            .for_each(|u| strip_all(&mut u.signatures));
        for sub in &mut compact.0.public_subkeys {
            compact_subkey(&primary, sub);
        }
        compact.0.verify_bindings().is_ok().then_some(compact)
    }

    fn add_revocations(&mut self, other: &Self) -> bool {
        let primary = self.0.primary_key.clone();
        let (held, offered) = (&mut self.0, &other.0);
        let mut changed = add_missing(
            &mut held.details.revocation_signatures,
            offered.details.revocation_signatures.iter(),
            Some(MAX_REVOCATIONS),
            stripped,
        );
        for sub in &offered.public_subkeys {
            let revocations = sub.signatures.iter().filter(|s| is_subkey_revocation(s));
            if let Some(mine) = held
                .public_subkeys
                .iter_mut()
                .find(|s| s.key.fingerprint() == sub.key.fingerprint())
            {
                let key = mine.key.clone();
                changed |= add_missing(
                    &mut mine.signatures,
                    revocations,
                    Some(MAX_REVOCATIONS),
                    |s| compact_binding(&primary, &key, s),
                );
            }
        }
        changed
    }

    fn add_rest(&mut self, other: &Self) -> bool {
        let primary = self.0.primary_key.clone();
        let (held, offered) = (&mut self.0, &other.0);
        let mut changed = add_missing(
            &mut held.details.direct_signatures,
            offered.details.direct_signatures.iter(),
            None,
            stripped,
        );
        for user in &offered.details.users {
            if let Some(mine) = held.details.users.iter_mut().find(|u| u.id == user.id) {
                changed |=
                    add_missing(&mut mine.signatures, user.signatures.iter(), None, stripped);
            } else {
                let mut new = user.clone();
                strip_all(&mut new.signatures);
                held.details.users.push(new);
                changed = true;
            }
        }
        for sub in &offered.public_subkeys {
            if let Some(mine) = held
                .public_subkeys
                .iter_mut()
                .find(|s| s.key.fingerprint() == sub.key.fingerprint())
            {
                let key = mine.key.clone();
                let bindings = sub.signatures.iter().filter(|s| !is_subkey_revocation(s));
                changed |= add_missing(&mut mine.signatures, bindings, None, |s| {
                    compact_binding(&primary, &key, s)
                });
            } else {
                let mut new = sub.clone();
                compact_subkey(&primary, &mut new);
                held.public_subkeys.push(new);
                changed = true;
            }
        }
        changed
    }
}

fn is_subkey_revocation(signature: &Signature) -> bool {
    signature.typ() == Some(SignatureType::SubkeyRevocation)
}

/// What a signature covers: its hashed area and its bytes. Two signatures differing only in the unhashed
/// area are one signature.
fn same_signature(a: &Signature, b: &Signature) -> bool {
    let covered = |s: &Signature| {
        s.config().map(|c| {
            let mut c = c.clone();
            c.unhashed_subpackets.clear();
            c
        })
    };
    let (ca, cb) = (covered(a), covered(b));
    ca.is_some() && ca == cb && a.signature() == b.signature()
}

/// `signature` with nothing in the unhashed area.
fn stripped(signature: &Signature) -> Option<Signature> {
    let mut config = signature.config()?.clone();
    config.unhashed_subpackets = vec![];
    let (left, bytes) = (
        signature.signed_hash_value()?,
        signature.signature()?.clone(),
    );
    Signature::from_config(config, left, bytes).ok()
}

/// A signature on `subkey`, stripped. A sign-flagged binding keeps a back-signature that stands in the
/// unhashed area, if it verifies, because rPGP and other programs need it there. In the hashed area one is
/// already covered, and an embedded signature anywhere else is what anyone can write: its own hashed area
/// is free, and would survive stripping.
fn compact_binding(
    primary: &PublicKey,
    subkey: &pgp::packet::PublicSubkey,
    signature: &Signature,
) -> Option<Signature> {
    let mut compact = stripped(signature)?;
    let needs_back = signature.typ() == Some(SignatureType::SubkeyBinding)
        && signature.key_flags().sign()
        && compact.embedded_signature().is_none();
    if !needs_back {
        return Some(compact);
    }
    let back = signature
        .embedded_signature()
        .and_then(stripped)
        .filter(|b| b.verify_primary_key_binding(subkey, primary).is_ok());
    if let Some(back) = back {
        let mut config = compact.config()?.clone();
        let sub = SubpacketData::EmbeddedSignature(Box::new(back));
        config.unhashed_subpackets = vec![Subpacket::regular(sub).ok()?];
        let (left, bytes) = (compact.signed_hash_value()?, compact.signature()?.clone());
        compact = Signature::from_config(config, left, bytes).ok()?;
    }
    Some(compact)
}

fn compact_subkey(primary: &PublicKey, sub: &mut SignedPublicSubKey) {
    let key = sub.key.clone();
    sub.signatures = sub
        .signatures
        .iter()
        .filter_map(|s| compact_binding(primary, &key, s))
        .collect();
}

fn strip_all(signatures: &mut Vec<Signature>) {
    *signatures = signatures.iter().filter_map(stripped).collect();
}

/// Adds what is new in `offered`, in compact form, up to `bound` signatures held.
fn add_missing<'a>(
    held: &mut Vec<Signature>,
    offered: impl Iterator<Item = &'a Signature>,
    bound: Option<usize>,
    compact: impl Fn(&Signature) -> Option<Signature>,
) -> bool {
    let before = held.len();
    for signature in offered {
        if bound.is_some_and(|b| held.iter().filter(|s| kept_revocation(s)).count() >= b) {
            break;
        }
        if !held.iter().any(|h| same_signature(h, signature))
            && let Some(kept) = compact(signature)
        {
            held.push(kept);
        }
    }
    held.len() != before
}

fn kept_revocation(signature: &Signature) -> bool {
    matches!(
        signature.typ(),
        Some(SignatureType::KeyRevocation | SignatureType::SubkeyRevocation)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openpgp::generate_key;

    /// What anyone can write without a key: a copy of the direct signature, padded in the unhashed area.
    fn padded(cert: &PublicCert, tag: &str) -> PublicCert {
        let mut form = cert.clone();
        let signature = &form.0.details.direct_signatures[0];
        let mut config = signature.config().unwrap().clone();
        let note = pgp::packet::Notation {
            readable: true,
            name: "pad@example.org".as_bytes().to_vec().into(),
            value: tag.as_bytes().to_vec().into(),
        };
        config
            .unhashed_subpackets
            .push(Subpacket::regular(SubpacketData::Notation(note)).unwrap());
        let copy = Signature::from_config(
            config,
            signature.signed_hash_value().unwrap(),
            signature.signature().unwrap().clone(),
        )
        .unwrap();
        form.0.details.direct_signatures.push(copy);
        form
    }

    #[test]
    fn a_signature_padded_in_the_unhashed_area_is_the_same_signature() {
        let cert = generate_key("a@x").unwrap().public();
        let form = padded(&cert, "anything");
        assert!(
            form.0.verify_bindings().is_ok(),
            "the padding verifies, it is not covered"
        );
        assert!(cert.merged_with(&form).is_none(), "and it adds nothing");
    }

    #[test]
    fn compacting_drops_what_is_not_covered_and_still_verifies() {
        let key = generate_key("a@x").unwrap();
        let form = padded(&key.public(), &"x".repeat(5000));
        let compact = form.compact().unwrap();
        assert!(compact.0.verify_bindings().is_ok());
        assert!(compact.to_binary().unwrap().len() < 2000);
        assert_eq!(compact.fingerprint(), key.fingerprint());
    }
}
