//! Explain encryption (accepted idea 9): say why a message is not encrypted and how to change that.
//!
//! Pattern: Specification: the reason is a **value** (`WhyNot`), chosen by a pure function from facts; the
//! words a person reads are in `words` (ADR 0057), and code never decides by comparing them.

use crate::model::Encryption;

/// Facts about one message or one conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    /// How it is protected now.
    pub encryption: Encryption,
    /// Whether the other side's key is known (for OpenPGP) or the room is encrypted (Matrix).
    pub other_side_ready: bool,
    /// Whether this kind of account can encrypt at all (plain mail with no keys cannot).
    pub account_can: bool,
}

/// Why a message is not encrypted. `None` from [`why_not`] means it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhyNot {
    /// The other side's key is not known yet.
    NoKeyOfTheOtherSide,
    /// The account's service cannot encrypt end to end.
    AccountCannot,
    /// Protected on the way only: the servers can read it.
    OnTheWireOnly,
    /// Not known: never shown as encrypted.
    NotKnown,
}

/// What to do about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remedy {
    /// Ask for a key or send a first signed message so the key is learned.
    ExchangeKeys,
    /// Use another account or channel that can encrypt.
    ChooseOtherChannel,
    /// Turn encryption on in the conversation.
    TurnOn,
    /// Nothing to do: only the program is waiting.
    Wait,
}

/// The reason and the remedy, or `None` for an end-to-end encrypted message.
#[must_use]
pub fn why_not(f: Facts) -> Option<(WhyNot, Remedy)> {
    match f.encryption {
        Encryption::Mls | Encryption::Openpgp | Encryption::Megolm => None,
        Encryption::TransportOnly => Some((WhyNot::OnTheWireOnly, Remedy::ChooseOtherChannel)),
        Encryption::None | Encryption::Unknown => Some(if !f.account_can {
            (WhyNot::AccountCannot, Remedy::ChooseOtherChannel)
        } else if !f.other_side_ready {
            (WhyNot::NoKeyOfTheOtherSide, Remedy::ExchangeKeys)
        } else if f.encryption == Encryption::Unknown {
            (WhyNot::NotKnown, Remedy::Wait)
        } else {
            (WhyNot::NotKnown, Remedy::TurnOn)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(encryption: Encryption, other: bool, can: bool) -> Facts {
        Facts {
            encryption,
            other_side_ready: other,
            account_can: can,
        }
    }

    #[test]
    fn end_to_end_needs_no_explanation() {
        for e in [Encryption::Mls, Encryption::Openpgp, Encryption::Megolm] {
            assert_eq!(why_not(facts(e, true, true)), None);
        }
    }

    #[test]
    fn a_service_that_cannot_says_so_first() {
        assert_eq!(
            why_not(facts(Encryption::None, false, false)),
            Some((WhyNot::AccountCannot, Remedy::ChooseOtherChannel))
        );
    }

    #[test]
    fn a_missing_key_points_to_the_exchange() {
        assert_eq!(
            why_not(facts(Encryption::None, false, true)),
            Some((WhyNot::NoKeyOfTheOtherSide, Remedy::ExchangeKeys))
        );
    }

    #[test]
    fn transport_only_is_never_shown_as_encrypted() {
        assert_eq!(
            why_not(facts(Encryption::TransportOnly, true, true)),
            Some((WhyNot::OnTheWireOnly, Remedy::ChooseOtherChannel))
        );
    }

    #[test]
    fn unknown_is_not_encrypted_either() {
        assert!(why_not(facts(Encryption::Unknown, true, true)).is_some());
    }
}
