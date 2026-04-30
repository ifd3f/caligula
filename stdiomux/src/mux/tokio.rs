use std::{
    error::Error,
    marker::PhantomData,
    num::NonZero,
    sync::{Arc, Weak},
    task::{Context, Poll, Waker},
    time::Duration,
};

use bytes::Bytes;
use futures::{FutureExt, Sink, SinkExt, Stream, TryStream, TryStreamExt as _};
use tokio::{
    sync::{mpsc, oneshot},
    time::MissedTickBehavior,
};
use tokio_util::sync::PollSender;

use crate::{
    channel::state::{AcceptRxError, ChannelBuffer, OpenChannelError},
    frame::{ChannelId, Frame, MuxControlHeader},
    mux::state::{ClosedReason, MuxNotOpen, MuxState},
};

pub struct AsyncMuxController<Rx, Tx>
where
    Rx: TryStream<Ok = Frame> + Unpin + Send + 'static,
    Tx: Sink<Frame> + Unpin + Send + 'static,
    Rx::Error: Error + Sync + Send,
    Tx::Error: Error + Sync + Send,
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

        // queue for transmissions
        let (txq_tx, txq_rx) = mpsc::unbounded_channel::<Frame>();

        // shared state
        let inner = Arc::new(Inner {
            state: std::sync::Mutex::new(MuxState::<MpscChannelBuffer>::opened()),
            txq: txq_tx.clone()
        });

        // start processing in background
        let _background = tokio::spawn({
            let inner = Arc::downgrade(&inner);
            async move {
                let r = tokio::select! {
                    r = rx_actor(inner.clone(), rx) => r,
                    r = tx_actor(inner.clone(), tx, txq_rx) => r,
                };
                Inner::close_weak(inner, r);
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

    pub fn do_sends_interval(&self, period: Duration) -> impl Future<Output = ()> + 'static {
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let inner = Arc::downgrade(&self.inner);
        async move {
            loop {
                interval.tick().await;

                let Some(inner) = inner.upgrade() else {
                    return Err(MuxNotOpen);
                };

                let mut cx = Context::from_waker(Waker::noop());
                let frames = inner.clone().poll_tx_frames(&mut cx)?;
                for f in frames {
                    inner.txq.send(f).map_err(|_| MuxNotOpen)?;
                }
            }
        }
        .map(|_: Result<(), MuxNotOpen>| ())
    }
}

unsafe impl<Rx, Tx> Sync for AsyncMuxController<Rx, Tx>
where
    Rx: TryStream<Ok = Frame> + Unpin + Send + 'static,
    Tx: Sink<Frame> + Unpin + Send + 'static,
    Rx::Error: Error + Sync + Send,
    Tx::Error: Error + Sync + Send,
{
}

unsafe impl<Rx, Tx> Send for AsyncMuxController<Rx, Tx>
where
    Rx: TryStream<Ok = Frame> + Unpin + Send + 'static,
    Tx: Sink<Frame> + Unpin + Send + 'static,
    Rx::Error: Error + Sync + Send,
    Tx::Error: Error + Sync + Send,
{
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
            inner.txq.send(r).map_err(|_| ClosedReason::QueueClosed)?;
        }
    }
}

struct Inner {
    state: std::sync::Mutex<MuxState<MpscChannelBuffer>>,
    txq: mpsc::UnboundedSender<Frame>,
}

impl Inner {
    fn poll_tx_frames(self: Arc<Self>, cx: &mut Context<'_>) -> Result<Vec<Frame>, MuxNotOpen> {
        let this = self.clone();
        this.state.lock().unwrap().poll_sends(cx)
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
            tx: PollSender::new(to_controller),
            rx: from_controller,
        },
        MpscChannelBuffer {
            inner: Some((to_user, from_user)),
        },
    )
}

#[derive(Debug)]
pub struct ChannelIo {
    tx: PollSender<Bytes>,
    rx: mpsc::Receiver<Bytes>,
}

impl Stream for ChannelIo {
    type Item = Bytes;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.rx.capacity()))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Channel is disconnected")]
pub struct Disconnected;

impl Sink<Bytes> for ChannelIo {
    type Error = Disconnected;

    fn poll_ready(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.tx.poll_ready_unpin(cx).map_err(|_| Disconnected)
    }

    fn start_send(mut self: std::pin::Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        self.tx.start_send_unpin(item).map_err(|_| Disconnected)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.tx.poll_flush_unpin(cx).map_err(|_| Disconnected)
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.tx.poll_close_unpin(cx).map_err(|_| Disconnected)
    }
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

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use bytes::Bytes;
    use futures::{StreamExt, sink, stream::BoxStream};
    use tokio_stream::wrappers::ReceiverStream;

    use crate::frame::ChannelControlHeader;

    use super::*;

    type FrameStream = BoxStream<'static, Result<Frame, std::io::Error>>;
    type FrameSink = Pin<Box<dyn Sink<Frame, Error = std::io::Error> + Send + 'static>>;

    type TestController = AsyncMuxController<FrameStream, FrameSink>;

    const SENDS_INTERVAL: Duration = Duration::from_millis(1);

    fn make_sink_stream_pair(name: String) -> (FrameSink, FrameStream) {
        let (tx, rx) = mpsc::channel::<Result<Frame, std::io::Error>>(128);
        (
            Box::pin(sink::unfold((name, tx), |(name, tx), f| async move {
                println!("sending {name}: {f:?}");
                tx.send(Ok(f)).await.map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stream closed")
                })?;
                Ok((name, tx))
            })),
            Box::pin(ReceiverStream::new(rx)),
        )
    }

    struct SingleHarness {
        sut: TestController,
        tx: FrameSink,
        rx: FrameStream,
    }

    async fn setup_single_harness() -> SingleHarness {
        let (txs, mut rxh) = make_sink_stream_pair("s->h".into());
        let (mut txh, rxs) = make_sink_stream_pair("h->s".into());

        txh.send(Frame::MuxControl(MuxControlHeader::Hello))
            .await
            .unwrap();
        let sut = TestController::open(rxs, txs).await.unwrap();
        assert_eq!(
            rxh.next().await.unwrap().unwrap(),
            Frame::MuxControl(MuxControlHeader::Hello)
        );

        println!("single hello passed");

        SingleHarness {
            sut,
            tx: txh,
            rx: rxh,
        }
    }

    async fn run_single_test<'a, FSut, FutSut, FH, FutH>(sut: FSut, harness: FH)
    where
        FSut: FnOnce(Pin<Arc<TestController>>) -> FutSut,
        FH: FnOnce(FrameStream, FrameSink) -> FutH,
        FutSut: IntoFuture<Output = ()> + 'static,
        FutH: IntoFuture<Output = ()> + 'static,
    {
        let h = setup_single_harness().await;
        let ca = Arc::pin(h.sut);
        let a = sut(ca.clone());
        let b = harness(h.rx, h.tx);
        let bg = tokio::spawn(ca.do_sends_interval(SENDS_INTERVAL));
        tokio::join!(a, b);
        bg.abort();
    }

    struct DoubleHarness {
        a: TestController,
        b: TestController,
    }

    async fn setup_double_harness() -> DoubleHarness {
        let (txa, rxb) = make_sink_stream_pair("a->b".into());
        let (txb, rxa) = make_sink_stream_pair("b->a".into());

        let a = TestController::open(rxa, txa);
        let b = TestController::open(rxb, txb);

        println!("double hello passed");

        let (a, b) = tokio::join!(a, b);
        DoubleHarness {
            a: a.unwrap(),
            b: b.unwrap(),
        }
    }

    async fn run_double_test<FA, FutA, FB, FutB>(a: FA, b: FB)
    where
        FA: FnOnce(Pin<Arc<TestController>>) -> FutA,
        FB: FnOnce(Pin<Arc<TestController>>) -> FutB,
        FutA: IntoFuture<Output = ()> + 'static,
        FutB: IntoFuture<Output = ()> + 'static,
    {
        let h = setup_double_harness().await;
        let (ca, cb) = (Arc::pin(h.a), Arc::pin(h.b));
        let a = a(ca.clone());
        let b = b(cb.clone());
        let bg = tokio::spawn(async move {
            tokio::join!(
                ca.do_sends_interval(SENDS_INTERVAL),
                cb.do_sends_interval(SENDS_INTERVAL)
            )
        });
        tokio::join!(a, b);
        bg.abort();
    }

    #[tokio::test]
    #[test_log::test]
    async fn hello_works() {
        run_single_test(|_c| async move {}, |_rx, _tx| async move {}).await
    }

    #[tokio::test]
    #[test_log::test]
    async fn double_hello_works() {
        run_double_test(|_c| async move {}, |_c| async move {}).await
    }

    #[tokio::test]
    #[test_log::test]
    async fn send_into_sut() {
        println!("starting test");
        run_single_test(
            |sut| async move {
                println!("sut: opening ch");
                let mut ch = sut.open_channel(ChannelId(10), 100).await.unwrap();
                println!("sut: ch is open");
                for _ in 0..100 {
                    ch.next().await.unwrap();
                }
            },
            |mut rx, mut tx| async move {
                println!("sut: opening ch");
                let open = Frame::ChannelControl(ChannelId(10), ChannelControlHeader::Open);
                tx.send(open).await.unwrap();
                println!("sut: ch is open");

                let reply = rx.next().await.unwrap().unwrap();
                assert_eq!(
                    reply,
                    Frame::ChannelControl(ChannelId(10), ChannelControlHeader::Open)
                );

                let adm = rx.next().await.unwrap().unwrap();
                assert_eq!(
                    adm,
                    Frame::ChannelControl(ChannelId(10), ChannelControlHeader::Admit(100))
                );

                for _ in 0..100 {
                    tx.send(Frame::ChannelData(
                        ChannelId(10),
                        Bytes::from_static(b"foobar").into(),
                    ))
                    .await
                    .unwrap();
                }
            },
        )
        .await
    }

    #[ignore]
    #[tokio::test]
    #[test_log::test]
    async fn open_channel_and_send_data() {
        println!("starting test");
        run_double_test(
            |c| async move {
                let mut ch = c.open_channel(ChannelId(10), 100).await.unwrap();
                for _ in 0..100 {
                    ch.send(Bytes::from_static(b"foobar")).await.unwrap();
                }
            },
            |c| async move {
                let mut ch = c.open_channel(ChannelId(10), 100).await.unwrap();
                for _ in 0..100 {
                    assert_eq!(ch.next().await, Some(Bytes::from_static(b"foobar")));
                }
            },
        )
        .await
    }
}
