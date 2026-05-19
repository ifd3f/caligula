use std::sync::Arc;

use bytes::Bytes;
use futures::{TryFutureExt, TryStreamExt, stream::LocalBoxStream};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::SetOnce,
};
use tracing::{Instrument, info_span};

use crate::stdiomux::BytestreamService;

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
pub fn open<R, W>(
    rx: R,
    tx: W,
) -> (
    BytestreamClient<R, W>,
    impl Future<Output = Result<(), ClientError>>,
)
where
    R: AsyncRead + Unpin + 'static,
    W: AsyncWrite + Unpin + 'static,
{
    (
        BytestreamClient {
            comm: Some((rx, tx)).into(),
        },
        std::future::pending(),
    )
}

/// A client to a remote [`BytestreamClient`] over a transport. Created using
/// the [`open()`] function.
///
/// Technically speaking, it only supports one request right now, and explodes
/// afterwards, but that's okay! Refactors will come Soon(tm).
pub struct BytestreamClient<R, W>
where
    R: AsyncRead + Unpin + 'static,
    W: AsyncWrite + Unpin + 'static,
{
    comm: std::sync::Mutex<Option<(R, W)>>,
}

struct Inner {
    /// Next request ID.
    next_req_id: u32,
}

impl<R, W> BytestreamService for BytestreamClient<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + 'static,
{
    type Error = ClientError;

    #[tracing::instrument(skip_all, name = "BytestreamClient_call")]
    fn call(
        &self,
        req: LocalBoxStream<'static, Bytes>,
    ) -> LocalBoxStream<'static, Result<Bytes, Self::Error>> {
        tracing::trace!("making a call");
        let (rx, tx) =
            self.comm.lock().unwrap().take().expect(
                "called more than once! multiple requests are not currently supported! sowwy!",
            );

        let err_notify_arc = Arc::new(SetOnce::<ClientError>::new());

        let _tx = tokio::task::spawn_local(
            inject_err_fut(
                drive_tx(tx, req).map_err(ClientError::Tx),
                err_notify_arc.clone(),
            )
            .instrument(info_span!("txdriver")),
        );

        let err_notify = err_notify_arc;
        let stream = inject_err_stream(drive_rx(rx).map_err(ClientError::Rx), err_notify);

        Box::pin(stream)
    }
}
