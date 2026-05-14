use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
    time::interval,
};

use crate::{
    byteseries::ByteSeries,
    compression::CompressionFormat,
    facade::{
        watch::Watch,
        workflow::{Workflow, WorkflowState},
    },
    hash::HashAlg,
    io_graph::{
        self, GraphContext, JunctionTracker, RecvJunction, Worker as _, worker::FileReader,
    },
};

const REFRESH_PERIOD: Duration = Duration::from_millis(10);
/// Parameters for starting a new hashing operation.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct HashWorkflow {
    /// File to use
    pub file: PathBuf,

    /// Algorithm to run
    pub alg: HashAlg,

    /// How to decompress the file before performing hash (if at all).
    pub compression: CompressionFormat,
}

impl Workflow for HashWorkflow {
    type State = HashingState;
}

/// Active, point-in-time state of a hashing operation.
pub struct HashingState {
    /// Read speed history.
    read_bytes_history: ByteSeries,
    /// How big the file is
    file_size_bytes: u64,
    /// Result of the operation. If [`None`], the operation is not yet finished.
    result: Option<Result<Bytes, Arc<std::io::Error>>>,
}

impl HashingState {
    fn new(now: Instant, file_size_bytes: u64) -> Self {
        Self {
            read_bytes_history: ByteSeries::new(now),
            file_size_bytes,
            result: None,
        }
    }

    fn failed(now: Instant, error: std::io::Error) -> Self {
        Self {
            read_bytes_history: ByteSeries::new(now),
            file_size_bytes: 0,
            result: Some(Err(error.into())),
        }
    }

    pub fn read_bytes_history(&self) -> &ByteSeries {
        &self.read_bytes_history
    }

    pub fn file_size_bytes(&self) -> u64 {
        self.file_size_bytes
    }
}

impl WorkflowState for HashingState {
    type Error = Arc<std::io::Error>;
    type Success = Bytes;

    fn result(&self) -> Option<&Result<Self::Success, Self::Error>> {
        self.result.as_ref()
    }
}

#[tracing::instrument]
pub async fn run(params: HashWorkflow) -> (Watch<HashingState>, Option<JoinHandle<()>>) {
    // state shared between worker threads and tracker coroutine
    let state = Arc::new((GraphContext::new(), JunctionTracker::new()));

    // spawn thread with channels for communicating one-off data
    let (tx_start, rx_start) = oneshot::channel();
    let (tx_end, mut rx_end) = oneshot::channel();
    let _thread = std::thread::spawn({
        let state = state.clone();
        let params = params.clone();
        move || {
            let (ctx, js) = state.as_ref();
            run_thread(&params, tx_start, tx_end, ctx, js)
        }
    });

    // ensure it started correctly
    let start = match rx_start.await.expect("thread panicked!") {
        Ok(r) => r,
        Err(e) => {
            // failed to start? return an error
            let (_tx, rx) = watch::channel(HashingState::failed(Instant::now(), e));
            return (Watch { rx }, None);
        }
    };

    tracing::info!(?start, "got successful start data");

    // start the tracker in a background task
    let (tx, rx) = watch::channel(HashingState::new(Instant::now(), start.file_size));
    let tracker_jh = tokio::task::spawn_local(async move {
        let (_ctx, js) = state.as_ref();
        tracker_coroutine(start.hasher_input_junction, js, &mut rx_end, tx).await;
    });

    (Watch { rx }, Some(tracker_jh))
}

#[derive(Debug, Clone)]
struct StartData {
    file_size: u64,
    hasher_input_junction: u32,
}

#[tracing::instrument(skip_all)]
async fn tracker_coroutine(
    hasher_input_junction: u32,
    js: &JunctionTracker,
    rx_end: &mut oneshot::Receiver<Result<Bytes, std::io::Error>>,
    tx: watch::Sender<HashingState>,
) {
    tracing::debug!("starting tracker coroutine");
    let mut interval = interval(REFRESH_PERIOD);

    loop {
        interval.tick().await;

        // log transfers into history
        let hist = js.take_transfers();
        if !hist.is_empty() {
            tracing::trace!(n_txfrs = ?hist.len(), "got multiple transfers to log");
            tx.send_modify(|s| {
                for (j, txfr) in hist {
                    if j == hasher_input_junction {
                        s.read_bytes_history.push(txfr.started(), txfr.bytes());
                    }
                }
            });
        }

        match rx_end.try_recv() {
            Ok(r) => {
                // done: notify and quit
                tx.send_modify(|s| {
                    s.read_bytes_history.push(Instant::now(), s.file_size_bytes);
                    s.result = Some(r.map_err(Arc::new));
                });
                return;
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                // no response: keep going
            }
            Err(oneshot::error::TryRecvError::Closed) => {
                panic!("Hasher thread panicked!")
            }
        }
    }
}

#[tracing::instrument(skip_all)]
fn run_thread(
    wf: &HashWorkflow,
    tx_start: oneshot::Sender<std::io::Result<StartData>>,
    tx_end: oneshot::Sender<std::io::Result<Bytes>>,
    ctx: &GraphContext,
    js: &JunctionTracker,
) {
    std::thread::scope(move |s| {
        // ensure file can be opened
        let file = match FileReader::new(&wf.file, 65536) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("error opening file for hashing: {e}");
                tx_start.send(Err(e)).ok();
                return;
            }
        };

        // construct the rest of the graph
        let hasher_input_junction = js.create();
        let (buf_input, buf_output) = io_graph::buf(1024);
        let hash = wf.alg.hash_worker();

        let start = StartData {
            file_size: file.size(),
            hasher_input_junction: hasher_input_junction.id(),
        };
        tracing::info!(?start, "notifying with start data");

        // notify the caller of success
        let Ok(()) = tx_start.send(Ok(start)) else {
            return;
        };

        // actually do the thing
        let r = std::thread::Builder::new()
            .name("fread".into())
            .spawn_scoped(s, move || file.run(ctx, buf_input.bind_to_thread()))
            .unwrap();
        let h = std::thread::Builder::new()
            .name("hash".into())
            .spawn_scoped(s, move || {
                hash.run(
                    ctx,
                    RecvJunction::new(buf_output.bind_to_thread(), hasher_input_junction),
                )
            })
            .unwrap();

        tracing::info!("threads spawned, joining on both");
        let r = r.join().unwrap();
        let h = h.join().unwrap();

        let end = r.and(h);

        // send final result
        tracing::info!(?end, "notifying with end data");
        tx_end.send(end).ok();
    })
}
