use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use pin_project::pin_project;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

pub struct DuplexKillController {
    pub a2bw: CancellationToken,
    pub b2aw: CancellationToken,
    pub a2br: CancellationToken,
    pub b2ar: CancellationToken,
}

pub struct DuplexKillPipes {
    pub a2bw: KillWrite<tokio_pipe::PipeWrite>,
    pub b2aw: KillWrite<tokio_pipe::PipeWrite>,
    pub a2br: KillRead<tokio_pipe::PipeRead>,
    pub b2ar: KillRead<tokio_pipe::PipeRead>,
}

pub fn duplex_kill_pipe() -> io::Result<(DuplexKillController, DuplexKillPipes)> {
    let (a2bw, a2br) = kill_pipe()?;
    let (b2aw, b2ar) = kill_pipe()?;
    let controller = DuplexKillController {
        a2bw: a2bw.token(),
        b2aw: b2aw.token(),
        a2br: a2br.token(),
        b2ar: b2ar.token(),
    };
    let pipes = DuplexKillPipes {
        a2bw,
        b2aw,
        a2br,
        b2ar,
    };
    Ok((controller, pipes))
}

pub fn kill_pipe() -> io::Result<(
    KillWrite<tokio_pipe::PipeWrite>,
    KillRead<tokio_pipe::PipeRead>,
)> {
    let (r, w) = tokio_pipe::pipe()?;
    Ok((KillWrite::new(w), KillRead::new(r)))
}

/// A reader that can be remotely killed.
#[pin_project]
pub struct KillRead<R: AsyncRead> {
    #[pin]
    r: R,
    cancel: CancellationToken,
}

impl<R: AsyncRead> KillRead<R> {
    pub fn new(r: R) -> Self {
        Self {
            r,
            cancel: CancellationToken::new(),
        }
    }

    /// Get the cancellation token
    pub fn token(&self) -> CancellationToken {
        self.cancel.clone()
    }
}

impl<R: AsyncRead> AsyncRead for KillRead<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.project().r.poll_read(cx, buf)
    }
}

/// A pipe that can be remotely killed.
#[pin_project]
pub struct KillWrite<W: AsyncWrite> {
    #[pin]
    w: W,
    cancel: CancellationToken,
}

impl<W: AsyncWrite> KillWrite<W> {
    pub fn new(w: W) -> Self {
        Self {
            w,
            cancel: CancellationToken::new(),
        }
    }

    /// Get the cancellation token
    pub fn token(&self) -> CancellationToken {
        self.cancel.clone()
    }
}

impl<W: AsyncWrite> AsyncWrite for KillWrite<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.cancel.is_cancelled() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "pipe killed",
            )));
        }
        self.project().w.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.cancel.is_cancelled() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "pipe killed",
            )));
        }
        self.project().w.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.cancel.is_cancelled() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "pipe killed",
            )));
        }
        self.project().w.poll_shutdown(cx)
    }
}
