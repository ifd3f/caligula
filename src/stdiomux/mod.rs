//! Utilities for multiplexed streamed RPC requests.

use std::error::Error;

use auto_impl::auto_impl;
use bytes::Bytes;
use futures::Stream;

mod channel_map;
pub mod client;
pub mod server;
pub use sync::{RemoteThreadBytestreamClient, make_remote};
mod common;
mod sync;

#[cfg(test)]
mod tests;

/// A service that, when called with a stream of bytes, returns another stream
/// of bytes.
#[auto_impl(&, Box, Rc, Arc)]
pub trait BytestreamService<Req: Stream<Item = Bytes>> {
    /// Error this service may return.
    type Error: Error;

    /// Response stream that this service returns.
    type Response: Stream<Item = Result<Bytes, Self::Error>>;

    /// Call this service.
    fn call(&self, req: Req) -> Self::Response;
}

/// Construct a [`BytestreamService`] from a function.
pub fn service_fn<F, E, Req, Res>(f: F) -> ServiceFn<F>
where
    F: Fn(Req) -> Res,
    E: Error,
    Req: Stream<Item = Bytes>,
    Res: Stream<Item = Result<Bytes, E>>,
{
    ServiceFn(f)
}

/// A [`BytestreamService`] built off of a simple function.
pub struct ServiceFn<F>(F);

impl<F, E, Req, Res> BytestreamService<Req> for ServiceFn<F>
where
    F: Fn(Req) -> Res,
    E: Error,
    Req: Stream<Item = Bytes>,
    Res: Stream<Item = Result<Bytes, E>>,
{
    type Error = E;
    type Response = Res;

    fn call(&self, req: Req) -> Self::Response {
        (self.0)(req)
    }
}
