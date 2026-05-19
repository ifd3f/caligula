use std::{cell::RefCell, convert::Infallible, fmt::Debug, future::poll_fn, pin::pin, rc::Rc, sync::Arc, task::{Context, Poll}};

use bytes::{Bytes, BytesMut};
use futures::{
    FutureExt, Stream, StreamExt as _,
    stream::{self, BoxStream, FusedStream, LocalBoxStream, Peekable}, task::AtomicWaker,
};
use tokio::{
    io::{AsyncBufRead, AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, BufReader, BufWriter},
    select,
    sync::{SetOnce, mpsc},
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{Instrument as _, info_span, trace_span};

use crate::stdiomux::{BytestreamService, channel_map::{ChannelMap, ChannelMapRxHalf}};


async fn drive_transport_steady_state<R,W>(
    tx_fut: &mut (impl Future<Output = (u16, Bytes)> + Unpin),
    rx_fut: &mut (impl Future<Output = std::io::Result<(u16, Bytes)>> + Unpin),
    server: &impl BytestreamService<Error = Infallible>,
    rx_map: &mut ChannelMapRxHalf,
    mut tx: W) 
-> std::io::Result<()>
where
    R: AsyncRead + Unpin ,
    W: AsyncWrite + Unpin,
{
    select! {
        (channel, bytes) = tx_fut => send_msg(tx, (channel, bytes)).await,
        rx = rx_fut => {
            let Ok(()) = rx_map.handle_rx(rx?) else {

            };
        }
    }
    Ok(())
}

async fn handle_tx(mut tx: impl AsyncWrite + Unpin, (channel, bytes) : (u16, Bytes)) {

    let mut rx = BufReader::with_capacity(4096, rx);
    let mut tx = BufWriter::with_capacity(4096, tx);

}

/// Send a single message over the wire.
async fn send_msg(mut tx: impl AsyncWrite + Unpin, (channel, bytes) : (u16, Bytes)) -> Result<(), std::io::Error> {
    tx.write_u16(channel).await?;
    tx.write_u32(bytes.len().try_into().unwrap()).await?;
    tx.write_all(&bytes).await?;
    tx.flush().await?;
    Ok(())
}

/// Receive a single message over the wire.
async fn recv_msg(mut rx: impl AsyncRead + Unpin) -> Result<(u16, Option<Bytes>) ,std::io::Error> {
  let channel = rx.read_u16().await?;
  let len = usize::try_from(rx.read_u32().await?).unwrap();
                if len == 0 {
                    // EOF sentinel
                    tracing::trace!("got EOF");
                    return Ok((channel, None));
                }

                tracing::trace!(?len, "got message");

                let mut msg = BytesMut::with_capacity(len);
                unsafe {
                    msg.set_len(len);
                }
                rx.read_exact(&mut msg)
                    .instrument(trace_span!("read_exact"))
                    .await?;

                Ok((channel, Some(msg.freeze())))
}


pub async fn drive_tx<W>(tx: W, channels: &RefCell<ChannelMap>) -> Result<(), Arc<std::io::Error>>
where
    W: AsyncWrite + Unpin + 'static,
{
    // wrap with a buffer big enough to wrap the header and a reasonably-sized
    // message
    let mut tx = BufWriter::with_capacity(4096, tx);

    let v = channels
        .borrow()
        .channel_iter()
        .map(|(id, cs)| );

    // pull from the request stream
    while let Some(bytes) = s.next().await {
        // don't send 0 bytes because that's a EOF sentinel
        if bytes.is_empty() {
            continue;
        }

        tracing::trace!(len = ?bytes.len(), "sending message");

        // length-framing
        async {
            fun_name(&mut tx, bytes).await?;
            Ok::<(), std::io::Error>(())
        }
        .instrument(trace_span!("write_msg"))
        .await?;
    }

    tracing::trace!("sending EOF");

    // out of requests -- write EOF sentinel
    tx.write_u32(0).await?;
    tx.flush().await?;
    Ok(())
}

pub fn drive_rx<R>(
    channels: &RefCell<ChannelState>,
    rx: R,
) -> impl Stream<Item = Result<Bytes, Arc<std::io::Error>>>
where
    R: AsyncRead + Unpin + 'static,
{
    // wrap with a buffer big enough to wrap the header and a reasonably-sized
    // message
    let rx = BufReader::with_capacity(4096, rx);

    stream::unfold(Some(rx), |st| {
        async move {
            let mut rx = st?;

            let recv = async {
                let len = usize::try_from(rx.read_u32().await?).unwrap();
                if len == 0 {
                    // EOF sentinel
                    tracing::trace!("got EOF");
                    return Ok(None);
                }

                tracing::trace!(?len, "got message");

                let mut msg = BytesMut::with_capacity(len);
                unsafe {
                    msg.set_len(len);
                }
                rx.read_exact(&mut msg)
                    .instrument(trace_span!("read_exact"))
                    .await?;
                Ok::<Option<Bytes>, Arc<std::io::Error>>(Some(msg.freeze()))
            };

            match recv.await {
                Ok(Some(msg)) => Some((Ok(msg), Some(rx))),
                Ok(None) => None,
                Err(err) => Some((Err(err), None)),
            }
        }
        .instrument(info_span!("rxdriver"))
    })
}

pub fn inject_err_stream<S, E>(
    stream: S,
    err_notify: Arc<SetOnce<E>>,
) -> impl Stream<Item = Result<Bytes, E>>
where
    E: Clone,
    S: Stream<Item = Result<Bytes, E>>,
{
    stream.map(move |r| match (r, err_notify.get()) {
        (_, Some(err)) => Err(err.clone()), // inject error from err_notify
        (Ok(r), None) => Ok(r),
        (Err(e), None) => {
            // inject error into err_notify
            err_notify.set(e.clone()).ok();
            Err(e)
        }
    })
}

pub async fn inject_err_fut<T, E, Fut>(fut: Fut, err_notify: Arc<SetOnce<E>>) -> Result<T, E>
where
    E: Debug + Clone,
    Fut: Future<Output = Result<T, E>>,
{
    let r = select! {
        biased;
        err = err_notify.wait() => { // inject errors from err_notify
            tracing::warn!(?err, "Quitting early due to signalled error");
            Err(err.clone())
        },
        r = fut => r,
    };

    if let Err(err) = &r {
        tracing::warn!(?err, "fut errored, sending signal");
        err_notify.set(err.clone()).ok(); // inject errors into err_notify
    }

    r
}
