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
    pub async fn open(mut r: Rx, mut w: Tx) -> Result<Self, OpenMuxError> {
        // send handshake
        w.send(Frame::MuxControl(MuxControlHeader::Hello))
            .await
            .map_err(|e| OpenMuxError::Transport(Arc::new(e)))?;

        // ensure we got the correct handshake
        match r.try_next().await {
            Ok(Some(Frame::MuxControl(MuxControlHeader::Hello))) => (),
            Ok(f) => return Err(OpenMuxError::NonHello(f)),
            Err(e) => return Err(OpenMuxError::Transport(Arc::new(e))),
        }

        // start processing
        let inner = Arc::new(Inner {
            state: std::sync::Mutex::new(MuxState::<MpscChannelBuffer>::opened()),
        });

        let (to_tx, mut from_rx) = mpsc::unbounded_channel::<Frame>();
        let _tx_actor = tokio::spawn({
            let inner = Arc::downgrade(&inner);
            async move {
                loop {
                    let f = match from_rx.recv().await {
                        Some(f) => f,
                        None => {
                            Inner::close_weak(inner, Ok(()));
                            return;
                        }
                    };

                    match w.send(f).await {
                        Ok(()) => (),
                        Err(e) => {
                            Inner::close_weak(
                                inner,
                                Err(ClosedReason::TransportFailure(Arc::new(e))),
                            );
                            return;
                        }
                    }
                }
            }
        });

        let _rx_actor = tokio::spawn({
            let inner = Arc::downgrade(&inner);
            async move {
                loop {
                    let f = match r.try_next().await {
                        Ok(Some(f)) => f,
                        Ok(None) => {
                            Inner::close_weak(inner, Ok(()));
                            return;
                        }
                        Err(e) => {
                            Inner::close_weak(
                                inner,
                                Err(ClosedReason::TransportFailure(Arc::new(e))),
                            );
                            return;
                        }
                    };

                    let Some(inner) = inner.upgrade() else {
                        return;
                    };
                    if let Some(r) = inner.state.lock().unwrap().on_recv(f) {
                        match to_tx.send(r) {
                            Ok(()) => (),
                            Err(e) => {
                                inner.close(Err(ClosedReason::TransportFailure(Arc::new(e))));
                                return;
                            }
                        }
                    }
                }
            }
        });
        Ok(Self {
            inner: inner,
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
