//! Utilities for multiplexed streamed RPC requests.

use std::error::Error;

use auto_impl::auto_impl;
use bytes::Bytes;
use futures::Stream;

mod channel_map;
pub mod client;
mod common;
pub mod server;
pub mod util;

#[cfg(test)]
mod tests;

/// A service that, when called with a generic request type, returns a stream of
/// bytes.
#[auto_impl(Box, Rc, Arc)]
pub trait StreamService<Req> {
    /// Error this service may return.
    type Error: Error;

    /// Response stream that this service returns.
    type Response: Stream<Item = Result<Bytes, Self::Error>>;

    /// Call this service.
    fn call(&self, req: Req) -> Self::Response;
}

impl<S, Req> StreamService<Req> for &S
where
    S: StreamService<Req>,
{
    type Error = S::Error;
    type Response = S::Response;

    fn call(&self, req: Req) -> Self::Response {
        S::call(self, req)
    }
}
