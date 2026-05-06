use bytes::Bytes;
use digest::Digest;
use std::{
    fs::File,
    io::{BufReader, Read},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::sync::oneshot;

use crate::{
    byteseries::ByteSeries,
    hash::{HashAlg, with_hasher},
    orchestrator::watch::Watch,
};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StartHashParams {
    pub file: PathBuf,
    pub alg: HashAlg,
}

pub struct HashStarted {
    pub state: Watch<HashState>,
}

#[derive(Debug, thiserror::Error)]
pub enum HashError {
    #[error("Error reading file: {0}")]
    ReadError(std::io::Error),
    #[error("Thread panicked!")]
    Panicked,
}

pub struct HashState {
    pub read_series: ByteSeries,
    pub result: Option<Result<Bytes, HashError>>,
}

impl HashState {
    pub fn new(now: Instant) -> Self {
        Self {
            read_series: ByteSeries::new(now),
            result: None,
        }
    }

    pub fn is_finished(&self) -> bool {
        self.result.is_some()
    }
}

pub struct HashReading {
    pub read: ByteSeries,
}

pub async fn run_bg_hash(
    p: StartHashParams,
) -> (
    Arc<AtomicU64>,
    std::thread::JoinHandle<std::io::Result<Bytes>>,
) {
    let tracker = Arc::new(AtomicU64::new(0));
    let tracker_clone = tracker.clone();
    let jh = std::thread::spawn(move || {
        let file = BufReader::new(File::open(p.file)?);
        with_hasher! {p.alg => H: {
            hash_thread::<H>(file, tracker_clone, 65536)
        }}
    });
    (tracker, jh)
}

fn hash_thread<D: Digest>(
    mut file: impl Read,
    read_bytes: Arc<AtomicU64>,
    buf_size: usize,
) -> std::io::Result<Bytes> {
    let mut d = D::new();

    let mut buf = vec![0u8; buf_size];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }

        read_bytes.fetch_add(read as u64, Ordering::Relaxed);

        d.update(&mut buf[..read]);
    }

    let hash = d.finalize();
    Ok(Bytes::copy_from_slice(&hash))
}
