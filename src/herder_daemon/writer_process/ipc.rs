use std::{fmt::Display, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::compression::CompressionFormat;
use crate::device::Type;
use crate::herder_daemon::ipc::{self, HerdAction};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteVerifyAction {
    pub dest: PathBuf,
    pub src: PathBuf,
    pub verify: bool,
    pub compression: CompressionFormat,
    pub target_type: Type,
    pub block_size: Option<u64>,
}

impl HerdAction for WriteVerifyAction {
    type Event = WriteVerifyEvent;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteVerifyEvent {
    InitSuccess(WriteVerifyStart),
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
    Success,
    Error(WriteVerifyError),
}

ipc::impl_try_from_top_level_herd_event!(Writer => WriteVerifyEvent);

impl ipc::HerdEvent for WriteVerifyEvent {
    type StartInfo = WriteVerifyStart;
    type Failure = WriteVerifyError;

    fn downcast_as_initial_info(self) -> Result<Self::StartInfo, Self> {
        match self {
            WriteVerifyEvent::InitSuccess(e) => Ok(e),
            other => Err(other),
        }
    }

    fn downcast_as_failure(self) -> Result<Self::Failure, Self> {
        match self {
            WriteVerifyEvent::Error(e) => Ok(e),
            other => Err(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteVerifyStart {
    pub input_file_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteVerifyError {
    EndOfOutput,
    PermissionDenied,
    VerificationFailed,
    UnexpectedTermination,
    UnknownChildProcError(String),
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

impl Display for WriteVerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteVerifyError::EndOfOutput => write!(
                f,
                "Unexpected end of output file. Is your output file too small?"
            ),
            WriteVerifyError::PermissionDenied => write!(f, "Permission denied while opening file"),
            WriteVerifyError::VerificationFailed => write!(f, "Disk verification failed!"),
            WriteVerifyError::UnexpectedTermination => {
                write!(f, "The child process unexpectedly terminated!")
            }
            WriteVerifyError::UnknownChildProcError(err) => {
                write!(f, "Unknown error occurred in child process: {err}")
            }
            WriteVerifyError::FailedToUnmount { message, exit_code } => write!(
                f,
                "Failed to unmount disk (exit code {exit_code})\n{message}"
            ),
        }
    }
}
