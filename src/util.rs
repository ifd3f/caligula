use std::{
    env,
    io::Read,
    path::PathBuf,
    pin::pin,
    process,
    sync::{Arc, atomic::AtomicU64},
    task::{Context, Poll},
    time::SystemTime,
};

use bytes::{Buf, Bytes};
use futures::FutureExt;
use pin_project::pin_project;
use tokio::{
    fs::DirBuilder,
    io::AsyncRead,
    sync::{broadcast, mpsc},
};

/// Create the directory to shove invocation-specific data into, like log files and sockets.
pub async fn ensure_state_dir() -> Result<PathBuf, futures_io::Error> {
    let dir = env::temp_dir().join(format!(
        "caligula-{}-{}",
        process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    ));

    DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&dir)
        .await?;

    Ok(dir)
}

/// Wraps an [AsyncRead] and counts how many bytes we've read in total, without
/// making any system calls.
#[pin_project]
pub struct AsyncCountRead<R: AsyncRead> {
    #[pin]
    r: R,
    count: Arc<AtomicU64>,
}

impl<R: AsyncRead> AsyncCountRead<R> {
    #[inline(always)]
    pub fn new(r: R, count: Arc<AtomicU64>) -> Self {
        Self { r, count }
    }

    #[inline(always)]
    pub fn count(&self) -> &Arc<AtomicU64> {
        &self.count
    }

    #[inline(always)]
    pub fn get_ref(&self) -> &R {
        &self.r
    }

    #[inline(always)]
    pub fn into_inner(self) -> R {
        self.r
    }
}

impl<R: AsyncRead> AsyncRead for AsyncCountRead<R> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.project();
        let before = buf.filled().len() as u64;
        let out = this.r.poll_read(cx, buf);
        let after = buf.filled().len() as u64;
        this.count
            .fetch_add(after - before, std::sync::atomic::Ordering::Relaxed);
        out
    }
}

pub trait ByteChannel {
    fn is_closed(&self) -> bool;

    /// Attempt to receive without blocking.
    fn try_recv(&mut self) -> Option<Bytes>;

    /// Polls to receive the next message on this channel.
    ///
    /// This method returns:
    ///
    ///  * `Poll::Pending` if no messages are available but the channel is not
    ///    closed, or if a spurious failure happens.
    ///  * `Poll::Ready(Some(message))` if a message is available.
    ///  * `Poll::Ready(None)` if the channel has been closed and all messages
    ///    sent before it was closed have been received.
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Bytes>>;

    /// Blocking receive to call outside of asynchronous contexts.
    ///
    /// This method returns None if the channel has been closed and there are no
    /// remaining messages in the channel's buffer.
    fn blocking_recv(&mut self) -> Option<Bytes>;
}

impl ByteChannel for mpsc::Receiver<Bytes> {
    #[inline(always)]
    fn is_closed(&self) -> bool {
        mpsc::Receiver::is_closed(self)
    }

    #[inline(always)]
    fn try_recv(&mut self) -> Option<Bytes> {
        mpsc::Receiver::try_recv(self).ok()
    }

    #[inline(always)]
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Bytes>> {
        mpsc::Receiver::poll_recv(self, cx)
    }

    #[inline(always)]
    fn blocking_recv(&mut self) -> Option<Bytes> {
        mpsc::Receiver::blocking_recv(self)
    }
}

impl ByteChannel for broadcast::Receiver<Bytes> {
    #[inline(always)]
    fn is_closed(&self) -> bool {
        broadcast::Receiver::is_closed(self)
    }

    #[inline(always)]
    fn try_recv(&mut self) -> Option<Bytes> {
        broadcast::Receiver::try_recv(self).ok()
    }

    #[inline(always)]
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Bytes>> {
        let mut x = pin! {broadcast::Receiver::recv(self)};

        x.poll_unpin(cx)
            .map(|r: Result<Bytes, broadcast::error::RecvError>| r.ok())
    }

    #[inline(always)]
    fn blocking_recv(&mut self) -> Option<Bytes> {
        broadcast::Receiver::blocking_recv(self).ok()
    }
}

pub struct ByteChannelReader<C: ByteChannel> {
    /// The channel we're reading from
    c: C,
    /// Bytes that could not fit into the caller's buffer last time we tried to
    /// read them in.
    unfinished: Bytes,
}

impl<C: ByteChannel> ByteChannelReader<C> {
    pub fn new(c: C) -> Self {
        Self {
            c,
            unfinished: Bytes::new(),
        }
    }

    /// Read as much as possible into the provided buffer without blocking.
    fn fill_without_blocking(&mut self, buf: &mut tokio::io::ReadBuf<'_>) {
        loop {
            if buf.remaining() == 0 {
                break;
            }

            if self.unfinished.is_empty() {
                let Some(b) = self.c.try_recv() else {
                    break;
                };
                self.unfinished = b;
            }

            if buf.remaining() >= self.unfinished.len() {
                buf.put_slice(&self.unfinished);
                self.unfinished.clear();
            } else {
                let bytes_to_copy = buf.remaining();
                buf.put_slice(&self.unfinished);
                self.unfinished.advance(bytes_to_copy);
            }
        }
    }
}

impl<C: ByteChannel> From<C> for ByteChannelReader<C> {
    fn from(c: C) -> Self {
        Self::new(c)
    }
}

impl<C: ByteChannel> Read for ByteChannelReader<C> {
    /// Read into the buffer (blockingly).
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Guarantee that the rest of the code won't have to deal with this edge case.
        if buf.len() == 0 {
            return Ok(0);
        }

        let mut buf = tokio::io::ReadBuf::uninit(buf);

        loop {
            // Attempt to fill the output buffer without blocking.
            self.fill_without_blocking(buf);

            if !buf.filled().is_empty() {
                break;
            }

            // If we failed to fill the buffer, try blocking on more bytes.
            if let Some(more) = self.c.blocking_recv() {
                // We successfully received more bytes. Loop around and keep going.
                self.unfinished = more;
            } else {
                // blocking_recv() returning None implies that the channel is closed.
                // Therefore, signal that we're done.
                return Ok(0);
            };
        }

        Ok(buf.filled().len())
    }
}

impl<C: ByteChannel + Unpin> AsyncRead for ByteChannelReader<C> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::task::Poll;

        // Guarantee that the rest of the code won't have to deal with this edge case.
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        loop {
            // Attempt to fill the buffer without blocking.
            self.fill_without_blocking(buf);

            if !buf.filled().is_empty() {
                break;
            }

            // If we failed to fill the buffer, try polling for more bytes.
            match self.c.poll_recv(cx) {
                Poll::Ready(Some(r)) => {
                    // We successfully received more bytes. Loop around and keep going.
                    self.unfinished = r;
                }
                Poll::Ready(None) => {
                    // The channel was closed.
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending => {
                    // We need to wait a bit more.
                    return Poll::Pending;
                }
            }
        }

        // If we filled the buffer with a nonzero number of bytes, signal Ready.
        Poll::Ready(Ok(()))
    }
}
