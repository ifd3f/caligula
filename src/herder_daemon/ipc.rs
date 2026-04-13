use serde::{Deserialize, Serialize};

pub use super::writer_process::ipc::{
    ErrorType, InitialInfo, StatusMessage, WriterProcessConfig,
};

/// Tell the herder to start a herd for performing an arbitrary action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartHerd {
    /// ID to associate with all of the herd's events
    pub id: u64,

    /// The action to perform
    pub action: WriterProcessConfig,
}
