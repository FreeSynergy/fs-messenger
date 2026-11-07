//! The rule for an offered key: [`decide`].
//!
//! Pattern: Specification (a pure function over what is known and what is offered). PROVISIONAL, stated
//! in `DESIGN.md`. In short: first seen wins, the same key only refreshes, and a different key replaces
//! the known one only with a proof a forger cannot produce: it was offered inside an encrypted payload
//! whose signature verifies with the known key. An outer `Autocrypt` header proves nothing, whatever
//! signature the mail carries, because that header is not covered by the signature.
//!
//! A peer may hold two keys, v6 in use and a v4 `legacy` beside it, for programs that read nothing newer.
//! The v4 key is a way to reach the person, and also a way to downgrade her, so it follows the same
//! discipline: it comes with the first sight of the pair, and afterwards it is added or replaced only
//! with the same proof as a rotation. A v4 offer without proof, to a peer whose key in use is v6, is
//! only ever a candidate: it is never encrypted to and never takes the place of the v6 key.

use super::learner::Offer;
use crate::keys::PeerKey;
use crate::openpgp::{KeyRefusal, usable_encryption_key};

/// What to do with an offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Nothing changes.
    Ignore,
    /// Nobody known yet: trust on first use.
    FirstSeen,
    /// The known key, seen again; possibly with more self-signatures.
    SameKey {
        /// Whether the offered certificate adds something to merge into the stored one.
        take_offered: bool,
    },
    /// A proven rotation.
    Rotate,
    /// Somebody offers another key without proof: keep the known one, remember the offer.
    Conflict,
    /// The v4 key beside a held v6 key: the offer is that key, seen again or proven as its replacement.
    Legacy,
}

/// Decide about `offer`, given what is `known`, at `now`.
pub fn decide(known: Option<&PeerKey>, offer: &Offer, now: u64) -> Decision {
    let usable = usable_encryption_key(&offer.cert, now).is_ok();
    let Some(known) = known else {
        return if usable {
            Decision::FirstSeen
        } else {
            Decision::Ignore
        };
    };
    let fingerprint = offer.cert.fingerprint();
    if known.fingerprint == fingerprint {
        return same_key(known, offer, now);
    }
    if !usable {
        return Decision::Ignore;
    }
    let proven = offer.proven_by_known_key && may_prove(known, now);
    let beside_v6 = offer.cert.is_v4() && known.certificate().is_ok_and(|held| held.is_v6());
    if beside_v6
        && known
            .legacy
            .as_ref()
            .is_some_and(|l| l.fingerprint == fingerprint)
    {
        return Decision::Legacy;
    }
    if beside_v6 && proven {
        return Decision::Legacy;
    }
    if proven && !beside_v6 {
        Decision::Rotate
    } else {
        Decision::Conflict
    }
}

/// A key that is revoked or ran out proves nothing any more: whoever holds it now may not be its owner.
fn may_prove(known: &PeerKey, now: u64) -> bool {
    known.certificate().is_ok_and(|held| {
        !matches!(
            usable_encryption_key(&held, now),
            Err(KeyRefusal::Revoked | KeyRefusal::Expired | KeyRefusal::Invalid | KeyRefusal::Weak)
        )
    })
}

/// The same primary key. A forger cannot add self-signatures, so what a form brings beyond the held one,
/// if its signatures verify, is the owner's: a revocation, a new subkey, a later expiry. The two forms are
/// merged, and nothing is ever given up: a form with a subkey stripped takes nothing away. What he can
/// write is the unhashed area of a copied signature: that counts as nothing new (`PublicCert::merged_with`).
fn same_key(known: &PeerKey, offer: &Offer, now: u64) -> Decision {
    let invalid = usable_encryption_key(&offer.cert, now) == Err(KeyRefusal::Invalid);
    let take_offered = !invalid
        && known
            .certificate()
            .is_ok_and(|held| held.merged_with(&offer.cert).is_some());
    Decision::SameKey { take_offered }
}
