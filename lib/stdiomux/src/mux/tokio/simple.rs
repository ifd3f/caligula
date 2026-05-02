use std::{
    marker::PhantomData,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::Bytes;
use dashmap::DashMap;
use futures::task::AtomicWaker;
use lockfree::queue::Queue;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    select,
    sync::mpsc::{self, UnboundedSender},
};

use crate::{
    frame::{
        Frame, Header, ReadFrameError, WriteFrameError,
        simple::{SimpleMuxFrame, SimpleMuxHeader},
        tokio::{FrameReader, FrameWriter},
    },
    mux::{ChannelHandle, MuxController},
    utils::{AnnounceError, make_hello_with_crate_version},
};

const HELLO: &'static [u8; libc::PIPE_BUF] =
    make_hello_with_crate_version!("simple mux controller");

#[derive(Debug, thiserror::Error)]
pub enum ClosedReason {
    #[error("Did not get expected hello")]
    BadHello,
    #[error("Transport error during handshake: {0}")]
    HandshakeTransport(std::io::Error),
    #[error("Error during receive: {0}")]
    RxError(#[from] ReadFrameError<SimpleMuxFrame>),
    #[error("Error during transmit: {0}")]
    TxError(#[from] WriteFrameError<SimpleMuxFrame>),
    #[error("SimpleMuxController dropped")]
    Dropped,
}

/// Very simple mux controller that runs in [`tokio::spawn`] tasks in the background.
///
/// WARNING: This does not implement buffering, backpressure, or priority queueing!
/// It's implemented with [`mpsc::unbounded_channel()`]s and is therefore extremely unsafe
/// to use if you expect to send large amounts of data!
pub struct SimpleMuxController<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    inner: Arc<Inner>,
    _phantom: PhantomData<(R, W)>,
}

impl<R, W> Drop for SimpleMuxController<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    fn drop(&mut self) {
        self.inner.error.announce(ClosedReason::Dropped);
    }
}
struct Inner {
    error: AnnounceError<ClosedReason>,
    txq: mpsc::UnboundedSender<SimpleMuxFrame>,
    channel_rxqs: DashMap<u16, Arc<WokeQueue>>,
}

impl<R, W> SimpleMuxController<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    pub async fn open(mut r: R, mut w: W) -> Result<Self, ClosedReason> {
        let fut = async {
            w.write_all(HELLO).await?;
            w.flush().await?;
            let mut buf = vec![0u8; HELLO.len()];
            r.read_exact(&mut buf).await?;
            Ok(buf)
        };

        let buf = fut.await.map_err(ClosedReason::HandshakeTransport)?;
        if buf != HELLO {
            Err(ClosedReason::BadHello)?
        }

        let r = FrameReader::new(r);
        let w = FrameWriter::new(w);

        let (txq, txq_rx) = mpsc::unbounded_channel();

        let inner = Arc::new(Inner {
            error: AnnounceError::new(),
            txq,
            channel_rxqs: DashMap::new(),
        });

        let _rx = tokio::spawn(rx_actor(inner.clone(), r));
        let _tx = tokio::spawn(tx_actor(inner.clone(), txq_rx, w));
        Ok(Self {
            inner,
            _phantom: PhantomData,
        })
    }
}

async fn rx_actor(inner: Arc<Inner>, mut r: FrameReader<impl AsyncRead + Unpin, SimpleMuxFrame>) {
    loop {
        let rx = select! {
            rx = r.read_frame() => rx,
            _ = inner.error.wait() => { return; }
        };

        match rx {
            Ok(rx) => {
                let entry = inner.channel_rxqs.entry(rx.channel).or_default();
                entry.q.push(rx.body);
                entry.w.wake();
            }
            Err(err) => {
                inner.error.announce(err.into());
            }
        }
    }
}

async fn tx_actor(
    inner: Arc<Inner>,
    mut txq_rx: mpsc::UnboundedReceiver<SimpleMuxFrame>,
    mut w: FrameWriter<impl AsyncWrite + Unpin, SimpleMuxFrame>,
) {
    loop {
        let Ok(()) = inner.error.assert_ok() else {
            break;
        };

        let tx = select! {
            rx = txq_rx.recv() => rx,
            _ = inner.error.wait() => { break; }
        };

        match tx {
            Some(f) => {
                inner
                    .error
                    .announce_result(w.write_frame(f).await.map_err(ClosedReason::TxError))
                    .ok();
            }
            None => {
                inner.error.announce(ClosedReason::Dropped);
            }
        }
    }
}

unsafe impl<R, W> Sync for SimpleMuxController<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
}

impl<R, W> MuxController for SimpleMuxController<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    type ChannelHandle = SimpleChannelHandle;

    type ChannelId = u16;

    type ClosedReason = Arc<ClosedReason>;

    type OpenChannelError = Arc<ClosedReason>;

    fn assert_open(&self) -> Result<(), Self::ClosedReason> {
        self.inner.error.assert_ok()
    }

    fn open_channel(
        &self,
        id: &Self::ChannelId,
    ) -> Result<Self::ChannelHandle, Self::OpenChannelError> {
        let id = *id;
        let rxq = self.inner.channel_rxqs.entry(id).or_default().clone();
        let txq = self.inner.txq.clone();
        Ok(SimpleChannelHandle {
            error: self.inner.error.clone(),
            rxq,
            txq,
            channel: id,
        })
    }
}

pub struct SimpleChannelHandle {
    error: AnnounceError<ClosedReason>,
    rxq: Arc<WokeQueue>,
    txq: UnboundedSender<SimpleMuxFrame>,
    channel: u16,
}

#[derive(Default)]
struct WokeQueue {
    q: Queue<Bytes>,
    w: AtomicWaker,
}

impl ChannelHandle for SimpleChannelHandle {
    const MAX: usize = SimpleMuxFrame::MTU - SimpleMuxHeader::SIZE;

    type ClosedReason = Arc<ClosedReason>;

    fn assert_open(&self) -> Result<(), Self::ClosedReason> {
        self.error.assert_ok()
    }

    fn poll_send(&self, _cx: &mut Context<'_>, bs: &Bytes) -> Poll<Result<(), Arc<ClosedReason>>> {
        self.error.assert_ok()?;
        self.txq
            .send(SimpleMuxFrame {
                channel: self.channel,
                body: bs.clone(),
            })
            .map_err(|_| self.error.announce(ClosedReason::Dropped))?;
        Poll::Ready(Ok(()))
    }

    fn poll_recv(&self, cx: &mut Context<'_>) -> Poll<Result<Bytes, Self::ClosedReason>> {
        match self.rxq.q.pop() {
            Some(b) => Poll::Ready(Ok(b)),
            None => {
                self.error.assert_ok()?;
                self.rxq.w.register(cx.waker());
                Poll::Pending
            }
        }
    }
}
