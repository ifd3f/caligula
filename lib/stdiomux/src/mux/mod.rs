pub mod basic;

use bytes::Bytes;
use futures::{Stream, stream::BoxStream};
use tower_service::Service;

/// Type alias for a [`BoxStream<'static, Bytes>`].
pub type BoxByteStream = BoxStream<'static, Bytes>;

/// Trait alias for a [`Service`] that accepts byte streams and returns byte streams.
pub trait ByteStreamService<Req>: Service<Req>
where
    Req: Stream<Item = Bytes>,
{
    type Response: Stream<Item = Bytes>;
}

impl<S, Req, Res> ByteStreamService<Req> for S
where
    S: Service<Req, Response = Res>,
    Req: Stream<Item = Bytes>,
    Res: Stream<Item = Bytes>,
{
    type Response = Res;
}
