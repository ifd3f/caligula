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
        self, GraphContext, JunctionTracker, SendJunction, Worker as _,
        worker::{DecompressError, DecompressorWorker, FileReader},
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
    /// History of cumulative bytes read.
    read_bytes_history: ByteSeries,
    /// How big the file is
    file_size_bytes: u64,
    /// Result of the operation. If [`None`], the operation is not yet finished.
    result: Option<Result<Bytes, Arc<HashingError>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum HashingError {
    #[error("Reader thread failed with error: {0}")]
    Read(std::io::Error),
    #[error("Decompressor thread failed with error: {0}")]
    Decompress(DecompressError),
    #[error("Hasher thread failed with error: {0}")]
    Hash(std::io::Error),
}

impl HashingState {
    fn new(now: Instant, file_size_bytes: u64) -> Self {
        Self {
            read_bytes_history: ByteSeries::new(now),
            file_size_bytes,
            result: None,
        }
    }

    fn failed(now: Instant, error: HashingError) -> Self {
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
    type Error = Arc<HashingError>;
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
    let _thread = std::thread::Builder::new()
        .name("hashworkflow".into())
        .spawn({
            let state = state.clone();
            let params = params.clone();
            move || {
                let (ctx, js) = state.as_ref();
                let end = run_thread(&params, tx_start, ctx, js);
                // send final result
                tracing::debug!(?end, "notifying with end data");
                tx_end.send(end).ok();
            }
        })
        .expect("Failed to spawn thread");

    // ensure it started correctly
    let start = match rx_start.await {
        Ok(r) => r,
        Err(_) => {
            // dropped without a value. check if end is populated and return error if so
            let err = rx_end
                .await
                .expect("Thread panicked!") // dropped
                .expect_err("rx_end was successful but rx_start was not! This is a logic error!");
            let (_tx, rx) = watch::channel(HashingState::failed(Instant::now(), err));
            return (Watch { rx }, None);
        }
    };

    tracing::info!(?start, "got successful start data");

    // start the tracker in a background task
    let (tx, rx) = watch::channel(HashingState::new(Instant::now(), start.file_size));
    let tracker_jh = tokio::task::spawn_local(async move {
        let (_ctx, js) = state.as_ref();
        tracker_coroutine(start.reader_junction, js, &mut rx_end, tx).await;
    });

    (Watch { rx }, Some(tracker_jh))
}

#[derive(Debug, Clone)]
struct StartData {
    file_size: u64,
    reader_junction: u32,
}

#[tracing::instrument(skip_all)]
async fn tracker_coroutine(
    hasher_input_junction: u32,
    js: &JunctionTracker,
    rx_end: &mut oneshot::Receiver<Result<Bytes, HashingError>>,
    tx: watch::Sender<HashingState>,
) {
    tracing::debug!(?REFRESH_PERIOD, "starting tracker coroutine");
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
                        // perform a cumulative sum
                        let last = s.read_bytes_history().last_datapoint().1;
                        s.read_bytes_history
                            .push(txfr.started(), last + txfr.bytes());
                    }
                }
            });
        }

        match rx_end.try_recv() {
            Ok(r) => {
                // done: notify and quit
                tracing::debug!("sending completion notification");
                tx.send_modify(|s| {
                    s.read_bytes_history.push(Instant::now(), s.file_size_bytes);
                    s.result = Some(r.map_err(Arc::from));
                });
                return;
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                // no response: keep going
                tracing::trace!("not complete yet, keep going");
            }
            Err(oneshot::error::TryRecvError::Closed) => {
                panic!("Hash workflow thread dropped its sender!")
            }
        }
    }
}

#[tracing::instrument(skip_all)]
fn run_thread(
    wf: &HashWorkflow,
    tx_start: oneshot::Sender<StartData>,
    ctx: &GraphContext,
    js: &JunctionTracker,
) -> Result<Bytes, HashingError> {
    std::thread::scope(move |s| -> Result<Bytes, HashingError> {
        // ensure file can be opened
        let file = FileReader::new(&wf.file, None).map_err(HashingError::Read)?;
        let file_size = file.size();

        // construct the other nodes in the graph
        let hash = wf.alg.hash_worker();
        let decompressor = match wf.compression {
            CompressionFormat::Identity => None,
            other => Some(DecompressorWorker::new(other)),
        };

        // calculate pipeline topology and what buffers are needed to connect things
        let (robjs, dobjs, hobjs) = match decompressor {
            Some(d) => {
                let (reader_send, reader_recv) = io_graph::buf(1024);
                let (decomp_send, decomp_recv) = io_graph::buf(1024);

                (
                    (file, reader_send),
                    Some((reader_recv, d, decomp_send)),
                    (decomp_recv, hash),
                )
            }
            None => {
                let (reader_send, reader_recv) = io_graph::buf(1024);

                ((file, reader_send), None, (reader_recv, hash))
            }
        };

        let reader_junction = js.create();

        let start = StartData {
            file_size,
            reader_junction: reader_junction.id(),
        };
        tracing::info!(?start, "notifying with start data");

        // notify the caller of success
        let Ok(()) = tx_start.send(start) else {
            // failed to notify? just return a sentinel
            return Ok(Bytes::new());
        };

        // actually do the thing
        let r = std::thread::Builder::new()
            .name("fileread".into())
            .spawn_scoped(s, move || {
                let (w, tx) = robjs;
                w.run(ctx, SendJunction::new(tx.bind_to_thread(), reader_junction))
            })
            .unwrap();

        let d = dobjs.map(|dobjs| {
            std::thread::Builder::new()
                .name("decompress".into())
                .spawn_scoped(s, move || {
                    let (rx, w, tx) = dobjs;
                    w.run(ctx, (rx.bind_to_thread(), tx.bind_to_thread()))
                })
                .unwrap()
        });

        let h = std::thread::Builder::new()
            .name("hash".into())
            .spawn_scoped(s, move || {
                let (rx, w) = hobjs;
                w.run(ctx, rx.bind_to_thread())
            })
            .unwrap();

        tracing::debug!("threads spawned, joining on all");
        r.join().unwrap().map_err(HashingError::Read)?;

        if let Some(d) = d {
            d.join().unwrap().map_err(HashingError::Decompress)?;
        }

        let out = h.join().unwrap().map_err(HashingError::Hash)?;

        Ok(out)
    })
}
