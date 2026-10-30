use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use futures::{Stream, StreamExt, stream::BoxStream};
use itertools::Itertools;
use serde::{Deserialize, Serialize};

pub type TransferStatBundle=Vec<(JunctionId, Vec<TransferStat>)>;

/// The [Vec] capacity of the shared junction state.
const STATS_VEC_CAPACITY: usize = 10000;


/// Allocate a [`Vec<TransferStat>`] with adequate capacity to prevent excessive
/// resizing.
fn presized_vec() -> Vec<TransferStat> {
    Vec::with_capacity(STATS_VEC_CAPACITY)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JunctionId(pub u32);

pub struct JunctionTracker {
    junctions: Vec<(JunctionId, Arc<JunctionInner>)>,
    next_id: AtomicU32,
}

impl JunctionTracker {
    pub fn new() -> Self {
        Self {
            junctions: Vec::new(),
            next_id: 0.into(),
        }
    }

    /// Create a new [`Junction`] that logs its transfers to this
    /// [`JunctionStorage`].
    pub fn create(&mut self) -> Junction {
        let id = JunctionId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let inner = Arc::new(JunctionInner::default());
        self.junctions.push((id, inner.clone()));
        Junction { id, inner }
    }

    /// Take all logged transfers out of this [`JunctionStorage`].
    pub fn take_transfers(&mut self) -> Vec<(JunctionId, Vec<TransferStat>)> {
        // preallocate vecs to swap in before entering critical sections
        let new_vecs = self.junctions.iter().map(|_| presized_vec()).collect_vec();

        // take a lock on everything first to "freeze the world" before
        // collecting stats. could be a bit better of a snapshot but it's not
        // the worst.
        // TODO: do some kind of double buffering
        let locks = self
            .junctions
            .iter()
            .map(|(id, inner)| {
                (
                    *id,
                    inner.stats.lock().expect("junction stat mutex is poisoned"),
                )
            })
            .collect_vec();

        // now take all the vecs out by swapping the new ones in, and drop the
        // locks
        let mut taken_vecs: Vec<(JunctionId, Vec<TransferStat>)> = vec![];
        for ((id, mut lock), mut new) in locks.into_iter().zip(new_vecs.into_iter()) {
            std::mem::swap(&mut *lock, &mut new);
            taken_vecs.push((id, new));
            drop(lock)
        }

        // only keep the ones that have data
        taken_vecs.retain(|(_, stats)| !stats.is_empty());

        taken_vecs
    }
}

/// A [`Junction`] represents a point at which data is being transfered. Worker
/// threads can log [`TransferStat`]s, and the reader of the parent
/// [`JunctionStorage`] can get a consistent snapshot of the state of work.
#[derive(Clone)]
pub struct Junction {
    id: JunctionId,
    inner: Arc<JunctionInner>,
}

impl Junction {
    pub fn id(&self) -> JunctionId {
        self.id
    }

    pub fn log(&self, stat: TransferStat) {
        self.inner
            .stats
            .lock()
            .expect("transfer stat mutex is poisoned")
            .push(stat)
    }
}

/// Data shared between the tracker and the junction instances.
struct JunctionInner {
    /// Contains all transfers logged to this object since the last snapshot.
    stats: Mutex<Vec<TransferStat>>,
}

impl Default for JunctionInner {
    fn default() -> Self {
        Self {
            stats: Mutex::new(presized_vec()),
        }
    }
}

/// Statistics representing a single transfer of bytes.
///
/// Transfers usually represent syscalls or calls to [`SendBytes`]/[`Read`].
#[derive(Debug, Clone, Copy)]
pub struct TransferStat {
    transfer_started: Instant,
    transfer_ended: Instant,
    bytes: u64,
}

impl TransferStat {
    pub fn new(transfer_started: Instant, transfer_ended: Instant, bytes: u64) -> Self {
        Self {
            transfer_started,
            transfer_ended,
            bytes,
        }
    }

    /// The relative time at which this operation started.
    pub fn started(&self) -> Instant {
        self.transfer_started
    }

    /// The relative time at which this operation ended.
    pub fn ended(&self) -> Instant {
        self.transfer_ended
    }

    /// How long this transfer took.
    #[expect(unused)]
    pub fn duration(&self) -> Duration {
        self.ended() - self.started()
    }

    /// How many bytes were transferred.
    ///
    /// 0 represents a failure.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Whether this operation was successful or a failure.
    #[expect(unused)]
    pub fn success(&self) -> bool {
        self.bytes != 0
    }
}
