use std::{pin::Pin, process::Stdio};
use tracing::debug;
use valuable::Valuable;

use tokio::{
    fs,
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

use super::{
    ipc::{BurnConfig, StatusMessage, TerminateResult},
    BURN_ENV,
};

pub struct Handle {
    child: Child,
    stdin: Pin<Box<dyn AsyncWrite>>,
    stdout: Pin<Box<dyn AsyncBufRead>>,
}

impl Handle {
    pub async fn start(args: &BurnConfig, escalate: bool) -> anyhow::Result<Self> {
        // Get path to this process
        let proc = fs::read_link("/proc/self/exe").await?;

        debug!(
            proc = proc.to_string_lossy().to_string(),
            "Read absolute path to this program"
        );

        let args = serde_json::to_string(args)?;
        debug!("Converted BurnConfig to JSON: {args}");

        let mut cmd = if escalate {
            let mut cmd = Command::new("sudo");
            cmd.arg(format!("{BURN_ENV}=1")).arg(proc);
            cmd
        } else {
            let mut cmd = Command::new(&proc);
            cmd.env(BURN_ENV, "1");
            cmd
        };

        cmd.arg(args)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        // .stderr(Stdio::());

        debug!(cmd = format!("{:?}", cmd), "Starting child process");
        let mut child = cmd.spawn()?;

        debug!("Opening pipes");
        let stdin = Box::pin(child.stdin.take().unwrap());
        let stdout = Box::pin(BufReader::new(child.stdout.take().unwrap()));

        let mut proc = Self {
            child,
            stdin,
            stdout,
        };

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

    pub async fn next_message(&mut self) -> anyhow::Result<Option<StatusMessage>> {
        let mut line = String::new();
        let count = self.stdout.read_line(&mut line).await?;
        if count == 0 {
            return Ok(None);
        }

        debug!(line, "Got line");

        let message = serde_json::from_str::<StatusMessage>(&line)?;
        debug!(message = format!("{message:?}"), "Parsed message");

        Ok(Some(message))
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StartProcessError {
    #[error("Unexpected first status: {0:?}")]
    UnexpectedFirstStatus(StatusMessage),
    #[error("Unexpected end of stdout")]
    UnexpectedEOF,
    #[error("Explicit failure signaled: {0:?}")]
    Failed(Option<TerminateResult>),
}
