use std::{
    marker::PhantomData,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::{Thread, park_timeout},
    time::Duration,
};

use bytes::Bytes;
use ringbuf::{
    LocalRb,
    storage::Heap,
    traits::{Consumer, Observer, Producer},
};

use crate::io_graph::{RecvBytes, SendBytes};

const PARK_TIMEOUT: Duration = Duration::from_millis(1);

/// Create a new paired [`BufSender`] and [`BufReceiver`].
///
/// This is backed by double buffering, with initial capacity set to
/// `size`.
///
/// These cannot be used until they are each assigned onto a thread by calling
/// `.into_local()`.
pub fn buf(size: usize) -> (BufSender, BufReceiver) {
    let inner = Arc::new(Inner {
        back_buffer: Mutex::new(LocalRb::new(size)),
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

        LocalBufSender {
            inner: self.inner,
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
            front_buf: LocalRb::new(size),
            _phantom: PhantomData,
        }
    }
}
struct Inner {
    /// The back buffer that gets appended to.
    back_buffer: Mutex<LocalRb<Heap<Bytes>>>,

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
            //
            // This operation is unlikely to have contention because the receiver holds this
            // lock for an extremely short period of time.
            let mut lock = self.inner.back_buffer.lock().unwrap();
            let queue = lock.as_mut();

            let result = queue.try_push(bytes);
            drop(lock);

            match result {
                Ok(()) => {
                    // Successfully inserted: wake up the receiver
                    self.inner.receiver.wait().unpark();
                    return Ok(());
                }
                Err(not_appended) => {
                    // It's full: park until we get woken up, then try again
                    bytes = not_appended;
                    park_timeout(PARK_TIMEOUT);
                }
            }
        }
    }

    fn close(self) -> std::io::Result<()> {
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
    front_buf: LocalRb<Heap<Bytes>>,

    /// Used to force this to be non-[`Send`].
    _phantom: PhantomData<*const ()>,
}

impl RecvBytes for LocalBufReceiver {
    fn recv(&mut self) -> std::io::Result<Option<Bytes>> {
        loop {
            if let Some(val) = self.front_buf.try_pop() {
                // item is available, so return it
                return Ok(Some(val));
            }

            // front buffer is empty, try taking the back buffer
            let mut lock = self.inner.back_buffer.lock().unwrap();
            std::mem::swap(lock.as_mut(), &mut self.front_buf);
            drop(lock);

            if !self.front_buf.is_empty() {
                // we have new elements -- loop around again
                continue;
            }

            // no new elements. are we EOF?
            if self.inner.eof.load(Ordering::SeqCst) {
                return Ok(None);
            }

            // no new elements, and not EOF. did sender dip?
            if self.inner.sender_dropped.load(Ordering::Relaxed) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "buf sender dropped",
                ));
            }
        }
    }
}

impl Drop for LocalBufReceiver {
    fn drop(&mut self) {
        self.inner.drop_receiver();
    }
}
