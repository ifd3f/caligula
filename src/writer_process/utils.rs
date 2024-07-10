use std::{
    io::{BufReader, Read, Seek, Write},
    pin::{pin, Pin},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use futures::{future::BoxFuture, FutureExt};
use pin_project::pin_project;
use tokio::{
    fs::File,
    io::{AsyncRead, AsyncSeek, AsyncWrite},
};

use crate::compression::{decompress, CompressionFormat, DecompressRead};

/// Wraps a reader and counts how many bytes we've read in total, without
/// making any system calls.
pub struct CountRead<R: Read> {
    r: R,
    count: u64,
}

impl<R: Read> CountRead<R> {
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
}

impl<R: Read> Read for CountRead<R> {
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let bytes = self.r.read(buf)?;
        self.count += bytes as u64;
        Ok(bytes)
    }
}

/// Wraps a writer and counts how many bytes we've written in total, without
/// making any system calls.
#[pin_project]
pub struct CountWrite<W: AsyncWrite> {
    #[pin]
    w: W,
    count: u64,
}

impl<W: AsyncWrite> CountWrite<W> {
    #[inline(always)]
    pub fn new(w: W) -> Self {
        Self { w, count: 0 }
    }

    #[inline(always)]
    pub fn count(&self) -> u64 {
        self.count
    }
}

fn inspect_poll<T>(p: Poll<T>, f: impl FnOnce(&T) -> ()) -> Poll<T> {
    match &p {
        Poll::Ready(x) => f(x),
        Poll::Pending => (),
    }
    p
}

impl<W: AsyncWrite> AsyncWrite for CountWrite<W> {
    #[inline(always)]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let proj = self.as_mut().project();
        let poll = proj.w.poll_write(cx, buf);
        inspect_poll(poll, move |r| {
            r.as_ref().inspect(|c| {
                *proj.count += **c as u64;
            });
        })
    }

    #[inline(always)]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.project().w.poll_flush(cx)
    }

    #[inline(always)]
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.project().w.poll_shutdown(cx)
    }
}

/// [`File::flush`] is a lie. It does literally nothing on most OSes. This is a
/// simple wrapper over [`File`] that:
///
/// - trivially delegates [`Read`] and [`Seek`]
/// - trivially delegates [`Write::write`]
/// - replaces [`Write::flush`] with the platform-specific synchronous call to ensure
///   that the data has been written to the disk.
#[pin_project]
pub struct SyncDataFile {
    #[pin]
    state: SyncDataState,
}

#[pin_project(project = SyncDataStateProj)]
enum SyncDataState {
    NotFlushing(#[pin] File),
    Flushing {
        file: Arc<File>,
        future: BoxFuture<'static, std::io::Result<()>>,
    },
}

impl SyncDataFile {
    fn new(file: File) -> Self {
        Self {
            state: SyncDataState::NotFlushing(file),
        }
    }

    fn try_get_file(self: Pin<&mut Self>) -> Option<Pin<&mut File>> {
        match self.project().state.project() {
            SyncDataStateProj::NotFlushing(f) => Some(f),
            _ => None,
        }
    }
}

impl AsyncRead for SyncDataFile {
    #[inline(always)]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.try_get_file() {
            Some(file) => file.poll_read(cx, buf),
            None => Poll::Pending,
        }
    }
}

impl AsyncWrite for SyncDataFile {
    #[inline(always)]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.try_get_file() {
            Some(file) => file.poll_write(cx, buf),
            None => Poll::Pending,
        }
    }

    #[inline(always)]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        #[cfg(target_os = "linux")]
        {
            let (file, mut fut) = match self.state {
                SyncDataState::NotFlushing(f) => {
                    let f = Arc::new(f);
                    let f2 = f.clone();
                    (f, async move { f2.sync_data().await }.boxed())
                }
                SyncDataState::Flushing { file, future } => (file, future),
            };
            let p = fut.poll_unpin(cx);
            self.state = match &p {
                Poll::Ready(_) => {
                    drop(fut);
                    SyncDataState::NotFlushing(
                        Arc::try_unwrap(file)
                            .expect("this should be the last instance of this Arc!"),
                    )
                }
                Poll::Pending => SyncDataState::Flushing { file, future: fut },
            };
            p
        }

        // On MacOS, calling sync_data() on a disk yields "Inappropriate ioctl for device (os error 25)"
        // so for now we will just no-op.
        #[cfg(target_os = "macos")]
        {
            Poll::Ready(Ok(()))
        }
    }

    #[inline(always)]
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.try_get_file() {
            Some(file) => file.poll_shutdown(cx),
            None => Poll::Pending,
        }
    }
}

impl AsyncSeek for SyncDataFile {
    fn start_seek(self: Pin<&mut Self>, position: std::io::SeekFrom) -> std::io::Result<()> {
        match self.try_get_file() {
            Some(file) => file.start_seek(position),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "other file operation is pending, call poll_complete before start_seek",
            )),
        }
    }

    fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        match self.try_get_file() {
            Some(file) => file.poll_complete(cx),
            None => Poll::Pending,
        }
    }
}

/// A reader type specifically for [`super::WriteOp`] and [`super::VerifyOp`] to
/// read stuff off of files.
///
/// It provides decompression, buffering, and instrumentation of read stats.
pub struct FileSourceReader<R: Read>(CountRead<DecompressRead<BufReader<CountRead<R>>>>);

impl<R: Read> FileSourceReader<R> {
    #[inline(always)]
    pub fn new(cf: CompressionFormat, buf_size: usize, r: R) -> Self {
        FileSourceReader(CountRead::new(
            decompress(cf, BufReader::with_capacity(buf_size, CountRead::new(r))).unwrap(),
        ))
    }

    /// How many bytes we've read from the file. In other words, pre-decompression size.
    #[inline(always)]
    pub fn read_file_bytes(&self) -> u64 {
        self.0.get_ref().get_ref().get_ref().count()
    }

    /// How many bytes we've read after decompression.
    #[inline(always)]
    pub fn decompressed_bytes(&self) -> u64 {
        self.0.count()
    }
}

impl<R: Read> Read for FileSourceReader<R> {
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> futures_io::Result<usize> {
        self.0.read(buf)
    }
}
