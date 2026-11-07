//! Error type of the messenger core.
//!
//! Pattern: none (plain error enum via `thiserror`): one `Error` for every public operation. The
//! refusal to send is a named variant, so no caller can mistake it for a transport problem.

use fs_objects::ObjectId;

use crate::guard_hook::DecisionProblem;
use crate::model::Encryption;

/// Result of every fallible operation of this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong in the messenger core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The object store failed.
    #[error("object store: {0}")]
    Store(#[from] fs_objects::Error),
    /// JSON could not be read or written.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// The explicit "not possible" (ADR 0045).
    #[error(transparent)]
    Unsupported(#[from] fs_interfaces::provider::Unsupported),
    /// No such object.
    #[error("{0} not found")]
    NotFound(ObjectId),
    /// ADR 0018: nothing left. There is no way around this.
    #[error(
        "refused to send (ADR 0018): the encryption plan is `{}`, recipients without a usable key: {}",
        .encryption.as_str(),
        .missing_keys.join(", ")
    )]
    EncryptionRefused {
        /// The plan that was refused.
        encryption: Encryption,
        /// Who lacks a key; every recipient if the provider named none.
        missing_keys: Vec<String>,
    },
    /// The provider reports weaker protection after sending than it planned. The message is stored
    /// with what the provider reported.
    #[error("message {message} left with encryption `{}`, not as planned", .encryption.as_str())]
    EncryptionDowngraded {
        /// The message.
        message: ObjectId,
        /// What the provider reported.
        encryption: Encryption,
    },
    /// ADR 0018, 0024: the program does not open on a store without encryption.
    #[error("the store is not encrypted; the messenger does not open on it (ADR 0024)")]
    StoreNotEncrypted,
    /// A message is larger than this program takes. Nothing of it was stored.
    #[error("message of {size} bytes refused, the limit is {limit}")]
    TooLarge {
        /// Its size, all parts counted.
        size: usize,
        /// The limit.
        limit: usize,
    },
    /// The object holding this message's text has gone: store damage. Its summary still lists.
    #[error("the text of message {message} is missing from the store (object {body})")]
    BodyMissing {
        /// The message.
        message: ObjectId,
        /// The object that should hold the text.
        body: ObjectId,
    },
    /// The input breaks a rule.
    #[error("invalid: {0}")]
    Invalid(String),
    /// The operation does not fit the current state.
    #[error("conflict: {0}")]
    Conflict(String),
    /// A provider failed.
    #[error("provider: {0}")]
    Provider(String),
    /// A decision was refused. Nothing changed; `problem` is what a machine goes by.
    #[error("decision refused: {detail}")]
    DecisionRefused {
        /// Which of the contract's named problems this is.
        problem: DecisionProblem,
        /// What a person reads.
        detail: String,
    },
    /// The guard refused it (ADR 0054). Nothing changed, unless the text says otherwise.
    #[error("guard: {0}")]
    GuardRefused(String),
}
