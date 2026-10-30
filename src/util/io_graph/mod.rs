use std::{
    alloc::Layout,
    error::Error,
    sync::{
        Arc,
        atomic::{self, AtomicBool},
    },
    thread::JoinHandle,
};

use bytes::Bytes;
use futures::{StreamExt as _, stream::FuturesUnordered};
use tokio::sync::oneshot;

#[expect(unused)]
pub use self::junction::{Junction, RecvJunction};
pub use self::{
    buf::buf,
    junction::{JunctionTracker, SendJunction},
};

mod buf;
mod junction;
mod sys;
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

impl<F, T, E> Worker for F
where
    F: for<'a> FnOnce(&'a GraphContext) -> Result<T, E> + Send,
    T: Send + 'static,
    E: Error + Send + 'static,
{
    type Error = E;
    type Output = T;

    fn run(self: Box<Self>, ctx: &GraphContext) -> Result<Self::Output, Self::Error> {
        (self)(ctx)
    }
}

/// Coordination object for workers.
pub struct GraphContext {
    pub(self) halt: AtomicBool,
}

impl GraphContext {
    pub fn new() -> Self {
        Self { halt: false.into() }
    }

    /// Returns `true` if a halt has been ordered.
    pub fn halt(&self) -> bool {
        self.halt.load(atomic::Ordering::Relaxed)
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

/// A handle to a set of workers running in the background.
pub struct GraphWorkersHandle<E: Send + Error + 'static> {
    ctx: Arc<GraphContext>,
    jhs: Vec<JoinHandle<()>>,
    futures: FuturesUnordered<oneshot::Receiver<Result<(), E>>>,
}

impl<E: Send + Error + 'static> GraphWorkersHandle<E> {
    /// Create a handle that may be used to remotely halt these workers.
    ///
    /// This is useful because [`Self::wait()`] will consume this object.
    pub fn halt_handle(&self) -> GraphHaltHandle {
        GraphHaltHandle {
            ctx: self.ctx.clone(),
        }
    }

    /// Issues a halt command to all workers. Workers will attempt to halt as
    /// soon as possible.
    pub fn halt(&self) {
        self.ctx.halt.store(true, atomic::Ordering::Relaxed);
    }

    /// Wait until all of the workers have completed. Returns the first error
    /// that occurred.
    ///
    /// If a worker panics, then the panic will be propagated out by this
    /// future.
    ///
    /// Cancelling this future will not cancel the workers.
    pub async fn wait(mut self) -> Result<(), E> {
        // drop this early as it won't be needed
        drop(self.ctx);

        while let Some(x) = self.futures.next().await {
            match x {
                Ok(Ok(())) => continue,
                Ok(Err(e)) => return Err(e),

                // RecvError. Sender got dropped, so someone panicked. propagate panic out
                Err(_) => {
                    for jh in self.jhs {
                        if jh.is_finished() {
                            jh.join().expect("Worker thread panicked")
                        }
                    }
                    panic!("Worker thread dropped sender but no thread panicked!");
                }
            }
        }

        Ok(())
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

/// Run a set of workers on threads. Returns a handle that can be used to wait
/// on or cancel the workers.
pub fn run_workers<E: Send + Error + 'static>(
    workers: impl IntoIterator<Item = (String, Box<dyn Worker<Output = (), Error = E>>)>,
) -> GraphWorkersHandle<E> {
    let ctx = Arc::new(GraphContext::new());

    let (jhs, futures): (
        Vec<JoinHandle<()>>,
        FuturesUnordered<oneshot::Receiver<Result<(), E>>>,
    ) = workers
        .into_iter()
        .map(|(name, w)| {
            let (tx, rx) = oneshot::channel();
            let ctx = ctx.clone();
            let jh = std::thread::Builder::new()
                .name(name)
                .spawn(move || {
                    tx.send(w.run(&ctx)).ok();
                })
                .expect("failed to spawn worker thread");
            (jh, rx)
        })
        .unzip();

    GraphWorkersHandle { ctx, jhs, futures }
}
