use std::{
    marker::PhantomData, num::NonZero, pin::Pin, sync::Arc, task::{Context, Poll, Waker}
};

use bytes::Bytes;
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    sync::{mpsc, oneshot, watch},
};

use crate::{
    channel::state::{AcceptRxError, ChannelBuffer, ChannelInUse, OpenChannelError},
    frame::{AsyncReadExt as _, AsyncWriteExt as _, ChannelId, Frame, MuxControlHeader},
    mux::state::{MuxNotOpen, MuxState},
    queue::priority_queue,
};

pub struct AsyncMuxController<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    inner: Inner,
    _phantom: PhantomData<(R, W)>,
}

impl<R, W> AsyncMuxController<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    pub async fn open(mut r: R, mut w: W) -> std::io::Result<Self> {
        w.write_frame_async(&Frame::MuxControl(MuxControlHeader::Hello))
            .await?;
        w.flush().await?;
        let read = r.read_frame_async().await?;
        if read != Frame::MuxControl(MuxControlHeader::Hello) {
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "did not receive a hello from the other end",
            ))?;
        }

        let inner = Inner {
            state: Arc::pin(std::sync::Mutex::new(
                MuxState::<MpscChannelBuffer>::opened(),
            )),
        };

        let (to_tx, mut from_rx) = mpsc::unbounded_channel::<Frame>();
        let _tx_actor = tokio::spawn({
            let inner = inner.clone();
            async move {
                while let Some(f) = from_rx.recv().await {
                    w.write_frame_async(&f)
                        .await
                        .expect("transport error while writing frame");
                }
            }
        });

        let _rx_actor = tokio::spawn({
            let inner = inner.clone();
            async move {
                loop {
                    let f = r.read_frame_async().await.expect("failed to read frame");
                    let mut state = inner.state.lock().unwrap();
                    if let Some(response) = state.on_recv(f) {
                        let Ok(()) = to_tx.send(response) else {
                            return;
                        };
                    }
                    if state.closed() {
                        return;
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

#[derive(Clone)]
struct Inner {
    state: Pin<Arc<std::sync::Mutex<MuxState<MpscChannelBuffer>>>>,
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
        cx: &mut std::task::Context<'_>,
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
