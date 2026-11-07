//! The hard ends: one call into the SDK, and one whole `send` or plan.
//!
//! Pattern: none (no pattern of the coding rules, section 2, fits; see DESIGN.md, open points). Two
//! methods that put a time limit around a future. The SDK's own limits do not make a hard end: it
//! waits as long as a server's `retry_after` says, whatever the retry limit.

use std::future::Future;

use fs_messenger_core::{Error as CoreError, Result as CoreResult};

use super::MatrixAccount;
use crate::error::MatrixError;

/// One call into the SDK may make several requests: sending shares keys first.
const REQUESTS_PER_CALL: u32 = 8;
/// A whole `send` or plan: several calls in turn (the room, its members' devices, the send itself).
const REQUESTS_PER_OPERATION: u32 = 16;

impl MatrixAccount {
    /// One call into the SDK, with a hard end. Dropping the call cancels it, so a message may have
    /// left all the same; it then counts as failed, like any lost answer.
    pub(crate) async fn bounded<T, E: Into<MatrixError>>(
        &self,
        work: impl Future<Output = Result<T, E>>,
    ) -> Result<T, MatrixError> {
        let limit = self.request_timeout * REQUESTS_PER_CALL;
        match tokio::time::timeout(limit, work).await {
            Ok(answer) => answer.map_err(Into::into),
            Err(_) => Err(MatrixError::Sdk(format!("no answer within {limit:?}"))),
        }
    }

    /// A whole `send` or plan, with one end on top of the ends of its calls: each of them may take its
    /// full time, and a room of hundreds of members makes hundreds. Passing it is an error and
    /// nothing counts as sent.
    pub(crate) async fn whole<T>(
        &self,
        work: impl Future<Output = CoreResult<T>>,
    ) -> CoreResult<T> {
        let limit = self.request_timeout * REQUESTS_PER_OPERATION;
        match tokio::time::timeout(limit, work).await {
            Ok(answer) => answer,
            Err(_) => Err(CoreError::Provider(format!(
                "matrix: not done within {limit:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn quick() -> MatrixAccount {
        let mut account = MatrixAccount::new("https://x.example", "@a:x");
        account.request_timeout = Duration::from_millis(5);
        account
    }

    #[tokio::test]
    async fn an_operation_that_never_ends_is_an_error() {
        let never = std::future::pending::<CoreResult<()>>();
        let error = quick().whole(never).await.unwrap_err();
        assert!(error.to_string().contains("not done within"), "{error}");
    }

    #[tokio::test]
    async fn an_answer_in_time_passes_as_it_is() {
        assert_eq!(quick().whole(async { Ok(7) }).await.unwrap(), 7);
    }
}
