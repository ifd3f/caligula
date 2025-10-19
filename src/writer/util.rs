use std::{
    io::Read,
    sync::{Arc, Mutex},
};

use pin_project::pin_project;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};

pub struct AsyncifiedRead<R: Read> {
    data: Arc<Mutex<Vec<u8>>>,
    r: R,
}

impl<R: Read> AsyncifiedRead<R> {
    pub fn new(r: R) -> Self {
        Self {
            data: Arc::new(Mutex::new(vec![0u8; 16384])),
            r,
        }
    }
}

impl<R: Read> AsyncRead for AsyncifiedRead<R> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        todo!()
    }
}

/// Wraps a reader and counts how many bytes we've read in total, without
/// making any system calls.
#[pin_project]
pub struct AsyncCountRead<R: AsyncRead> {
    #[pin]
    r: R,
    count: u64,
}

impl<R: AsyncRead> AsyncCountRead<R> {
    #[inline(always)]
    pub fn new(r: R) -> Self {
        Self { r, count: 0 }
    }

    #[inline(always)]
    pub fn count(&self) -> u64 {
        self.count
    }

    #[inline(always)]
    pub fn get_ref(&self) -> &R {
        &self.r
    }

    #[inline(always)]
    pub fn into_inner(self) -> R {
        self.r
    }
}

impl<R: AsyncRead> AsyncRead for AsyncCountRead<R> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.project();
        let before = buf.filled().len() as u64;
        let out = this.r.poll_read(cx, buf);
        let after = buf.filled().len() as u64;
        *this.count += after - before;
        out
    }
}

/// Wraps a writer and counts how many bytes we've written in total, without
/// making any system calls.
#[pin_project]
pub struct CountAsyncWrite<W: AsyncWrite> {
    #[pin]
    w: W,
    count: u64,
}

impl<W: AsyncWrite> CountAsyncWrite<W> {
    #[inline(always)]
    pub fn new(w: W) -> Self {
        Self { w, count: 0 }
    }

    #[inline(always)]
    pub fn count(&self) -> u64 {
        self.count
    }
}

impl<W: AsyncWrite> AsyncWrite for CountAsyncWrite<W> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        let mut this = self.project();
        let out = this.r.poll_write(cx, buf);
        match &out {
            std::task::Poll::Ready(Ok(size)) => self.count += size,
            std::task::Poll::Ready(r) => todo!(),
            std::task::Poll::Pending => todo!(),
        }
        out
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.project().w.poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.project().w.poll_shutdown(cx)
    }
}
