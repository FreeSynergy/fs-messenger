//! Errors of the Matrix provider.
//!
//! Pattern: none (plain error enum via `thiserror`). Every variant carries text from a transport, or
//! a parser's position, never a secret: passwords, tokens and keys are only held where they cannot be
//! printed, and a parser's own message is not used, because it can quote what it read.

/// Everything that can go wrong in this crate.
#[derive(Debug, thiserror::Error)]
pub enum MatrixError {
    /// The configuration cannot work, for example a plain `http://` homeserver that is not loopback.
    #[error("config: {0}")]
    Config(String),
    /// No stored session: a password login is needed.
    #[error("no stored session for this account; log in with a password")]
    NoSession,
    /// The SDK or the homeserver failed.
    #[error("matrix: {0}")]
    Sdk(String),
    /// The homeserver refused with `M_FORBIDDEN`: for a read of one room that does not pass by
    /// retrying (banned, forgotten, never a member). Kept apart by the error's kind, not its text.
    #[error("matrix: {0}")]
    Forbidden(String),
    /// A persistent store of the SDK failed.
    #[error("store: {0}")]
    Store(#[from] StoreFail),
    /// A cursor this provider did not write.
    #[error("cursor: {0}")]
    Cursor(String),
}

/// `M_FORBIDDEN` with HTTP 403, both: the specification pairs them, and a 5xx that borrows the code
/// is a server in trouble, not a verdict.
fn is_forbidden(e: Option<&matrix_sdk::ruma::api::error::Error>) -> bool {
    use matrix_sdk::ruma::api::error::ErrorKind;
    e.is_some_and(|e| {
        e.status_code.as_u16() == 403 && matches!(e.error_kind(), Some(ErrorKind::Forbidden { .. }))
    })
}

impl From<matrix_sdk::Error> for MatrixError {
    fn from(e: matrix_sdk::Error) -> Self {
        if is_forbidden(e.as_client_api_error()) {
            return Self::Forbidden(e.to_string());
        }
        Self::Sdk(e.to_string())
    }
}

impl From<matrix_sdk::HttpError> for MatrixError {
    fn from(e: matrix_sdk::HttpError) -> Self {
        if is_forbidden(e.as_client_api_error()) {
            return Self::Forbidden(e.to_string());
        }
        Self::Sdk(e.to_string())
    }
}

impl From<MatrixError> for fs_messenger_core::Error {
    fn from(e: MatrixError) -> Self {
        Self::Provider(e.to_string())
    }
}

/// What went wrong below the SDK's stores. Holds a description, never a value that was stored.
#[derive(Debug, thiserror::Error)]
pub enum StoreFail {
    /// The store is not encrypted, and refused for it (ADR 0018, 0024).
    #[error("the store is not encrypted; the Matrix stores do not open on it (ADR 0024)")]
    NotEncrypted,
    /// `fs-store` failed.
    #[error("fs-store: {0}")]
    Backend(#[from] fs_store::Error),
    /// A record could not be read or written. The value is not named.
    #[error("record: {0}")]
    Record(String),
}

impl From<serde_json::Error> for StoreFail {
    fn from(e: serde_json::Error) -> Self {
        Self::Record(where_json_failed(&e))
    }
}

/// Where a parser failed. Never its message: `invalid type: string "..."` quotes the value, and the
/// value can be key material.
pub(crate) fn where_json_failed(e: &serde_json::Error) -> String {
    format!(
        "{:?} at line {} column {}",
        e.classify(),
        e.line(),
        e.column()
    )
}

impl From<std::convert::Infallible> for StoreFail {
    fn from(e: std::convert::Infallible) -> Self {
        match e {}
    }
}

impl From<matrix_sdk::store::StoreError> for StoreFail {
    fn from(e: matrix_sdk::store::StoreError) -> Self {
        Self::Record(e.to_string())
    }
}

impl From<StoreFail> for matrix_sdk::store::StoreError {
    fn from(e: StoreFail) -> Self {
        Self::backend(e)
    }
}

impl From<StoreFail> for matrix_sdk_base::crypto::CryptoStoreError {
    fn from(e: StoreFail) -> Self {
        Self::backend(e)
    }
}

/// Result inside this crate.
pub(crate) type Result<T, E = MatrixError> = std::result::Result<T, E>;
