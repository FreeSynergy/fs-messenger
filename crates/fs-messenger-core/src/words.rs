//! Words a person reads, apart from the code that decides (ADR 0057). Code never decides by comparing these.
//!
//! Pattern: none, allowed by section 2 (constants). Code never decides by comparing these.

use crate::why_plain::{Remedy, WhyNot};

/// Why a message is not encrypted, in a sentence.
#[must_use]
pub fn why_not_text(why: WhyNot) -> &'static str {
    match why {
        WhyNot::NoKeyOfTheOtherSide => "Der Schlüssel der anderen Seite ist noch nicht bekannt.",
        WhyNot::AccountCannot => "Dieses Konto kann nicht Ende-zu-Ende verschlüsseln.",
        WhyNot::OnTheWireOnly => "Nur der Weg ist geschützt; die Server können mitlesen.",
        WhyNot::NotKnown => "Es ist nicht bekannt, ob die Nachricht geschützt ist.",
    }
}

/// What the person can do about it.
#[must_use]
pub fn remedy_text(remedy: Remedy) -> &'static str {
    match remedy {
        Remedy::ExchangeKeys => {
            "Schlüssel austauschen: eine signierte Nachricht senden oder um den Schlüssel bitten."
        }
        Remedy::ChooseOtherChannel => "Einen Kanal oder ein Konto wählen, das verschlüsseln kann.",
        Remedy::TurnOn => "Die Verschlüsselung in dieser Unterhaltung einschalten.",
        Remedy::Wait => "Nichts zu tun; das Programm wartet noch auf Auskunft.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_and_remedy_has_a_text() {
        for w in [
            WhyNot::NoKeyOfTheOtherSide,
            WhyNot::AccountCannot,
            WhyNot::OnTheWireOnly,
            WhyNot::NotKnown,
        ] {
            assert_ne!(why_not_text(w), "");
        }
        for r in [
            Remedy::ExchangeKeys,
            Remedy::ChooseOtherChannel,
            Remedy::TurnOn,
            Remedy::Wait,
        ] {
            assert_ne!(remedy_text(r), "");
        }
    }
}
