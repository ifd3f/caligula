use std::error::Error;

use bincode::Options as _;
use bytes::Bytes;
use futures::Stream;
pub use preamble::{preamble_request_client, preamble_request_server};
pub use sync::{RemoteThreadBytestreamClient, make_remote};

use crate::util::stdiomux::StreamService;

mod preamble;
mod sync;

/// Construct a [`StreamService`] from a function.
pub fn service_fn<F, E, Req, Res>(f: F) -> ServiceFn<F>
where
    F: Fn(Req) -> Res,
    E: Error,
    Res: Stream<Item = Result<Bytes, E>>,
{
    ServiceFn(f)
}

/// A [`StreamService`] built off of a simple function.
pub struct ServiceFn<F>(F);

impl<F: Clone> Clone for ServiceFn<F> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<F, E, Req, Res> StreamService<Req> for ServiceFn<F>
where
    F: Fn(Req) -> Res,
    E: Error,
    Res: Stream<Item = Result<Bytes, E>>,
{
    type Error = E;
    type Response = Res;

    fn call(&self, req: Req) -> Self::Response {
        (self.0)(req)
    }
}

/// An error that may have happened either in the application, or the transport
/// layer below it.
#[derive(Debug, thiserror::Error)]
pub enum LayerError<App, Trans> {
    #[error("Application error: {0}")]
    App(App),
    #[error("Transport error: {0}")]
    Transport(Trans),
}

/// Given a [`Result`] with [`LayerError`]s, eliminate the [`LayerError`]s by
/// rotating the application-level errors into the [`Ok`].
pub fn rotate_layer_error<T, App, Trans>(
    res: Result<T, LayerError<App, Trans>>,
) -> Result<Result<T, App>, Trans> {
    match res {
        Ok(x) => Ok(Ok(x)),
        Err(LayerError::App(app)) => Ok(Err(app)),
        Err(LayerError::Transport(trans)) => Err(trans),
    }
}

/// Common bincode options to use for inter-process communication.
#[inline]
fn bincode_options() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_native_endian()
        .with_limit(1024)
}
