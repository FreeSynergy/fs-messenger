//! What one read comes to: which headers are believed, and what the signature says for the sender.
//!
//! Pattern: Specification (pure functions): the rules of "What a signature vouches for" in `DESIGN.md`.

use fs_messenger_core::{Encryption, SignatureCheck};

use super::read::{ReadMail, offer_of, offering};
use crate::mime::MailFields;
use crate::openpgp::SignatureState;

/// What the state means for the sender: `Valid` only with a key known before this mail, over a part that
/// names the sender itself. A mail that vouches for itself, or a bare body, is not verified.
pub(super) fn check_of(state: &SignatureState, known: bool, names_sender: bool) -> SignatureCheck {
    match state {
        SignatureState::NotSigned => SignatureCheck::NotSigned,
        SignatureState::Valid { .. } if known && names_sender => SignatureCheck::Valid,
        SignatureState::Valid { .. } | SignatureState::Unverified => SignatureCheck::NotVerified,
    }
}

/// The payload is open. Inner headers win if a signature covers them or they declare Header Protection;
/// otherwise the headers stay the outer ones, and only body and files come from inside.
pub(super) fn opened_mail(
    outer: MailFields,
    inner: MailFields,
    state: &SignatureState,
    (known, proving): (bool, Option<&str>),
) -> ReadMail {
    let signature = check_of(state, known, inner.from.is_some());
    // Only the key in use proves an offer; a signature of the legacy key beside it verifies the mail
    // and nothing more.
    let verified = signature == SignatureCheck::Valid
        && matches!(state, SignatureState::Valid { fingerprint } if Some(fingerprint.as_str()) == proving);
    let signed_ok = signature == SignatureCheck::Valid;
    let believed = signed_ok || inner.header_protected;
    let sender = if believed { inner.from.clone() } else { None };
    let sender = sender.or_else(|| outer.from.clone()).unwrap_or_default();
    // The rule of `autocrypt::rule`: only an offer the known key signed, in a part that declares its
    // headers as headers (`hp`), is a proof. Someone who signs a text with a header-like line in it did
    // not offer a key.
    let inner_offer = offer_of(&inner.autocrypt, &sender);
    let offer = match inner_offer {
        Some(cert) => offering(&sender, Some(cert), verified && inner.header_protected),
        None => offering(&sender, offer_of(&outer.autocrypt, &sender), false),
    };
    let message_id = if believed {
        inner
            .message_id
            .clone()
            .or_else(|| outer.message_id.clone())
    } else {
        outer.message_id.clone()
    };
    ReadMail {
        fields: merged(outer, inner, believed),
        message_id,
        encryption: Encryption::Openpgp,
        undecrypted: false,
        signature,
        offer,
    }
}

pub(super) fn plain(fields: MailFields, signature: SignatureCheck) -> ReadMail {
    let sender = fields.from.clone().unwrap_or_default();
    let offer = offering(&sender, offer_of(&fields.autocrypt, &sender), false);
    ReadMail {
        message_id: fields.message_id.clone(),
        fields,
        encryption: Encryption::None,
        undecrypted: false,
        signature,
        offer,
    }
}

pub(super) fn undecrypted(outer: MailFields, message_id: Option<String>) -> ReadMail {
    let sender = outer.from.clone().unwrap_or_default();
    let offer = offering(&sender, offer_of(&outer.autocrypt, &sender), false);
    ReadMail {
        fields: MailFields {
            text_body: String::new(),
            text_media_type: "text/plain".to_string(),
            attachments: vec![],
            ..outer
        },
        message_id,
        encryption: Encryption::None,
        undecrypted: true,
        signature: SignatureCheck::NotVerified,
        offer,
    }
}

fn merged(outer: MailFields, inner: MailFields, believed: bool) -> MailFields {
    if believed {
        return inner;
    }
    MailFields {
        text_body: inner.text_body,
        text_media_type: inner.text_media_type,
        attachments: inner.attachments,
        ..outer
    }
}
