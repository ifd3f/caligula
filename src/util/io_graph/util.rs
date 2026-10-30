use std::{
    error::Error,
    io::{BufRead, Read},
    sync::{Arc, atomic},
    thread::JoinHandle,
};

use bytes::{Buf, Bytes};
use futures::{StreamExt, stream::FuturesUnordered};
use itertools::Itertools;
use tokio::sync::oneshot;

use super::RecvBytes;
use crate::util::io_graph::{GraphContext, Worker};

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

pub trait WorkerExt: Worker {
    /// Allows the caller to wait on the result of the worker.
    ///
    /// If the worker returns successfully, then it will be returned in the
    /// future. If the worker fails, the future will never complete.
    fn into_future(
        self: Box<Self>,
    ) -> (
        impl Future<Output = Self::Output>,
        impl Worker<Output = (), Error = Self::Error>,
    ) {
        let (tx, rx) = oneshot::channel();

        let w = self.map(|x| {
            tx.send(x).ok();
        });

        let f = async move {
            let Ok(r) = rx.await else {
                return std::future::pending().await;
            };
            r
        };

        (f, w)
    }

    /// Maps the successful result type.
    fn map<T>(
        self: Box<Self>,
        f: impl FnOnce(Self::Output) -> T + Send,
    ) -> impl Worker<Output = T, Error = Self::Error>
    where
        T: Send + 'static,
    {
        move |ctx: &GraphContext| self.run(ctx).map(f)
    }

    /// Maps the error type.
    fn map_err<E>(
        self: Box<Self>,
        f: impl FnOnce(Self::Error) -> E + Send,
    ) -> impl Worker<Output = Self::Output, Error = E>
    where
        E: Send + Error + 'static,
    {
        move |ctx: &GraphContext| self.run(ctx).map_err(f)
    }
}

impl<W: Worker> WorkerExt for W {}

