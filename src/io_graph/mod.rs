use std::{
    error::Error,
    io::{Read, Write},
    sync::atomic::{AtomicBool, Ordering},
};

use bytes::{Bytes, BytesMut};
use serde::{Serialize, de::DeserializeOwned};

pub use crate::io_graph::{
    active::{file_reader::FileReader, hash::HashWorker},
    junction::{Junction, JunctionTracker, RecvJunction},
    passive::buf::BufNode,
};

mod active;
mod junction;
mod passive;

/// A [`Node`] is an object in the I/O graph connected to other objects.
#[must_use]
pub trait Node<'a> {
    /// Extra information this node adds to a [`NodeInfo`].
    type Info: Serialize + DeserializeOwned + 'static;

    /// Generate the information about this node.
    fn info(&self) -> NodeInfo<'a, Self::Info>;
}

/// An active worker thread.
#[must_use]
pub trait Worker: Send {
    /// Final, successful value computed by this [`Worker`].
    type Output: Send + 'static;

    /// Error this [`Worker`] may encounter.
    type Error: Error + Send + 'static;

    fn run(self: Box<Self>, context: &GraphContext) -> Result<Self::Output, Self::Error>;
}

pub struct GraphContext {
    halt: AtomicBool,
}

impl GraphContext {
    pub fn new() -> Self {
        Self { halt: false.into() }
    }

    pub fn halt(&self) -> bool {
        self.halt.load(Ordering::Relaxed)
    }
}

/// An object you can send [`Bytes`] to.
pub trait SendBytes {
    /// Send the given [`Bytes`]. Blocks until value is received.
    fn send(&mut self, bytes: Bytes) -> std::io::Result<()>;

    /// Gracefully close this sender.
    fn close(self) -> std::io::Result<()>;
}

/// An object you can receive [`Bytes`] from.
pub trait RecvBytes {
    /// Try to receive some [`Bytes`]. If it returns `None`, there are no more
    /// bytes to be read.
    fn recv(&mut self) -> std::io::Result<Option<Bytes>>;
}

/// Information associated with a [`Node`]
pub struct NodeInfo<'a, T> {
    pub extra: T,
    pub inputs: Vec<Junction<'a>>,
    pub outputs: Vec<Junction<'a>>,
}

/// Information associated with a [`Node`]
pub struct NodeData<T> {
    pub extra: T,
    pub inputs: Vec<u32>,
    pub outputs: Vec<u32>,
}

/// Adapter from [`Write`] to [`SendBytes`].
pub struct WriteSender<W: Write> {
    inner: W,
}

impl<W: Write> WriteSender<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W: Write> SendBytes for WriteSender<W> {
    #[inline]
    fn send(&mut self, bytes: Bytes) -> std::io::Result<()> {
        self.inner.write_all(&bytes)
    }

    fn close(mut self) -> std::io::Result<()> {
        self.inner.flush()?;
        Ok(())
    }
}

/// Adapter from [`Read`] to [`RecvBytes`].
pub struct ReadReceiver<R: Read> {
    inner: R,
    read_size: usize,
}

impl<R: Read> ReadReceiver<R> {
    /// Construct a new [`ReadReceiver`]. The `read_size` is the maximum size we
    /// can read at once.
    pub fn new(inner: R, read_size: usize) -> Self {
        Self { inner, read_size }
    }
}

impl<R: Read> RecvBytes for ReadReceiver<R> {
    #[inline]
    fn recv(&mut self) -> std::io::Result<Option<Bytes>> {
        let mut buf = BytesMut::with_capacity(self.read_size);

        // SAFETY: We are going to overwrite these bytes immediately.
        // The bytes we don't read will get trimmed down to size.
        // If you're concerned that `R` may read these bytes, that's just way too
        // paranoid.
        unsafe {
            buf.set_len(self.read_size);
        }

        // read and truncate to size
        let len = self.inner.read(&mut buf)?;

        // no bytes? no more data
        if len == 0 {
            return Ok(None);
        }

        // otherwise, truncate and return
        buf.truncate(len);
        Ok(Some(buf.freeze()))
    }
}
