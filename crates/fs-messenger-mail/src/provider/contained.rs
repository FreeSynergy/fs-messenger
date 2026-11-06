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

#[cfg(test)]
mod tests {
    use super::*;

    /// `read_cut_off` and `learn_contained` run through this. No input is known that makes `cut_off`
    /// panic, so what is tested is the path, not that function.
    #[tokio::test]
    async fn a_panic_becomes_the_fallback() {
        let answer = contained(|| -> u8 { panic!("a bug in a parser") }, || 7).await;
        assert_eq!(answer, 7);
        assert_eq!(contained(|| 1, || 7).await, 1);
    }
}
