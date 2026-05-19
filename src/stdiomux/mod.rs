//! Utilities for multiplexed streamed RPC requests.

use std::error::Error;

use auto_impl::auto_impl;
use bytes::Bytes;
use futures::stream::LocalBoxStream;

mod channel_map;
pub mod client;
pub mod server;
mod util;

#[cfg(test)]
mod tests;

/// A service that, when called with a stream of bytes, returns another stream
/// of bytes.
#[auto_impl(&, Box, Rc, Arc)]
pub trait BytestreamService {
    /// Error this service may return.
    type Error: Error;

    /// Call this service.
    fn call(
        &self,
        req: LocalBoxStream<'static, Bytes>,
    ) -> LocalBoxStream<'static, Result<Bytes, Self::Error>>;
}

/// Construct a [`BytestreamService`] from a function.
pub fn service_fn<F, E>(f: F) -> ServiceFn<F>
where
    F: Fn(LocalBoxStream<'static, Bytes>) -> LocalBoxStream<'static, Result<Bytes, E>>,
{
    ServiceFn(f)
}

/// A [`BytestreamService`] built off of a simple function.
pub struct ServiceFn<F>(F);

impl<F, E> BytestreamService for ServiceFn<F>
where
    F: Fn(LocalBoxStream<'static, Bytes>) -> LocalBoxStream<'static, Result<Bytes, E>>,
    E: Error,
{
    type Error = E;

    fn call(
        &self,
        req: LocalBoxStream<'static, Bytes>,
    ) -> LocalBoxStream<'static, Result<Bytes, Self::Error>> {
        (self.0)(req)
    }
}
