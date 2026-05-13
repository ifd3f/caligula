use std::{path::PathBuf, sync::Arc, time::Instant};

use bytes::Bytes;
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};

use crate::{
    byteseries::ByteSeries,
    compression::CompressionFormat,
    facade::{
        watch::Watch,
        workflow::{Workflow, WorkflowState},
    },
    hash::HashAlg,
    io_graph::{BufNode, FileReader, GraphContext, JunctionTracker, RecvJunction, Worker as _},
};

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
    result: Option<std::io::Result<Bytes>>,
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
            result: Some(Err(error)),
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
    type Error = std::io::Error;
    type Success = Bytes;

    fn result(&self) -> Option<&Result<Self::Success, Self::Error>> {
        self.result.as_ref()
    }
}

pub async fn run(params: HashWorkflow) -> (Watch<HashingState>, Option<JoinHandle<()>>) {
    let state = Arc::new((GraphContext::new(), JunctionTracker::new()));

    let (tx_start, rx_start) = oneshot::channel();
    let (tx_end, rx_end) = oneshot::channel();

    let _thread = std::thread::spawn({
        let state = state.clone();
        let params = params.clone();
        move || {
            let (ctx, js) = state.as_ref();
            run_thread(&params, tx_start, tx_end, ctx, js)
        }
    });

    let start = match rx_start.await.expect("thread panicked!") {
        Ok(r) => r,
        Err(e) => {
            let (_tx, rx) = watch::channel(HashingState::failed(Instant::now(), e));
            return (Watch { rx }, None);
        }
    };

    let (_tx, rx) = watch::channel(HashingState::new(Instant::now(), start.size));
    let jh = tokio::task::spawn_local(async move {
        let (_ctx, _js) = state.as_ref();

        rx_end.await.unwrap().unwrap();
    });

    (Watch { rx }, Some(jh))
}

struct StartData {
    size: u64,
    hasher_input_junction: u32,
}

fn run_thread(
    wf: &HashWorkflow,
    tx_start: oneshot::Sender<std::io::Result<StartData>>,
    tx_end: oneshot::Sender<std::io::Result<Bytes>>,
    ctx: &GraphContext,
    js: &JunctionTracker,
) {
    std::thread::scope(move |s| {
        let setup = (|| {
            let buf = BufNode::new(1024);

            let j = js.create();

            let read = FileReader::new(&wf.file, buf.input, 65536)?;
            let start_data = StartData {
                size: read.size(),
                hasher_input_junction: j.id(),
            };
            let hash = wf.alg.hash_worker(RecvJunction::new(buf.output, j));

            Ok::<_, std::io::Error>((start_data, read, hash))
        })();

        let (read, hash) = match setup {
            Ok((start_data, read, hash)) => {
                let Ok(()) = tx_start.send(Ok(start_data)) else {
                    return;
                };
                (read, hash)
            }
            Err(err) => {
                tx_start.send(Err(err)).ok();
                return;
            }
        };

        let r = s.spawn(move || read.run(ctx));
        let h = s.spawn(move || hash.run(ctx));

        let r = r.join().unwrap();
        let h = h.join().unwrap();

        tx_end.send(r.and(h)).ok();
    })
}
