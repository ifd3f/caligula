use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::Stream;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    mux::{ByteStream, ByteStreamService, basic::client},
    utils::{HandshakeError, exchange_handshake, make_hello_with_crate_version},
};

pub(crate) const HELLO: &[u8] = make_hello_with_crate_version!("basic mux server");

pub struct BasicMuxServer<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    r: R,
    w: W,
}

impl<R, W> BasicMuxServer<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub async fn open(mut r: R, mut w: W) -> Result<Self, HandshakeError> {
        exchange_handshake(&mut r, &mut w, HELLO, client::HELLO).await?;
        Ok(Self { r, w })
    }

    pub async fn run_with<S>(&mut self, s: S) -> Result<(), std::io::Error>
    where
        S: ByteStreamService<RequestStream, ByteStream>,
    {
        todo!()
    }
}

pub struct RequestStream;

impl Stream for RequestStream {
    type Item = Bytes;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        todo!()
    }
}
