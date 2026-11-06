//! The rule "encrypted only" (ADR 0018), in one place.
//!
//! Pattern: Specification: pure rules over a plan. Every way out of the core passes `check`, and
//! the type a provider needs to send (`provider::Outgoing`) can only be built after it. There is no
//! override parameter, by design.

use crate::error::{Error, Result};
use crate::model::EncryptionPlan;

/// Whichever way a provider answers, a plan without end-to-end encryption names who stands in the way.
pub(crate) fn normalise(mut plan: EncryptionPlan, recipients: &[String]) -> EncryptionPlan {
    if !plan.encryption.is_end_to_end() && plan.missing_keys.is_empty() {
        plan.missing_keys = recipients.to_vec();
    }
    plan
}

/// A recipient goes on into a mail header: a CRLF there would add one.
pub(crate) fn check_recipients(recipients: &[String]) -> Result<()> {
    if recipients.iter().any(|r| r.contains(char::is_control)) {
        return Err(Error::Invalid(
            "a recipient cannot contain control characters".into(),
        ));
    }
    Ok(())
}

/// Only end-to-end, and only if nobody lacks a key.
pub(crate) fn check(plan: &EncryptionPlan, recipients: &[String]) -> Result<()> {
    if recipients.is_empty() {
        return Err(Error::Invalid("no recipients".into()));
    }
    check_recipients(recipients)?;
    if !plan.encryption.is_end_to_end() || !plan.missing_keys.is_empty() {
        let plan = normalise(plan.clone(), recipients);
        return Err(Error::EncryptionRefused {
            encryption: plan.encryption,
            missing_keys: plan.missing_keys,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Encryption;

    fn plan(encryption: Encryption, missing: &[&str]) -> EncryptionPlan {
        EncryptionPlan {
            encryption,
            missing_keys: missing.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn only_end_to_end_without_gaps_passes() {
        let to = vec!["a".to_string(), "b".to_string()];
        for ok in [Encryption::Mls, Encryption::Openpgp, Encryption::Megolm] {
            assert!(check(&plan(ok, &[]), &to).is_ok());
        }
        for bad in [
            Encryption::TransportOnly,
            Encryption::None,
            Encryption::Unknown,
        ] {
            let Err(Error::EncryptionRefused { missing_keys, .. }) = check(&plan(bad, &[]), &to)
            else {
                panic!("{bad:?} must be refused");
            };
            assert_eq!(
                missing_keys, to,
                "a plan naming nobody blames all recipients"
            );
        }
        let Err(Error::EncryptionRefused { missing_keys, .. }) =
            check(&plan(Encryption::Mls, &["b"]), &to)
        else {
            panic!("a gap must be refused even with a good plan");
        };
        assert_eq!(missing_keys, ["b"]);
    }

    #[test]
    fn no_recipients_is_invalid() {
        assert!(matches!(
            check(&plan(Encryption::Mls, &[]), &[]),
            Err(Error::Invalid(_))
        ));
    }
}
