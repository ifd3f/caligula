use futures::{Sink, SinkExt, StreamExt};
use futures_core::stream::Stream;
use std::{pin::Pin, process::Stdio};
use tracing::{debug};
use valuable::Valuable;

use async_bincode::{
    tokio::{AsyncBincodeReader, AsyncBincodeWriter},
};
use tokio::{
    fs,
    process::{Child, Command},
};

use super::{
    ipc::{BurnConfig, StatusMessage, TerminateResult},
    BURN_ENV,
};

pub struct Handle {
    child: Child,
    stdin: Pin<Box<dyn Sink<BurnConfig, Error = Box<bincode::ErrorKind>>>>,
    stdout: Pin<Box<dyn Stream<Item = Result<StatusMessage, Box<bincode::ErrorKind>>>>>,
}

impl Handle {
    pub async fn start(args: BurnConfig, escalate: bool) -> anyhow::Result<Self> {
        // Get path to this process
        let proc = fs::read_link("/proc/self/exe").await?;

        debug!(
            proc = proc.to_string_lossy().to_string(),
            "Read absolute path to this program"
        );

        let mut cmd = if escalate {
            let mut cmd = Command::new("sudo");
            cmd.arg(proc);
            cmd
        } else {
            Command::new(&proc)
        };

        cmd.env(BURN_ENV, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        // .stderr(Stdio::());

        debug!(cmd = format!("{:?}", cmd), "Starting child process");
        let mut child = cmd.spawn()?;

        debug!("Opening pipes");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        // let stderr = child.stderr.take().unwrap();

        let mut proc = Self {
            child,
            stdin: Box::pin(AsyncBincodeWriter::from(stdin)),
            stdout: Box::pin(AsyncBincodeReader::from(stdout)),
        };

        debug!("Writing args to stdin");
        proc.stdin.send(args).await?;

        debug!("Reading results from stdout");
        let first_msg = proc.next_message().await?;
        debug!(
            first_msg = first_msg.as_value(),
            "Read raw result from stdout"
        );

        match first_msg {
            Some(StatusMessage::FileOpenSuccess) => Ok(proc),
            Some(StatusMessage::Terminate(t)) => Err(StartProcessError::Failed(Some(t)))?,
            Some(other) => Err(StartProcessError::UnexpectedFirstStatus(other))?,
            None => Err(StartProcessError::UnexpectedEOF)?,
        }
    }

    pub async fn next_message(&mut self) -> Result<Option<StatusMessage>, Box<bincode::ErrorKind>> {
        let message = self.stdout.next().await;
        debug!(message = format!("{:?}", message), "Got message");
        match message {
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
