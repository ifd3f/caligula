use std::{
    sync::{
        RwLock,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use lockfree::queue::Queue;

use super::{RecvBytes, SendBytes};

pub struct JunctionTracker {
    /// Contains all transfers logged to this object since the last snapshot.
    ///
    /// Ironically, the "read" part of the lock is for the writers, while the
    /// "write" part of the lock is for the readers. The reason to do it
    /// this way is because the reader wants to get a consistent snapshot of
    /// the entire state of the operation, but multiple writers can write to
    /// the same thing at once.
    transfers: RwLock<Inner>,
    next_id: AtomicU32,
}

#[derive(Default)]
struct Inner {
    q: Queue<(u32, TransferStat)>,
}

impl JunctionTracker {
    pub fn new() -> Self {
        Self {
            transfers: RwLock::new(Inner { q: Queue::new() }),
            next_id: 0.into(),
        }
    }

    /// Create a new [`Junction`] that logs its transfers to this
    /// [`JunctionStorage`].
    pub fn create<'a>(&'a self) -> Junction<'a> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        Junction { id, parent: self }
    }

    /// Take all logged transfers out of this [`JunctionStorage`].
    pub fn take_transfers(&self) -> Vec<(u32, TransferStat)> {
        let mut lock = self.transfers.write().unwrap();
        let values = std::mem::take(&mut *lock);
        drop(lock);

        values.q.pop_iter().collect()
    }
}

/// A [`Junction`] represents a point at which data is being transfered. Worker
/// threads can log [`TransferStat`]s, and the reader of the parent
/// [`JunctionStorage`] can get a consistent snapshot of the state of work.
#[derive(Clone)]
pub struct Junction<'a> {
    id: u32,
    parent: &'a JunctionTracker,
}

impl<'a> Junction<'a> {
    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn log(&self, stat: TransferStat) {
        self.parent
            .transfers
            .read()
            .unwrap()
            .q
            .push((self.id, stat));
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

/// A [`RecvBytes`] that logs to a [`Junction`].
#[must_use]
pub struct RecvJunction<'a, Rx: RecvBytes> {
    recv: Rx,
    junction: Junction<'a>,
}

impl<'a, Rx: RecvBytes> RecvJunction<'a, Rx> {
    #[expect(unused)]
    pub fn new(recv: Rx, junction: Junction<'a>) -> Self {
        Self { recv, junction }
    }

    #[expect(unused)]
    pub fn junction(&self) -> &Junction<'a> {
        &self.junction
    }
}

impl<'a, Rx: RecvBytes> RecvBytes for RecvJunction<'a, Rx> {
    fn recv(&mut self) -> std::io::Result<Option<bytes::Bytes>> {
        let start = Instant::now();
        let result = self.recv.recv();
        let end = Instant::now();

        let len = match &result {
            Ok(Some(b)) => b.len() as u64,
            _ => 0,
        };

        self.junction.log(TransferStat::new(start, end, len));

        result
    }
}

/// A [`SendBytes`] that logs to a [`Junction`].
#[must_use]
pub struct SendJunction<'a, Tx: SendBytes> {
    send: Tx,
    junction: Junction<'a>,
}

impl<'a, Tx: SendBytes> SendJunction<'a, Tx> {
    pub fn new(send: Tx, junction: Junction<'a>) -> Self {
        Self { send, junction }
    }

    #[expect(unused)]
    pub fn junction(&self) -> &Junction<'a> {
        &self.junction
    }
}

impl<'a, Tx: SendBytes> SendBytes for SendJunction<'a, Tx> {
    fn send(&mut self, bytes: bytes::Bytes) -> std::io::Result<()> {
        let len = bytes.len();
        let start = Instant::now();
        let result = self.send.send(bytes);
        let end = Instant::now();

        self.junction.log(TransferStat::new(
            start,
            end,
            result.as_ref().map(|_| len as u64).unwrap_or(0),
        ));

        result
    }

    fn close(self) -> std::io::Result<()> {
        self.send.close()
    }
}
