use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use lockfree::queue::Queue;

/// Represents a point where a worker writes to a sink, or a worker reads
/// from a source.
#[derive(Clone)]
pub struct Junction {
    inner: Arc<Inner>,
}

struct Inner {
    transfers: Queue<TransferStat>,
}

impl Junction {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                transfers: Queue::new(),
            }),
        }
    }

    pub fn log_transfer(&self, transfer: TransferStat) {
        self.inner.transfers.push(transfer);
    }

    pub fn take_transfers(&self) -> Vec<TransferStat> {
        self.inner.transfers.pop_iter().collect()
    }
    
    pub fn id(&self) -> u64 {
        Arc::as_ptr(&self.inner) as u64
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TransferStat {
    time_started_us: u64,
    duration_us: u64,
    bytes: u64,
}

impl TransferStat {
    pub fn new(bytes: u64, action_start: Instant, transfer_start: Instant) -> Self {
        let now = Instant::now();
        Self {
            time_started_us: u64::try_from((now - action_start).as_micros()).expect("too long!"),
            duration_us: u64::try_from((now - transfer_start).as_micros()).expect("too long!"),
            bytes,
        }
    }

    pub fn time_started(&self) -> Duration {
        Duration::from_micros(self.time_started_us)
    }

    pub fn duration(&self) -> Duration {
        Duration::from_micros(self.duration_us)
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}
