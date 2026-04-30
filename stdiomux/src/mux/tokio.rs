use std::{marker::PhantomData, num::NonZero, sync::Arc, task::Poll};

use bytes::Bytes;
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    sync::{mpsc, watch},
};

use crate::{
    channel::state::{AcceptRxError, ChannelBuffer},
    frame::{AsyncReadExt as _, AsyncWriteExt as _, Frame, MuxControlHeader},
    mux::state::MuxState,
    queue::priority_queue,
};

pub struct AsyncMuxController<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    inner: Arc<std::sync::Mutex<Inner>>,
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

        let inner = Arc::new(std::sync::Mutex::new(Inner {
            state: MuxState::<MpscChannelBuffer>::opened(),
        }));

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
                    let mut inner = inner.lock().unwrap();
                    if let Some(response) = inner.on_recv(f) {
                        let Ok(()) = to_tx.send(response) else {
                            return;
                        };
                    }
                    if inner.state.closed() {
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

    // pub async fn open_channel(
    //     &self,
    //     channel_id: ChannelId,
    //     initial_rx_buffer: usize,
    // ) -> Result<ChannelIo, OpenChannelError> {
    //     todo!()
    // }
}

struct Inner {
    state: MuxState<MpscChannelBuffer>,
}

impl Inner {
    fn on_recv(&mut self, f: Frame) -> Option<Frame> {
        let (new_state, f) = std::mem::take(&mut self.state).on_recv(f);
        self.state = new_state;
        f
    }
}

fn make_channel_io() -> (ChannelIo, MpscChannelBuffer) {
    let (to_user, from_controller) = mpsc::channel(128);
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
