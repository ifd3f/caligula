use serde::{Deserialize, Serialize};

use crate::writer_process::ipc::WriterProcessConfig;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscalatedDaemonInitConfig {
    // Okay, there's nothing here right now, but there might be someday!
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnWriter {
    pub log_file: String,
    pub init_config: WriterProcessConfig,
}
