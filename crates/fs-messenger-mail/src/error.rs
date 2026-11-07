//! Errors of the mail provider.
//!
//! Pattern: none (plain error enum via `thiserror`). Every variant carries text from a transport or a
//! parser, never a secret: passwords are only held in `Password`, which cannot be printed.

/// Everything that can go wrong in this crate.
#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// TLS could not be set up.
    #[error("tls: {0}")]
    Tls(String),
    /// The IMAP server or connection failed.
    #[error("imap: {0}")]
    Imap(String),
    /// The JMAP server or connection failed.
    #[error("jmap: {0}")]
    Jmap(String),
    /// The SMTP server or connection failed.
    #[error("smtp: {0}")]
    Smtp(String),
    /// A message could not be built.
    #[error("mime: {0}")]
    Mime(String),
    /// OpenPGP failed.
    #[error("openpgp: {0}")]
    Pgp(String),
    /// Key storage failed, or holds something unreadable.
    #[error("keys: {0}")]
    Keys(String),
    /// The object store failed.
    #[error("store: {0}")]
    Store(#[from] fs_objects::Error),
    /// JSON could not be read or written.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<MailError> for fs_messenger_core::Error {
    fn from(e: MailError) -> Self {
        match e {
            MailError::Store(e) => Self::Store(e),
            MailError::Json(e) => Self::Json(e),
            other => Self::Provider(other.to_string()),
        }
    }
}

impl From<pgp::errors::Error> for MailError {
    fn from(e: pgp::errors::Error) -> Self {
        Self::Pgp(e.to_string())
    }
}

/// Result inside this crate.
pub(crate) type Result<T, E = MailError> = std::result::Result<T, E>;
