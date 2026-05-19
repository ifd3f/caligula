use std::{error::Error, sync::Arc};

use bytes::Bytes;
use futures::stream::LocalBoxStream;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    select,
    sync::SetOnce,
};
use tracing::{Instrument, info_span};

use super::{BytestreamService, common::common_driver};

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

    let (fut, _, _) = common_driver(
        rx,
        tx,
        s,
        {
            let en = err_notify.clone();
            move |e| {
                en.set(ServerError::Rx(e.into())).ok();
            }
        },
        {
            let en = err_notify.clone();
            move |e| {
                en.set(ServerError::Tx(e.into())).ok();
            }
        },
        {
            let en = err_notify.clone();
            move |e| {
                en.set(ServerError::Service(e.into())).ok();
            }
        },
    );

    let driver = async move {
        select! {
           _ = fut => Ok(()),
           e = err_notify.wait() => Err(e.clone()),
        }
    }
    .instrument(info_span!("driver"));

    driver.await
}
