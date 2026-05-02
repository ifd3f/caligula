use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Sink, Stream};
use pin_project::pin_project;

use crate::mux::ChannelHandle;

pub trait ChannelHandleExt: ChannelHandle {
    /// Create a future that receives an item on the channel.
    fn send(&self, data: Bytes) -> SendToChannel<'_, Self> {
        SendToChannel {
            ch: self,
            data: data,
        }
    }

    /// Create a future that sends an item to the channel.
    fn recv(&self) -> RecvFromChannel<'_, Self> {
        RecvFromChannel { ch: self }
    }

    /// Convert this [`ChannelHandle`] into an object that implements both
    /// [`Sink`] and [`Stream`].
    fn into_stream(self) -> ChannelIo<Self> {
        ChannelIo {
            ch: self,
            send: VecDeque::new(),
        }
    }
}

impl<H: ChannelHandle> ChannelHandleExt for H {}

pub struct RecvFromChannel<'a, H> {
    ch: &'a H,
}

impl<'a, H: ChannelHandle> Future for RecvFromChannel<'a, H> {
    type Output = Result<Bytes, H::ClosedReason>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.ch.poll_recv(cx)
    }
}

pub struct SendToChannel<'a, H> {
    ch: &'a H,
    data: Bytes,
}

impl<'a, H: ChannelHandle> Future for SendToChannel<'a, H> {
    type Output = Result<(), H::ClosedReason>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.ch.poll_send(cx, &self.data)
    }
}

/// Wraps a [`ChannelHandle`] and implements [`Sink`] and [`Stream`].
#[pin_project]
pub struct ChannelIo<H> {
    ch: H,
    send: VecDeque<Bytes>,
}

impl<H> ChannelIo<H> {
    pub fn inner(&self) -> &H {
        &self.ch
    }

    pub fn inner_mut(&mut self) -> &mut H {
        &mut self.ch
    }

    pub fn into_inner(self) -> H {
        self.ch
    }
}

impl<H: ChannelHandle> Sink<Bytes> for ChannelIo<H> {
    type Error = H::ClosedReason;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.ch.assert_open()?;
        Poll::Ready(Ok(()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        self.ch.assert_open()?;
        self.send.push_back(item);
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        let send = this.send;
        while let Some(x) = send.pop_front() {
            match this.ch.poll_send(cx, &x) {
                Poll::Ready(r) => {
                    return Poll::Ready(r);
                }
                Poll::Pending => {
                    send.push_front(x);
                    return Poll::Pending;
                }
            }
        }
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.poll_flush(cx)
    }
}

impl<H: ChannelHandle> Stream for ChannelIo<H> {
    type Item = Bytes;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.ch.poll_recv(cx).map(|x| x.ok())
    }
}
