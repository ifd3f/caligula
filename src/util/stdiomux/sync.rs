use std::error::Error;

use bytes::Bytes;
use futures::{
    StreamExt as _,
    stream::{self, BoxStream},
};
use tokio::{
    sync::{mpsc, oneshot},
    task::spawn_local,
};

use crate::util::{stdiomux::BytestreamService, stream::forward_stream_remotely};

/// Given a [`BytestreamService`] that might not be thread-safe, creates a
/// handle for it that is thread-safe and a driver that must be run on the
/// current thread.
pub fn make_remote<S, Req>(
    svc: S,
    buffer: usize,
) -> (
    RemoteThreadBytestreamClient<Req, S::Error>,
    impl Future<Output = ()>,
)
where
    Req: Send + 'static,
    S: BytestreamService<Req> + 'static,
    S::Error: Send + Error + 'static,
{
    // mailbox for adding requests to the local thread
    let (mailbox_tx, mut mailbox_rx) = mpsc::channel(16);
    let handle = RemoteThreadBytestreamClient { tx: mailbox_tx };

    let actor = async move {
        while let Some((req, reply)) = mailbox_rx.recv().await {
            // make the request to the wrapped service
            let (res, fut) = forward_stream_remotely(svc.call(req), buffer);

            // spawn a local handler for it to forward stream items into the mpsc
            let _reqhandler = spawn_local(fut);

            // now that it's all set up, send the reply
            let Ok(()) = reply.send(res) else {
                // other end dropped, we should quit
                break;
            };
        }
    };

    (handle, actor)
}

/// A client to a [`BytestreamService`] on a different thread. Created using
/// the [`make_remote()`] function.
pub struct RemoteThreadBytestreamClient<Req, Err> {
    tx: mpsc::Sender<(Req, oneshot::Sender<BoxStream<'static, Result<Bytes, Err>>>)>,
}

impl<Req, Err> BytestreamService<Req> for RemoteThreadBytestreamClient<Req, Err>
where
    Req: Send + 'static,
    Err: Send + Error + 'static,
{
    type Error = Err;
    type Response = BoxStream<'static, Result<Bytes, Self::Error>>;

    #[tracing::instrument(skip_all, name = "RemoteThreadBytestreamClient_call")]
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
