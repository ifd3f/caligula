use std::{
    cell::RefCell,
    convert::Infallible,
    fmt::Debug,
    future::poll_fn,
    ops::ControlFlow,
    pin::pin,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use futures::{
    FutureExt, Stream, StreamExt,
    future::{Either, select},
    stream::{self, BoxStream, FusedStream, LocalBoxStream, Peekable},
    task::AtomicWaker,
};
use infinite_stream::InfiniteStreamExt;
use tokio::{
    io::{
        AsyncBufRead, AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, BufReader,
        BufWriter,
    },
    select,
    sync::{SetOnce, mpsc},
};
use tracing::{Instrument as _, info_span, trace_span};

use crate::{
    herder_api::error::LayerError,
    stdiomux::{
        BytestreamService,
        channel_map::{ChannelMap, ChannelMapRxHalf, ChannelMapTxHalf},
    },
};

type RxResult = Result<(u16, Option<Bytes>), std::io::Error>;

pub async fn drive_transport<R, W, S, C>(
    mut rx: R,
    mut tx: W,
    server: S,
    mut new_channels: C,
) -> Result<(), LayerError<S::Error, std::io::Error>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    S: BytestreamService,
    C: Stream<Item = LocalBoxStream<'static, Bytes>>,
{
    // Having this on the outside of the loop ensures that if rx_stream.next() gets
    // interrupted in the middle of the steady-state, it will simply keep getting
    // queued up.
    let mut map = ChannelMap::new();

    let mut active_read_fut = None;
    let mut active_write_fut = None;

    loop {
        let (mut rx_map, mut tx_map) = map.split();

        let mut tx_stream = tx_map.tx_stream();

        let rx_fut = poll_fn(|cx| {
            poll_rx(
                cx,
                &mut active_read_fut,
                || Box::pin(read_msg(&mut rx)),
                |res| handle_rx(&mut rx_map, res),
            )
        });
        let tx_fut = poll_fn(|cx| {
            poll_tx(
                cx,
                &mut active_write_fut,
                |x| Box::pin(write_msg(&mut tx, x)),
                || tx_stream.next(),
            )
        });

        let mut tx_stream = pin!(tx_map.as_stream().peekable());

        enum TxFut<Pull, Push> {
            Pull(Pull),
            Push(Push),
        }

        let mut tx_fut = TxFut::Pull(pin!(tx_stream.peek()));
        let mut rx_fut = pin!(rx_stream.peek());
    }
}

/// Drive the reception state.
///
/// Doing it this way ensures that we read things atomically and don't have
/// cutoffs.
fn poll_rx<T, R, Fut>(
    cx: &mut Context<'_>,
    active_read_fut: &mut Option<Fut>,
    recv_msg: impl FnOnce() -> Fut,
    handle_rx: impl FnOnce(std::io::Result<T>) -> R,
) -> Poll<R>
where
    Fut: Future<Output = std::io::Result<T>> + Unpin,
{
    if let Some(f) = active_read_fut {
        // there is an active reception: drive it
        return f.poll_unpin(cx).map(handle_rx);
    }

    // no active reception: put a new reception on
    *active_read_fut = Some(recv_msg());
    Poll::Pending
}

/// Drive the transmission state.
///
/// Doing it this way ensures that we write things atomically and don't have
/// cutoffs.
fn poll_tx<T, Fut>(
    cx: &mut Context<'_>,
    active_write_fut: &mut Option<Fut>,
    send_msg: impl FnOnce(T) -> Fut,
    next_tx_msg: impl AsyncFnOnce() -> T,
) -> Poll<std::io::Result<()>>
where
    Fut: Future<Output = std::io::Result<()>> + Unpin,
{
    if let Some(f) = active_write_fut {
        // there is an active transmission: drive it and do NOT pull a new message
        return f.poll_unpin(cx);
    }

    // no active transmission. try pulling a new message
    let Poll::Ready(new_msg) = pin!(next_tx_msg()).poll(cx) else {
        return Poll::Pending;
    };

    // message successfully pulled. create a new transmission future to be polled
    // later
    *active_write_fut = Some(send_msg(new_msg));
    Poll::Ready(Ok(()))
}

enum SteadyStateBreak {
    TransportError(std::io::Error),
    UnrecognizedStream(u16, Bytes),
}

fn handle_tx_result(res: std::io::Result<()>) -> ControlFlow<SteadyStateBreak> {
    match res {
        Ok(_) => (),
        Err(e) => ControlFlow::Break(SteadyStateBreak::TransportError(e)),
    }
}

fn handle_rx(map: &mut ChannelMapRxHalf<'_>, res: RxResult) -> ControlFlow<SteadyStateBreak> {
    let (id, bs) = match res {
        Ok(x) => x,
        Err(e) => return ControlFlow::Break(SteadyStateBreak::TransportError(e)),
    };

    let Some(bs) = bs else {
        map.close_rx(id);
        return ControlFlow::Continue(());
    };

    if let Err(_) = map.handle_rx((id, bs.clone())) {
        SteadyStateBreak::UnrecognizedStream(id, bs);
    }

    ControlFlow::Continue(())
}

/// Build a stream out of the received message set.
fn rx_stream(rx: impl AsyncRead + Unpin) -> impl Stream<Item = RxResult> {
    stream::unfold(rx, |mut rx| async move {
        let res = read_msg(&mut rx).await;
        Some((res, rx))
    })
}

/// Send a single message over the wire.
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
async fn read_msg(mut rx: impl AsyncRead + Unpin) -> Result<(u16, Option<Bytes>), std::io::Error> {
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
