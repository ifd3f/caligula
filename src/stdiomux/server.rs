use std::{error::Error, rc::Rc, sync::Arc};

use bytes::Bytes;
use futures::{FutureExt as _, TryFutureExt, TryStreamExt, stream::LocalBoxStream};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{SetOnce, mpsc::unbounded_channel},
    try_join,
};
use tracing::{Instrument, info_span};

use super::BytestreamService;
use crate::stdiomux::{
    channel_map::ChannelMap,
    common::{drive_rx, drive_tx, inject_err_fut, inject_err_stream_ok, rx_stream},
};

#[derive(Debug, thiserror::Error)]
pub enum ServerError<E: Error> {
    #[error("Error receiving data: {0}")]
    Rx(Arc<std::io::Error>),
    #[error("Error sending data: {0}")]
    Tx(Arc<std::io::Error>),
    #[error("Error in service: {0}")]
    Service(Arc<E>),
}

impl<E: Error> Clone for ServerError<E> {
    fn clone(&self) -> Self {
        match self {
            Self::Rx(arg0) => Self::Rx(arg0.clone()),
            Self::Tx(arg0) => Self::Tx(arg0.clone()),
            Self::Service(arg0) => Self::Service(arg0.clone()),
        }
    }
}

/// Run a [`BytestreamService`] as a server over the given transport.
#[tracing::instrument(skip_all, name = "stdiomux_server")]
pub async fn run<R, W, S>(rx: R, tx: W, s: S) -> Result<(), ServerError<S::Error>>
where
    R: AsyncRead + Unpin + 'static,
    W: AsyncWrite + Unpin + 'static,
    S: BytestreamService<LocalBoxStream<'static, Bytes>>,
    S::Response: Unpin + 'static,
    S::Error: Error + 'static,
{
    let err_notify = Arc::new(SetOnce::<ServerError<S::Error>>::new());
    let channel_map = Rc::new(ChannelMap::new());
    let (txq_tx, txq_rx) = unbounded_channel();

    let svc_err_handler = {
        let err_notify = err_notify.clone();
        move |e: S::Error| {
            err_notify.set(ServerError::Service(Arc::new(e))).ok();
        }
    };
    let rx_driver = drive_rx(
        channel_map.clone(),
        inject_err_stream_ok(
            rx_stream(rx).map_err(|e| ServerError::Rx(Arc::new(e))),
            err_notify.clone(),
        ),
        s,
        svc_err_handler,
        txq_tx.clone(),
    )
    .map(|_| Ok(()));

    let tx_driver = inject_err_fut(
        drive_tx(txq_rx, tx).map_err(|e| ServerError::Tx(Arc::new(e))),
        err_notify.clone(),
    );

    async move { try_join!(rx_driver, tx_driver) }
        .instrument(info_span!("driver"))
        .await
        .map(|_| ())
}
