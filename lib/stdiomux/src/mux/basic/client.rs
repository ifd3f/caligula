use std::{
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::Bytes;
use dashmap::DashMap;
use futures::{FutureExt, Stream, future::BoxFuture};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    select,
    sync::mpsc,
};
use tower_service::Service;

use crate::{
    frame::{ReadFrameError, WriteFrameError, simple::SimpleMuxFrame, tokio::FrameReader},
    mux::BoxByteStream,
    utils::{AnnounceError, HandshakeError, exchange_handshake, make_hello_with_crate_version},
};

pub(crate) const HELLO: &[u8] = make_hello_with_crate_version!("basic mux client");

/// Errors that the  [`BasicMuxClient`] may encounter.
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
    exchange_handshake(&mut r, &mut w, HELLO, super::server::HELLO).await?;

    // create shared objects
    let error = AnnounceError::new();
    let rx_map = Arc::new(DashMap::new());
    let (txq_tx, txq_rx) = mpsc::unbounded_channel();

    // create driver task composed of parallel rx and tx drivers
    let rx_task = drive_rx(r, rx_map.clone());
    let tx_task = super::drive_unbounded_txq_tx(w, txq_rx);
    let driver = Box::pin({
        let error = error.clone();
        async move {
            let e = select! {
                r = rx_task => r,
                r = tx_task => r.map_err(Error::Tx),
            };
            let result = error.announce_result(e);
            tracing::debug!(?result, "client driver closing");
            result
        }
    });

    let client = BasicMuxClient {
        next_channel: 0,
        txq: txq_tx,
        rx_map,
        error,
    };
    let driver = BasicMuxClientDriver {
        fut: driver,
        _phantom: PhantomData,
    };
    Ok((client, driver))
}

#[tracing::instrument(skip_all, level = "debug")]
async fn drive_rx(
    r: impl AsyncRead + Unpin,
    rx_map: Arc<DashMap<u16, mpsc::UnboundedSender<Bytes>>>,
) -> Result<(), Error> {
    let mut r = FrameReader::new(r);
    loop {
        // read a frame
        let f = r.read_frame().await?;

        let _span = tracing::trace_span!("handling frame", ?f).entered();

        let dashmap::Entry::Occupied(occ) = rx_map.entry(f.channel) else {
            // receiver is dead -- drop the frame
            tracing::warn!(?f, "got frame addressed to dead receiver");
            continue;
        };

        if f.body_len() == 0 {
            // 0-len body means close
            tracing::trace!("closing rx channel");
            occ.remove();
            continue;
        }

        tracing::trace!("sending frame to receiver");
        let Ok(()) = occ.get().send(f.body) else {
            // receiver is dead
            tracing::warn!("got frame addressed to dead receiver");
            occ.remove();
            continue;
        };
    }
}

/// A dead simple mux client exposing a [`crate::mux::ByteStreamService`].
///
/// WARNING: This client does unbounded buffering of requests and responses! No backpressure
/// mechanisms are implemented whatsoever! It's not suitable for handling large amounts of data!
///
/// To create one, call [`open()`]. See that function for more info on usage.
pub struct BasicMuxClient {
    next_channel: u16,
    error: AnnounceError<Error>,
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
    fut: BoxFuture<'static, Result<(), Arc<Error>>>,
    _phantom: PhantomData<(R, W)>,
}

impl<R, W> Future for BasicMuxClientDriver<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    type Output = Result<(), Arc<Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.fut.poll_unpin(cx)
    }
}

impl Service<BoxByteStream> for BasicMuxClient {
    type Response = ResponseStream;

    type Error = Arc<Error>;

    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.error.assert_ok()?;
        Poll::Ready(Ok(()))
    }

    #[tracing::instrument(skip_all, level = "debug")]
    fn call(&mut self, req: BoxByteStream) -> Self::Future {
        // Ensure we aren't errored
        if let Err(e) = self.error.assert_ok() {
            return std::future::ready(Err(e));
        }

        // Allocate a channel ID
        // TODO: actually check for overlapping channel numbers
        let id = self.next_channel;
        self.next_channel += self.next_channel.wrapping_add(1);

        // Add a rxq to the rx map
        let (res_tx, res_rx) = mpsc::unbounded_channel();
        self.rx_map.insert(id, res_tx);

        // Clone off the txq and send an initial 0-length frame to signal that it's being opened
        let txq = self.txq.clone();
        txq.send((id, Bytes::new())).ok();

        // Spawn the driver in the background
        tokio::spawn(super::drive_user_provided_stream(req, id, txq));
        std::future::ready(Ok(ResponseStream { rxq: res_rx }))
    }
}
