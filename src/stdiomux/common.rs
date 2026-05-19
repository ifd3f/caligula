use std::{fmt::Debug, pin::pin};

use bytes::{Bytes, BytesMut};
use futures::{
    Stream, StreamExt as _, TryStreamExt,
    stream::{self, LocalBoxStream},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, BufReader, BufWriter},
    select,
    sync::{
        SetOnce,
        mpsc::{UnboundedReceiver, UnboundedSender},
    },
};
use tracing::{Instrument as _, debug, debug_span, info_span, trace, trace_span};

use crate::stdiomux::{BytestreamService, channel_map::ChannelMap};

/// Handle the transmission forwarding for a single channel.
///
/// This transmission queue may contain zero-byte EOF sentinels.
pub async fn drive_channel_tx(
    id: u16,
    mut s: impl Stream<Item = Bytes> + Unpin,
    txq: UnboundedSender<(u16, Bytes)>,
) {
    debug!("starting tx channel");

    // pull from the request stream
    while let Some(bs) = s.next().await {
        // don't send 0 bytes because that's a EOF sentinel
        if bs.is_empty() {
            trace!("skipping 0-byte message");
            continue;
        }

        trace!(len = ?bs.len(), "sending message");

        let Ok(()) = txq.send((id, bs)) else {
            break;
        };
    }

    debug!("sending EOF");

    // out of requests -- write EOF sentinel
    txq.send((id, Bytes::new())).ok();
}

/// Forward frames from a transmission queue into the given [AsyncWrite].
///
/// This transmission queue may contain zero-byte EOF sentinels.
#[tracing::instrument(skip_all)]
pub async fn drive_tx<W>(
    mut txq: UnboundedReceiver<(u16, Bytes)>,
    tx: W,
) -> Result<(), std::io::Error>
where
    W: AsyncWrite + Unpin + 'static,
{
    // wrap with a buffer big enough to wrap the header and a reasonably-sized
    // message
    let mut tx = BufWriter::with_capacity(4096, tx);

    // pull from the request stream
    while let Some((id, bs)) = txq.recv().await {
        let _span = trace_span!("send_msg", chan = id);

        trace!(len = ?bs.len(), "sending message");

        // length-framing
        write_msg(&mut tx, (id, bs)).await?;
    }

    // txq dropped
    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn drive_rx<S, E>(
    channel_map: impl AsRef<ChannelMap>,
    stream: impl Stream<Item = (u16, Option<Bytes>)>,
    svc: S,
    svc_err_handler: impl Fn(E) + Clone + 'static,
    txq: UnboundedSender<(u16, Bytes)>,
) where
    S: BytestreamService<LocalBoxStream<'static, Bytes>, Error = E>,
    S::Response: Unpin + 'static,
    E: Debug + 'static,
{
    let channel_map = channel_map.as_ref();
    let mut stream = pin!(stream.peekable());

    // entirely for logging, incremented only when stream is advanced
    let mut msg_count = 0u64;

    while let Some((id, bs)) = stream.as_mut().peek().await {
        let id = *id;

        let _span = debug_span!("handle_rx", n = msg_count, chan = id).entered();

        let Some(bs) = bs else {
            // EOF sentinel
            debug!("EOF sentinel received, closing channel's RX half and advancing stream");
            channel_map.close(id);
            stream.next().await;
            msg_count += 1;
            continue;
        };

        match channel_map.handle_rx(id, bs) {
            Ok(_) => {
                trace!("channel consumed successfully, advancing stream");

                // successfully consumed, advance the stream
                stream.next().await;
                msg_count += 1;
            }
            Err(_) => {
                debug!("new ID detected, inserting new channel");

                // no existing channel, create a new one using the service
                // TODO: handle the case where we run out of channels
                let rx = channel_map.insert_new_channel(id).unwrap();
                let res = svc.call(Box::pin(rx));

                // inject errors from the stream into the global signal
                let err_handler = svc_err_handler.clone();
                let res = res
                    .inspect(|x| trace!("response stream yields {x:?}"))
                    .map_err(err_handler)
                    .filter_map(|x| std::future::ready(x.ok()));

                // spawn task in background
                tokio::task::spawn_local(
                    drive_channel_tx(id, res, txq.clone()).instrument(debug_span!("drive_channel")),
                );

                // do NOT advance the stream or else we will drop first payload
            }
        }
    }
}

/// Convert an [AsyncRead] into a stream of frames.
pub fn rx_stream<R>(rx: R) -> impl Stream<Item = Result<(u16, Option<Bytes>), std::io::Error>>
where
    R: AsyncRead + Unpin + 'static,
{
    // wrap with a buffer big enough to wrap the header and a reasonably-sized
    // message
    let rx = BufReader::with_capacity(4096, rx);

    stream::unfold(rx, |mut rx| {
        async move { Some((read_msg(&mut rx).await, rx)) }.instrument(info_span!("rx_stream"))
    })
}

/// Send a single message over the wire.
#[tracing::instrument(skip_all, level = "trace")]
fn write_msg<'a>(
    tx: &'a mut (impl AsyncWrite + Unpin),
    (channel, bytes): (u16, Bytes),
) -> impl Future<Output = Result<(), std::io::Error>> + 'a {
    async move {
        tx.write_u16(channel).await?;
        tx.write_u32(bytes.len().try_into().unwrap()).await?;
        tx.write_all(&bytes).await?;
        tx.flush().await?;
        Ok(())
    }
}

/// Receive a single message over the wire.
#[tracing::instrument(skip_all, level = "trace")]
async fn read_msg(mut rx: impl AsyncRead + Unpin) -> Result<(u16, Option<Bytes>), std::io::Error> {
    let channel = rx.read_u16().await?;

    let len = usize::try_from(rx.read_u32().await?).unwrap();
    if len == 0 {
        // EOF sentinel
        tracing::trace!(?channel, "got EOF");
        return Ok((channel, None));
    }

    tracing::trace!(?channel, ?len, "got message");

    let mut msg = BytesMut::with_capacity(len);
    unsafe {
        msg.set_len(len);
    }
    rx.read_exact(&mut msg)
        .instrument(trace_span!("read_exact"))
        .await?;

    Ok((channel, Some(msg.freeze())))
}

pub fn inject_err_stream_ok<T, E, E0, S>(
    stream: S,
    err_notify: impl AsRef<SetOnce<E>>,
) -> impl Stream<Item = T>
where
    S: Stream<Item = Result<T, E0>>,
    E: Clone + From<E0>,
{
    inject_err_stream(stream, err_notify).filter_map(|r| std::future::ready(r.ok()))
}

pub fn inject_err_stream<T, E, E0, S>(
    stream: S,
    err_notify: impl AsRef<SetOnce<E>>,
) -> impl Stream<Item = Result<T, E>>
where
    S: Stream<Item = Result<T, E0>>,
    E: Clone + From<E0>,
{
    stream.map(move |r| {
        let err_notify = err_notify.as_ref();
        match (r.map_err(E::from), err_notify.get()) {
            (_, Some(err)) => Err(err.clone()), // inject error from err_notify
            (Ok(r), None) => Ok(r),
            (Err(e), None) => {
                // inject error into err_notify
                err_notify.set(e.clone()).ok();
                Err(e)
            }
        }
    })
}

pub async fn inject_err_fut<T, E, E0, Fut>(
    fut: Fut,
    err_notify: impl AsRef<SetOnce<E>>,
) -> Result<T, E>
where
    E: Debug + Clone + From<E0>,
    Fut: Future<Output = Result<T, E0>>,
{
    let err_notify = err_notify.as_ref();

    let r = select! {
        biased;
        err = err_notify.wait() => { // inject errors from err_notify
            tracing::warn!(?err, "Quitting early due to signalled error");
            Err(err.clone())
        },
        r = fut => r.map_err(E::from),
    };

    if let Err(err) = &r {
        tracing::warn!(?err, "fut errored, sending signal");
        err_notify.set(err.clone()).ok(); // inject errors into err_notify
    }

    r
}
