use std::{error::Error, sync::Arc};

use futures::{StreamExt, TryFutureExt, TryStreamExt};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::SetOnce,
};

use super::BytestreamService;

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

/// Run a [`BytestreamService`] over the given transport.
///
/// TODO: make it support multiple requests and responses
#[tracing::instrument(skip_all, name = "BytestreamServer_run")]
pub async fn run<R, W, S, E>(rx: R, tx: W, s: S) -> Result<(), ServerError<E>>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
    S: BytestreamService<Error = E>,
    E: Error + Send + Sync + 'static,
{
    let err_notify = Arc::new(SetOnce::<ServerError<E>>::new());

    // handle errors using inject_err_stream and only send the Ok's into the service
    let req = inject_err_stream(drive_rx(rx).map_err(ServerError::Rx), err_notify.clone());
    let req = req.filter_map(|r| std::future::ready(r.ok()));

    // make the call to the service
    let res = s.call(Box::pin(req));

    // now do the same thing on the outgoing end
    let res = inject_err_stream(
        res.map_err(|e| ServerError::Service(Arc::new(e))),
        err_notify.clone(),
    );
    let res = res.filter_map(|r| std::future::ready(r.ok()));

    let fut = drive_tx(tx, res).map_err(|e| ServerError::Tx(e));
    inject_err_fut(fut, err_notify).await?;
    Ok(())
}
