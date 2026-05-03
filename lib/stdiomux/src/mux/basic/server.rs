use std::{
    collections::{HashMap, hash_map},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Stream, future::poll_fn};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    select,
    sync::mpsc,
};

use crate::{
    frame::{ReadFrameError, WriteFrameError, simple::SimpleMuxFrame, tokio::FrameReader},
    mux::{
        ByteStream, ByteStreamService,
        basic::{client, drive_user_provided_stream},
    },
    utils::{AnnounceError, HandshakeError, exchange_handshake, make_hello_with_crate_version},
};

pub(crate) const HELLO: &[u8] = make_hello_with_crate_version!("basic mux server");

/// Errors that the [`BasicMuxServer`] may encounter.
#[derive(Debug, thiserror::Error)]
pub enum Error<E> {
    #[error("Error reading frame: {0}")]
    Rx(#[from] ReadFrameError<SimpleMuxFrame>),
    #[error("Error writing frame: {0}")]
    Tx(#[from] WriteFrameError<SimpleMuxFrame>),
    #[error("User-provided service signaled error condition")]
    Service(E),
}

/// A dead simple mux server exposing a [`crate::mux::ByteStreamService`].
///
/// WARNING: This server does unbounded buffering of requests and responses! No backpressure
/// mechanisms are implemented whatsoever! It's not suitable for handling large amounts of data!
pub struct BasicMuxServer<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    r: R,
    w: W,
}

impl<R, W> BasicMuxServer<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    /// Open a [`BasicMuxServer`].
    pub async fn open(mut r: R, mut w: W) -> Result<Self, HandshakeError> {
        exchange_handshake(&mut r, &mut w, HELLO, client::HELLO).await?;
        Ok(Self { r, w })
    }

    /// Execute the opened [`BasicMuxServer`] with the given [`ByteStreamService`]
    pub async fn run_with<S>(self, s: S) -> Result<(), Arc<Error<S::Error>>>
    where
        S: ByteStreamService<RequestStream, ByteStream> + Send + Clone + 'static,
        S::Future: Send,
        S::Error: Sync + Send + 'static,
    {
        // create shared objects
        let error = AnnounceError::new();
        let (txq_tx, txq_rx) = mpsc::unbounded_channel();

        // create parallel rx and tx drivers
        let rx_task = drive_rx::<S::Error>(self.r, |id, req| {
            let error = error.clone();

            let fut = drive_request(s.clone(), id, req, txq_tx.clone());
            tokio::spawn(async move {
                error.announce_result(fut.await).ok();
            });
        });
        let tx_task = super::drive_tx(self.w, txq_rx);

        // drive it
        let r = select! {
            r = rx_task => r,
            r = tx_task => r.map_err(Error::Tx),
        };
        error.announce_result(r)?;

        Ok(())
    }
}

async fn drive_request<S>(
    mut s: S,
    id: u16,
    rxq: RequestStream,
    txq: mpsc::UnboundedSender<(u16, Bytes)>,
) -> Result<(), Error<S::Error>>
where
    S: ByteStreamService<RequestStream, ByteStream>,
{
    poll_fn(|cx| s.poll_ready(cx))
        .await
        .map_err(Error::Service)?;
    let res = s.call(rxq).await.map_err(Error::Service)?;

    drive_user_provided_stream(res, id, txq).await;
    Ok(())
}

/// server rx driver task.
///
/// `handle_new_connection` is called on new connections.
async fn drive_rx<E>(
    r: impl AsyncRead + Unpin,
    mut handle_new_connection: impl FnMut(u16, RequestStream),
) -> Result<(), Error<E>> {
    let mut rx_map: HashMap<u16, mpsc::UnboundedSender<Bytes>> = HashMap::new();
    let mut r = FrameReader::new(r);
    loop {
        // read a frame
        let f = r.read_frame().await?;

        match rx_map.entry(f.channel) {
            // channel is currently being serviced
            hash_map::Entry::Occupied(occ) => {
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

            // channel is not currently being serviced
            hash_map::Entry::Vacant(vac) => {
                if f.body_len() == 0 {
                    // 0-len body means close, don't even insert
                    continue;
                }

                // try creating a new connection
                let (rxq_tx, rxq_rx) = mpsc::unbounded_channel();
                (handle_new_connection)(f.channel, RequestStream { rxq: rxq_rx });

                let Ok(()) = rxq_tx.send(f.body) else {
                    // receiver died as soon as we made it :(
                    continue;
                };

                // it's good to insert
                vac.insert(rxq_tx);
            }
        }
    }
}

pub struct RequestStream {
    rxq: mpsc::UnboundedReceiver<Bytes>,
}

impl Stream for RequestStream {
    type Item = Bytes;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rxq.poll_recv(cx)
    }
}
