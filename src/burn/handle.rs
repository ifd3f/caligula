use futures::StreamExt;
use futures_core::stream::Stream;
use std::{pin::Pin, process::Stdio};

use async_bincode::{
    tokio::{AsyncBincodeReader, AsyncBincodeWriter},
    BincodeWriterFor,
};
use tokio::{
    fs,
    process::{Child, ChildStderr, Command},
};

use super::{
    ipc::{BurnConfig, StatusMessage, TerminateResult},
    BURN_ENV,
};

pub struct Handle {
    child: Child,
    child_stdout: Pin<Box<dyn Stream<Item = Result<StatusMessage, Box<bincode::ErrorKind>>>>>,
    child_stderr: ChildStderr,
}

impl Handle {
    pub async fn start(args: BurnConfig, escalate: bool) -> anyhow::Result<Self> {
        // Get path to this process
        let proc = fs::read_link("/proc/self/exe").await?;

        let mut cmd = if escalate {
            let mut cmd = Command::new("sudo");
            cmd.arg(proc);
            cmd
        } else {
            Command::new(&proc)
        };

        let mut child = cmd
            .env(BURN_ENV, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let child_stdin = child.stdin.take().unwrap();
        let child_stdout = child.stdout.take().unwrap();
        let child_stderr = child.stderr.take().unwrap();

        AsyncBincodeWriter::from(child_stdin).append(args)?;

        let child_stdout = Box::pin(AsyncBincodeReader::from(child_stdout));

        let mut proc = Self {
            child,
            child_stdout,
            child_stderr,
        };

        let first_msg = proc.next_message().await?;

        match first_msg {
            Some(StatusMessage::FileOpenSuccess) => Ok(proc),
            Some(StatusMessage::Terminate(t)) => Err(StartProcessError::Failed(Some(t)))?,
            Some(other) => Err(StartProcessError::UnexpectedFirstStatus(other))?,
            None => Err(StartProcessError::UnexpectedEOF)?,
        }
    }

    pub async fn next_message(&mut self) -> Result<Option<StatusMessage>, Box<bincode::ErrorKind>> {
        match self.child_stdout.next().await {
            Some(Ok(x)) => Ok(Some(x)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StartProcessError {
    #[error("Unexpected first status: {0:?}")]
    UnexpectedFirstStatus(StatusMessage),
    #[error("Unexpected end of stdout")]
    UnexpectedEOF,
    #[error("Explicit failure signaled: {0:?}")]
    Failed(Option<TerminateResult>),
}
