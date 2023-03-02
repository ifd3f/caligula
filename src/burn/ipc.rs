use std::{path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};

use crate::cli::BurnMode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BurnConfig {
    pub dest: PathBuf,
    pub src: PathBuf,
    pub mode: BurnMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusMessage {
    FileOpenSuccess,
    TotalBytesWritten(usize),
    BlockSizeChanged(u64),
    BlockSizeSpeedInfo {
        blocks_written: usize,
        block_size: usize,
        duration: Duration,
    },
    Terminate(TerminateResult),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminateResult {
    PermissionDenied,
    EndOfInput,
    EndOfOutput,
    ThreadAlreadyFinished,
    UnknownError(String),
}

impl From<std::io::Error> for TerminateResult {
    fn from(value: std::io::Error) -> Self {
        match value.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            _ => Self::UnknownError(value.to_string()),
        }
    }
}
