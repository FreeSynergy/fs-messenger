//! The small enums of a message: [`Encryption`], [`Kind`], [`SenderKind`], [`DeliveryState`].
//!
//! Pattern: Value objects with fail-safe defaults. An unset or unrecognised encryption reads as
//! `unknown`, which is never proof of encryption, and an unset sender as `unknown`: a record can
//! only ever claim less than is true, never more. These enums travel by name, not by ordinal, so
//! `#[serde(other)]` can require the fallback to be declared last while the contract has it at 0.

use serde::{Deserialize, Serialize};

/// How a message is protected (ADR 0018). Stored as the vendor property `freesynergy.net:encryption`
/// with the values of `data/messages/technical.md`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Encryption {
    /// MLS, RFC 9420.
    #[serde(rename = "mls")]
    Mls,
    /// OpenPGP, RFC 9580.
    #[serde(rename = "openpgp")]
    Openpgp,
    /// Matrix Megolm.
    #[serde(rename = "megolm")]
    Megolm,
    /// Protected on the wire only; the servers can read it.
    #[serde(rename = "transport-only", alias = "transportOnly")]
    TransportOnly,
    /// Known not to be protected.
    #[serde(rename = "none")]
    None,
    /// Not known. Unset and unrecognised values read as this; never shown as encrypted, and sending
    /// with it is refused. Ordinal 0 of the contract.
    #[default]
    #[serde(rename = "unknown", other)]
    Unknown,
}

impl Encryption {
    /// Whether only the participants can read it. Only this may be sent (ADR 0018).
    pub fn is_end_to_end(self) -> bool {
        matches!(self, Self::Mls | Self::Openpgp | Self::Megolm)
    }

    /// The stored value, as in `data/messages/technical.md`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mls => "mls",
            Self::Openpgp => "openpgp",
            Self::Megolm => "megolm",
            Self::TransportOnly => "transport-only",
            Self::None => "none",
            Self::Unknown => "unknown",
        }
    }
}

/// What sort of conversation a message belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// E-mail.
    Mail,
    /// Chat or messenger.
    Chat,
    /// Comment thread.
    Comment,
    /// Not known: no guess. Not in the contract, whose ordinal 0 is `mail`; see `DESIGN.md`.
    #[default]
    #[serde(other)]
    Unknown,
}

impl Kind {
    /// The stored value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mail => "mail",
            Self::Chat => "chat",
            Self::Comment => "comment",
            Self::Unknown => "unknown",
        }
    }
}

/// Who wrote a message (ADR 0054): from the sender's card, never a guess from the text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SenderKind {
    /// A person.
    Person,
    /// Plain software.
    Software,
    /// An AI, marked so that everyone can see it is not human.
    Ai,
    /// Not known. Unset and unrecognised values read as this.
    #[default]
    #[serde(other)]
    Unknown,
}

/// What a provider's signature check found. Not in the contract. Only `Valid` says anything for a
/// sender, and unset or unrecognised reads as not verified.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SignatureCheck {
    /// It carries no signature.
    NotSigned,
    /// It verifies with a key known for the sender.
    Valid,
    /// Not checked, or it could not be verified.
    #[default]
    #[serde(other)]
    NotVerified,
}

/// Where an outgoing message is, stored locally (ADR 0037).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryState {
    /// On its way out.
    Sending,
    /// Handed to the transport.
    Sent,
    /// The other side has it.
    Delivered,
    /// The other side read it.
    Read,
    /// It did not leave.
    Failed,
    /// Not known: no guess. Not in the contract, whose ordinal 0 is `sending`; see `DESIGN.md`.
    #[default]
    #[serde(other)]
    Unknown,
}
