use std::io::Write;
use std::{fmt::Display, path::PathBuf};

use bincode::Options;
use byteorder::{BigEndian, WriteBytesExt};
use serde::{Deserialize, Serialize};

use tokio::io::{AsyncRead, AsyncReadExt};
use tracing::{trace, trace_span};
use valuable::Valuable;

use crate::compression::CompressionFormat;
use crate::device::Type;

#[inline]
pub fn bincode_options() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_native_endian()
        .with_limit(1024)
}

pub fn write_msg(mut w: impl Write, msg: &StatusMessage) -> anyhow::Result<()> {
    let _span = trace_span!("Writing", msg = msg.as_value());
    let buf = bincode_options().serialize(msg)?;
    w.write_u32::<BigEndian>(buf.len() as u32)?;
    w.write_all(&buf)?;
    Ok(())
}

#[tracing::instrument(level = "trace", skip_all)]
pub async fn read_msg_async(mut r: impl AsyncRead + Unpin) -> std::io::Result<StatusMessage> {
    let size = r.read_u32().await?;
    let mut buf = vec![0; size as usize];
    r.read_exact(&mut buf).await?;

    let msg: StatusMessage = bincode_options()
        .deserialize(&buf)
        .expect("Failed to parse bincode from stream");
    trace!(msg = msg.as_value(), "Parsed message");
    Ok(msg)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Valuable)]
pub struct BurnConfig {
    pub dest: PathBuf,
    pub src: PathBuf,
    pub logfile: PathBuf,
    pub verify: bool,
    pub compression: CompressionFormat,
    pub target_type: Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Valuable)]
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

#[cfg(test)]
mod tests {
    use super::{read_msg_async, write_msg, InitialInfo, StatusMessage};

    #[tokio::test]
    async fn write_read_roundtrip() {
        let messages = &[
            StatusMessage::InitSuccess(InitialInfo {
                input_file_bytes: 32,
            }),
            StatusMessage::TotalBytes {
                src: 438,
                dest: 483,
            },
            StatusMessage::TotalBytes {
                src: 438,
                dest: 483,
            },
            StatusMessage::FinishedWriting { verifying: false },
        ];
        let mut buf = Vec::new();

        for msg in messages {
            write_msg(&mut buf, &msg).unwrap();
        }

        let mut reader = &buf[..];
        for msg in messages {
            let out = read_msg_async(&mut reader).await.unwrap();
            assert_eq!(&out, msg);
        }
    }
}
