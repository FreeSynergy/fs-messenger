//! The rule for an offered key: [`decide`].
//!
//! Pattern: Specification (a pure function over what is known and what is offered). PROVISIONAL, stated
//! in `DESIGN.md`. In short: first seen wins, the same key only refreshes, and a different key replaces
//! the known one only with a proof a forger cannot produce: it was offered inside an encrypted payload
//! whose signature verifies with the known key. An outer `Autocrypt` header proves nothing, whatever
//! signature the mail carries, because that header is not covered by the signature.

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
    /// The known key, seen again; possibly with newer self-signatures.
    SameKey {
        /// Whether to take the offered certificate over the stored one.
        take_offered: bool,
    },
    /// A proven rotation.
    Rotate,
    /// Somebody offers another key without proof: keep the known one, remember the offer.
    Conflict,
}

/// Decide about `offer`, given what is `known`, at `now`.
pub fn decide(known: Option<&PeerKey>, offer: &Offer, now: u64) -> Decision {
    let usable = usable_encryption_key(&offer.cert, now).is_ok();
    let Some(known) = known else {
        return if usable { Decision::FirstSeen } else { Decision::Ignore };
    };
    if known.fingerprint == offer.cert.fingerprint() {
        return same_key(known, offer, now);
    }
    if !usable {
        return Decision::Ignore;
    }
    if offer.proven_by_known_key {
        Decision::Rotate
    } else {
        Decision::Conflict
    }
}

/// The same primary key: the offered form may carry a revocation or a later expiry, and a forger cannot
/// add self-signatures. He can only offer an older form, so a revocation is never given up.
fn same_key(known: &PeerKey, offer: &Offer, now: u64) -> Decision {
    let check = |cert| usable_encryption_key(cert, now);
    let Ok(held) = known.certificate() else {
        return Decision::SameKey { take_offered: false };
    };
    let take_offered = match (check(&held), check(&offer.cert)) {
        // Newly revoked: only if its signatures verify, which `Invalid` would say.
        (Err(KeyRefusal::Revoked), _) | (_, Err(KeyRefusal::Invalid)) => false,
        (Ok(_) | Err(_), Err(KeyRefusal::Revoked)) => true,
        // A later self-signature, and it does not make a live key unusable.
        (held_state, offered_state) => {
            let newer = offer.cert.newest_self_signature() > held.newest_self_signature();
            newer && (held_state.is_err() || offered_state.is_ok())
        }
    };
    Decision::SameKey { take_offered }
}
