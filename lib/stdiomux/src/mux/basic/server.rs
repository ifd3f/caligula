use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::Stream;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::mux::{ByteStream, ByteStreamService};

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
    pub async fn open(r: R, w: W) -> Result<Self, std::io::Error> {
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
