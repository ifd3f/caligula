use std::{
    error::Error,
    io::{BufRead, Read},
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, Bytes};
use futures::FutureExt as _;

use super::{GraphContext, GraphHandle, IoGraph, RecvBytes};
use crate::util::io_graph::Counter;

/// Simple adapter turning a [`RecvBytes`] into a blocking [`Read`].
pub struct RecvBytesReader<Rx: RecvBytes> {
    buf: Bytes,
    rx: Rx,
}

impl<Rx: RecvBytes> RecvBytesReader<Rx> {
    #[inline]
    pub fn new(rx: Rx) -> Self {
        Self {
            buf: Bytes::new(),
            rx,
        }
    }
}

impl<Rx: RecvBytes> From<Rx> for RecvBytesReader<Rx> {
    fn from(value: Rx) -> Self {
        Self::new(value)
    }
}

impl<Rx: RecvBytes> Read for RecvBytesReader<Rx> {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if !self.buf.is_empty() {
                // there are bytes in the internal buf, send those
                match self.buf.try_copy_to_slice(buf) {
                    Ok(()) => {
                        // buf was completely filled
                        return Ok(buf.len());
                    }
                    Err(copied) => {
                        // buf was not completely filled. fine either way
                        return Ok(copied.available);
                    }
                }
            }

            // internal buf is empty, try to fill it
            // if this returns an empty slice, then we reached EOF
            if self.fill_buf()?.is_empty() {
                return Ok(0);
            }
        }
    }
}

impl<Rx: RecvBytes> BufRead for RecvBytesReader<Rx> {
    #[inline]
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        loop {
            if !self.buf.is_empty() {
                // there is data -- return the buffered data
                return Ok(&self.buf);
            }

            // there is no data buffered -- read from the rx
            let Some(r) = self.rx.recv()? else {
                // None indicates EOF
                return Ok(&[]);
            };

            self.buf = r;
            // defensively loop around in case we got an empty value
        }
    }

    #[inline]
    fn consume(&mut self, amount: usize) {
        self.buf.advance(amount);
    }
}

/// Create a simple [`IoGraph`] out of a function.
pub fn io_graph_fn<F, H>(f: F) -> IoGraphFn<F>
where
    F: FnOnce(&mut GraphContext<'_>) -> H,
    H: GraphHandle,
    H::Error: Error + Send + 'static,
{
    IoGraphFn(f)
}

pub struct IoGraphFn<F>(F);

impl<F, H> IoGraph for IoGraphFn<F>
where
    F: FnOnce(&mut GraphContext<'_>) -> H,
    H: GraphHandle,
    H::Error: Error + Send + 'static,
{
    type Error = H::Error;
    type Handle = H;
    type Output = H::Output;
    type Snapshot = H::Snapshot;

    fn spawn(self, ctx: &mut GraphContext<'_>) -> Self::Handle {
        (self.0)(ctx)
    }
}

/// Create a simple [`GraphHandle`] from:
/// - a reference to a [`Counter`] for taking snapshots from, and
/// - a [`Future`] for waiting on the final result.
pub fn counter_future_handle<C, Fut, T, E>(counter: C, future: Fut) -> CounterFutureHandle<C, Fut>
where
    C: Counter,
    E: Error + Send + 'static,
    Fut: Future<Output = Result<T, E>>,
{
    CounterFutureHandle {
        c: counter,
        fut: Box::pin(future),
    }
}

pub struct CounterFutureHandle<C, Fut> {
    c: C,
    fut: Pin<Box<Fut>>,
}

impl<C, Fut, T, E> GraphHandle for CounterFutureHandle<C, Fut>
where
    C: Counter,
    E: Error + Send + 'static,
    Fut: Future<Output = Result<T, E>> + Unpin,
{
    type Error = E;
    type Output = T;
    type Snapshot = C::Snapshot;

    fn poll_status(
        &mut self,
        cx: &mut Context<'_>,
        snapshot: &mut Self::Snapshot,
    ) -> Poll<Result<Self::Output, Self::Error>> {
        self.c.snapshot_into(snapshot);
        self.fut.poll_unpin(cx)
    }
}
