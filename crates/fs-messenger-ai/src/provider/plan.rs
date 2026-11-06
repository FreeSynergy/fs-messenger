//! What the plan says, and what this provider refuses before anything goes out.
//!
//! Pattern: Specification: pure rules. A model must read the text, so no end-to-end encryption can reach
//! it. The plan says what is true of the way there, and names the model as the one without a key. The
//! core refuses that plan, and it is meant to: `DESIGN.md` has the decision that is open.

use fs_messenger_core::{Encryption, EncryptionPlan, Outgoing};

use crate::error::AiError;

/// The honest answer for `recipients`: every one of them stands in the way.
pub(super) fn honest_plan(encryption: Encryption, recipients: &[String]) -> EncryptionPlan {
    EncryptionPlan {
        encryption,
        missing_keys: recipients.to_vec(),
    }
}

/// Only to this model, only text.
pub(super) fn deliverable(message: &Outgoing, profile: &str) -> Result<(), AiError> {
    let everyone = message.to().iter().chain(message.cc()).chain(message.bcc());
    if let Some(other) = everyone.into_iter().find(|r| r.as_str() != profile) {
        return Err(AiError::NotForThisModel(format!(
            "{other} is not this model"
        )));
    }
    if !message.attachments().is_empty() {
        return Err(AiError::NotForThisModel(
            "files cannot go to a model yet; nothing was sent".into(),
        ));
    }
    if message.text().trim().is_empty() {
        return Err(AiError::NotForThisModel("there is no text".into()));
    }
    Ok(())
}
