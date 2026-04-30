use std::{
    error::Error,
    future::poll_fn,
    marker::PhantomData,
    num::NonZero,
    sync::{Arc, Weak},
    task::Poll,
};

use bytes::Bytes;
use futures::{Sink, SinkExt, TryStream, TryStreamExt as _};
use tokio::sync::{mpsc, oneshot};

use crate::{
    channel::state::{AcceptRxError, ChannelBuffer, OpenChannelError},
    frame::{ChannelId, Frame, MuxControlHeader},
    mux::state::{ClosedReason, MuxNotOpen, MuxState},
};

pub struct AsyncMuxController<Rx, Tx>
where
    Rx: TryStream<Ok = Frame> + Unpin + Send + 'static,
    Tx: Sink<Frame> + Unpin + Send + 'static,
    Rx::Error: Error + Sync,
    Tx::Error: Error + Sync,
{
    inner: Arc<Inner>,
    txq: mpsc::UnboundedSender<Frame>,
    _phantom: PhantomData<(Rx, Tx)>,
}

#[derive(Debug, thiserror::Error)]
pub enum OpenMuxError {
    #[error("Got a non-hello handshake frame: {0:?}")]
    NonHello(Option<Frame>),
    #[error("transport error: {0}")]
    Transport(#[from] Arc<dyn Error>),
}

impl<Rx, Tx> AsyncMuxController<Rx, Tx>
where
    Rx: TryStream<Ok = Frame> + Unpin + Send + 'static,
    Tx: Sink<Frame> + Unpin + Send + 'static,
    Rx::Error: Error + Sync + Send,
    Tx::Error: Error + Sync + Send,
{
    pub async fn open(mut rx: Rx, mut tx: Tx) -> Result<Self, OpenMuxError> {
        // send handshake
        tx.send(Frame::MuxControl(MuxControlHeader::Hello))
            .await
            .map_err(|e| OpenMuxError::Transport(Arc::new(e)))?;

        // ensure we got the correct handshake
        match rx.try_next().await {
            Ok(Some(Frame::MuxControl(MuxControlHeader::Hello))) => (),
            Ok(f) => return Err(OpenMuxError::NonHello(f)),
            Err(e) => return Err(OpenMuxError::Transport(Arc::new(e))),
        }

        // shared state
        let inner = Arc::new(Inner {
            state: std::sync::Mutex::new(MuxState::<MpscChannelBuffer>::opened()),
        });

        // queue for transmissions
        let (txq_tx, txq_rx) = mpsc::unbounded_channel::<Frame>();

        // start processing in background
        let _background = tokio::spawn({
            let inner = Arc::downgrade(&inner);
            let txq_tx = txq_tx.clone();
            async move {
                let r = tokio::select! {
                    r = rx_actor(inner.clone(), rx, txq_tx) => r,
                    r = tx_actor(inner.clone(), tx, txq_rx) => r,
                };
                Inner::close_weak(inner, r);
            }
        });

        Ok(Self {
            inner: inner,
            txq: txq_tx,
            _phantom: PhantomData,
        })
    }

    pub async fn open_channel(
        &self,
        channel_id: ChannelId,
        initial_rx_buffer: usize,
    ) -> Result<ChannelIo, OpenChannelError> {
        let (tx, rx) = oneshot::channel();
        let poll = self.inner.state.lock().unwrap().open_channel(
            channel_id,
            Box::new(move || {
                let (user, buffer) = make_channel_io(initial_rx_buffer);
                match tx.send(user) {
                    Ok(_) => Some(buffer),
                    Err(_) => None,
                }
            }),
        );

        match poll {
            Poll::Ready(r) => r?,
            Poll::Pending => (),
        }
        Ok(rx
            .await
            .map_err(|_| OpenChannelError::MuxNotOpen(MuxNotOpen))?)
    }
}

async fn tx_actor<Tx>(
    _inner: Weak<Inner>,
    mut tx: Tx,
    mut txq: mpsc::UnboundedReceiver<Frame>,
) -> Result<(), ClosedReason>
where
    Tx: Sink<Frame> + Unpin + Send + 'static,
    Tx::Error: Error + Sync + Send,
{
    loop {
        let f = txq.recv().await.ok_or_else(|| ClosedReason::QueueClosed)?;

        tx.send(f)
            .await
            .map_err(|e| ClosedReason::TransportFailure(Arc::new(e)))?;
    }
}

async fn rx_actor<Rx>(
    inner: Weak<Inner>,
    mut rx: Rx,
    txq: mpsc::UnboundedSender<Frame>,
) -> Result<(), ClosedReason>
where
    Rx: TryStream<Ok = Frame> + Unpin + Send + 'static,
    Rx::Error: Error + Sync + Send,
{
    loop {
        let f = match rx.try_next().await {
            Ok(Some(f)) => f,
            Ok(None) => Err(ClosedReason::TransportClosed)?,
            Err(e) => Err(ClosedReason::TransportFailure(Arc::new(e)))?,
        };

        let Some(inner) = inner.upgrade() else {
            return Ok(());
        };
        if let Some(r) = inner.state.lock().unwrap().on_recv(f) {
            txq.send(r).map_err(|_| ClosedReason::QueueClosed)?;
        }
    }
}

struct Inner {
    state: std::sync::Mutex<MuxState<MpscChannelBuffer>>,
}

impl Inner {
    async fn get_tx_frames(self: Arc<Self>) -> Result<Vec<Frame>, MuxNotOpen> {
        let this = self.clone();
        poll_fn(move |cx| this.state.lock().unwrap().poll_sends(cx)).await
    }

    /// Close the mux with a result if you only have a weak pointer.
    fn close_weak(this: Weak<Self>, r: Result<(), ClosedReason>) {
        let Some(i) = this.upgrade() else { return };
        i.close(r);
    }

    /// Close the mux with a result
    fn close(&self, r: Result<(), ClosedReason>) {
        let mut s = self.state.lock().unwrap();
        if s.closed() {
            // don't overwrite the existing reason if already closed
            return;
        }
        *s = MuxState::Closed(r);
    }
}

fn make_channel_io(initial_channel_buffer: usize) -> (ChannelIo, MpscChannelBuffer) {
    let (to_user, from_controller) = mpsc::channel(initial_channel_buffer);
    let (to_controller, from_user) = mpsc::channel(128);

    (
        ChannelIo {
            tx: to_controller,
            rx: from_controller,
        },
        MpscChannelBuffer {
            inner: Some((to_user, from_user)),
        },
    )
}

#[derive(Debug)]
pub struct ChannelIo {
    tx: mpsc::Sender<Bytes>,
    rx: mpsc::Receiver<Bytes>,
}

#[derive(Debug, Default)]
struct MpscChannelBuffer {
    inner: Option<(mpsc::Sender<Bytes>, mpsc::Receiver<Bytes>)>,
}

impl ChannelBuffer for MpscChannelBuffer {
    fn poll_rx_capacity(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Option<NonZero<u64>>> {
        let Some((tx, _rx)) = &mut self.inner else {
            return Poll::Ready(None);
        };

        match NonZero::try_from(tx.capacity() as u64) {
            Err(_) => Poll::Pending,
            Ok(c) => Poll::Ready(Some(c)),
        }
    }

    fn accept_rx(&mut self, data: Bytes) -> Result<(), AcceptRxError> {
        let Some((tx, _rx)) = &mut self.inner else {
            return Err(AcceptRxError::Disconnected);
        };

        tx.try_send(data).map_err(|_| AcceptRxError::OutOfCapacity)
    }

    fn poll_tx(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<bytes::Bytes>> {
        let Some((_tx, rx)) = &mut self.inner else {
            return Poll::Ready(None);
        };

        match rx.try_recv() {
            Ok(x) => Poll::Ready(Some(x)),
            Err(mpsc::error::TryRecvError::Empty) => Poll::Pending,
            Err(mpsc::error::TryRecvError::Disconnected) => Poll::Ready(None),
        }
    }
}
