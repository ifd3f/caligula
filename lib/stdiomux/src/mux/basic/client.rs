use std::{
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::Bytes;
use dashmap::DashMap;
use futures::{FutureExt, Stream, StreamExt, future::BoxFuture};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    select,
    sync::mpsc,
};
use tower_service::Service;

use crate::{
    frame::{
        ReadFrameError, WriteFrameError,
        simple::SimpleMuxFrame,
        tokio::{FrameReader, FrameWriter},
    },
    mux::{ByteStream, basic::server},
    utils::{HandshakeError, exchange_handshake, make_hello_with_crate_version},
};

pub(crate) const HELLO: &[u8] = make_hello_with_crate_version!("basic mux client");

/// Errors that the [`BasicMuxClient`] may encounter.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error reading frame: {0}")]
    Rx(#[from] ReadFrameError<SimpleMuxFrame>),
    #[error("Error writing frame: {0}")]
    Tx(#[from] WriteFrameError<SimpleMuxFrame>),
}

/// Open a [`BasicMuxClient`]. Returns the client and its associated [`BasicMuxClientDriver`],
/// which must be polled in the background to drive the reading and writing.
///
/// WARNING: This client does unbounded buffering of requests and responses! No backpressure
/// mechanisms are implemented whatsoever! It's not suitable for handling large amounts of data!
pub async fn open<R, W>(
    mut r: R,
    mut w: W,
) -> Result<(BasicMuxClient, BasicMuxClientDriver<R, W>), HandshakeError>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    exchange_handshake(&mut r, &mut w, HELLO, server::HELLO).await?;

    let rx_map = Arc::new(DashMap::new());
    let (txq_tx, txq_rx) = mpsc::unbounded_channel();

    let rx_task = drive_rx(r, rx_map.clone());
    let tx_task = drive_tx(w, txq_rx);
    let driver = Box::pin(async move {
        select! {
            r = rx_task => r,
            r = tx_task => r,
        }
    });

    let client = BasicMuxClient {
        next_channel: 0,
        txq: txq_tx,
        rx_map,
    };
    let driver = BasicMuxClientDriver {
        fut: driver,
        _phantom: PhantomData,
    };
    Ok((client, driver))
}

async fn drive_rx(
    r: impl AsyncRead + Unpin,
    rx_map: Arc<DashMap<u16, mpsc::UnboundedSender<Bytes>>>,
) -> Result<(), Error> {
    let mut r = FrameReader::new(r);
    loop {
        // read a frame
        let f = r.read_frame().await?;

        let dashmap::Entry::Occupied(occ) = rx_map.entry(f.channel) else {
            // receiver is dead -- drop the frame
            continue;
        };

        if f.body_len() == 0 {
            // 0-len body means close
            occ.remove();
            continue;
        }

        let Ok(()) = occ.get().send(f.body) else {
            // receiver is dead
            occ.remove();
            continue;
        };
    }
}

async fn drive_tx(
    w: impl AsyncWrite + Unpin,
    mut txq: mpsc::UnboundedReceiver<(u16, Bytes)>,
) -> Result<(), Error> {
    let mut w = FrameWriter::new(w);
    while let Some((ch, bs)) = txq.recv().await {
        w.write_frame(SimpleMuxFrame {
            channel: ch,
            body: bs,
        })
        .await?;
    }
    Ok(())
}

/// A dead simple mux client exposing a [`crate::mux::ByteStreamService`].
///
/// WARNING: This client does unbounded buffering of requests and responses! No backpressure
/// mechanisms are implemented whatsoever! It's not suitable for handling large amounts of data!
///
/// To create one, call [`open()`]. See that function for more info on usage.
pub struct BasicMuxClient {
    next_channel: u16,
    txq: mpsc::UnboundedSender<(u16, Bytes)>,
    rx_map: Arc<DashMap<u16, mpsc::UnboundedSender<Bytes>>>,
}

/// A response from a request to a [BasicMuxClient].
pub struct ResponseStream {
    rxq: mpsc::UnboundedReceiver<Bytes>,
}

impl Stream for ResponseStream {
    type Item = Bytes;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rxq.poll_recv(cx)
    }
}

/// This is a [`Future`] that must be constantly polled, likely in a background task, in order to
/// drive its associated [`BasicMuxClient`]'s multiplexing.
pub struct BasicMuxClientDriver<R, W> {
    /// This is just a newtype wrapper around this future :)
    fut: BoxFuture<'static, Result<(), Error>>,
    _phantom: PhantomData<(R, W)>,
}

impl<R, W> Future for BasicMuxClientDriver<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.fut.poll_unpin(cx)
    }
}

impl Service<ByteStream> for BasicMuxClient {
    type Response = ResponseStream;

    type Error = Error;

    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: ByteStream) -> Self::Future {
        let id = self.next_channel;
        self.next_channel += 1;

        let (res_tx, res_rx) = mpsc::unbounded_channel();
        self.rx_map.insert(id, res_tx);
        let txq = self.txq.clone();
        tokio::spawn(async move {
            while let Some(req) = req.next().await {
                if req.len() == 0 {
                    continue;
                }
                let Ok(()) = txq.send((id, req)) else {
                    break;
                };
            }
            txq.send((id, Bytes::new())).ok();
        });
        std::future::ready(Ok(ResponseStream { rxq: res_rx }))
    }
}
