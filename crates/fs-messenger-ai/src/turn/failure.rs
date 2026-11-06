//! [`TurnFailure`]: how a turn ended that did not end as it should.
//!
//! Pattern: none (plain error enum via `thiserror`). Built only from status codes, short names and our own
//! words, never from a server's text, so neither the key nor anything else a server echoes can pass
//! through here.

/// Why a turn ended as `failed`. The text so far is kept.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TurnFailure {
    /// A person, or the guard, stopped it.
    #[error("stopped")]
    Stopped,
    /// The server answered with an error status.
    #[error("the server answered {status} ({kind})")]
    Http {
        /// The status code.
        status: u16,
        /// `error.type`, as a short safe name.
        kind: String,
    },
    /// The server named a wait over the cap.
    #[error("rate limited; the server asks to wait {wait_seconds} seconds")]
    RateLimited {
        /// What it named.
        wait_seconds: u64,
    },
    /// The server tried to send us elsewhere. Not followed.
    #[error("refused a redirect ({0})")]
    Redirected(u16),
    /// A success that is no event stream.
    #[error("the answer is no event stream")]
    WrongContentType,
    /// No connection, or no TLS with a certificate that counts.
    #[error("could not connect")]
    Connection,
    /// Nothing moved for too long.
    #[error("nothing arrived for too long")]
    IdleTimeout,
    /// The turn took too long.
    #[error("the turn took too long")]
    TurnTimeout,
    /// More bytes than a turn may take.
    #[error("the answer is over the limit")]
    ResponseTooLarge,
    /// The request would be over the limit; nothing left.
    #[error("the request is over the limit")]
    RequestTooLarge,
    /// A line is not UTF-8.
    #[error("the stream is not UTF-8")]
    InvalidUtf8,
    /// The stream is not what its format says.
    #[error("the stream is malformed: {0}")]
    Malformed(String),
    /// The stream repeated what it can only say once.
    #[error("the stream repeated itself: {0}")]
    Repeated(String),
    /// The connection or the body ended before the stream did.
    #[error("the stream broke off")]
    BrokeOff,
    /// The stream carried an error.
    #[error("the server reported {0}")]
    ServerError(String),
    /// The server ended the answer early: cut off, refused, or for a reason not known here.
    #[error("the answer ended early: {0}")]
    Cut(String),
    /// The guard refused between the send and the first byte.
    #[error("the guard did not admit this model")]
    NotAdmitted,
    /// The core did not record the person's message in time, so nothing was sent.
    #[error("the person's message was not recorded")]
    NotRecorded,
    /// The answer could not be stored. Its text stays only in the live view.
    #[error("the answer could not be stored")]
    NotStored,
}
