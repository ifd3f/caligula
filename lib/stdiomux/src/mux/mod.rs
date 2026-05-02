pub mod basic;

use bytes::Bytes;
use futures::{Stream, stream::BoxStream};
use tower_service::Service;

/// Type alias for a [`BoxStream<'static, Bytes>`].
pub type ByteStream = BoxStream<'static, Bytes>;

/// Trait alias for a [`Service`] that accepts byte streams and returns byte streams.
pub trait ByteStreamService<Req, Res>: Service<Req, Response = Res>
where
    Req: Stream<Item = Bytes>,
    Res: Stream<Item = Bytes>,
{
}

impl<S, Req, Res> ByteStreamService<Req, Res> for S
where
    S: Service<Req, Response = Res>,
    Req: Stream<Item = Bytes>,
    Res: Stream<Item = Bytes>,
{
}
