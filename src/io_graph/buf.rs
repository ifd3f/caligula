use std::{
    cell::UnsafeCell,
    marker::PhantomData,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::{Thread, park_timeout},
    time::Duration,
};

use atomicbox::AtomicBox;
use bytes::Bytes;
use ringbuf::{
    LocalRb,
    storage::Heap,
    traits::{Consumer, Observer, Producer},
};

use crate::io_graph::{RecvBytes, SendBytes};

const PARK_TIMEOUT: Duration = Duration::from_millis(2);
type Rb = LocalRb<Heap<Bytes>>;

/// Create a new paired [`BufSender`] and [`BufReceiver`].
///
/// This is backed by triple buffering. with initial capacity of all three
/// buffers set to `size`.
///
/// These cannot be used until they are each assigned onto a thread by calling
/// `.into_local()`.
pub fn buf(size: usize) -> (BufSender, BufReceiver) {
    let inner = Arc::new(Inner {
        middle_buffer: MiddleBuffer::new(size),
        receiver: OnceLock::new(),
        sender: OnceLock::new(),
        receiver_dropped: false.into(),
        sender_dropped: false.into(),
        eof: false.into(),
        size,
    });

    let tx = BufSender {
        inner: inner.clone(),
    };
    let rx = BufReceiver { inner };

    (tx, rx)
}

#[must_use]
pub struct BufSender {
    inner: Arc<Inner>,
}

impl BufSender {
    pub fn into_local(self) -> LocalBufSender {
        self.inner
            .sender
            .set(std::thread::current())
            .expect("Inner's sender thread already set!");

        let size = self.inner.size;
        LocalBufSender {
            inner: self.inner,
            back_buffer: Box::new(LocalRb::new(size)),
            _phantom: PhantomData,
        }
    }
}

#[must_use]
pub struct BufReceiver {
    inner: Arc<Inner>,
}

impl BufReceiver {
    pub fn into_local(self) -> LocalBufReceiver {
        self.inner
            .receiver
            .set(std::thread::current())
            .expect("Inner's receiver thread already set!");

        let size = self.inner.size;
        LocalBufReceiver {
            inner: self.inner,
            front_buf: Box::new(LocalRb::new(size)),
            _phantom: PhantomData,
        }
    }
}
struct Inner {
    middle_buffer: MiddleBuffer,

    /// Flag for the current state of the buffer.
    eof: AtomicBool,

    /// Is the receiver dropped?
    receiver_dropped: AtomicBool,

    /// Is the sender dropped?
    sender_dropped: AtomicBool,

    /// Receiver thread
    receiver: OnceLock<Thread>,

    /// Sender thread
    sender: OnceLock<Thread>,

    /// How big this thing is
    size: usize,
}

/// The middle buffer is the one point of shared contention between the sender
/// and receiver.
struct MiddleBuffer {
    buf: AtomicBox<Rb>,
    is_empty: AtomicBool,
}

impl MiddleBuffer {
    fn new(size: usize) -> Self {
        Self {
            buf: AtomicBox::new(Box::new(LocalRb::new(size))),
            is_empty: true.into(),
        }
    }

    /// Attempt to swap in a buffer that may or may not contain data.
    ///
    /// Returns whether or not the swap went through.
    fn try_swap(&self, replacement: &mut Box<Rb>) -> bool {
        match (
            replacement.is_empty(),
            self.is_empty.load(Ordering::Relaxed),
        ) {
            // Both empty -- don't swap, it's a no-op
            (true, true) => false,

            // Both full -- don't swap, data will be overwritten
            (false, false) => false,

            // Replacement empty, self full: this is a take operation, mark us as empty
            (true, false) => {
                self.buf.swap_mut(replacement, Ordering::SeqCst);
                self.is_empty.store(true, Ordering::SeqCst);
                true
            }

            // Replacement full, self empty: this is a put operation, mark us as full
            (false, true) => {
                self.buf.swap_mut(replacement, Ordering::SeqCst);
                self.is_empty.store(false, Ordering::SeqCst);
                true
            }
        }
    }
}

impl Inner {
    fn drop_sender(&self) {
        self.sender_dropped.store(true, Ordering::Relaxed);
        self.receiver.get().inspect(|t| t.unpark());
    }

    fn drop_receiver(&self) {
        self.receiver_dropped.store(true, Ordering::Relaxed);
        self.sender.get().inspect(|t| t.unpark());
    }
}

#[must_use]
pub struct LocalBufSender {
    inner: Arc<Inner>,

    /// Active buffer we're writing to.
    back_buffer: Box<Rb>,

    /// Used to force this to be non-[`Send`].
    _phantom: PhantomData<*const ()>,
}

impl SendBytes for LocalBufSender {
    fn send(&mut self, mut bytes: Bytes) -> std::io::Result<()> {
        if bytes.is_empty() {
            // no-op for 0 bytes because it's a waste
            return Ok(());
        }

        loop {
            // ensure receiver hasn't dipped
            if self.inner.receiver_dropped.load(Ordering::Relaxed) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "buf receiver dropped",
                ));
            }

            // Try to insert into the back buffer.
            match self.back_buffer.try_push(bytes) {
                Ok(()) => {
                    // Successfully inserted, we're done here
                    return Ok(());
                }
                Err(not_appended) => {
                    // It's full: we'll have to go empty it by swapping with middle buffer.
                    bytes = not_appended;
                }
            }

            match self.inner.middle_buffer.try_swap(&mut self.back_buffer) {
                true => {
                    // Successfully went through. Wake up the receiver, loop around, and insert into
                    // the freshly empty buffer.
                    self.inner.receiver.wait().unpark();
                }
                false => {
                    // Didn't work. Park, loop, and try again
                    park_timeout(PARK_TIMEOUT);
                }
            }
        }
    }

    fn close(mut self) -> std::io::Result<()> {
        // put the rest of the buffered data into the middle buffer
        while !self.back_buffer.is_empty() {
            self.inner.middle_buffer.try_swap(&mut self.back_buffer);
            park_timeout(PARK_TIMEOUT);
        }

        // Set the EOF flag and wake the reader
        self.inner.eof.store(true, Ordering::SeqCst);
        self.inner.receiver.wait().unpark();
        Ok(())
    }
}

impl Drop for LocalBufSender {
    fn drop(&mut self) {
        self.inner.drop_sender();
    }
}

#[must_use]
pub struct LocalBufReceiver {
    inner: Arc<Inner>,

    /// Current buffer we're reading from.
    front_buf: Box<Rb>,

    /// Used to force this to be non-[`Send`].
    _phantom: PhantomData<*const ()>,
}

impl RecvBytes for LocalBufReceiver {
    fn recv(&mut self) -> std::io::Result<Option<Bytes>> {
        loop {
            // Try to pull from the back buffer.
            if let Some(out) = self.front_buf.try_pop() {
                return Ok(Some(out));
            }

            // It's empty: we'll have to go swap with middle buffer.
            if self.inner.middle_buffer.try_swap(&mut self.front_buf) {
                // Successfully went through. Wake up the sender, loop around, and pull from
                // the freshly filled buffer.
                self.inner.sender.wait().unpark();
            }

            // No new data. Ensure not EOF
            if self.inner.eof.load(Ordering::Relaxed) {
                return Ok(None);
            }

            // Not EOF. Ensure receiver hasn't dipped
            if self.inner.sender_dropped.load(Ordering::Relaxed) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "buf receiver dropped",
                ));
            }

            // Park, loop, and try again
            park_timeout(PARK_TIMEOUT);
        }
    }
}

impl Drop for LocalBufReceiver {
    fn drop(&mut self) {
        self.inner.drop_receiver();
    }
}
