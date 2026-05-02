use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Stream, future::Ready};
use tokio::io::{AsyncRead, AsyncWrite};
use tower_service::Service;

use crate::mux::ByteStream;

pub struct BasicMuxClient {}

pub struct ResponseStream {}

impl Stream for ResponseStream {
    type Item = Bytes;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        todo!()
    }
}

pub struct BasicMuxClientDriver<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    r: R,
    w: W,
}

impl<R, W> Future for BasicMuxClientDriver<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    type Output = Result<(), std::io::Error>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        todo!()
    }
}

pub async fn open_client<R, W>(
    r: R,
    w: W,
) -> Result<(BasicMuxClient, BasicMuxClientDriver<R, W>), std::io::Error>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    todo!()
}

impl Service<ByteStream> for BasicMuxClient {
    type Response = ResponseStream;

    type Error = std::io::Error;

    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        todo!()
    }

    fn call(&mut self, req: ByteStream) -> Self::Future {
        todo!()
    }
}
