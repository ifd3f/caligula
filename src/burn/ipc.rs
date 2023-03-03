use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::cli::BurnMode;
use valuable::Valuable;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Valuable)]
pub struct BurnConfig {
    pub dest: PathBuf,
    pub src: PathBuf,
    pub mode: BurnMode,
    pub verify: bool,
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
    Terminate(TerminateResult),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Valuable)]
pub struct InitialInfo {
    pub input_file_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Valuable)]
pub enum TerminateResult {
    Success,
    EndOfOutput,
    PermissionDenied,
    ThreadAlreadyFinished,
    UnknownError(String),
}

impl From<std::io::Error> for TerminateResult {
    fn from(value: std::io::Error) -> Self {
        match value.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            _ => Self::UnknownError(format!("{value}")),
        }
    }
}

impl From<serde_json::Error> for TerminateResult {
    fn from(value: serde_json::Error) -> Self {
        Self::UnknownError(value.to_string())
    }
}
