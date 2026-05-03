use std::{
    convert::Infallible,
    fmt::Debug,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{FutureExt, Stream, StreamExt, stream};
use tokio::sync::{
    mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    oneshot,
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tower::Service;

use super::action::*;
use crate::mux::ByteStream;

pub async fn test_single_channel<C, F, Fut>(
    mut mux_client: C,
    run_server: F,
    actions: impl IntoIterator<Item = SidedAction<ChannelAction>>,
) where
    C: Service<ByteStream, Response = ByteStream> + Sync,
    C::Error: Debug,
    F: FnOnce(TestServer) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let (req_stream_tx, req_stream_rx) = unbounded_channel();
    let (res_stream_tx, res_stream_rx) = unbounded_channel();

    let (server_req_tx, server_req_rx) = oneshot::channel();

    let server = tokio::spawn(run_server(TestServer {
        inner: Arc::new(Mutex::new(Some(Inner {
            res_stream_rx,
            server_req_tx,
        }))),
    }));

    let mut client_res = mux_client
        .call(Box::pin(UnboundedReceiverStream::new(req_stream_rx)))
        .await
        .expect("request open failed");

    enum ServerReqCell {
        StreamNotSent(oneshot::Receiver<ByteStream>),
        StreamSent(ByteStream),
    }

    let mut server_req: ByteStream = Box::pin(stream::unfold(
        ServerReqCell::StreamNotSent(server_req_rx),
        move |c| async move {
            let mut stream = match c {
                ServerReqCell::StreamNotSent(receiver) => receiver
                    .await
                    .expect("server dropped their stream oneshot sender handle"),
                ServerReqCell::StreamSent(stream) => stream,
            };
            stream
                .next()
                .await
                .map(|x| (x, ServerReqCell::StreamSent(stream)))
        },
    ));

    let mut req_stream_tx = Some(req_stream_tx);
    let mut res_stream_tx = Some(res_stream_tx);
    for a in actions {
        match a {
            SidedAction::Client(a) => {
                execute_action_on_channel(&mut req_stream_tx, &mut client_res, a).await
            }
            SidedAction::Server(a) => {
                execute_action_on_channel(&mut res_stream_tx, &mut server_req, a).await
            }
        }
    }

    server.abort();
}

async fn execute_action_on_channel(
    tx: &mut Option<UnboundedSender<Bytes>>,
    rx: &mut ByteStream,
    a: ChannelAction,
) {
    match a {
        ChannelAction::Tx(bytes) => tx
            .as_ref()
            .expect("unexpectedly dropped tx already! error in channel action sequence generation")
            .send(bytes)
            .expect("failed to send tx!"),
        ChannelAction::Rx(bytes) => assert_eq!(rx.next().await.expect("failed to receive!"), bytes),
        ChannelAction::CloseTx => drop(tx.take()),
        ChannelAction::AssertRxClosed => assert!(rx.next().await.is_none()),
    }
}

#[derive(Clone)]
pub struct TestServer {
    inner: Arc<Mutex<Option<Inner>>>,
}
struct Inner {
    res_stream_rx: UnboundedReceiver<Bytes>,
    server_req_tx: oneshot::Sender<ByteStream>,
}

impl<BS> Service<BS> for TestServer
where
    BS: Stream<Item = Bytes> + Send + 'static,
{
    type Response = ByteStream;

    type Error = Infallible;

    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: BS) -> Self::Future {
        let inner = self
            .inner
            .lock()
            .unwrap()
            .take()
            .expect("server got requests multiple times");

        let Ok(()) = inner.server_req_tx.send(Box::pin(req)) else {
            panic!("harness dropped our oneshot handle");
        };
        std::future::ready(Ok::<ByteStream, Infallible>(Box::pin(
            UnboundedReceiverStream::new(inner.res_stream_rx),
        )))
    }
}
