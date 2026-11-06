//! [`IdleStream`]: a stream that fails when nothing moves for too long.
//!
//! Pattern: Decorator. A total timeout would punish a large mail on a slow line, and let a sender stall an
//! account by making one large enough. What counts is that bytes keep moving. It sits under TLS, so a
//! silent handshake counts as well. A server that trickles forever is not stopped by this (`DESIGN.md`).

use std::future::Future as _;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{Instant, Sleep};

/// `inner`, failing with `TimedOut` after `limit` without progress.
#[derive(Debug)]
pub(super) struct IdleStream<S> {
    inner: S,
    limit: Duration,
    deadline: Pin<Box<Sleep>>,
}

impl<S> IdleStream<S> {
    pub(super) fn new(inner: S, limit: Duration) -> Self {
        Self {
            inner,
            limit,
            deadline: Box::pin(tokio::time::sleep(limit)),
        }
    }

    fn watched<T>(
        &mut self,
        cx: &mut Context<'_>,
        poll: Poll<io::Result<T>>,
    ) -> Poll<io::Result<T>> {
        match poll {
            Poll::Ready(done) => {
                self.deadline.as_mut().reset(Instant::now() + self.limit);
                Poll::Ready(done)
            }
            Poll::Pending => match self.deadline.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "no data moved within the idle timeout",
                ))),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for IdleStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.inner).poll_read(cx, buf);
        this.watched(cx, poll)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for IdleStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.inner).poll_write(cx, buf);
        this.watched(cx, poll)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.inner).poll_flush(cx);
        this.watched(cx, poll)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
