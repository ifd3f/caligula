use std::{
    collections::HashMap,
    error::Error,
    io::{BufRead, Read},
    time::Instant,
};

use bytes::{Buf, Bytes};
use tokio::sync::oneshot;

use super::RecvBytes;
use crate::util::io_graph::{
    GraphContext, Junction, SendBytes, Worker,
    junction::{JunctionId, TransferStat},
    worker_fn,
};

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
    fn into_output_future(
        self: Box<Self>,
    ) -> (
        impl Future<Output = Self::Output>,
        Box<dyn Worker<Output = (), Error = Self::Error>>,
    )
    where
        Self: 'static,
    {
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
        f: impl FnOnce(Self::Output) -> T + Send + 'static,
    ) -> Box<dyn Worker<Output = T, Error = Self::Error>>
    where
        T: Send + 'static,
        Self: 'static,
    {
        worker_fn(move |ctx: &GraphContext| self.run(ctx).map(f))
    }

    /// Maps the error type.
    fn map_err<E>(
        self: Box<Self>,
        f: impl FnOnce(Self::Error) -> E + Send + 'static,
    ) -> Box<dyn Worker<Output = Self::Output, Error = E>>
    where
        E: Send + Error + 'static,
        Self: 'static,
    {
        worker_fn(move |ctx: &GraphContext| self.run(ctx).map_err(f))
    }
}

impl<W: Worker + ?Sized> WorkerExt for W {}

pub trait SendBytesExt: SendBytes {
    /// Wrap this object in a new [SendBytes] that logs transfers to the given
    /// junction.
    fn junctionize_send(self, junction: Junction) -> SendJunction<Self>
    where
        Self: Sized,
    {
        SendJunction {
            send: self,
            junction,
        }
    }
}

impl<T: SendBytes> SendBytesExt for T {}

pub trait RecvBytesExt: RecvBytes {
    /// Wrap this object in a new [RecvBytes] that logs transfers to the given
    /// junction.
    fn junctionize_recv(self, junction: Junction) -> RecvJunction<Self>
    where
        Self: Sized,
    {
        RecvJunction {
            recv: self,
            junction,
        }
    }
}

impl<T: RecvBytes> RecvBytesExt for T {}
/// A [`RecvBytes`] that logs to a [`Junction`].
#[must_use]
pub struct RecvJunction<Rx: RecvBytes> {
    recv: Rx,
    junction: Junction,
}

impl<Rx: RecvBytes> RecvJunction<Rx> {
    #[expect(unused)]
    pub fn junction(&self) -> &Junction {
        &self.junction
    }
}

impl<Rx: RecvBytes> RecvBytes for RecvJunction<Rx> {
    fn recv(&mut self) -> std::io::Result<Option<bytes::Bytes>> {
        let start = Instant::now();
        let result = self.recv.recv();
        let end = Instant::now();

        let len = match &result {
            Ok(Some(b)) => b.len() as u64,
            _ => 0,
        };

        self.junction.log(TransferStat::new(start, end, len));

        result
    }
}

/// A [`SendBytes`] that logs to a [`Junction`].
#[must_use]
pub struct SendJunction<Tx: SendBytes> {
    send: Tx,
    junction: Junction,
}

impl<Tx: SendBytes> SendJunction<Tx> {
    pub fn new(send: Tx, junction: Junction) -> Self {
        Self { send, junction }
    }

    #[expect(unused)]
    pub fn junction(&self) -> &Junction {
        &self.junction
    }
}

impl<Tx: SendBytes> SendBytes for SendJunction<Tx> {
    fn send(&mut self, bytes: bytes::Bytes) -> std::io::Result<()> {
        let len = bytes.len();
        let start = Instant::now();
        let result = self.send.send(bytes);
        let end = Instant::now();

        self.junction.log(TransferStat::new(
            start,
            end,
            result.as_ref().map(|_| len as u64).unwrap_or(0),
        ));

        result
    }

    fn close(self) -> std::io::Result<()> {
        self.send.close()
    }
}

/// Helper for accumulating and grouping junction statistics output.
#[derive(Default, Clone)]
pub struct JunctionAccumulator {
    junctions: HashMap<JunctionId, Vec<TransferStat>>,
}

impl JunctionAccumulator {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn stats(&self, id: JunctionId) -> &[TransferStat] {
        self.junctions
            .get(&id)
            .map(|x| x.as_ref())
            .unwrap_or_default()
    }

    pub fn accumulate(
        &mut self,
        transfers: impl IntoIterator<Item = (JunctionId, Vec<TransferStat>)>,
    ) {
        for (id, transfers) in transfers {
            self.junctions
                .entry(id)
                .or_default()
                .extend_from_slice(&transfers);
        }
    }

    pub fn clear(&mut self) {
        self.junctions.clear();
    }
}
