use std::{fmt::Display, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::compression::CompressionFormat;
use crate::device::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriterProcessConfig {
    pub dest: PathBuf,
    pub src: PathBuf,
    pub verify: bool,
    pub compression: CompressionFormat,
    pub target_type: Type,
    pub block_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusMessage {
    InitSuccess(InitialInfo),
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
    Error(ErrorType),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialInfo {
    pub input_file_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorType {
    EndOfOutput,
    PermissionDenied,
    VerificationFailed,
    UnexpectedTermination,
    UnknownChildProcError(String),
    FailedToUnmount { message: String, exit_code: i32 },
}

impl From<std::io::Error> for ErrorType {
    fn from(value: std::io::Error) -> Self {
        match value.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            _ => Self::UnknownChildProcError(format!("{value:#}")),
        }
    }
}

impl Display for ErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorType::EndOfOutput => write!(
                f,
                "Unexpected end of output file. Is your output file too small?"
            ),
            ErrorType::PermissionDenied => write!(f, "Permission denied while opening file"),
            ErrorType::VerificationFailed => write!(f, "Disk verification failed!"),
            ErrorType::UnexpectedTermination => {
                write!(f, "The child process unexpectedly terminated!")
            }
            ErrorType::UnknownChildProcError(err) => {
                write!(f, "Unknown error occurred in child process: {err}")
            }
            ErrorType::FailedToUnmount { message, exit_code } => write!(
                f,
                "Failed to unmount disk (exit code {exit_code})\n{message}"
            ),
        }
    }
}
