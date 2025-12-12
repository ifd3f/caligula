//! Single producer, multiple consumer byte pipe.

use std::{
    cmp::{self, min},
    io::{Read, Write},
    pin::{pin, Pin},
    slice::{self},
    sync::{
        atomic::{self, AtomicUsize}, Arc, Weak
    },
    task::Poll,
    usize,
};

use futures::{FutureExt, executor::block_on};
use tokio::{
    io::AsyncWrite,
    runtime::Handle,
    sync::{Notify, watch},
};
use tracing_subscriber::fmt::writer;

pub struct HyperpipeWriter {
    s: Arc<Shared>,

    /// References to the readers' positions.
    reader_positions: Vec<Weak<AtomicUsize>>,

    /// When this Arc gets dropped, the readers are notified that the
    /// connection is closed.
    _writer_liveness_token: Arc<()>,
}

pub struct HyperpipeReader {
    s: Arc<Shared>,

    /// Position of this reader.
    pos: Arc<AtomicUsize>,
}

struct Shared {
    /// The buffer backing our pipe.
    buf: Vec<u8>,
    /// This value is monotonically increasing.
    writer_pos: Arc<AtomicUsize>,
    /// This is fired when the writer's position is advanced.
    notify_writer_advanced: Notify,
    /// This is fired when ANY reader is advanced.
    notify_reader_advanced: Notify,
    /// A reference to a token owned by the writer. This mechanism is used to
    /// signal writer being finished.
    writer_liveness_token: Weak<()>,
}

impl Shared {
    fn capacity(&self) -> usize {
        self.buf.len()
    }

    fn buf_ptr(&self) -> *const u8 {
        self.buf.as_ptr()
    }

    fn load_writer_pos(&self) -> usize {
        self.writer_pos.load(atomic::Ordering::SeqCst)
    }
}

impl HyperpipeWriter {
    pub fn new(capacity: usize) -> Self {
        let mut buf = Vec::with_capacity(capacity);
        unsafe {
            buf.set_len(capacity);
        }

        let liveness_token = Arc::new(());

        let s = Arc::new(Shared {
            buf,
            writer_pos: Arc::new(AtomicUsize::new(0)),
            notify_writer_advanced: Notify::new(),
            notify_reader_advanced: Notify::new(),
            writer_liveness_token: Arc::downgrade(&liveness_token),
        });
        Self {
            s,
            reader_positions: vec![],
            _writer_liveness_token: liveness_token,
        }
    }

    /// Create a new [HyperpipeReader]
    pub fn tee(&mut self) -> HyperpipeReader {
        let reader_pos = Arc::new(AtomicUsize::new(self.s.load_writer_pos()));
        self.reader_positions.push(Arc::downgrade(&reader_pos));

        HyperpipeReader {
            s: self.s.clone(),
            pos: reader_pos,
        }
    }

    /// Write onto the pipe without performing any copies.
    pub fn write_inplace<R>(&mut self, f: impl FnOnce(&mut [u8]) -> (R, usize)) -> R {
        let slice = self.next_writeable_slice();
        let (out, advance) = f(slice);
        assert!(
            advance <= slice.len(),
            "advance should not pass slice length"
        );
        self.advance(advance);
        out
    }

    pub async fn write_inplace_async<Fut, R>(&mut self, f: impl FnOnce(&mut [u8]) -> Fut) -> R
    where
        Fut: IntoFuture<Output = (R, usize)>,
    {
        let slice = self.next_writeable_slice();
        let (out, advance) = f(slice).await;
        assert!(
            advance <= slice.len(),
            "advance should not pass slice length"
        );
        self.advance(advance);
        out
    }

    /// Number of bytes we can still write before we block due to backpressure
    #[inline(always)]
    pub fn available_capacity(&mut self) -> usize {
        let writer_pos = self.s.load_writer_pos();
        let reader_pos = self.slowest_reader_position();
        self.s.capacity() - (writer_pos - reader_pos)
    }

    #[inline(always)]
    fn advance(&mut self, amount: usize) {
        self.s
            .writer_pos
            .fetch_add(amount, atomic::Ordering::SeqCst);
        self.s.notify_writer_advanced.notify_waiters();
    }

    pub async fn wait_until_can_write(&mut self) {
        while self.available_capacity() == 0 {
            self.s.notify_reader_advanced.notified().await;
        }
    }

    fn next_writeable_slice(&mut self) -> &mut [u8] {
        let writer_pos = self.s.load_writer_pos();
        let reader_pos = self.slowest_reader_position();

        if writer_pos - reader_pos >= self.s.capacity() {
            // Ran out of capacity
            return &mut [];
        }

        let writer_index = self.s.load_writer_pos() % self.s.capacity();
        let reader_index = self.slowest_reader_position() % self.s.capacity();
        let writer_ptr = self.s.buf_ptr().wrapping_add(writer_index) as *mut u8;

        unsafe {
            match writer_index.cmp(&reader_index) {
                cmp::Ordering::Less => {
                    // slowest reader is in front of writer
                    // |VVVVV|-----|VVVVV|
                    //       W     |
                    //             R
                    slice::from_raw_parts_mut(writer_ptr, reader_index - writer_index)
                }
                cmp::Ordering::Equal => {
                    // readers and writer are at the same place
                    // |-----|-----------|
                    //       W
                    //       R
                    slice::from_raw_parts_mut(writer_ptr, self.s.capacity() - writer_index)
                }
                cmp::Ordering::Greater => {
                    // slowest reader is behind writer
                    // |-----|VVVVV|-----|
                    //       |     W
                    //       R
                    slice::from_raw_parts_mut(writer_ptr, self.s.capacity() - writer_index)
                }
            }
        }
    }

    /// Returns a snapshot of the slowest reader's absolute position.
    ///
    /// Since positions are always monotonically increasing, if this value gets
    /// out-of-date, it will imply less available capacity than there actually is.
    fn slowest_reader_position(&mut self) -> usize {
        // If there are no readers, returning the current writer position implies
        // that the pipe is completely available for writing.
        let mut min = self.s.load_writer_pos();

        // Garbage-collect the released readers while searching for the reader
        // lagging behind the most
        let mut gced_readers = vec![];
        for weak in self.reader_positions.iter() {
            let Some(upgraded) = weak.upgrade() else {
                continue;
            };
            gced_readers.push(weak.clone());
            let loaded = upgraded.load(atomic::Ordering::SeqCst);

            if loaded < min {
                min = loaded;
            }
        }

        self.reader_positions = gced_readers;

        min
    }
}

impl HyperpipeReader {
    pub fn read_inplace<R>(&mut self, f: impl FnOnce(&[u8]) -> (R, usize)) -> R {
        let slice = self.next_readable_slice();
        let (out, advance) = f(slice);
        assert!(
            advance <= slice.len(),
            "advance should not pass slice length"
        );
        self.advance(advance);
        out
    }

    pub async fn read_inplace_async<Fut, R>(&mut self, f: impl FnOnce(&[u8]) -> Fut) -> R
    where
        Fut: IntoFuture<Output = (R, usize)>,
    {
        let slice = self.next_readable_slice();
        let (out, advance) = f(slice).await;
        assert!(
            advance <= slice.len(),
            "advance should not pass slice length"
        );
        self.advance(advance);
        out
    }

    #[inline(always)]
    fn advance(&mut self, amount: usize) {
        self.pos.fetch_add(amount, atomic::Ordering::SeqCst);
        self.s.notify_reader_advanced.notify_waiters();
    }

    /// Number of bytes we can still read before we block waiting for the writer
    #[inline(always)]
    pub fn available_capacity(&mut self) -> usize {
        let writer_pos = self.s.load_writer_pos();
        let reader_pos = self.pos.load(atomic::Ordering::SeqCst);
        self.s.capacity() - (writer_pos - reader_pos)
    }

    pub async fn wait_until_can_read(&mut self) {
        while self.available_capacity() == 0 {
            self.s.notify_reader_advanced.notified().await;
        }
    }

    fn next_readable_slice(&self) -> &[u8] {
        let reader_pos = self.pos.load(atomic::Ordering::SeqCst);
        let writer_pos = self.s.load_writer_pos();

        if reader_pos >= writer_pos {
            // We are out of data to read
            return &[];
        }

        let reader_index = reader_pos % self.s.capacity();
        let writer_index = writer_pos % self.s.capacity();
        let reader_ptr = self.s.buf_ptr().wrapping_add(reader_index);

        unsafe {
            match writer_index.cmp(&reader_index) {
                cmp::Ordering::Less => {
                    // slowest reader is in front of writer
                    // |VVVVV|-----|VVVVV|
                    //       W     |
                    //             R
                    slice::from_raw_parts(reader_ptr, self.s.capacity() - reader_index)
                }
                cmp::Ordering::Equal => {
                    // readers and writer are at the same place
                    // |-----|-----------|
                    //       W
                    //       R
                    slice::from_raw_parts(reader_ptr, 0)
                }
                cmp::Ordering::Greater => {
                    // slowest reader is behind writer
                    // |-----|VVVVV|-----|
                    //       |     W
                    //       R
                    slice::from_raw_parts(reader_ptr, writer_index - reader_index)
                }
            }
        }
    }
}

impl Write for HyperpipeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        loop {
            let written = self.write_inplace(|dst| {
                let amount = min(buf.len(), dst.len());
                for i in 0..amount {
                    dst[i] = buf[i];
                }
                (amount, amount)
            });

            if written == 0 {
                Handle::current().block_on(self.wait_until_can_write());
                continue;
            }

            return Ok(written);
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl AsyncWrite for HyperpipeWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let this = self.get_mut();

        loop {
            let written = this.write_inplace(|dst| {
                let amount = min(buf.len(), dst.len());
                for i in 0..amount {
                    dst[i] = buf[i];
                }
                (amount, amount)
            });

            if written == 0 {
                let fut = pin!(this.wait_until_can_write());
                match fut.poll(cx) {
                    Poll::Ready(_) => continue,
                    Poll::Pending => return Poll::Pending,
                }
            }

            return Poll::Ready(Ok(written));
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl Read for HyperpipeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.read_inplace(|src| {
                let amount = min(buf.len(), src.len());
                for i in 0..amount {
                    buf[i] = src[i];
                }
                (amount, amount)
            });

            if read == 0 {
                Handle::current().block_on(self.wait_until_can_read());
                continue;
            }

            return Ok(read);
        }
    }
}

#[cfg(test)]
mod test {

    use std::io::{Read, Write};

    use itertools::Itertools;
    use quickcheck_macros::quickcheck;

    use crate::util::hyperpipe::HyperpipeWriter;

    #[tokio::test]
    #[quickcheck]
    async fn write_read_multiple( xs: Vec<u8>) {
        let mut writer = HyperpipeWriter::new(xs.len() * 2);
        let readers = (0..10).map(|_| writer.tee()).collect_vec();
        writer.write(&xs).unwrap();

        for mut r in readers {
            let mut buf = vec![0u8; xs.len()];
            r.read(&mut buf).unwrap();

            assert_eq!(buf, xs);
        }
    }
}
