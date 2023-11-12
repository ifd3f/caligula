use serde::{Deserialize, Serialize};
use sesh::Recv;

use crate::writer_process::ipc::WriterProcessConfig;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SpawnRequest {
    pub child_id: u64,
    pub config: WriterProcessConfig,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum StatusMessage {
    SpawnFailure { child_id: u64, error: String },
    ChildExited { child_id: u64, code: Option<i32> },
    FatalError { error: String },
}
