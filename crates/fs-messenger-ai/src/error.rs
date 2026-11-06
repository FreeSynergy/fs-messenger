//! Errors of the model provider, before a turn runs. How a running turn ends is `TurnFailure`.
//!
//! Pattern: none (plain error enum via `thiserror`). No variant carries a secret: the key is only held in
//! `ApiKey`, which cannot be printed, and nothing here takes text from a server.

/// What can be refused before a turn runs.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// The origin is not usable.
    #[error("endpoint: {0}")]
    Endpoint(String),
    /// The configuration is not usable.
    #[error("configuration: {0}")]
    Config(String),
    /// TLS could not be set up.
    #[error("tls: {0}")]
    Tls(String),
    /// A turn runs in this conversation already.
    #[error("a turn is already running in this conversation")]
    Busy,
    /// This message cannot go to this model; nothing left.
    #[error("not for this model: {0}")]
    NotForThisModel(String),
    /// Nobody chose to send exactly this text to a model that leaves the device; nothing left.
    #[error("sending to a remote model needs the person's visible choice for exactly this text")]
    RemoteNotChosen,
    /// The guard did not give its word; nothing left.
    #[error("the guard did not admit this model: {0}")]
    NotAdmitted(String),
    /// The messenger has closed, or did not answer.
    #[error("messenger: {0}")]
    Host(String),
}

impl From<AiError> for fs_messenger_core::Error {
    fn from(e: AiError) -> Self {
        match e {
            AiError::NotAdmitted(_) => Self::GuardRefused(e.to_string()),
            AiError::NotForThisModel(_) => Self::Invalid(e.to_string()),
            other => Self::Provider(other.to_string()),
        }
    }
}
