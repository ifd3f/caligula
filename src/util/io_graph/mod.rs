use std::{
    alloc::Layout,
    error::Error,
    sync::{
        Arc,
        atomic::{self, AtomicBool},
    },
};

use bytes::Bytes;

mod buf;
mod builder;
mod junction;
mod sys;
pub mod util;
pub mod worker;

pub use self::{
    buf::buf,
    builder::GraphBuilder,
    junction::{Junction, JunctionId, TransferStatBundle, TransferStat},
};

/// [`Layout`] to allocate buffer pages with.
///
/// Later on we probably want to figure out how to auto-tune these values, but
/// for now, this hardcoded constant is probably reasonable.
const ALLOC_LAYOUT: Layout =
    unsafe { Layout::from_size_align_unchecked(READ_SIZE, BUFFER_ALIGNMENT) };

/// How big our buffers are.
///
/// Later on we probably want to figure out how to auto-tune these values, but
/// for now, this hardcoded constant is probably reasonable.
const READ_SIZE: usize = 65536;

/// What to align our buffer pages to.
///
/// Later on we probably want to figure out actual alignment values, but for
/// now, this hardcoded constant is probably reasonable.
const BUFFER_ALIGNMENT: usize = 16384;

/// A worker routine that is ready to be moved onto a thread and executed.
#[must_use]
pub trait Worker: Send {
    /// Final, successful value computed by this [`Worker`].
    type Output: Send + 'static;

    /// Error this [`Worker`] may encounter.
    type Error: Error + Send + 'static;

    /// Run this worker thread.
    fn run(self: Box<Self>, ctx: &GraphContext) -> Result<Self::Output, Self::Error>;
}

/// A worker that is backed by a [`FnOnce`]. It is constructed via
/// [`worker_fn()`].
pub struct FnWorker<F>(F);

pub fn worker_fn<F, T, E>(f: F) -> Box<FnWorker<F>>
where
    F: for<'a> FnOnce(&'a GraphContext) -> Result<T, E> + Send,
    T: Send + 'static,
    E: Error + Send + 'static,
{
    Box::new(FnWorker(f))
}

impl<F, T, E> Worker for FnWorker<F>
where
    F: for<'a> FnOnce(&'a GraphContext) -> Result<T, E> + Send,
    T: Send + 'static,
    E: Error + Send + 'static,
{
    type Error = E;
    type Output = T;

    fn run(self: Box<Self>, ctx: &GraphContext) -> Result<Self::Output, Self::Error> {
        (self.0)(ctx)
    }
}

/// Coordination object for workers.
pub struct GraphContext {
    pub(self) halt: AtomicBool,
}

impl GraphContext {
    pub(self) fn new() -> Self {
        Self { halt: false.into() }
    }

    /// Returns `true` if a halt has been ordered.
    pub fn halt(&self) -> bool {
        self.halt.load(atomic::Ordering::Relaxed)
    }
}

/// A handle that may be used to halt a set of workers running in the
/// background.
#[derive(Clone)]
pub struct GraphHaltHandle {
    ctx: Arc<GraphContext>,
}

impl GraphHaltHandle {
    /// Issues a halt command to all workers. Workers will attempt to halt as
    /// soon as possible.
    pub fn halt(&self) {
        self.ctx.halt.store(true, atomic::Ordering::Relaxed);
    }
}

/// An object you can send [`Bytes`] to.
pub trait SendBytes: Send {
    /// Send the given [`Bytes`]. Blocks until value is received.
    fn send(&mut self, bytes: Bytes) -> std::io::Result<()>;

    /// Gracefully close this sender.
    ///
    /// WARNING: If the SendBytes is dropped before this is explicitly called,
    /// the receiver will encounter an error!
    fn close(self) -> std::io::Result<()>;
}

/// An object you can receive [`Bytes`] from.
pub trait RecvBytes: Send {
    /// Try to receive some [`Bytes`]. If it returns `None`, there are no more
    /// bytes to be read. Blocks until result is received.
    fn recv(&mut self) -> std::io::Result<Option<Bytes>>;
}

pub enum FileType {
    File,
    Disk,
}
