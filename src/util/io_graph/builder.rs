use std::{
    collections::BTreeMap,
    error::Error,
    sync::{
        Arc,
        atomic::{self, AtomicBool},
    },
    thread::JoinHandle,
    time::Duration,
};

use futures::{
    FutureExt, StreamExt as _,
    future::BoxFuture,
    stream::{BoxStream, FuturesUnordered},
};
use tokio::sync::oneshot;

use crate::util::io_graph::{
    GraphContext, GraphHaltHandle, Junction, Worker,
    junction::{JunctionTracker, TransferStatBundle},
};

pub struct GraphBuilder<E: Error + Send + 'static> {
    jt: JunctionTracker,
    workers: BTreeMap<String, Box<dyn Worker<Output = (), Error = E>>>,
    transfer_stat_poll_interval: Duration,
}

impl<E: Error + Send + 'static> Default for GraphBuilder<E> {
    fn default() -> Self {
        Self {
            workers: Default::default(),
            jt: JunctionTracker::new(),
            transfer_stat_poll_interval: Duration::from_millis(100), // 10Hz
        }
    }
}

impl<E: Error + Send + 'static> GraphBuilder<E> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new junction for tracking transfers.
    pub fn new_junction(&mut self) -> Junction {
        self.jt.create()
    }

    /// Register a new worker with the given thread name.
    ///
    /// Panics if the name is already in use.
    pub fn add_worker(
        &mut self,
        name: String,
        w: Box<dyn Worker<Output = (), Error = E> + 'static>,
    ) {
        if self.workers.insert(name, w).is_some() {
            panic!("attempted to create two workers with same name")
        }
    }

    pub fn set_transfer_stat_poll_interval(&mut self, transfer_stat_poll_interval: Duration) {
        self.transfer_stat_poll_interval = transfer_stat_poll_interval;
    }

    pub fn start(self) -> GraphStartResult<E> {
        let ctx = Arc::new(GraphContext::new());

        // launch all of these threads
        let (jhs, mut wait_futures): (
            Vec<JoinHandle<()>>,
            FuturesUnordered<oneshot::Receiver<Result<(), E>>>,
        ) = self
            .workers
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

        // this is really ugly but whatever, this flag is used to signal completion, and wait is the
        // future that updates it while stats is the stream that needs it
        // TODO: make this a bit nicer
        let is_completed = Arc::new(AtomicBool::new(false));

        let halt = GraphHaltHandle { ctx };

        let stats = {
            let mut interval = tokio::time::interval(self.transfer_stat_poll_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            futures::stream::unfold(
                (self.jt, interval, is_completed.clone()),
                move |(mut jt, mut interval, is_completed)| async move {
                    loop {
                        interval.tick().await;

                        let hist = jt.take_transfers();

                        if hist.is_empty() {
                            // empty and done? quit
                            if is_completed.load(atomic::Ordering::Relaxed) {
                                return None;
                            }
                            // otherwise spin around and keep waiting
                            continue;
                        }

                        return Some((hist, (jt, interval, is_completed)));
                    }
                },
            )
        };

        let wait = async move {
            while let Some(x) = wait_futures.next().await {
                match x {
                    Ok(Ok(())) => continue,
                    Ok(Err(e)) => {
                        is_completed.store(true, atomic::Ordering::Relaxed);
                        return Err(e);
                    }

                    // RecvError. Sender got dropped, so someone panicked. propagate panic out
                    Err(_) => {
                        is_completed.store(true, atomic::Ordering::Relaxed);
                        for jh in jhs {
                            if jh.is_finished() {
                                jh.join().expect("Worker thread panicked")
                            }
                        }
                        panic!("Worker thread dropped sender but no thread panicked!");
                    }
                }
            }
            Ok(())
        };

        GraphStartResult {
            halt,
            stats: stats.boxed(),
            wait: wait.boxed(),
        }
    }
}

/// The result of a bunch of workers running in the background.
pub struct GraphStartResult<E: Send + Error + 'static> {
    /// A handle that may be used to remotely halt these workers.
    ///
    /// This is useful because [`Self::wait()`] will consume this object.
    pub halt: GraphHaltHandle,
    /// Wait until all of the workers have completed. If there was an error,
    /// this returns the first error that occurred.
    ///
    /// If a worker panics, then the panic will be propagated out by this
    /// future.
    ///
    /// Cancelling this future will not cancel the workers.
    pub wait: BoxFuture<'static, Result<(), E>>,
    pub stats: BoxStream<'static, TransferStatBundle>,
}
