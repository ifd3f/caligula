//! Utilities for multiplexed streamed RPC requests.

use std::error::Error;

use auto_impl::auto_impl;
use bytes::Bytes;
use futures::Stream;
pub use preamble::{PreambleRequestClient, preamble_request_client, preamble_request_server};

mod channel_map;
pub mod client;
mod common;
mod preamble;
pub mod server;

#[cfg(test)]
mod tests;

/// A service that, when called with a generic request type, returns a stream of
/// bytes.
#[auto_impl(&, Box, Rc, Arc)]
pub trait BytestreamService<Req> {
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
    Res: Stream<Item = Result<Bytes, E>>,
{
    type Error = E;
    type Response = Res;

    fn call(&self, req: Req) -> Self::Response {
        (self.0)(req)
    }
}
