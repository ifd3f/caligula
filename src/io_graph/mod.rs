use std::{
    error::Error,
    sync::atomic::{AtomicBool, Ordering},
};

use bytes::Bytes;
use serde::{Serialize, de::DeserializeOwned};

pub use crate::io_graph::{
    buf::buf,
    junction::{Junction, JunctionTracker, RecvJunction},
};

pub mod worker;
mod buf;
mod junction;

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
    /// bytes to be read. Blocks until result is received.
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
