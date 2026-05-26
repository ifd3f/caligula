use std::{
    alloc::Layout,
    error::Error,
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::FutureExt;
use tokio::sync::oneshot;

#[expect(unused)]
pub use self::junction::{Junction, RecvJunction};
pub(crate) use self::{
    buf::buf,
    builder::{GraphContext, WorkerInfo},
    counter::{Counter, define_counter_struct},
    junction::{JunctionTracker, SendJunction},
};

mod buf;
mod builder;
mod counter;
mod junction;
pub mod util;
pub mod worker;

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

pub trait IoGraph {
    type Snapshot;
    type Output;
    type Error: Error + Send + 'static;

    type Handle: GraphHandle<Output = Self::Output, Error = Self::Error, Snapshot = Self::Snapshot>;

    fn spawn(self, ctx: &mut GraphContext<'_>) -> Self::Handle;
}

pub trait GraphHandle {
    type Snapshot;
    type Output;
    type Error: Error + Send + 'static;

    fn poll_status(
        &mut self,
        cx: &mut Context<'_>,
        snapshot: &mut Self::Snapshot,
    ) -> Poll<Result<Self::Output, Self::Error>>;
}

/// A worker thread ready to be moved onto a thread and started with the given
/// [`Args`].
#[must_use]
pub trait Worker<Args>: Send {
    /// Final, successful value computed by this [`Worker`].
    type Output: Send + 'static;

    /// Error this [`Worker`] may encounter.
    type Error: Error + Send + 'static;

    /// Run this worker thread.
    fn run(
        self: Box<Self>,
        context: &OldGraphContext,
        args: Args,
    ) -> Result<Self::Output, Self::Error>;
}

pub struct OldGraphContext {
    halt: AtomicBool,
}

impl OldGraphContext {
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
