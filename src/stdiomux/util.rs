use std::{
    cell::RefCell,
    fmt::Debug,
    future::poll_fn,
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::Poll,
};

use bytes::{Bytes, BytesMut};
use futures::{
    Stream, StreamExt as _,
    stream::{self, BoxStream, FusedStream, LocalBoxStream, Peekable},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, BufReader, BufWriter},
    select,
    sync::{SetOnce, mpsc},
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{Instrument as _, info_span, trace_span};

/// Reasonable limit on the number of simultaneous active channels.
const MAX_CHANNELS: u16 = 128;

#[derive(Debug, thiserror::Error)]
#[error("Channel already exists with ID {0}")]
pub struct ChannelExists(u16);

#[derive(Debug, thiserror::Error)]
#[error("Receiver already closed")]
pub struct RecvClosed;

/// Helper for mapping from channel ID to its associated state.
pub struct ChannelMap {
    /// Actual channel values.
    values: Box<[Option<ChannelState>; MAX_CHANNELS as usize]>,

    /// This value is cached to make linear probing during
    /// [create_new_channel()] more efficient.
    last_inserted_channel_id: u16,
}

impl ChannelMap {
    fn new() -> Self {
        let values = Box::new([const { None }; MAX_CHANNELS as usize]);
        Self {
            values,
            last_inserted_channel_id: 0,
        }
    }

    /// Attempt to set up a new channel with any ID.
    pub fn alloc_new_channel(
        &mut self,
        tx_stream: LocalBoxStream<'static, Bytes>,
    ) -> Option<(u16, BoxStream<'static, Bytes>)> {
        // linearly probe through the ID space until you find a free ID.
        // start from the last inserted value and quit if nothing is alive.
        let mut id = self.last_inserted_channel_id;
        loop {
            id = id.wrapping_add(1) % MAX_CHANNELS;

            if id != self.last_inserted_channel_id {
                // fully looped around. no free slots
                return None;
            }

            if self.get_channel(id).is_none() {
                // it's not living. insert it
                return self.insert_new_channel(id, tx_stream).ok();
            }
        }
    }

    /// Setup a new channel with a specific ID.
    pub fn insert_new_channel(
        &mut self,
        channel_id: u16,
        tx_stream: LocalBoxStream<'static, Bytes>,
    ) -> Result<(u16, BoxStream<'static, Bytes>), ChannelExists> {
        if self.get_channel(channel_id).is_some() {
            return Err(ChannelExists(channel_id));
        }

        self.last_inserted_channel_id = channel_id;

        // now insert a new value in the free slot
        let (cs, rx_stream) = new_channel_state();
        self.values[channel_id as usize] = Some(cs);
        Ok((channel_id, rx_stream))
    }

    /// Get the channel located at the given cell. Returns [None] if the channel
    /// is dead.
    pub fn get_channel(&self, channel_id: u16) -> Option<&ChannelState> {
        self.values
            .get(channel_id as usize)
            .and_then(|v| v.as_ref())
            .filter(|x| x.is_alive())
    }

    /// Get an iterator through all of the living channels of this [ChannelMap].
    pub fn channel_iter<'a>(&'a self) -> impl Iterator<Item = (u16, &'a ChannelState)> + 'a {
        (0..MAX_CHANNELS).filter_map(|id| self.get_channel(id).map(|c| (id, c)))
    }
}

fn new_channel_state() -> (ChannelState, LocalBoxStream<'static, Bytes>) {
    let (rxq_tx, rxq_rx) = mpsc::unbounded_channel();
    let rx_stream = Box::pin(UnboundedReceiverStream::new(rxq_rx));
    let cs = ChannelState {};

    (cs, rx_stream)
}

/// State of a single channel.
pub struct ChannelState {
    tx: Rc<RefCell<TxState>>,
    rx: RefCell<RxState>, // TODO: make backpressure work
}

struct RxState {
    rxq: Option<mpsc::UnboundedSender<Bytes>>,
}

impl RxState {
    fn alive(&self) -> bool {
        self.rxq.is_some()
    }
}

struct TxState {
    tx_stream: Peekable<LocalBoxStream<'static, Bytes>>,
}

impl TxState {
    fn alive(&self) -> bool {
        !self.tx_stream.is_terminated()
    }
}

impl ChannelState {
    /// Whether or not this stream is alive.
    pub fn is_alive(&self) -> bool {
        self.rx.borrow().alive() && self.tx.alive()
    }

    /// Handle receiving a new value.
    pub fn handle_rx(&self, payload: Bytes) -> Result<(), RecvClosed> {
        let mut lock = self.rx.borrow_mut();
        let rxq = lock.rxq.as_ref().ok_or(RecvClosed)?;

        rxq.send(payload).map_err(move |e| {
            lock.rxq = None;
            RecvClosed
        })
    }

    /// Returns a future to wait on the next transmission to send, if any.
    ///
    /// This is cancel safe.
    pub fn next_tx(&self) -> impl IntoFuture<Output = Option<Bytes>> + 'static {
        let txs = Rc::downgrade(&self.tx_stream);

        poll_fn(move |cx| {
            let Some(s) = txs.upgrade() else {
                return Poll::Ready(None);
            };

            let mut lock = s.borrow_mut();
            let r = lock.poll_next_unpin(cx);

            let out = lock.next().await;
            self.tx_dead_flag.store(out.is_none(), Ordering::Relaxed);
            r
        })
    }
}

pub async fn drive_tx<W>(
    tx: W,
    mut s: impl Stream<Item = Bytes> + Unpin,
) -> Result<(), Arc<std::io::Error>>
where
    W: AsyncWrite + Unpin + 'static,
{
    // wrap with a buffer big enough to wrap the header and a reasonably-sized
    // message
    let mut tx = BufWriter::with_capacity(4096, tx);

    // pull from the request stream
    while let Some(bytes) = s.next().await {
        // don't send 0 bytes because that's a EOF sentinel
        if bytes.is_empty() {
            continue;
        }

        tracing::trace!(len = ?bytes.len(), "sending message");

        // length-framing
        async {
            tx.write_u32(bytes.len().try_into().unwrap()).await?;
            tx.write_all(&bytes).await?;
            tx.flush().await?;
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

pub fn drive_rx<R>(rx: R) -> impl Stream<Item = Result<Bytes, Arc<std::io::Error>>>
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
