use std::{path::PathBuf, sync::atomic::AtomicU64};

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::herding::{WorkerFactory, WorkerSpawned};

pub struct VerifierFactory;

impl WorkerFactory for VerifierFactory {
    type Params = VerifierParams;

    type Response = ();

    type Error = SpawnVerifierError;

    type State = VerifierState;

    type Report = VerifierReport;

    async fn spawn(
        &self,
        r: Self::Params,
    ) -> Result<WorkerSpawned<Self::Response, Self::State, BoxFuture<'static, ()>>, Self::Error>
    {
        todo!()
    }

    fn report(&self, s: &Self::State) -> Self::Report {
        Self::Report {
            bytes_read: s.bytes_read.load(std::sync::atomic::Ordering::Relaxed),
            error: s.error.lock().unwrap().clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifierParams {
    /// File to verify
    pub file: PathBuf,
    /// How many bytes from the front of the file to digest for our calculation
    pub bytes_to_verify: u64,
    /// The expected hash at the end
    pub expected_sha256: Vec<u8>,
}

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum SpawnVerifierError {}

#[derive(Debug, thiserror::Error, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum RunVerifierError {}

#[derive(Debug)]
pub struct VerifierState {
    /// How many bytes we've read so far
    bytes_read: AtomicU64,

    /// Error encountered, if any
    error: std::sync::Mutex<Option<RunVerifierError>>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct VerifierReport {
    /// How many bytes we've read so far
    pub bytes_read: u64,

    /// Error encountered, if any
    pub error: Option<RunVerifierError>,
}
