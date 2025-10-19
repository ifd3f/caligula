use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::compression::CompressionFormat;
use crate::device::Type;

/// An abstract service that handles the herding of a set of writers. The writers may be located
/// on this process, or on a different process.
///
/// Why "Herder"? Caligula liked his horse, and horses are herded. I think. I'm not a farmer.
pub trait Herder:
    tower::Service<SpawnWriterRequest, Response = SpawnWriterResponse, Error = SpawnWriterError>
    + tower::Service<
        SpawnVerifierRequest,
        Response = SpawnVerifierResponse,
        Error = SpawnVerifierError,
    >
{
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub path: PathBuf,
    pub compression: CompressionFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub compression: CompressionFormat,
    pub target_type: Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnWriterRequest {
    pub src: Source,
    pub dest: Target,
    pub block_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnWriterResponse {
    pub id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum SpawnWriterError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnVerifierRequest {
    pub target: Target,
    pub block_size: Option<u64>,
    pub expected_hash: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnVerifierResponse {
    pub id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum SpawnVerifierError {}
