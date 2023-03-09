use std::{fmt::Display, path::PathBuf};

use serde::{Deserialize, Serialize};

use valuable::Valuable;

use crate::device::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Valuable)]
pub struct BurnConfig {
    pub dest: PathBuf,
    pub src: PathBuf,
    pub logfile: PathBuf,
    pub verify: bool,
    pub target_type: Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Valuable)]
pub enum StatusMessage {
    InitSuccess(InitialInfo),
    TotalBytes(usize),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Valuable)]
pub struct InitialInfo {
    pub input_file_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Valuable)]
pub enum ErrorType {
    EndOfOutput,
    PermissionDenied,
    VerificationFailed,
    UnexpectedTermination,
    UnknownChildProcError(String),
}

impl From<std::io::Error> for ErrorType {
    fn from(value: std::io::Error) -> Self {
        match value.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            _ => Self::UnknownChildProcError(format!("{value}")),
        }
    }
}

impl From<serde_json::Error> for ErrorType {
    fn from(value: serde_json::Error) -> Self {
        Self::UnknownChildProcError(value.to_string())
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
        }
    }
}
