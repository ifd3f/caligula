use std::{convert::Infallible, rc::Rc, sync::Arc};

use bytes::Bytes;
use futures::{
    Stream, StreamExt,
    stream::{BoxStream, LocalBoxStream},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    select,
    sync::{SetOnce, mpsc::Sender},
};
use tracing::{Instrument, debug_span, info_span};

use super::{
    BytestreamService,
    channel_map::ChannelMap,
    common::{common_driver, drive_channel_tx, inject_err_stream},
    service_fn,
};

#[derive(Debug, thiserror::Error, Clone)]
pub enum ClientError {
    #[error("Error receiving data: {0}")]
    Rx(Arc<std::io::Error>),
    #[error("Error sending data: {0}")]
    Tx(Arc<std::io::Error>),
}

/// Open a [`BytestreamClient`] over the given transport. Returns the client
/// itself, along with a driver future that must be polled in the background in
/// order for requests and responses to be handled.
#[tracing::instrument(skip_all, name = "stdiomux_client")]
pub fn open<R, W>(
    rx: R,
    tx: W,
) -> (
    LocalBytestreamClient,
    impl Future<Output = Result<(), ClientError>>,
)
where
    R: AsyncRead + Unpin + 'static,
    W: AsyncWrite + Unpin + 'static,
{
    let err_notify = Arc::new(SetOnce::<ClientError>::new());

    let (fut, channel_map, txq) = common_driver(
        rx,
        tx,
        service_fn(|_| -> LocalBoxStream<'static, Result<Bytes, Infallible>> {
            panic!("Client received an unexpected channel!")
        }),
        {
            let en = err_notify.clone();
            move |e| {
                en.set(ClientError::Rx(e.into())).ok();
            }
        },
        {
            let en = err_notify.clone();
            move |e| {
                en.set(ClientError::Tx(e.into())).ok();
            }
        },
        move |_| panic!("infallible error can never happen"),
    );

    let en = err_notify.clone();
    let driver = async move {
        select! {
           _ = fut => Ok(()),
           e = en.wait() => Err(e.clone()),
        }
    }
    .instrument(info_span!("driver"));

    let client = LocalBytestreamClient {
        channel_map,
        err_notify,
        txq,
    };

    (client, driver)
}

/// A client to a remote [`BytestreamClient`] over a transport. Created using
/// the [`open()`] function.
pub struct LocalBytestreamClient {
    channel_map: Rc<ChannelMap>,
    err_notify: Arc<SetOnce<ClientError>>,
    txq: Sender<(u16, Bytes)>,
}

impl<Req> BytestreamService<Req> for LocalBytestreamClient
where
    Req: Stream<Item = Bytes> + Unpin + 'static,
{
    type Error = ClientError;
    type Response = BoxStream<'static, Result<Bytes, Self::Error>>;

    #[tracing::instrument(skip_all, name = "stdiomux_client_call")]
    fn call(&self, req: Req) -> Self::Response {
        let (ch, rx) = self
            .channel_map
            .alloc_new_channel()
            .expect("ran out of channels!");

        tokio::task::spawn_local(
            drive_channel_tx(ch, req, self.txq.clone())
                .instrument(debug_span!("stdiomux_client_drive_tx")),
        );

        Box::pin(inject_err_stream(
            rx.map(Ok::<_, ClientError>),
            self.err_notify.clone(),
        ))
    }
}
