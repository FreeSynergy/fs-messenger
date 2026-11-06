//! Work on what a stranger wrote runs where a panic cannot take the fetch down.
//!
//! Pattern: Decorator over `spawn_blocking`: parsers and rPGP read input from anyone. rPGP 0.21 panics on
//! an unknown AEAD byte (`DESIGN.md`, to report upstream), and a panic here becomes the fallback.

/// `work` on a blocking thread, or `fallback` if it panicked.
pub(super) async fn contained<T, W, F>(work: W, fallback: F) -> T
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    F: FnOnce() -> T,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(done) => done,
        Err(e) => {
            tracing::warn!(error = %e, "reading a mail panicked; going on without that step");
            fallback()
        }
    }
}
