use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::compression::CompressionFormat;
use crate::device::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteVerifyAction {
    pub dest: PathBuf,
    pub src: PathBuf,
    pub verify: bool,
    pub compression: CompressionFormat,
    pub target_type: Type,
    pub block_size: Option<u64>,
}

impl super::HerderAction for WriteVerifyAction {
    type Start = WriteVerifyStart;

    type Error = WriteVerifyError;

    type Event = WriteVerifyEvent;
    
    type Success = ();
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteVerifyEvent {
    TotalBytes {
        src: u64,
        dest: u64,
    },
    FinishedWriting {
        verifying: bool,
    },
    BlockSizeChanged(u64),
    BlockSizeSpeedInfo {
        blocks_written: usize,
        block_size: usize,
        duration_millis: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteVerifyStart {
    pub input_file_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum WriteVerifyError {
    #[error("Unexpected end of output file. Is your output file too small?")]
    EndOfOutput,
    #[error("Permission denied while opening file")]
    PermissionDenied,
    #[error("Disk verification failed!")]
    VerificationFailed,
    #[error("The child process unexpectedly terminated!")]
    UnexpectedTermination,
    #[error("Unknown error occurred in child process: {0}")]
    UnknownChildProcError(String),
    #[error("Failed to unmount disk (exit code {exit_code})\n{message}")]
    FailedToUnmount { message: String, exit_code: i32 },
}

impl From<std::io::Error> for WriteVerifyError {
    fn from(value: std::io::Error) -> Self {
        match value.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            _ => Self::UnknownChildProcError(format!("{value:#}")),
        }
    }
}
