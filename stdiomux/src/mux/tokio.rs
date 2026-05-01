use std::{
    error::Error,
    num::NonZero,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use bytes::Bytes;
use futures::{Sink, SinkExt, Stream, TryStream, TryStreamExt as _};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::PollSender;

use super::channel::{AcceptRxError, ChannelBuffer, OpenChannelError};
use crate::{
    frame::{ChannelControlHeader, ChannelId, Frame, MuxControlHeader},
    mux::state::{ClosedReason, MuxNotOpen, MuxState},
};

pub struct AsyncMuxController<Rx, Tx>
where
    Rx: TryStream<Ok = Frame> + Unpin + Send + 'static,
    Rx::Error: Error + Sync + Send,
    Tx: Sink<Frame> + Unpin + Send + 'static,
    Tx::Error: Error + Sync + Send,
{
    inner: Arc<Inner<Rx, Tx>>,
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
    Rx::Error: Error + Sync + Send,
    Tx: Sink<Frame> + Unpin + Send + 'static,
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
            rx_state: RxState { rx }.into(),
            tx_state: TxState {
                tx,
                priority_txq: txq_rx,
            }
            .into(),
            priority_txq: txq_tx,
        });

        Ok(Self { inner: inner })
    }

    /// Perform a single round of sending.
    ///
    /// Must be called continuously to ensure data is actually sent.
    ///
    /// Note that this may run for longer than just the current round if more frames have
    /// been added!
    ///
    /// The reason sends aren't immediate is because we want to batch transmissions
    /// together for efficiency.
    pub fn tx_round(&self) -> impl Future<Output = Result<(), ClosedReason>> + Send + 'static {
        let inner = self.inner.clone();
        async move { inner.tx_round().await }
    }

    /// Perform a single round of receiving.
    ///
    /// Must be called continuously to ensure data is actually received.
    ///
    /// Note that this may run for longer than just the current round if more frames have
    /// been added!
    ///
    /// The reason sends aren't immediate is because we want to batch transmissions
    /// together for efficiency.
    pub fn rx_round(&self) -> impl Future<Output = Result<(), ClosedReason>> + Send + 'static {
        let inner = self.inner.clone();
        async move { inner.rx_round().await }
    }

    /// Repeatedly receive and transmit until we are closed.
    pub fn rxtx_loop(&self) -> impl Future<Output = Result<(), ClosedReason>> + Send + 'static {
        let rx = self.inner.clone();
        let tx = self.inner.clone();

        async move {
            let rx = async move {
                loop {
                    match rx.rx_round().await {
                        Ok(()) => (),
                        Err(e) => return e,
                    }
                }
            };
            let tx = async move {
                loop {
                    match tx.tx_round().await {
                        Ok(()) => (),
                        Err(e) => return e,
                    }
                }
            };
            let e = tokio::select! {
                e = rx => e,
                e = tx => e,
            };
            Err(e)
        }
    }

    /// Perform a single round each of sending and receiving. Only useful for testing!
    #[cfg(test)]
    fn rxtx_round(&self) -> impl Future<Output = Result<(), ClosedReason>> + Send + 'static {
        let inner = self.inner.clone();
        async move {
            let i = inner.clone();
            tokio::try_join!(i.rx_round(), inner.tx_round())?;
            Ok(())
        }
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
        self.inner
            .priority_txq
            .send(Frame::ChannelControl(
                channel_id,
                ChannelControlHeader::Open,
            ))
            .map_err(|_| OpenChannelError::MuxNotOpen(MuxNotOpen))?;

        Ok(rx
            .await
            .map_err(|_| OpenChannelError::MuxNotOpen(MuxNotOpen))?)
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

struct Inner<Rx, Tx> {
    state: std::sync::Mutex<MuxState<MpscChannelBuffer>>,
    rx_state: tokio::sync::Mutex<RxState<Rx>>,
    tx_state: tokio::sync::Mutex<TxState<Tx>>,
    priority_txq: mpsc::UnboundedSender<Frame>,
}

struct RxState<Rx> {
    rx: Rx,
}

struct TxState<Tx> {
    tx: Tx,
    priority_txq: mpsc::UnboundedReceiver<Frame>,
}

impl<Tx> TxState<Tx> {}

impl<Rx, Tx> Inner<Rx, Tx>
where
    Rx: TryStream<Ok = Frame> + Unpin + Send + 'static,
    Rx::Error: Error + Sync + Send,
    Tx: Sink<Frame> + Unpin + Send + 'static,
    Tx::Error: Error + Sync + Send,
{
    /// Do a single round of receives
    async fn rx_round(&self) -> Result<(), ClosedReason> {
        let fut = async move {
            // hold the rx state lock for a single rx
            let mut rxs = self.rx_state.lock().await;
            let f = match rxs.rx.try_next().await {
                Ok(Some(f)) => f,
                Ok(None) => Err(ClosedReason::TransportClosed)?,
                Err(e) => Err(ClosedReason::TransportFailure(Arc::new(e)))?,
            };
            drop(rxs);
            println!("{f:?}");

            // the reply is always going to be high-priority
            if let Some(r) = self.state.lock().unwrap().on_recv(f) {
                self.priority_txq
                    .send(r)
                    .expect("queue unexpectedly closed");
            }
            Ok::<(), ClosedReason>(())
        };

        if let Err(e) = fut.await {
            self.close(e);
        }

        Ok(())
    }

    /// Do a single round of sends
    fn tx_round(&self) -> impl Future<Output = Result<(), ClosedReason>> + Send + '_ {
        let mut cx = Context::from_waker(Waker::noop());
        let polled_frames = self.state.lock().unwrap().poll_sends(&mut cx);
        drop(cx);

        async move {
            // sort polled frames into high-pri and low-pri
            let mut lpframes = vec![];
            for f in polled_frames? {
                match f {
                    f @ Frame::MuxControl(_) | f @ Frame::ChannelControl(_, _) => self
                        .priority_txq
                        .send(f)
                        .expect("insert into priority txq should never fail"),
                    f @ Frame::ChannelData(_, _) => lpframes.push(f),
                }
            }

            let fut = async move {
                // hold the tx state lock over this loop
                let mut txs = self.tx_state.lock().await;
                let mut lpframes = lpframes.into_iter();
                loop {
                    let f = match txs.priority_txq.try_recv() {
                        Ok(f) => Some(f),
                        Err(mpsc::error::TryRecvError::Empty) => lpframes.next(),
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            panic!("this should never fail");
                        }
                    };

                    let Some(f) = f else {
                        break;
                    };

                    txs.tx
                        .send(f)
                        .await
                        .map_err(|e| ClosedReason::TransportFailure(Arc::new(e)))?;
                }
                Ok::<(), ClosedReason>(())
            };

            if let Err(e) = fut.await {
                self.close(e);
            }

            Ok(())
        }
    }

    /// Close the mux with a result
    fn close(&self, r: ClosedReason) {
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

    use assert_matches::assert_matches;
    use bytes::Bytes;
    use futures::{FutureExt, StreamExt, sink, stream::BoxStream};
    use tokio::sync::Barrier;
    use tokio_stream::wrappers::ReceiverStream;

    use crate::frame::ChannelControlHeader;

    use super::*;

    type FrameStream = BoxStream<'static, Result<Frame, std::io::Error>>;
    type FrameSink = Pin<Box<dyn Sink<Frame, Error = std::io::Error> + Send + 'static>>;

    type TestController = AsyncMuxController<FrameStream, FrameSink>;

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
        tokio::join!(a, b);
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
        tokio::join!(a, b);
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
    async fn sut_open_channel_works() {
        const CHANNEL: ChannelId = ChannelId(10);
        const BUFFER: usize = 128;

        println!("starting test");
        run_single_test(
            |sut| async move {
                println!("sut: opening ch");
                let (ch, r) = tokio::join!(sut.open_channel(CHANNEL, BUFFER), sut.rxtx_round(),);
                let (_ch, _) = (ch.unwrap(), r.unwrap());
                println!("sut: ch is open");

                // send adms
                sut.tx_round().await.unwrap();
            },
            |mut rx, mut tx| async move {
                println!("h: opening ch");
                let open = Frame::ChannelControl(CHANNEL, ChannelControlHeader::Open);
                tx.send(open).await.unwrap();
                println!("h: ch is open");

                let reply = rx.next().await.unwrap().unwrap();
                assert_eq!(
                    reply,
                    Frame::ChannelControl(CHANNEL, ChannelControlHeader::Open)
                );

                let adm = rx.next().await.unwrap().unwrap();
                assert_eq!(
                    adm,
                    Frame::ChannelControl(CHANNEL, ChannelControlHeader::Admit(BUFFER as u8))
                );
            },
        )
        .await
    }

    #[tokio::test]
    #[test_log::test]
    async fn send_into_sut_channel() {
        const CHANNEL: ChannelId = ChannelId(10);
        const BUFFER: usize = 128;

        println!("starting test");
        run_single_test(
            |sut| async move {
                println!("sut: opening ch");
                let (ch, r) = tokio::join!(sut.open_channel(CHANNEL, BUFFER), sut.rxtx_round(),);
                let (mut ch, _) = (ch.unwrap(), r.unwrap());
                println!("sut: ch is open");

                // send adms
                sut.tx_round().await.unwrap();

                // read the rest of it
                for _ in 0..BUFFER {
                    let (bs, r) = tokio::join!(ch.next(), sut.rx_round());
                    bs.unwrap();
                    r.unwrap();
                }
            },
            |mut rx, mut tx| async move {
                println!("sut: opening ch");
                let open = Frame::ChannelControl(CHANNEL, ChannelControlHeader::Open);
                tx.send(open).await.unwrap();
                println!("sut: ch is open");

                // recv open reply
                let reply = rx.next().await.unwrap().unwrap();
                assert_eq!(
                    reply,
                    Frame::ChannelControl(CHANNEL, ChannelControlHeader::Open)
                );

                // recv adm
                let adm = rx.next().await.unwrap().unwrap();
                assert_eq!(
                    adm,
                    Frame::ChannelControl(CHANNEL, ChannelControlHeader::Admit(BUFFER as u8))
                );

                // send datas
                for _ in 0..BUFFER {
                    tx.send(Frame::ChannelData(
                        CHANNEL,
                        Bytes::from_static(b"foobar").into(),
                    ))
                    .await
                    .unwrap();
                }
            },
        )
        .await
    }

    #[tokio::test]
    #[test_log::test]
    async fn send_from_sut_channel() {
        const CHANNEL: ChannelId = ChannelId(10);
        const BUFFER: usize = 128;

        println!("starting test");
        run_single_test(
            |sut| async move {
                println!("sut: opening ch");
                let (ch, r) = tokio::join!(sut.open_channel(CHANNEL, 1), sut.rxtx_round(),);
                let (mut ch, _) = (ch.unwrap(), r.unwrap());
                println!("sut: ch is open");

                // exchange adms
                sut.rxtx_round().await.unwrap();

                // queue bytes for sending
                for _ in 0..BUFFER {
                    ch.send(Bytes::from_static(b"foobar")).await.unwrap();
                }

                // actually send
                for _ in 0..BUFFER {
                    sut.tx_round().await.unwrap();
                }
            },
            |mut rx, mut tx| async move {
                println!("sut: opening ch");
                let open = Frame::ChannelControl(CHANNEL, ChannelControlHeader::Open);
                tx.send(open).await.unwrap();
                println!("sut: ch is open");

                // recv open reply
                let reply = rx.next().await.unwrap().unwrap();
                assert_eq!(
                    reply,
                    Frame::ChannelControl(CHANNEL, ChannelControlHeader::Open)
                );

                // get adms
                let adm = rx.next().await.unwrap().unwrap();
                assert_eq!(
                    adm,
                    Frame::ChannelControl(CHANNEL, ChannelControlHeader::Admit(1))
                );

                // send adm
                tx.send(Frame::ChannelControl(
                    CHANNEL,
                    ChannelControlHeader::Admit(BUFFER as u8),
                ))
                .await
                .unwrap();

                for _ in 0..BUFFER {
                    let f = rx.next().await.unwrap().unwrap();
                    assert_eq!(
                        f,
                        Frame::ChannelData(CHANNEL, Bytes::from_static(b"foobar").into())
                    );
                }
            },
        )
        .await
    }

    #[tokio::test]
    #[test_log::test]
    async fn open_channel_and_send_data() {
        const CHANNEL: ChannelId = ChannelId(10);
        const BUFFER: usize = 128;

        run_double_test(
            |sut| async move {
                println!("sut: opening ch");
                let (ch, r) = tokio::join!(sut.open_channel(CHANNEL, BUFFER), sut.rxtx_round(),);
                let (mut ch, _) = (ch.unwrap(), r.unwrap());
                println!("sut: ch is open");

                // exchange adms
                sut.rxtx_round().await.unwrap();

                // read the rest of it
                for _ in 0..BUFFER {
                    let (bs, r) = tokio::join!(ch.next(), sut.rx_round());
                    bs.unwrap();
                    r.unwrap();
                }
            },
            |sut| async move {
                println!("sut: opening ch");
                let (ch, r) = tokio::join!(sut.open_channel(CHANNEL, BUFFER), sut.rxtx_round(),);
                let (mut ch, _) = (ch.unwrap(), r.unwrap());
                println!("sut: ch is open");

                // exchange adms
                sut.rxtx_round().await.unwrap();

                // buffer sends
                for _ in 0..BUFFER {
                    ch.send(Bytes::from_static(b"foobar")).await.unwrap();
                }

                // actually send
                for _ in 0..BUFFER {
                    sut.tx_round().await.unwrap();
                }
            },
        )
        .await
    }

    #[ignore]
    #[tokio::test]
    #[test_log::test]
    async fn backpressured_sends() {
        const CHANNEL: ChannelId = ChannelId(10);
        const COUNT: usize = 128;
        const BUFFER: usize = 4;

        run_double_test(
            |sut| async move {
                let bg = tokio::spawn(sut.rxtx_loop());

                println!("sut: opening ch");
                let mut ch = sut.open_channel(CHANNEL, BUFFER).await.unwrap();
                println!("sut: ch is open");

                for _ in 0..COUNT {
                    let f = ch.next().await.unwrap();
                    assert_eq!(f, Bytes::from_static(b"foobar"));
                }
                bg.abort();
            },
            |sut| async move {
                let bg = tokio::spawn(sut.rxtx_loop());

                println!("sut: opening ch");
                let mut ch = sut.open_channel(CHANNEL, BUFFER).await.unwrap();
                println!("sut: ch is open");

                for _ in 0..COUNT {
                    ch.send(Bytes::from_static(b"foobar")).await.unwrap();
                }
                bg.abort();
            },
        )
        .await
    }

    #[ignore]
    #[tokio::test]
    #[test_log::test]
    async fn backpressure_works() {
        const CHANNEL: ChannelId = ChannelId(10);
        const BUFFER: usize = 4;
        let barrier = Arc::new(Barrier::new(2));
        let barrier2 = barrier.clone();

        run_double_test(
            move |sut| async move {
                println!("sut: opening ch");
                let (ch, r) = tokio::join!(sut.open_channel(CHANNEL, BUFFER), sut.rxtx_round(),);
                let (mut ch, _) = (ch.unwrap(), r.unwrap());
                println!("sut: ch is open");

                // exchange adms
                sut.rxtx_round().await.unwrap();

                // read the full buffer
                for _ in 0..BUFFER {
                    let (bs, r) = tokio::join!(ch.next(), sut.rx_round());
                    r.unwrap();
                    assert_eq!(bs.unwrap(), Bytes::from_static(b"before the line"));
                }

                // wait for the next thing
                barrier.wait().await;

                // send adm
                sut.tx_round().await.unwrap();

                // get the last message
                let (bs, r) = tokio::join!(ch.next(), sut.rx_round());
                r.unwrap();
                assert_eq!(bs.unwrap(), Bytes::from_static(b"OVER THE LINE"));
            },
            move |sut| async move {
                let barrier = barrier2;
                println!("sut: opening ch");
                let (ch, r) = tokio::join!(sut.open_channel(CHANNEL, BUFFER), sut.rxtx_round(),);
                let (mut ch, _) = (ch.unwrap(), r.unwrap());
                println!("sut: ch is open");

                // exchange adms
                sut.rxtx_round().await.unwrap();

                // fill buffer with sends
                for _ in 0..BUFFER {
                    ch.send(Bytes::from_static(b"before the line"))
                        .await
                        .unwrap();
                }

                // this one should go over the buffer
                let mut fut = ch.send(Bytes::from_static(b"OVER THE LINE"));
                assert_matches!(
                    fut.poll_unpin(&mut Context::from_waker(Waker::noop())),
                    Poll::Pending
                );

                // actually send and drain the buffer
                for _ in 0..BUFFER {
                    sut.tx_round().await.unwrap();
                }

                barrier.wait().await;

                // now this should go through
                fut.await.unwrap();
                sut.tx_round().await.unwrap();
            },
        )
        .await
    }
}
