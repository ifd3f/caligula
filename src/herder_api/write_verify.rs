use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::HerderAction;
use crate::{
    codec::compression::CompressionFormat,
    herder_api::error::{DiskError, InputFileError, IoError, UnmountError},
    util::{device::Type, io_graph::worker::DecompressError},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WVAction {
    pub dest: PathBuf,
    pub src: PathBuf,
    pub verify: bool,
    pub compression: CompressionFormat,
    pub target_type: Type,
    pub block_size: Option<u64>,
}

impl HerderAction for WVAction {
    type Error = WVError;
    type Event = WVEvent;
    type Start = WVStart;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WVEvent {
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WVStart {
    pub input_file_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum WVError {
    #[error("Unexpected end of output file. Is your output file too small?")]
    EndOfOutput,
    #[error("Unknown error occurred in child process: {0}")]
    UnknownChildProcError(String),
    #[error("Failed to unmount disk: {0}")]
    FailedToUnmount(#[from] UnmountError),
    #[error("The child process unexpectedly terminated!")]
    UnexpectedTermination,
    #[error("Disk verification failed!")]
    VerificationFailed,
    #[error("Error handling input: {0}")]
    InputFile(#[from] IoError<InputFileError>),
    #[error("Error handling output: {0}")]
    OutputFile(#[from] IoError<DiskError>),
    #[error("Decompression error: {0}")]
    Decompress(String), // TODO: make it a proper type. needed bc decompresserror is anyhow::error
    #[error("Output has invalid path name: {0}")]
    OutputHasInvalidPathName(String),
    #[error("Target is too small! Input file has size {in_size}, output disk has size {out_size}")]
    TargetTooSmall { in_size: u64, out_size: u64 },
}

impl WVError {
    pub fn from_reader(error: std::io::Error) -> Self {
        Self::InputFile(error.into())
    }

    pub fn from_writer(error: std::io::Error) -> Self {
        Self::OutputFile(error.into())
    }
}

impl From<DecompressError> for WVError {
    fn from(error: DecompressError) -> Self {
        Self::Decompress(error.to_string())
    }
}
