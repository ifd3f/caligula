use std::marker::PhantomData;

use bytes::Bytes;
use futures::{
    StreamExt as _,
    stream::{self, BoxStream},
};
use tokio::sync::{mpsc, oneshot};

use crate::util::stdiomux::BytestreamService;

/// Given a [`BytestreamService`] that might not be thread-safe, creates a
/// handle for it that is thread-safe and a driver that must be run on the
/// current thread.
pub fn make_remote<S, Req>(
    svc: S,
) -> (
    RemoteThreadBytestreamClient<S, Req>,
    impl Future<Output = ()>,
)
where
    Req: Send + 'static,
    S: BytestreamService<Req>,
    S::Error: Send + 'static,
    S::Response: Send + 'static,
{
    let (tx, mut rx) = mpsc::channel(1);

    let handle = RemoteThreadBytestreamClient {
        tx,
        _phantom: PhantomData,
    };

    let fut = async move {
        while let Some((req, reply)) = rx.recv().await {
            let Ok(()) = reply.send(svc.call(req)) else {
                break;
            };
        }
    };

    (handle, fut)
}

/// A client to a [`BytestreamService`] on a different thread. Created using
/// the [`make_remote()`] function.
pub struct RemoteThreadBytestreamClient<S, Req>
where
    Req: Send + 'static,
    S: BytestreamService<Req>,
    S::Error: Send + 'static,
    S::Response: Send + 'static,
{
    tx: mpsc::Sender<(Req, oneshot::Sender<S::Response>)>,
    _phantom: PhantomData<fn() -> S>,
}

impl<Req, S> BytestreamService<Req> for RemoteThreadBytestreamClient<S, Req>
where
    Req: Send + 'static,
    S: BytestreamService<Req>,
    S::Error: Send + 'static,
    S::Response: Send + 'static,
{
    type Error = S::Error;
    type Response = BoxStream<'static, Result<Bytes, Self::Error>>;

    #[tracing::instrument(skip_all, name = "LocalBytestreamClient_call")]
    fn call(&self, req: Req) -> BoxStream<'static, Result<Bytes, Self::Error>> {
        let tx = self.tx.clone();

        stream::once(async move {
            let (reply_tx, reply_rx) = oneshot::channel();
            tx.send((req, reply_tx)).await.unwrap();
            reply_rx.await.unwrap()
        })
        .flatten()
        .boxed()
    }
}
