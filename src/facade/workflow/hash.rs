use std::{
    path::PathBuf,
    pin::pin,
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures::{
    FutureExt as _, Stream, StreamExt as _,
    future::{BoxFuture, Either, select},
    stream::BoxStream,
};
use tokio::{
    select,
    sync::{oneshot, watch},
    task::JoinHandle,
    time::interval,
};

use crate::{
    codec::{compression::CompressionFormat, hash::HashAlg},
    facade::{
        watch::Watch,
        workflow::{Workflow, WorkflowState},
    },
    util::{
        byteseries::ByteSeries,
        io_graph::{
            self, GraphBuilder, GraphContext, GraphHaltHandle, JunctionId, TransferStatBundle,
            Worker as _,
            util::{JunctionAccumulator, RecvBytesExt as _, SendBytesExt as _, WorkerExt as _},
            worker::{self, DecompressError},
        },
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

#[derive(Debug, Clone)]
struct StartData {
    file_size: u64,
    reader_junction: u32,
}

pub fn run(wf: &HashWorkflow) -> Watch<HashingState> {
    match run_inner(&wf) {
        Ok((rx, _halt)) => Watch { rx },
        Err(err) => Watch {
            rx: watch::channel(HashingState::failed(Instant::now(), err)).1,
        },
    }
}

#[tracing::instrument(skip_all)]
fn run_inner(
    wf: &HashWorkflow,
) -> Result<(watch::Receiver<HashingState>, GraphHaltHandle), HashingError> {
    let start = Instant::now();
    let mut builder = GraphBuilder::new();

    // calculate pipeline topology and build buffers
    let (rparms, dparms, hparms) = match wf.compression {
        CompressionFormat::Identity => {
            let (reader_send, hasher_recv) = io_graph::buf(1024);

            (reader_send, None, hasher_recv)
        }
        cf => {
            let (reader_send, decomp_recv) = io_graph::buf(1024);
            let (decomp_send, hasher_recv) = io_graph::buf(1024);

            (
                reader_send,
                Some((decomp_recv, cf, decomp_send)),
                hasher_recv,
            )
        }
    };

    // open reader
    let reader_send = rparms.junctionize_send(builder.new_junction());
    let reader_junction = reader_send.junction().id();
    let (metadata, reader) = worker::file_reader(
        &wf.file,
        reader_send.junctionize_send(builder.new_junction()),
    )
    .map_err(HashingError::Read)?;
    let file_size = metadata.len();
    builder.add_worker("reader".into(), reader.map_err(HashingError::Read));

    // construct decompressor if you must
    dparms.map(|(rx, cf, tx)| {
        builder.add_worker(
            "decompressor".into(),
            worker::decompressor(cf, rx, tx).map_err(HashingError::Decompress),
        );
    });

    // and hasher
    let hash_recv = hparms;
    let (hash_result, hasher) = wf
        .alg
        .hash_worker(hash_recv)
        .map_err(HashingError::Hash)
        .into_output_future();
    builder.add_worker("hasher".into(), hasher);

    let r = builder.start();

    let (tx, rx) = watch::channel(HashingState::new(start, file_size));

    tokio::spawn(tracker_coroutine(
        reader_junction,
        r.stats,
        async move {
            r.wait.await?;
            Ok(hash_result.await)
        },
        tx,
    ));

    Ok((rx, r.halt))
}

#[tracing::instrument(skip_all)]
async fn tracker_coroutine(
    reader_junction: JunctionId,
    stats: impl Stream<Item = TransferStatBundle> + 'static,
    result: impl Future<Output = Result<Bytes, HashingError>>,
    tx: watch::Sender<HashingState>,
) {
    tracing::debug!(?REFRESH_PERIOD, "starting tracker coroutine");

    let mut stats = pin!(stats.fuse());
    let mut result = pin!(result);

    let mut acc = JunctionAccumulator::new();
    let mut handle_stat_stream =
        move |val: Option<TransferStatBundle>, tx: &watch::Sender<HashingState>| {
            let Some(bundle) = val else {
                return;
            };

            acc.accumulate(bundle);

            tx.send_modify(|s| {
                for txfr in acc.stats(reader_junction) {
                    // perform a cumulative sum
                    let last = s.read_bytes_history().last_datapoint().1;
                    s.read_bytes_history
                        .push(txfr.started(), last + txfr.bytes());
                }
            });

            acc.clear();
        };

    let res = loop {
        select! {
            val = stats.next() => handle_stat_stream(val, &tx),
            out = &mut result => {
                break out;
            }
        }
    };

    tx.send_modify(|s| {
        s.result = Some(res.map_err(Arc::new));
    });
}
