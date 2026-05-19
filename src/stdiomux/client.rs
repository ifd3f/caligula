use std::{convert::Infallible, rc::Rc, sync::Arc};

use bytes::Bytes;
use futures::{
    FutureExt, Stream, StreamExt, TryFutureExt, TryStreamExt,
    stream::{BoxStream, LocalBoxStream},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{
        SetOnce,
        mpsc::{UnboundedSender, unbounded_channel},
    },
    try_join,
};
use tracing::{Instrument, debug_span, info_span};

use crate::stdiomux::{
    BytestreamService,
    channel_map::ChannelMap,
    common::{
        drive_channel_tx, drive_rx, drive_tx, inject_err_fut, inject_err_stream,
        inject_err_stream_ok, rx_stream,
    },
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
    let channel_map = Rc::new(ChannelMap::new());
    let (txq_tx, txq_rx) = unbounded_channel();

    let rx_driver = drive_rx(
        channel_map.clone(),
        inject_err_stream_ok(
            rx_stream(rx).map_err(|e| ClientError::Rx(Arc::new(e))),
            err_notify.clone(),
        ),
        service_fn(|_| -> LocalBoxStream<'static, Result<Bytes, Infallible>> {
            panic!("Client received an unexpected channel!")
        }),
        |_: Infallible| panic!("infallible error can never happen"),
        txq_tx.clone(),
    )
    .map(|_| Ok(()));

    let tx_driver = inject_err_fut(
        drive_tx(txq_rx, tx).map_err(|e| ClientError::Tx(Arc::new(e))),
        err_notify.clone(),
    );

    let driver =
        async move { try_join!(rx_driver, tx_driver).map(|_| ()) }.instrument(info_span!("driver"));

    let client = LocalBytestreamClient {
        channel_map,
        err_notify,
        txq: txq_tx,
    };

    (client, driver)
}

/// A client to a remote [`BytestreamClient`] over a transport. Created using
/// the [`open()`] function.
pub struct LocalBytestreamClient {
    channel_map: Rc<ChannelMap>,
    err_notify: Arc<SetOnce<ClientError>>,
    txq: UnboundedSender<(u16, Bytes)>,
}

impl<Req: Stream<Item = Bytes> + Unpin + 'static> BytestreamService<Req> for LocalBytestreamClient {
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
