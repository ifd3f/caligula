use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{self, AtomicU64},
    },
};

use futures::{StreamExt, future::BoxFuture, stream::FuturesOrdered};
use serde::{Deserialize, Serialize};
use tokio::fs::{File, OpenOptions};

use crate::{
    compression::CompressionFormat,
    device,
    herding::{WorkerFactory, WorkerSpawned, writer::xplat::open_blockdev},
};

pub struct WriterFactory;

impl WorkerFactory for WriterFactory {
    type Params = WriterParams;

    type Response = ();

    type Error = SpawnWriterError;

    type State = WriterState;

    type Report = WriterReport;

    async fn spawn(
        &self,
        r: Self::Params,
    ) -> Result<WorkerSpawned<Self::Response, Self::State, BoxFuture<'static, ()>>, Self::Error>
    {
        let mut src_file = File::open(r.src).await?;

        let mut dst_file_futures: FuturesOrdered<_> = r
            .dests
            .iter()
            .map(|d| async {
                match d.device_type {
                    device::Type::File => {
                        OpenOptions::new()
                            .read(true)
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(&d.path)
                            .await
                    }
                    device::Type::Disk | device::Type::Partition => open_blockdev(&d.path).await,
                }
            })
            .collect::<FuturesOrdered<_>>();

        let mut dst_files: Vec<File> = vec![];
        while let Some(f) = dst_file_futures.next().await {
            dst_files.push(f?);
        }

        let state = Arc::new(WriterState::default());

        WorkerSpawned {
            response: (),
            state,
            future,
        }
    }

    fn report(&self, s: &Self::State) -> Self::Report {
        Self::Report {
            error: s.error.lock().unwrap().clone(),
            raw_src_bytes_read: s.raw_src_bytes_read.load(atomic::Ordering::Relaxed),
            decompressed_bytes_read: s.decompressed_bytes_read.load(atomic::Ordering::Relaxed),
            bytes_written: s.bytes_written.load(atomic::Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WriterParams {
    /// Source file to read from
    pub src: PathBuf,
    /// Compression format of the input file
    pub compression: CompressionFormat,
    /// Destination files to write to
    pub dests: Vec<Destination>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Destination {
    pub path: PathBuf,
    pub device_type: device::Type,
}

#[derive(Debug, Default)]
pub struct WriterState {
    /// How many bytes we've read so far
    pub raw_src_bytes_read: Arc<AtomicU64>,

    /// How many bytes we've read so far
    pub decompressed_bytes_read: Arc<AtomicU64>,

    /// How many bytes we've written so far
    pub bytes_written: Arc<AtomicU64>,

    /// Error encountered, if any
    pub error: std::sync::Mutex<Option<RunWriterError>>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct WriterReport {
    /// How many bytes we've read so far
    pub raw_src_bytes_read: u64,

    /// How many bytes we've read so far
    pub decompressed_bytes_read: u64,

    /// How many bytes we've written so far
    pub bytes_written: u64,

    /// Error encountered, if any
    pub error: Option<RunWriterError>,
}

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum SpawnWriterError {
    #[error("Permission denied")]
    PermissionDenied,
    #[error("I/O Error: {0}")]
    IOError(String),
}

impl From<std::io::Error> for SpawnWriterError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            _ => Self::IOError(e.to_string()),
        }
    }
}

#[derive(Debug, thiserror::Error, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum RunWriterError {}
